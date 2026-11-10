//! The integrity check (SPEC 5.2, W15, 9's Check-marked invariants, decision 97): every query
//! runs on the connection the caller gives (a read transaction for `nage check`, the test's own
//! transaction in a test), reports "not initialized" when the book has no row, and otherwise
//! lists every violation of INV-1, 2, 4, 8 to 13 and 26 and of SQLite's foreign-key check, each
//! with the invariant's id. Nothing here writes.

use std::fmt;

/// The Check-marked invariants this slice's check knows (SPEC 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Invariant {
    Inv1,
    Inv2,
    Inv4,
    Inv8,
    Inv9,
    Inv10,
    Inv11,
    Inv12,
    Inv13,
    Inv26,
}

impl Invariant {
    /// The ten invariants, in SPEC 9's order.
    pub const ALL: [Invariant; 10] = [
        Invariant::Inv1,
        Invariant::Inv2,
        Invariant::Inv4,
        Invariant::Inv8,
        Invariant::Inv9,
        Invariant::Inv10,
        Invariant::Inv11,
        Invariant::Inv12,
        Invariant::Inv13,
        Invariant::Inv26,
    ];

    /// The id as SPEC 9 writes it: `INV-1`, ..., `INV-26`.
    pub const fn id(self) -> &'static str {
        match self {
            Invariant::Inv1 => "INV-1",
            Invariant::Inv2 => "INV-2",
            Invariant::Inv4 => "INV-4",
            Invariant::Inv8 => "INV-8",
            Invariant::Inv9 => "INV-9",
            Invariant::Inv10 => "INV-10",
            Invariant::Inv11 => "INV-11",
            Invariant::Inv12 => "INV-12",
            Invariant::Inv13 => "INV-13",
            Invariant::Inv26 => "INV-26",
        }
    }
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// One violation: the invariant it breaks (`None` for a row of SQLite's foreign-key check,
/// labelled `FK`) and what breaks it, naming the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub invariant: Option<Invariant>,
    pub detail: String,
}

impl Violation {
    /// `INV-n`, or `FK` for a foreign-key row.
    pub fn label(&self) -> &'static str {
        self.invariant.map_or("FK", Invariant::id)
    }
}

impl fmt::Display for Violation {
    /// `<label> <detail>`, one line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.label(), self.detail)
    }
}

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The book has no row: nothing to check (SPEC 5.2).
    NotInitialized,
    /// Every invariant ran; the violations, in the order found, empty when the books are sound.
    Checked { violations: Vec<Violation> },
}

/// The result of one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    /// How many invariants the check knows (ten).
    pub invariants: usize,
}

impl Report {
    /// The violations found; empty when the database is not initialized or sound.
    pub fn violations(&self) -> &[Violation] {
        match &self.outcome {
            Outcome::NotInitialized => &[],
            Outcome::Checked { violations } => violations,
        }
    }

    /// Whether the run found no violation (a database that is not initialized counts as sound,
    /// SPEC 5.2).
    pub fn is_sound(&self) -> bool {
        self.violations().is_empty()
    }

    /// The summary line of `nage check` (decision 97): `check: not initialized`,
    /// `check: ok (10 invariants)` or `check: N violation(s)`.
    pub fn summary(&self) -> String {
        match &self.outcome {
            Outcome::NotInitialized => String::from("check: not initialized"),
            Outcome::Checked { violations } if violations.is_empty() => {
                format!("check: ok ({} invariants)", self.invariants)
            }
            Outcome::Checked { violations } => {
                format!("check: {} violation(s)", violations.len())
            }
        }
    }
}

/// Runs the check on `conn` (decision 97): `NotInitialized` when `book` has no row, before any
/// invariant runs; otherwise the queries of INV-1, 2, 4, 8 to 13 and 26 in SPEC 9's order, then
/// `PRAGMA foreign_key_check`, each violation reported. A database failure is returned as it
/// is; the caller decides what it means.
pub async fn run(conn: &mut sqlx::SqliteConnection) -> Result<Report, sqlx::Error> {
    let books = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM book"#)
        .fetch_one(&mut *conn)
        .await?;
    let invariants = Invariant::ALL.len();
    if books == 0 {
        return Ok(Report {
            outcome: Outcome::NotInitialized,
            invariants,
        });
    }
    let mut violations = Vec::new();
    inv1(conn, &mut violations).await?;
    inv2(conn, &mut violations).await?;
    inv4(conn, &mut violations).await?;
    let rows = load_rows(conn).await?;
    inv8(conn, &rows, &mut violations).await?;
    inv9(&rows, &mut violations);
    inv10(&rows, &mut violations);
    inv11(&rows, &mut violations);
    inv12(conn, &rows, &mut violations).await?;
    inv13(conn, &rows, &mut violations).await?;
    inv26(conn, &rows, &mut violations).await?;
    foreign_keys(conn, &mut violations).await?;
    Ok(Report {
        outcome: Outcome::Checked { violations },
        invariants,
    })
}

use std::collections::{BTreeMap, BTreeSet};

use sqlx::Row as _;

use crate::accounts::{AccountId, Kind, MAX_DEPTH, SystemKey};
use crate::fold::fold;
use crate::money::Money;
use crate::timestamp::Timestamp;
use crate::users::UserId;

/// The columns of `accounts` the tree checks read.
struct Row {
    id: AccountId,
    name: String,
    name_folded: String,
    parent_id: Option<AccountId>,
    kind: Kind,
    system_key: Option<SystemKey>,
    archived_at: Option<Timestamp>,
}

/// Every account, by id.
async fn load_rows(conn: &mut sqlx::SqliteConnection) -> Result<Vec<Row>, sqlx::Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id AS "id!: AccountId", name, name_folded, parent_id AS "parent_id?: AccountId", kind AS "kind!: Kind", system_key AS "system_key?: SystemKey", archived_at AS "archived_at?: Timestamp" FROM accounts ORDER BY id"#
    )
    .fetch_all(conn)
    .await
}

/// `account <id> (<path>)`, the path being the names from the root down joined with a colon, or
/// as far as the parent chain goes within a few steps more than the depth limit (a corrupt
/// cycle stops there); an unknown id is named by its number alone.
fn label(rows: &[Row], id: AccountId) -> String {
    let by_id: BTreeMap<AccountId, &Row> = rows.iter().map(|row| (row.id, row)).collect();
    let mut segments = Vec::new();
    let mut cursor = Some(id);
    while let Some(current) = cursor {
        let Some(row) = by_id.get(&current) else {
            break;
        };
        if segments.len() > MAX_DEPTH.saturating_add(1) {
            break;
        }
        segments.push(row.name.as_str());
        cursor = row.parent_id;
    }
    if segments.is_empty() {
        return format!("account {id}");
    }
    segments.reverse();
    format!("account {id} ({})", segments.join(":"))
}

/// Adds a violation of `invariant`.
fn report(violations: &mut Vec<Violation>, invariant: Invariant, detail: String) {
    violations.push(Violation {
        invariant: Some(invariant),
        detail,
    });
}

/// INV-1: every transaction, live or deleted, whose postings do not sum to zero.
async fn inv1(
    conn: &mut sqlx::SqliteConnection,
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", COALESCE(SUM(p.amount), 0) AS "total!: Money" FROM transactions t LEFT JOIN postings p ON p.transaction_id = t.id GROUP BY t.id HAVING COALESCE(SUM(p.amount), 0) <> 0 ORDER BY t.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in rows {
        report(
            violations,
            Invariant::Inv1,
            format!(
                "transaction {}: its postings sum to {}, not 0",
                row.id,
                row.total.minor()
            ),
        );
    }
    Ok(())
}

/// INV-2: a transaction with fewer than two postings, and a posting with amount zero.
async fn inv2(
    conn: &mut sqlx::SqliteConnection,
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let counts = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", COUNT(p.id) AS "count!: i64" FROM transactions t LEFT JOIN postings p ON p.transaction_id = t.id GROUP BY t.id HAVING COUNT(p.id) < 2 ORDER BY t.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in counts {
        report(
            violations,
            Invariant::Inv2,
            format!(
                "transaction {} has {} posting(s), fewer than 2",
                row.id, row.count
            ),
        );
    }
    let zeros = sqlx::query!(
        r#"SELECT p.id AS "id!: i64", p.transaction_id AS "transaction_id!: i64" FROM postings p WHERE p.amount = 0 ORDER BY p.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in zeros {
        report(
            violations,
            Invariant::Inv2,
            format!(
                "posting {} of transaction {} has amount 0",
                row.id, row.transaction_id
            ),
        );
    }
    Ok(())
}

/// INV-4: the live postings sum to zero.
async fn inv4(
    conn: &mut sqlx::SqliteConnection,
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let total = sqlx::query_scalar!(
        r#"SELECT COALESCE(SUM(p.amount), 0) AS "total!: Money" FROM postings p JOIN transactions t ON t.id = p.transaction_id WHERE t.deleted_at IS NULL"#
    )
    .fetch_one(&mut *conn)
    .await?;
    if total != Money::ZERO {
        report(
            violations,
            Invariant::Inv4,
            format!("the live postings sum to {}, not 0", total.minor()),
        );
    }
    Ok(())
}

/// INV-8: exactly five roots, one per kind, each with its kind's key, and no posting on a root.
async fn inv8(
    conn: &mut sqlx::SqliteConnection,
    rows: &[Row],
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let roots: Vec<&Row> = rows.iter().filter(|row| row.parent_id.is_none()).collect();
    if roots.len() != 5 {
        report(
            violations,
            Invariant::Inv8,
            format!("{} roots, not 5", roots.len()),
        );
    }
    for kind in Kind::ALL {
        if !roots.iter().any(|root| root.kind == kind) {
            report(
                violations,
                Invariant::Inv8,
                format!("no root of kind {kind}"),
            );
        }
    }
    for root in &roots {
        let expected = root.kind.root_key();
        if root.system_key != Some(expected) {
            let actual = root.system_key.map_or_else(
                || String::from("no system key"),
                |key| format!("the key {key}"),
            );
            report(
                violations,
                Invariant::Inv8,
                format!(
                    "root {} ({}) has {actual}, not the key {expected}",
                    root.id, root.name
                ),
            );
        }
    }
    let on_roots = sqlx::query!(
        r#"SELECT p.id AS "id!: i64", p.transaction_id AS "transaction_id!: i64", a.name AS "name!: String" FROM postings p JOIN accounts a ON a.id = p.account_id WHERE a.parent_id IS NULL ORDER BY p.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in on_roots {
        report(
            violations,
            Invariant::Inv8,
            format!(
                "transaction {}: posting {} is on the root {}",
                row.transaction_id, row.id, row.name
            ),
        );
    }
    Ok(())
}

/// The depth of `row`: 1 for a root; `None` when its parent chain reaches no root within a few
/// steps more than the depth limit (a cycle or an orphan).
fn depth_of(by_id: &BTreeMap<AccountId, &Row>, row: &Row) -> Option<usize> {
    let mut depth = 1_usize;
    let mut cursor = row;
    for _ in 0..=MAX_DEPTH.saturating_add(1) {
        match cursor.parent_id {
            None => return Some(depth),
            Some(parent) => {
                cursor = by_id.get(&parent)?;
                depth = depth.saturating_add(1);
            }
        }
    }
    None
}

/// INV-9: a child's kind equals its parent's, every parent chain reaches a root, and depth is at
/// most six.
fn inv9(rows: &[Row], violations: &mut Vec<Violation>) {
    let by_id: BTreeMap<AccountId, &Row> = rows.iter().map(|row| (row.id, row)).collect();
    for row in rows {
        if let Some(parent_id) = row.parent_id {
            match by_id.get(&parent_id) {
                None => report(
                    violations,
                    Invariant::Inv9,
                    format!(
                        "{}: its parent {parent_id} does not exist",
                        label(rows, row.id)
                    ),
                ),
                Some(parent) if parent.kind != row.kind => report(
                    violations,
                    Invariant::Inv9,
                    format!(
                        "{}: kind {} differs from its parent's {}",
                        label(rows, row.id),
                        row.kind,
                        parent.kind
                    ),
                ),
                Some(_) => {}
            }
        }
        match depth_of(&by_id, row) {
            Some(depth) if depth > MAX_DEPTH => report(
                violations,
                Invariant::Inv9,
                format!("{}: depth {depth} exceeds {MAX_DEPTH}", label(rows, row.id)),
            ),
            Some(_) => {}
            None => report(
                violations,
                Invariant::Inv9,
                format!(
                    "{}: no root is reached from its parent chain",
                    label(rows, row.id)
                ),
            ),
        }
    }
}

/// INV-10: no colon in a name (as stored or after NFKC), every stored fold equal to the fold
/// recomputed now, and no two siblings or roots whose recomputed folds collide.
fn inv10(rows: &[Row], violations: &mut Vec<Violation>) {
    for row in rows {
        let folded = fold(&row.name);
        if row.name.contains(':') || folded.contains(':') {
            report(
                violations,
                Invariant::Inv10,
                format!("{} has a colon in its name", label(rows, row.id)),
            );
        }
        if row.name_folded != folded {
            report(
                violations,
                Invariant::Inv10,
                format!(
                    "{}: the stored fold {:?} differs from {:?}",
                    label(rows, row.id),
                    row.name_folded,
                    folded
                ),
            );
        }
    }
    let mut groups: BTreeMap<(Option<AccountId>, String), Vec<&Row>> = BTreeMap::new();
    for row in rows {
        groups
            .entry((row.parent_id, fold(&row.name)))
            .or_default()
            .push(row);
    }
    for ((_, folded), members) in groups {
        if members.len() > 1 {
            let named: Vec<String> = members
                .iter()
                .map(|member| format!("{} ({})", member.id, member.name))
                .collect();
            report(
                violations,
                Invariant::Inv10,
                format!("accounts {} fold alike as {folded:?}", named.join(" and ")),
            );
        }
    }
}

/// INV-11: each of the eight keys held by exactly one account, none archived, the three
/// non-roots under the root of their kind, every root named and kinded as its key says.
fn inv11(rows: &[Row], violations: &mut Vec<Violation>) {
    let by_id: BTreeMap<AccountId, &Row> = rows.iter().map(|row| (row.id, row)).collect();
    for key in SystemKey::ALL {
        let holders: Vec<&Row> = rows
            .iter()
            .filter(|row| row.system_key == Some(key))
            .collect();
        let holder = match holders.as_slice() {
            [] => {
                report(
                    violations,
                    Invariant::Inv11,
                    format!("the system account {key} is missing"),
                );
                continue;
            }
            [holder] => *holder,
            many => {
                report(
                    violations,
                    Invariant::Inv11,
                    format!("the system key {key} is held by {} accounts", many.len()),
                );
                continue;
            }
        };
        if holder.archived_at.is_some() {
            report(
                violations,
                Invariant::Inv11,
                format!("the system account {key} ({}) is archived", holder.name),
            );
        }
        if holder.kind != key.kind() {
            report(
                violations,
                Invariant::Inv11,
                format!(
                    "the system account {key} ({}) has kind {}, not {}",
                    holder.name,
                    holder.kind,
                    key.kind()
                ),
            );
        }
        match key.parent_key() {
            None => {
                if holder.parent_id.is_some() {
                    report(
                        violations,
                        Invariant::Inv11,
                        format!("the root {key} ({}) has a parent", holder.name),
                    );
                }
                if holder.name != key.name() {
                    report(
                        violations,
                        Invariant::Inv11,
                        format!(
                            "the root {key} is named {:?}, not {:?}",
                            holder.name,
                            key.name()
                        ),
                    );
                }
            }
            Some(parent_key) => {
                let under_root = holder
                    .parent_id
                    .and_then(|parent| by_id.get(&parent))
                    .is_some_and(|parent| parent.system_key == Some(parent_key));
                if !under_root {
                    report(
                        violations,
                        Invariant::Inv11,
                        format!(
                            "the system account {key} ({}) is not under its root {parent_key}",
                            holder.name
                        ),
                    );
                }
            }
        }
    }
}

/// INV-12: an archived account's live postings sum to zero, every child of an archived account is
/// archived, and no transaction touching an archived account was created after its archive time.
async fn inv12(
    conn: &mut sqlx::SqliteConnection,
    rows: &[Row],
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let balances = sqlx::query!(
        r#"SELECT a.id AS "id!: AccountId", COALESCE(SUM(p.amount), 0) AS "total!: Money" FROM accounts a JOIN postings p ON p.account_id = a.id JOIN transactions t ON t.id = p.transaction_id WHERE a.archived_at IS NOT NULL AND t.deleted_at IS NULL GROUP BY a.id HAVING COALESCE(SUM(p.amount), 0) <> 0 ORDER BY a.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in balances {
        report(
            violations,
            Invariant::Inv12,
            format!(
                "{} is archived with a live balance of {}",
                label(rows, row.id),
                row.total.minor()
            ),
        );
    }
    let children = sqlx::query!(
        r#"SELECT c.id AS "id!: AccountId" FROM accounts c JOIN accounts a ON a.id = c.parent_id WHERE a.archived_at IS NOT NULL AND c.archived_at IS NULL ORDER BY c.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in children {
        report(
            violations,
            Invariant::Inv12,
            format!("{} is live under an archived parent", label(rows, row.id)),
        );
    }
    let touched = sqlx::query!(
        r#"SELECT DISTINCT t.id AS "id!: i64", a.id AS "account_id!: AccountId", t.created_at AS "created_at!: Timestamp", a.archived_at AS "archived_at!: Timestamp" FROM transactions t JOIN postings p ON p.transaction_id = t.id JOIN accounts a ON a.id = p.account_id WHERE a.archived_at IS NOT NULL AND t.created_at > a.archived_at ORDER BY t.id, a.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in touched {
        report(
            violations,
            Invariant::Inv12,
            format!(
                "transaction {} touches {} and was created at {}, after its archive at {}",
                row.id,
                label(rows, row.account_id),
                row.created_at,
                row.archived_at
            ),
        );
    }
    Ok(())
}

/// INV-13: every live opening-balance transaction has exactly two postings, one on
/// `Equity:Opening Balances` and one on a non-root Assets or Liabilities account, and no account
/// has two.
async fn inv13(
    conn: &mut sqlx::SqliteConnection,
    rows: &[Row],
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let legs = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", p.account_id AS "account_id!: AccountId" FROM transactions t JOIN postings p ON p.transaction_id = t.id WHERE t.origin = 'opening_balance' AND t.deleted_at IS NULL ORDER BY t.id, p.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut by_transaction: BTreeMap<i64, Vec<AccountId>> = BTreeMap::new();
    for leg in legs {
        by_transaction
            .entry(leg.id)
            .or_default()
            .push(leg.account_id);
    }
    let by_id: BTreeMap<AccountId, &Row> = rows.iter().map(|row| (row.id, row)).collect();
    let opening = rows
        .iter()
        .find(|row| row.system_key == Some(SystemKey::OpeningBalances))
        .map(|row| row.id);
    let mut seen: BTreeSet<AccountId> = BTreeSet::new();
    for (transaction, accounts) in by_transaction {
        if accounts.len() != 2 {
            report(
                violations,
                Invariant::Inv13,
                format!(
                    "transaction {transaction}: an opening balance has {} posting(s), not 2",
                    accounts.len()
                ),
            );
            continue;
        }
        let Some(opening) = opening else {
            continue;
        };
        if !accounts.contains(&opening) {
            report(
                violations,
                Invariant::Inv13,
                format!("transaction {transaction}: no posting on Equity:Opening Balances"),
            );
            continue;
        }
        let Some(other) = accounts.iter().copied().find(|account| *account != opening) else {
            report(
                violations,
                Invariant::Inv13,
                format!("transaction {transaction}: both postings are on Equity:Opening Balances"),
            );
            continue;
        };
        match by_id.get(&other) {
            Some(account)
                if account.parent_id.is_some()
                    && matches!(account.kind, Kind::Asset | Kind::Liability) => {}
            Some(_) => report(
                violations,
                Invariant::Inv13,
                format!(
                    "transaction {transaction}: the other posting is on {}, not a non-root Assets or Liabilities account",
                    label(rows, other)
                ),
            ),
            None => report(
                violations,
                Invariant::Inv13,
                format!(
                    "transaction {transaction}: the other posting is on the unknown account {other}"
                ),
            ),
        }
        if !seen.insert(other) {
            report(
                violations,
                Invariant::Inv13,
                format!(
                    "transaction {transaction}: a second opening balance for {}",
                    label(rows, other)
                ),
            );
        }
    }
    Ok(())
}

/// INV-26: every user but the first admin (the lowest id) names its creator, and every account and
/// transaction names users that exist as its creator and editor.
async fn inv26(
    conn: &mut sqlx::SqliteConnection,
    rows: &[Row],
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let orphans = sqlx::query!(
        r#"SELECT id AS "id!: UserId", username FROM users WHERE created_by IS NULL AND id <> (SELECT MIN(id) FROM users) ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in orphans {
        report(
            violations,
            Invariant::Inv26,
            format!(
                "user {} ({}) has no creator and is not the first admin",
                row.id, row.username
            ),
        );
    }
    let accounts = sqlx::query!(
        r#"SELECT a.id AS "id!: AccountId", a.created_by AS "created_by!: UserId", a.updated_by AS "updated_by!: UserId" FROM accounts a WHERE a.created_by NOT IN (SELECT id FROM users) OR a.updated_by NOT IN (SELECT id FROM users) ORDER BY a.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in accounts {
        report(
            violations,
            Invariant::Inv26,
            format!(
                "{}: created_by {} or updated_by {} names no user",
                label(rows, row.id),
                row.created_by,
                row.updated_by
            ),
        );
    }
    let transactions = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.created_by AS "created_by!: UserId", t.updated_by AS "updated_by!: UserId" FROM transactions t WHERE t.created_by NOT IN (SELECT id FROM users) OR t.updated_by NOT IN (SELECT id FROM users) ORDER BY t.id"#
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in transactions {
        report(
            violations,
            Invariant::Inv26,
            format!(
                "transaction {}: created_by {} or updated_by {} names no user",
                row.id, row.created_by, row.updated_by
            ),
        );
    }
    Ok(())
}

/// SQLite's foreign-key check (decision 97): each dangling reference as an `FK` line, sorted by
/// table, row and parent (SQLite lists the tables in its own order).
async fn foreign_keys(
    conn: &mut sqlx::SqliteConnection,
    violations: &mut Vec<Violation>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *conn)
        .await?;
    let mut dangling = Vec::with_capacity(rows.len());
    for row in rows {
        let table: String = row.try_get("table")?;
        let rowid: Option<i64> = row.try_get("rowid")?;
        let parent: String = row.try_get("parent")?;
        dangling.push((table, rowid, parent));
    }
    // SQLite lists the tables in its schema hash's order; sorted, the report is stable.
    dangling.sort();
    for (table, rowid, parent) in dangling {
        let rowid = rowid.map_or_else(|| String::from("?"), |id| id.to_string());
        violations.push(Violation {
            invariant: None,
            detail: format!("{table} row {rowid} references a missing {parent} row"),
        });
    }
    Ok(())
}
