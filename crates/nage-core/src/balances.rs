//! Balances (SPEC 6.2's roll-up, 6.3's signs, INV-25, decision 96): every account's own live
//! postings dated on or before a date and the roll-up over its descendants, and the five kinds'
//! totals in stored and natural sign. A balance "as of" a day is the balance at the end of that
//! day. One aggregate query, then the roll-up in Rust over the loaded tree; an overflow is
//! refused, never wrapped.

use std::fmt;

use crate::accounts::{Account, AccountError, AccountId, Kind};
use crate::date::Date;
use crate::money::Money;

/// One account's balance as of a date: its own postings and the roll-up over its descendants,
/// both in stored sign (SPEC 6.3); [`natural`] converts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountBalance {
    pub account: Account,
    /// The full name of SPEC 6.2, such as `Expenses:Food:Groceries`.
    pub full_name: String,
    /// The account's own live postings dated on or before the date.
    pub own: Money,
    /// `own` plus every descendant's `own` (SPEC 6.2: reports roll children up into parents).
    pub total: Money,
}

/// One kind's total as of a date, in stored and natural sign (SPEC 6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindBalance {
    pub kind: Kind,
    pub stored: Money,
    pub natural: Money,
}

/// Every balance as of a date: one entry per account (archived ones included), by id, and the
/// five kinds in SPEC 6.2's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balances {
    pub as_of: Date,
    pub accounts: Vec<AccountBalance>,
    pub kinds: Vec<KindBalance>,
}

/// The balances as of `as_of` (decision 96): the live postings (INV-25: deleted transactions
/// excluded) dated on or before the date, summed per account, rolled up from children to
/// parents, and totalled per kind; `Overflow` when a sum or a natural sign does not fit.
pub async fn balances_as_of(
    conn: &mut sqlx::SqliteConnection,
    as_of: Date,
) -> Result<Balances, BalanceError> {
    let all = accounts::accounts(&mut *conn).await?;
    let sums = sqlx::query!(
        r#"SELECT p.account_id AS "account_id!: AccountId", COALESCE(SUM(p.amount), 0) AS "total!: Money" FROM postings p JOIN transactions t ON t.id = p.transaction_id WHERE t.deleted_at IS NULL AND t.date <= ?1 GROUP BY p.account_id"#,
        as_of
    )
    .fetch_all(&mut *conn)
    .await?;
    let own: BTreeMap<AccountId, Money> = sums
        .into_iter()
        .map(|row| (row.account_id, row.total))
        .collect();
    let mut children: BTreeMap<AccountId, Vec<AccountId>> = BTreeMap::new();
    let mut names: BTreeMap<AccountId, &str> = BTreeMap::new();
    let mut parents: BTreeMap<AccountId, Option<AccountId>> = BTreeMap::new();
    for account in &all {
        names.insert(account.id, account.name.as_str());
        parents.insert(account.id, account.parent_id);
        if let Some(parent) = account.parent_id {
            children.entry(parent).or_default().push(account.id);
        }
    }
    let mut totals: BTreeMap<AccountId, Money> = BTreeMap::new();
    let mut balances = Vec::with_capacity(all.len());
    for account in &all {
        let own_amount = own.get(&account.id).copied().unwrap_or(Money::ZERO);
        let total = roll_up(account.id, &own, &children, &mut totals)?;
        balances.push(AccountBalance {
            account: account.clone(),
            full_name: path(account.id, &names, &parents),
            own: own_amount,
            total,
        });
    }
    let mut kinds = Vec::with_capacity(Kind::ALL.len());
    for kind in Kind::ALL {
        let stored = Money::checked_sum(
            all.iter()
                .filter(|account| account.kind == kind)
                .map(|account| own.get(&account.id).copied().unwrap_or(Money::ZERO)),
        )
        .ok_or(BalanceError::Overflow)?;
        let natural_amount = natural(stored, kind).ok_or(BalanceError::Overflow)?;
        kinds.push(KindBalance {
            kind,
            stored,
            natural: natural_amount,
        });
    }
    Ok(Balances {
        as_of,
        accounts: balances,
        kinds,
    })
}

/// The account's own live postings over all dates (SPEC 6.2's archive rule, INV-12).
pub async fn own_balance(
    conn: &mut sqlx::SqliteConnection,
    account: AccountId,
) -> Result<Money, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE(SUM(p.amount), 0) AS "total!: Money" FROM postings p JOIN transactions t ON t.id = p.transaction_id WHERE t.deleted_at IS NULL AND p.account_id = ?1"#,
        account
    )
    .fetch_one(conn)
    .await
}

/// The natural sign of SPEC 6.3: `stored` for Assets and Expenses, `-stored` for Liabilities,
/// Income and Equity; `None` when the negation does not fit (the most negative amount).
pub fn natural(stored: Money, kind: Kind) -> Option<Money> {
    if kind.sign() < 0 {
        stored.checked_neg()
    } else {
        Some(stored)
    }
}

/// Why balances could not be computed.
#[derive(Debug)]
pub enum BalanceError {
    /// A sum, or the natural sign of a total, does not fit in 64 bits.
    Overflow,
    /// The tree could not be read.
    Accounts(AccountError),
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for BalanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => f.write_str("a balance is too large"),
            Self::Accounts(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for BalanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Accounts(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Overflow => None,
        }
    }
}

impl From<AccountError> for BalanceError {
    fn from(error: AccountError) -> Self {
        Self::Accounts(error)
    }
}

impl From<sqlx::Error> for BalanceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use std::collections::BTreeMap;

use crate::accounts;

/// `id`'s own amount plus every descendant's, memoized in `totals`; `Overflow` when a sum does
/// not fit. The recursion is as deep as the tree, at most six under INV-9; a corrupt cycle,
/// which `nage check` reports under INV-9, is not guarded here and overflows the stack.
fn roll_up(
    id: AccountId,
    own: &BTreeMap<AccountId, Money>,
    children: &BTreeMap<AccountId, Vec<AccountId>>,
    totals: &mut BTreeMap<AccountId, Money>,
) -> Result<Money, BalanceError> {
    if let Some(total) = totals.get(&id) {
        return Ok(*total);
    }
    let mut total = own.get(&id).copied().unwrap_or(Money::ZERO);
    for child in children.get(&id).into_iter().flatten() {
        let below = roll_up(*child, own, children, totals)?;
        total = total.checked_add(below).ok_or(BalanceError::Overflow)?;
    }
    totals.insert(id, total);
    Ok(total)
}

/// The full name of `id` from the loaded names and parents: the names from the root down joined
/// with a colon, at most [`accounts::MAX_DEPTH`] plus one steps up (a corrupt cycle stops there).
fn path(
    id: AccountId,
    names: &BTreeMap<AccountId, &str>,
    parents: &BTreeMap<AccountId, Option<AccountId>>,
) -> String {
    let mut segments = Vec::new();
    let mut cursor = Some(id);
    while let Some(current) = cursor {
        if segments.len() > accounts::MAX_DEPTH {
            break;
        }
        segments.push(names.get(&current).copied().unwrap_or("?"));
        cursor = parents.get(&current).copied().flatten();
    }
    segments.reverse();
    segments.join(":")
}
