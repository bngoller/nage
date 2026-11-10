//! `nage check`'s core (SPEC 5.2, W15, 10.3's check row, decision 97): "not initialized" on a
//! fresh database, nothing on a sound chart, and one corruption per Check-marked invariant and
//! per query, planted with unchecked SQL, either through the storage's write transaction (the
//! transactions and postings the schema accepts, and one uncommitted rename) or through one
//! plain SQLite connection of the test's own (foreign keys on, except for a dangling reference;
//! check constraints ignored or a trigger dropped where the schema would refuse the corruption;
//! decision 98), and the foreign-key line.

mod common;

use nage_core::check::{self, Invariant, Outcome, Violation};
use nage_core::test_support::open_migrated;

use common::{
    LATER_TEXT, NOW_TEXT, account_named, begin, check_report, commit, corrupt, insert_transaction,
    labels, reader, seeded,
};

/// Asserts that `report` holds exactly the labels `expected`, in order, and that each detail
/// holds its `needle`.
#[track_caller]
fn assert_violations(report: &check::Report, expected: &[(&str, &str)]) {
    let found: Vec<(&str, &str)> = report
        .violations()
        .iter()
        .map(|violation| (violation.label(), violation.detail.as_str()))
        .collect();
    assert_eq!(
        labels(report),
        expected.iter().map(|(label, _)| *label).collect::<Vec<_>>(),
        "the labels of {found:?}"
    );
    for ((label, needle), violation) in expected.iter().zip(report.violations()) {
        assert!(
            violation.detail.contains(needle),
            "{label}'s detail does not hold {needle:?}: {}",
            violation.detail
        );
    }
}

/// A fresh, migrated database has no book row: the report is `NotInitialized`, knows ten
/// invariants, holds no violation, counts as sound and summarizes as `check: not initialized`.
///
/// Proves: SPEC 5.2 ("not initialized" on a database with no book row), decision 97.
#[tokio::test]
async fn a_fresh_database_is_not_initialized() {
    let (_dir, storage) = open_migrated().await;
    let report = check_report(&storage).await;
    assert_eq!(report.outcome, Outcome::NotInitialized);
    assert_eq!(report.invariants, 10);
    assert!(report.violations().is_empty());
    assert!(report.is_sound());
    assert_eq!(report.summary(), "check: not initialized");
    storage.close().await;
}

/// The standard setup, with and without the starter chart, passes every invariant: the outcome
/// is `Checked` with no violation and the summary `check: ok (10 invariants)`; the run also
/// works inside a write transaction, where it sees that transaction's own rows.
///
/// Proves: W15 (check over a sound book), INV-8, INV-11 (the system accounts as setup writes
/// them), decision 97.
#[tokio::test]
async fn a_sound_chart_reports_nothing() {
    for starter_chart in [true, false] {
        let (_dir, storage, _initialized) = seeded(starter_chart).await;
        let report = check_report(&storage).await;
        assert_eq!(
            report.outcome,
            Outcome::Checked {
                violations: Vec::new()
            },
            "starter chart {starter_chart}"
        );
        assert!(report.is_sound());
        assert_eq!(report.summary(), "check: ok (10 invariants)");
        let mut tx = begin(&storage, "an uncommitted corruption").await;
        sqlx::query("UPDATE accounts SET name = 'Food:Fruit', name_folded = 'food:fruit' WHERE system_key = 'uncategorized'")
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|error| panic!("cannot rename inside the transaction: {error}"));
        let inside = check::run(&mut tx)
            .await
            .unwrap_or_else(|error| panic!("check inside a write transaction failed: {error}"));
        assert_eq!(
            labels(&inside),
            ["INV-10"],
            "the run sees the transaction's own rows"
        );
        common::roll_back(tx, "an uncommitted corruption").await;
        storage.close().await;
    }
}

/// INV-1: a transaction whose postings do not sum to zero is reported with its id, live or
/// deleted; a transaction whose postings are all removed sums to zero and is INV-2's alone
/// (fewer than two postings); the sum of a live unbalanced transaction also breaks INV-4, a
/// deleted one does not.
///
/// Proves: INV-1 (Check half), INV-2 (Check half: a transaction without postings), INV-4
/// (Check half), decision 97.
#[tokio::test]
async fn inv1_and_inv4_find_unbalanced_transactions() {
    let (dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "unbalanced").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let live = insert_transaction(
        &mut tx,
        "2026-01-01",
        "manual",
        false,
        NOW_TEXT,
        &[(checking, 100), (salary, -50)],
    )
    .await;
    let deleted = insert_transaction(
        &mut tx,
        "2026-01-02",
        "manual",
        true,
        NOW_TEXT,
        &[(checking, 7), (salary, -70)],
    )
    .await;
    let balanced = insert_transaction(
        &mut tx,
        "2026-01-03",
        "manual",
        false,
        NOW_TEXT,
        &[(checking, 10), (salary, -10)],
    )
    .await;
    commit(tx, "unbalanced").await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-1", &format!("transaction {live}")),
            ("INV-1", &format!("transaction {deleted}")),
            ("INV-4", "50"),
        ],
    );
    assert!(
        !report.violations().iter().any(|violation| violation
            .detail
            .contains(&format!("transaction {balanced}"))),
        "the balanced transaction is reported"
    );
    assert_eq!(report.summary(), "check: 3 violation(s)");
    corrupt(
        &dir.db_path(),
        false,
        &[&format!(
            "DELETE FROM postings WHERE transaction_id = {deleted}"
        )],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-1", &format!("transaction {live}")),
            ("INV-2", &format!("transaction {deleted}")),
            ("INV-4", "50"),
        ],
    );
    storage.close().await;
}

/// INV-2: a transaction with one posting and a posting with amount zero (planted with the
/// check constraints ignored) are reported.
///
/// Proves: INV-2 (Check half), decision 97.
#[tokio::test]
async fn inv2_finds_too_few_postings_and_zero_amounts() {
    let (dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "shape").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let lone = insert_transaction(
        &mut tx,
        "2026-01-01",
        "manual",
        false,
        NOW_TEXT,
        &[(checking, 5)],
    )
    .await;
    commit(tx, "shape").await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            "PRAGMA ignore_check_constraints = ON",
            &format!("UPDATE postings SET amount = 0 WHERE transaction_id = {lone}"),
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-2", &format!("transaction {lone} has 1 posting")),
            ("INV-2", "amount 0"),
        ],
    );
    storage.close().await;
}

/// INV-8: a deleted root (four left; also a missing key for INV-11), a sixth root, a root
/// without its kind's key (also INV-11), and a posting on a root are each reported.
///
/// Proves: INV-8 (Check half), INV-11 (Check half: a missing key), decision 97.
#[tokio::test]
async fn inv8_finds_wrong_roots_and_postings_on_roots() {
    let (dir, storage, _initialized) = seeded(false).await;
    corrupt(
        &dir.db_path(),
        false,
        &["DELETE FROM accounts WHERE system_key = 'income'"],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-8", "4 roots"),
            ("INV-8", "no root of kind income"),
            ("INV-11", "income"),
        ],
    );
    storage.close().await;

    let (dir, storage, _initialized) = seeded(false).await;
    corrupt(
        &dir.db_path(),
        false,
        &[&format!(
            "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('Stray', 'stray', NULL, 'asset', NULL, NULL, 0, NULL, 1, '{NOW_TEXT}', 1, '{NOW_TEXT}')"
        )],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-8", "6 roots"), ("INV-8", "Stray")]);
    storage.close().await;

    let (dir, storage, _initialized) = seeded(false).await;
    corrupt(
        &dir.db_path(),
        false,
        &["UPDATE accounts SET system_key = NULL WHERE system_key = 'assets'"],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-8", "Assets"), ("INV-11", "assets")]);
    storage.close().await;

    let (_dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a posting on a root").await;
    let assets = account_named(&mut tx, "Assets").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let on_root = insert_transaction(
        &mut tx,
        "2026-01-01",
        "manual",
        false,
        NOW_TEXT,
        &[(assets, 100), (salary, -100)],
    )
    .await;
    commit(tx, "a posting on a root").await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-8", &format!("transaction {on_root}"))]);
    storage.close().await;
}

/// INV-9: a child whose kind differs from its parent's (planted after dropping the kind
/// triggers), a chain seven segments deep, named by its full path from the root down, and a
/// cycle of two accounts that never reaches a root are each reported.
///
/// Proves: INV-9 (Check half), decision 97 (an account is named with its full name).
#[tokio::test]
async fn inv9_finds_kind_mismatches_depth_and_cycles() {
    let (dir, storage, _initialized) = seeded(true).await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            "DROP TRIGGER accounts_kind_matches_parent_on_insert",
            "DROP TRIGGER accounts_kind_matches_parent_on_move",
            "DROP TRIGGER accounts_kind_never_changes",
            "UPDATE accounts SET kind = 'income' WHERE name = 'Groceries'",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-9", "Groceries")]);
    storage.close().await;

    let (dir, storage, _initialized) = seeded(true).await;
    let mut statements = Vec::new();
    let mut parent = String::from("(SELECT id FROM accounts WHERE name = 'Groceries')");
    for name in ["Three", "Four", "Five", "Six", "Seven"] {
        statements.push(format!(
            "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('{name}', '{}', {parent}, 'expense', NULL, NULL, 0, NULL, 1, '{NOW_TEXT}', 1, '{NOW_TEXT}')",
            name.to_lowercase()
        ));
        parent = format!("(SELECT id FROM accounts WHERE name = '{name}')");
    }
    let statements: Vec<&str> = statements.iter().map(String::as_str).collect();
    corrupt(&dir.db_path(), false, &statements).await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-9", "Seven")]);
    assert!(
        report.violations()[0].detail.contains("depth 7"),
        "the detail does not name the depth: {}",
        report.violations()[0].detail
    );
    assert!(
        report.violations()[0]
            .detail
            .contains("(Expenses:Groceries:Three:Four:Five:Six:Seven)"),
        "the detail does not name the full path from the root down: {}",
        report.violations()[0].detail
    );
    storage.close().await;

    let (dir, storage, _initialized) = seeded(true).await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            "UPDATE accounts SET parent_id = (SELECT id FROM accounts WHERE name = 'Dining') WHERE name = 'Groceries'",
            "UPDATE accounts SET parent_id = (SELECT id FROM accounts WHERE name = 'Groceries') WHERE name = 'Dining'",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(&report, &[("INV-9", "Groceries"), ("INV-9", "Dining")]);
    assert!(
        report
            .violations()
            .iter()
            .all(|violation| violation.detail.contains("no root")),
        "a cycle's detail does not say that no root is reached: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// INV-10: a name with a colon, a name whose colon shows only after NFKC (a fullwidth colon,
/// its stored fold the fold recomputed now, so that the colon line alone reports it), a stored
/// fold that differs from the fold recomputed now, and two siblings whose recomputed folds
/// collide (their stored folds planted apart) are each reported; two roots whose recomputed
/// folds collide are reported by a collision line of their own, beside the stale fold that the
/// roots' unique index forces on one of them.
///
/// Proves: INV-10 (Check half), decision 95 (the fold recomputed), decision 97.
#[tokio::test]
async fn inv10_finds_colons_stale_folds_and_colliding_names() {
    let (dir, storage, _initialized) = seeded(true).await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            "UPDATE accounts SET name = 'Food:Fruit', name_folded = 'food:fruit' WHERE name = 'Groceries'",
            "UPDATE accounts SET name_folded = 'stale' WHERE name = 'Dining'",
            "UPDATE accounts SET name = 'HOUSING', name_folded = 'housing-2' WHERE name = 'Utilities'",
            "UPDATE accounts SET name = 'Fees\u{FF1A}Bank', name_folded = 'fees:bank' \
             WHERE name = 'Fees'",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-10", "Food:Fruit"),
            ("INV-10", "Dining"),
            ("INV-10", "HOUSING"),
            ("INV-10", "Fees\u{FF1A}Bank"),
            ("INV-10", "Housing"),
        ],
    );
    assert!(
        report.violations()[1].detail.contains("stale"),
        "the stale fold is not named: {}",
        report.violations()[1].detail
    );
    assert!(
        report.violations()[3]
            .detail
            .contains("has a colon in its name"),
        "the fullwidth colon is not reported as a colon: {}",
        report.violations()[3].detail
    );
    storage.close().await;

    let (dir, storage, _initialized) = seeded(false).await;
    corrupt(&dir.db_path(), false, &["UPDATE accounts SET name = 'INCOME', name_folded = 'income-2' WHERE system_key = 'expenses'"]).await;
    let report = check_report(&storage).await;
    assert!(
        labels(&report).contains(&"INV-10"),
        "two roots folding alike are not reported: {:?}",
        report.violations()
    );
    assert!(
        report
            .violations()
            .iter()
            .any(|violation| violation.label() == "INV-10"
                && violation.detail.contains("fold alike")
                && violation.detail.contains("INCOME")),
        "the two roots folding alike have no collision line: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// INV-11: an archived system account, a system account under a parent other than its root,
/// a renamed root, and a deleted system account are each reported.
///
/// Proves: INV-11 (Check half), decision 97.
#[tokio::test]
async fn inv11_finds_archived_moved_renamed_and_missing_system_accounts() {
    let (dir, storage, _initialized) = seeded(true).await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            &format!("UPDATE accounts SET archived_at = '{LATER_TEXT}' WHERE system_key = 'valuation_changes'"),
            "UPDATE accounts SET parent_id = (SELECT id FROM accounts WHERE name = 'Groceries') WHERE system_key = 'uncategorized'",
            "UPDATE accounts SET name = 'Spending', name_folded = 'spending' WHERE system_key = 'expenses'",
            "DELETE FROM accounts WHERE system_key = 'opening_balances'",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-11", "expenses"),
            ("INV-11", "opening_balances"),
            ("INV-11", "valuation_changes"),
            ("INV-11", "uncategorized"),
        ],
    );
    assert!(
        report.violations()[0].detail.contains("Spending"),
        "the renamed root's name is not shown: {}",
        report.violations()[0].detail
    );
    storage.close().await;
}

/// INV-12: an archived account whose live postings do not sum to zero, a live child of an
/// archived parent, and a transaction created later than the archive instant that touches the
/// archived account are each reported, a deleted such transaction too, whose posting stays out
/// of the archived account's live balance; a transaction created in the same second as the
/// archive is not.
///
/// Proves: INV-12 (Check half: the balance over live postings, the later touch live or
/// deleted), decision 97 (the same second passes).
#[tokio::test]
async fn inv12_finds_broken_archives() {
    let (dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "archives").await;
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    let fees = account_named(&mut tx, "Expenses:Fees").await;
    let travel = account_named(&mut tx, "Expenses:Travel").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let unbalanced_archive = insert_transaction(
        &mut tx,
        "2026-01-01",
        "manual",
        false,
        NOW_TEXT,
        &[(gifts, 2_500), (checking, -2_500)],
    )
    .await;
    let same_second = insert_transaction(
        &mut tx,
        "2026-01-02",
        "manual",
        false,
        NOW_TEXT,
        &[(fees, 100), (checking, -100)],
    )
    .await;
    let later_touch = insert_transaction(
        &mut tx,
        "2026-01-03",
        "manual",
        false,
        LATER_TEXT,
        &[(fees, -100), (checking, 100)],
    )
    .await;
    let deleted_touch = insert_transaction(
        &mut tx,
        "2026-01-04",
        "manual",
        true,
        LATER_TEXT,
        &[(travel, 300), (checking, -300)],
    )
    .await;
    commit(tx, "archives").await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            &format!("UPDATE accounts SET archived_at = '{NOW_TEXT}' WHERE id IN ({}, {}, {})", gifts.0, fees.0, travel.0),
            "UPDATE accounts SET parent_id = (SELECT id FROM accounts WHERE name = 'Travel') WHERE name = 'Dining'",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-12", "Gifts"),
            ("INV-12", "Dining"),
            ("INV-12", &format!("transaction {later_touch}")),
            ("INV-12", &format!("transaction {deleted_touch} touches")),
        ],
    );
    assert!(
        report.violations()[3].detail.contains("Travel"),
        "the deleted transaction's line does not name the archived account: {}",
        report.violations()[3].detail
    );
    assert!(
        !report
            .violations()
            .iter()
            .any(|violation| violation.detail.contains("Travel")
                && violation.detail.contains("live balance")),
        "a deleted transaction's posting counts in the archived account's balance: {:?}",
        report.violations()
    );
    assert!(
        !report.violations().iter().any(|violation| violation
            .detail
            .contains(&format!("transaction {same_second}"))),
        "a transaction created in the same second as the archive is reported"
    );
    assert!(
        !report.violations().iter().any(|violation| violation
            .detail
            .contains(&format!("transaction {unbalanced_archive}"))),
        "the balance violation names the transaction instead of the account"
    );
    storage.close().await;
}

/// INV-13: a live opening-balance transaction with three postings, one whose two postings miss
/// `Equity:Opening Balances`, one on an expense account, and a second one for the same account
/// are each reported; a correct one on an Assets account, a correct one on a Liabilities
/// account and a deleted broken one are not.
///
/// Proves: INV-13 (Check half: live opening balances only, on Assets or Liabilities accounts),
/// decision 97.
#[tokio::test]
async fn inv13_finds_broken_opening_balances() {
    let (_dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "opening balances").await;
    let opening = account_named(&mut tx, "Equity:Opening Balances").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let correct = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(checking, 125_000), (opening, -125_000)],
    )
    .await;
    let card_opening = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(card, -80_000), (opening, 80_000)],
    )
    .await;
    let three = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(savings, 100), (cash, 100), (opening, -200)],
    )
    .await;
    let no_equity = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(card, -500), (salary, 500)],
    )
    .await;
    let on_expense = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(groceries, 900), (opening, -900)],
    )
    .await;
    let second = insert_transaction(
        &mut tx,
        "2025-12-30",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(checking, 1), (opening, -1)],
    )
    .await;
    let deleted = insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        true,
        NOW_TEXT,
        &[(cash, 5), (salary, -5)],
    )
    .await;
    commit(tx, "opening balances").await;
    let report = check_report(&storage).await;
    let details: Vec<&str> = report
        .violations()
        .iter()
        .map(|violation| violation.detail.as_str())
        .collect();
    assert!(
        labels(&report).iter().all(|label| *label == "INV-13"),
        "other labels: {:?}",
        labels(&report)
    );
    assert_eq!(report.violations().len(), 4, "{details:?}");
    for (id, needle) in [
        (three, "3 posting(s)"),
        (no_equity, "Opening Balances"),
        (on_expense, "Groceries"),
        (second, "second opening balance"),
    ] {
        assert!(
            details
                .iter()
                .any(|detail| detail.contains(&format!("transaction {id}"))
                    && detail.contains(needle)),
            "transaction {id} with {needle:?} is not among {details:?}"
        );
    }
    for id in [correct, card_opening, deleted] {
        assert!(
            !details
                .iter()
                .any(|detail| detail.contains(&format!("transaction {id}"))),
            "transaction {id} is reported: {details:?}"
        );
    }
    storage.close().await;
}

/// INV-26: a second user without a creator is reported although it is an admin like the first
/// (the first admin, the lowest id, is not), and so are an account and a transaction whose
/// stamps name no user (planted with foreign keys off), which the foreign-key line reports too.
///
/// Proves: INV-26 (Check half), decision 97 (the first admin is the lowest id; the `FK` line).
#[tokio::test]
async fn inv26_finds_creator_less_users_and_dangling_stamps() {
    let (dir, storage, _initialized) = seeded(false).await;
    corrupt(
        &dir.db_path(),
        true,
        &[
            &format!(
                "INSERT INTO users (username, username_folded, display_name, password_hash, \
                 is_admin, created_by, created_at, updated_by, updated_at, disabled_by, \
                 disabled_at) VALUES ('orphan', 'orphan', 'Orphan', 'hash', 1, NULL, \
                 '{LATER_TEXT}', NULL, NULL, NULL, NULL)"
            ),
            "UPDATE accounts SET updated_by = 99 WHERE system_key = 'uncategorized'",
            &format!("INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES ('2026-01-01', '', '', 'manual', 1, 98, '{NOW_TEXT}', 98, '{NOW_TEXT}', NULL, NULL)"),
            "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE system_key = 'uncategorized'), 1)",
            "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE system_key = 'opening_balances'), -1)",
        ],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[
            ("INV-26", "user 2"),
            ("INV-26", "Uncategorized"),
            ("INV-26", "transaction 1"),
            ("FK", "accounts row"),
            ("FK", "transactions row 1"),
            ("FK", "transactions row 1"),
        ],
    );
    assert!(
        report.violations()[0].detail.contains("no creator"),
        "{}",
        report.violations()[0].detail
    );
    storage.close().await;
}

/// The foreign-key line: a posting on an account that does not exist (planted with foreign keys
/// off) is reported as `FK postings row <id> references a missing accounts row`.
///
/// Proves: decision 97 (the foreign-key line).
#[tokio::test]
async fn the_foreign_key_line_names_the_dangling_row() {
    let (dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a dangling posting").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    insert_transaction(
        &mut tx,
        "2026-01-01",
        "manual",
        false,
        NOW_TEXT,
        &[(checking, 10), (salary, -10)],
    )
    .await;
    commit(tx, "a dangling posting").await;
    corrupt(
        &dir.db_path(),
        true,
        &["UPDATE postings SET account_id = 999 WHERE amount = 10"],
    )
    .await;
    let report = check_report(&storage).await;
    assert_violations(
        &report,
        &[("FK", "postings row 1 references a missing accounts row")],
    );
    assert_eq!(report.violations()[0].invariant, None);
    assert_eq!(
        report.violations()[0].to_string(),
        "FK postings row 1 references a missing accounts row"
    );
    assert_eq!(report.summary(), "check: 1 violation(s)");
    let mut conn = reader(&storage).await;
    let direct = check::run(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("check on a pool connection failed: {error}"));
    assert_eq!(
        direct, report,
        "the same report on a plain reader connection"
    );
    drop(conn);
    storage.close().await;
}

/// The invariants print their ids, a violation prints as `<id> <detail>`, and the labels of the
/// ten invariants are SPEC 9's.
///
/// Proves: W15 (violations by id), decision 97.
#[test]
fn violations_print_by_id() {
    assert_eq!(
        Invariant::ALL.map(Invariant::id),
        [
            "INV-1", "INV-2", "INV-4", "INV-8", "INV-9", "INV-10", "INV-11", "INV-12", "INV-13",
            "INV-26"
        ]
    );
    let violation = Violation {
        invariant: Some(Invariant::Inv9),
        detail: String::from("account 12 (Expenses:Food): depth 7 exceeds 6"),
    };
    assert_eq!(violation.label(), "INV-9");
    assert_eq!(
        violation.to_string(),
        "INV-9 account 12 (Expenses:Food): depth 7 exceeds 6"
    );
    assert_eq!(Invariant::Inv26.to_string(), "INV-26");
}
