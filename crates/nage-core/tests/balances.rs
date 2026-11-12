//! Balances by account and by kind as of a date (SPEC 6.2's roll-up, 6.3's signs, INV-25,
//! decision 96). The live transactions are written by `create_transaction` (decisions 99 and
//! 104); a deleted one is still planted with unchecked SQL, until the soft delete of milestone 4
//! slice 2 (decision 98). The first test runs `nage check` over its committed ledger; the three
//! overflow tests at the end do not: SQLite's own sums can stop on such amounts, depending on
//! the order it sums in (decision 99), and the second plants a second Assets root, a corruption.

mod common;

use nage_core::accounts::{self, Kind, move_account};
use nage_core::balances::{self, BalanceError, KindBalance};
use nage_core::money::Money;

use common::{
    NOW_TEXT, account_named, begin, check_report, commit, corrupt, date, insert_transaction, later,
    money, now, post, seeded,
};

/// The balances as of `as_of` through the storage's read pool.
async fn balances(storage: &nage_core::storage::Storage, as_of: &str) -> balances::Balances {
    let mut conn = common::reader(storage).await;
    balances::balances_as_of(&mut conn, date(as_of))
        .await
        .unwrap_or_else(|error| panic!("cannot compute the balances as of {as_of}: {error}"))
}

/// The entry of `full_name` in `all`.
fn entry<'a>(all: &'a balances::Balances, full_name: &str) -> &'a balances::AccountBalance {
    all.accounts
        .iter()
        .find(|balance| balance.full_name == full_name)
        .unwrap_or_else(|| panic!("no balance for {full_name}"))
}

/// The kind entry of `kind` in `all`.
fn kind_entry(all: &balances::Balances, kind: Kind) -> KindBalance {
    *all.kinds
        .iter()
        .find(|balance| balance.kind == kind)
        .unwrap_or_else(|| panic!("no balance for {kind}"))
}

/// Balances as of a date sum the live postings dated on or before that day (a posting dated the
/// day itself counts, a later one does not, a deleted transaction's never), roll children up
/// into parents (a parent's own postings stay its own), include every account, and total each
/// kind in stored and natural sign; the five live kinds sum to zero (INV-4) and `nage check`
/// finds nothing wrong with the rows.
///
/// Proves: SPEC 6.2 (reports roll children up), 6.3 (stored and natural signs), INV-4, INV-25
/// (deleted transactions excluded), decision 96 (as of the end of the day), decision 104 (the
/// balances re-proved through `create_transaction`).
#[tokio::test]
async fn balances_as_of_sum_live_postings_on_or_before_the_day_and_roll_up() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the ledger").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let dining = account_named(&mut tx, "Expenses:Dining").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let food = accounts::create_account(
        &mut tx,
        admin,
        later(),
        &accounts::NewAccount {
            name: String::from("Food"),
            parent: expenses,
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create Food: {error}"))
    .id;
    move_account(&mut tx, admin, later(), groceries, food)
        .await
        .unwrap_or_else(|error| panic!("cannot move Groceries under Food: {error}"));
    move_account(&mut tx, admin, later(), dining, food)
        .await
        .unwrap_or_else(|error| panic!("cannot move Dining under Food: {error}"));
    // Paycheck on the 1st, groceries on the card on the 10th, dining on the 15th, a card
    // payment on the 20th, a deleted duplicate of the paycheck on the 25th, a posting on Food
    // itself on the 28th.
    post(
        &mut tx,
        admin,
        now(),
        "2026-03-01",
        &[(checking, 300_000), (salary, -300_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-03-10",
        &[(card, -4_500), (groceries, 4_500)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-03-15",
        &[(card, -6_000), (dining, 6_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-03-20",
        &[(checking, -10_500), (card, 10_500)],
    )
    .await;
    insert_transaction(
        &mut tx,
        "2026-03-25",
        "manual",
        true,
        NOW_TEXT,
        &[(checking, 300_000), (salary, -300_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-03-28",
        &[(checking, -2_000), (food, 2_000)],
    )
    .await;
    commit(tx, "the ledger").await;

    let at_15 = balances(&storage, "2026-03-15").await;
    assert_eq!(at_15.as_of, date("2026-03-15"));
    assert_eq!(
        at_15.accounts.len(),
        28,
        "one entry per account, the new one included"
    );
    assert_eq!(
        (
            entry(&at_15, "Assets:Checking").own,
            entry(&at_15, "Assets:Checking").total
        ),
        (money(300_000), money(300_000))
    );
    assert_eq!(
        entry(&at_15, "Assets").own,
        money(0),
        "a root has no own postings"
    );
    assert_eq!(
        entry(&at_15, "Assets").total,
        money(300_000),
        "Assets rolls Checking up"
    );
    assert_eq!(
        entry(&at_15, "Liabilities:Credit Card").total,
        money(-10_500),
        "the card as of the 15th: both purchases, the 15th's included"
    );
    assert_eq!(entry(&at_15, "Expenses:Food:Groceries").total, money(4_500));
    assert_eq!(entry(&at_15, "Expenses:Food:Dining").total, money(6_000));
    assert_eq!(
        (
            entry(&at_15, "Expenses:Food").own,
            entry(&at_15, "Expenses:Food").total
        ),
        (money(0), money(10_500)),
        "Food rolls its children up and has nothing of its own yet"
    );
    assert_eq!(entry(&at_15, "Expenses").total, money(10_500));
    assert_eq!(entry(&at_15, "Income:Salary").total, money(-300_000));
    assert_eq!(
        entry(&at_15, "Expenses:Travel").total,
        money(0),
        "an untouched account is zero"
    );
    assert_eq!(
        kind_entry(&at_15, Kind::Asset),
        KindBalance {
            kind: Kind::Asset,
            stored: money(300_000),
            natural: money(300_000)
        }
    );
    assert_eq!(
        kind_entry(&at_15, Kind::Liability),
        KindBalance {
            kind: Kind::Liability,
            stored: money(-10_500),
            natural: money(10_500)
        },
        "the card shows what is owed"
    );
    assert_eq!(
        kind_entry(&at_15, Kind::Income),
        KindBalance {
            kind: Kind::Income,
            stored: money(-300_000),
            natural: money(300_000)
        },
        "income shows as a positive amount"
    );
    assert_eq!(
        kind_entry(&at_15, Kind::Expense),
        KindBalance {
            kind: Kind::Expense,
            stored: money(10_500),
            natural: money(10_500)
        }
    );
    assert_eq!(
        kind_entry(&at_15, Kind::Equity),
        KindBalance {
            kind: Kind::Equity,
            stored: money(0),
            natural: money(0)
        }
    );
    assert_eq!(
        at_15
            .kinds
            .iter()
            .map(|balance| balance.kind)
            .collect::<Vec<_>>(),
        Kind::ALL.to_vec(),
        "the kinds in SPEC 6.2's order"
    );
    let live_total = Money::checked_sum(at_15.kinds.iter().map(|balance| balance.stored))
        .unwrap_or_else(|| panic!("the kinds' sum overflows"));
    assert_eq!(live_total, money(0), "INV-4 as of the 15th");

    let at_31 = balances(&storage, "2026-03-31").await;
    assert_eq!(
        entry(&at_31, "Assets:Checking").total,
        money(287_500),
        "the paycheck less the card payment and the Food posting; the deleted duplicate does not count"
    );
    assert_eq!(
        entry(&at_31, "Liabilities:Credit Card").total,
        money(0),
        "the card is paid off"
    );
    assert_eq!(
        (
            entry(&at_31, "Expenses:Food").own,
            entry(&at_31, "Expenses:Food").total
        ),
        (money(2_000), money(12_500)),
        "Food's own posting joins the roll-up"
    );
    assert_eq!(
        entry(&at_31, "Income:Salary").total,
        money(-300_000),
        "the deleted duplicate paycheck does not count"
    );
    assert_eq!(
        Money::checked_sum(at_31.kinds.iter().map(|balance| balance.stored)),
        Some(money(0)),
        "INV-4 as of the 31st"
    );

    let before = balances(&storage, "2026-01-31").await;
    assert!(
        before
            .accounts
            .iter()
            .all(|balance| balance.own == money(0) && balance.total == money(0)),
        "nothing before the first posting"
    );
    let on_the_first = balances(&storage, "2026-03-01").await;
    assert_eq!(
        entry(&on_the_first, "Assets:Checking").total,
        money(300_000),
        "a posting dated the day itself counts"
    );
    let day_before = balances(&storage, "2026-02-28").await;
    assert_eq!(
        entry(&day_before, "Assets:Checking").total,
        money(0),
        "the day before the first posting"
    );
    for balance in &at_31.accounts {
        let kind_sign_check = balances::natural(balance.total, balance.account.kind);
        assert!(
            kind_sign_check.is_some(),
            "the natural sign of {} overflows",
            balance.full_name
        );
    }
    let report = check_report(&storage).await;
    assert!(
        report.is_sound(),
        "check over the fixture rows: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// `own_balance` sums an account's own live postings over all dates, a deleted transaction's
/// excluded; an archived account keeps its past balances in `balances_as_of` even though its
/// all-dates balance is zero: its own entry, its parent's roll-up and its kind's stored total
/// as of a past date all hold them.
///
/// Proves: SPEC 6.2 (the archive rule's input), INV-12 (Core half: the archive rule's balance),
/// INV-25, decisions 96 and 104.
#[tokio::test]
async fn own_balance_spans_all_dates_and_archived_accounts_keep_their_past() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "own balances").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let other = account_named(&mut tx, "Income:Other").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-05",
        &[(savings, 50_000), (other, -50_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-06-05",
        &[(savings, -50_000), (checking, 50_000)],
    )
    .await;
    insert_transaction(
        &mut tx,
        "2026-07-05",
        "manual",
        true,
        NOW_TEXT,
        &[(savings, 999), (other, -999)],
    )
    .await;
    let own = balances::own_balance(&mut tx, savings)
        .await
        .unwrap_or_else(|error| panic!("cannot sum Savings: {error}"));
    assert_eq!(
        own,
        money(0),
        "Savings over all dates, the deleted posting excluded"
    );
    let own_other = balances::own_balance(&mut tx, other)
        .await
        .unwrap_or_else(|error| panic!("cannot sum Other: {error}"));
    assert_eq!(own_other, money(-50_000));
    let fees = account_named(&mut tx, "Expenses:Fees").await;
    let own_untouched = balances::own_balance(&mut tx, fees)
        .await
        .unwrap_or_else(|error| panic!("cannot sum Fees: {error}"));
    assert_eq!(own_untouched, money(0));
    accounts::archive_account(&mut tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Savings: {error}"));
    commit(tx, "own balances").await;
    let in_march = balances(&storage, "2026-03-31").await;
    let savings_entry = entry(&in_march, "Assets:Savings");
    assert!(savings_entry.account.is_archived(), "Savings is archived");
    assert_eq!(
        savings_entry.total,
        money(50_000),
        "an archived account's balance as of a past date"
    );
    assert_eq!(
        entry(&in_march, "Assets").total,
        money(50_000),
        "the archived child rolled up into its parent"
    );
    assert_eq!(
        kind_entry(&in_march, Kind::Asset).stored,
        money(50_000),
        "the kind's stored total with the archived account's past balance"
    );
    let in_june = balances(&storage, "2026-06-30").await;
    assert_eq!(entry(&in_june, "Assets:Savings").total, money(0));
    storage.close().await;
}

/// `natural` follows the kind's sign and refuses the one amount whose negation does not fit;
/// a kind total that cannot be negated is an overflow of the balances, never a wrap.
///
/// Proves: SPEC 6.3, SPEC 6.10 (checked arithmetic), decision 96.
#[tokio::test]
async fn natural_signs_follow_the_kind_and_refuse_an_overflow() {
    assert_eq!(balances::natural(money(100), Kind::Asset), Some(money(100)));
    assert_eq!(
        balances::natural(money(100), Kind::Expense),
        Some(money(100))
    );
    assert_eq!(
        balances::natural(money(-100), Kind::Liability),
        Some(money(100))
    );
    assert_eq!(
        balances::natural(money(-100), Kind::Income),
        Some(money(100))
    );
    assert_eq!(balances::natural(money(-5), Kind::Equity), Some(money(5)));
    assert_eq!(
        balances::natural(money(7), Kind::Liability),
        Some(money(-7))
    );
    assert_eq!(balances::natural(money(0), Kind::Income), Some(money(0)));
    assert_eq!(
        balances::natural(money(i64::MIN), Kind::Liability),
        None,
        "the most negative amount has no natural sign"
    );
    assert_eq!(
        balances::natural(money(i64::MIN), Kind::Asset),
        Some(money(i64::MIN))
    );

    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an overflowing liability").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-01",
        &[(card, i64::MIN), (checking, i64::MAX), (salary, 1)],
    )
    .await;
    let error = match balances::balances_as_of(&mut tx, date("2026-12-31")).await {
        Ok(all) => panic!(
            "a liability total of i64::MIN got a natural sign: {:?}",
            kind_entry(&all, Kind::Liability)
        ),
        Err(error) => error,
    };
    assert!(
        matches!(error, BalanceError::Overflow),
        "the refusal is not Overflow: {error:?}"
    );
    common::roll_back(tx, "an overflowing liability").await;
    storage.close().await;
}

/// The roll-up refuses a sum that does not fit although the kind's total does: `Assets:Checking`
/// at the largest amount, `Assets:Savings` at 1 and `Assets:Cash` at -1 each fit, and so does
/// their total, but the roll-up of Assets adds Checking and Savings first; as of the day before
/// Savings' posting the same book has its balances.
///
/// Proves: SPEC 6.10 (an overflow is refused, never wrapped), decision 96 (the roll-up), decision
/// 104 (the roll-up's `Overflow`).
#[tokio::test]
async fn the_roll_up_refuses_an_overflow_that_the_kind_total_does_not_reach() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an overflowing roll-up").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-01",
        &[(checking, i64::MAX), (salary, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-02",
        &[(savings, 1), (cash, -1)],
    )
    .await;
    for (account, own) in [(checking, i64::MAX), (savings, 1), (cash, -1)] {
        let sum = balances::own_balance(&mut tx, account)
            .await
            .unwrap_or_else(|error| panic!("cannot sum account {account}: {error}"));
        assert_eq!(sum, money(own), "the own balance of account {account}");
    }
    assert_eq!(
        Money::checked_sum([money(i64::MAX), money(1), money(-1)]),
        Some(money(i64::MAX)),
        "the total of the three Assets accounts fits"
    );
    let error = match balances::balances_as_of(&mut tx, date("2026-12-31")).await {
        Ok(all) => panic!(
            "a roll-up beyond 64 bits was computed: {:?}",
            entry(&all, "Assets")
        ),
        Err(error) => error,
    };
    assert!(
        matches!(error, BalanceError::Overflow),
        "the refusal is not Overflow: {error:?}"
    );
    let before = balances::balances_as_of(&mut tx, date("2026-01-01"))
        .await
        .unwrap_or_else(|error| panic!("the balances before Savings' posting: {error}"));
    assert_eq!(entry(&before, "Assets").total, money(i64::MAX));
    assert_eq!(kind_entry(&before, Kind::Asset).stored, money(i64::MAX));
    common::roll_back(tx, "an overflowing roll-up").await;
    storage.close().await;
}

/// A kind's total refuses a sum that does not fit. On a sound book the root's roll-up reaches
/// such a sum first, so a second Assets root is planted with unchecked SQL as a corruption:
/// its child at the largest amount and `Assets:Checking` at 1 each roll up within range under
/// their own root, and only the total of the kind's accounts leaves 64 bits; as of the day
/// before Checking's posting the same book has its balances.
///
/// Proves: SPEC 6.10 (an overflow is refused, never wrapped), decision 96 (the kinds' totals),
/// decision 104 (the kind total's `Overflow`).
#[tokio::test]
async fn the_kind_total_refuses_an_overflow_under_a_planted_second_root() {
    let (dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    corrupt(
        &dir.db_path(),
        false,
        &[&format!(
            "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ('Stray', 'stray', NULL, 'asset', NULL, NULL, 0, NULL, 1, '{NOW_TEXT}', 1, '{NOW_TEXT}')"
        )],
    )
    .await;
    let mut tx = begin(&storage, "an overflowing kind").await;
    let stray = account_named(&mut tx, "Stray").await;
    let vault = accounts::create_account(
        &mut tx,
        admin,
        later(),
        &accounts::NewAccount {
            name: String::from("Vault"),
            parent: stray,
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create Vault: {error}"))
    .id;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-01",
        &[(vault, i64::MAX), (salary, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-02",
        &[(checking, 1), (card, -1)],
    )
    .await;
    let error = match balances::balances_as_of(&mut tx, date("2026-12-31")).await {
        Ok(all) => panic!(
            "a kind total beyond 64 bits was computed: {:?}",
            kind_entry(&all, Kind::Asset)
        ),
        Err(error) => error,
    };
    assert!(
        matches!(error, BalanceError::Overflow),
        "the refusal is not Overflow: {error:?}"
    );
    let before = balances::balances_as_of(&mut tx, date("2026-01-01"))
        .await
        .unwrap_or_else(|error| panic!("the balances before Checking's posting: {error}"));
    assert_eq!(
        (
            entry(&before, "Stray").total,
            entry(&before, "Assets").total
        ),
        (money(i64::MAX), money(0)),
        "each root's roll-up fits"
    );
    assert_eq!(kind_entry(&before, Kind::Asset).stored, money(i64::MAX));
    common::roll_back(tx, "an overflowing kind").await;
    storage.close().await;
}

/// The known limit of decision 99: an account whose own postings sum beyond 64 bits (the
/// largest amount and 1, in two transactions that each balance) stops SQLite's `SUM`, so its
/// own balance and the balances as of a date are a database error that says `integer
/// overflow`, not `Overflow`; as of the day before the second posting the same book has its
/// balances.
///
/// Proves: decision 99 (the known limit of SQLite's sum), decision 104 (its datum).
#[tokio::test]
async fn an_account_whose_own_sum_leaves_64_bits_is_a_database_error() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an overflowing sum").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-01",
        &[(checking, i64::MAX), (salary, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-01-02",
        &[(checking, 1), (card, -1)],
    )
    .await;
    let error = match balances::own_balance(&mut tx, checking).await {
        Ok(sum) => panic!("a sum beyond 64 bits was computed: {sum:?}"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("integer overflow"),
        "the own balance's error is not SQLite's overflow: {error}"
    );
    let error = match balances::balances_as_of(&mut tx, date("2026-12-31")).await {
        Ok(all) => panic!(
            "a sum beyond 64 bits was computed: {:?}",
            entry(&all, "Assets:Checking")
        ),
        Err(error) => error,
    };
    assert!(
        matches!(error, BalanceError::Database(_)),
        "the error is not a database error: {error:?}"
    );
    assert!(
        error.to_string().contains("integer overflow"),
        "the balances' error is not SQLite's overflow: {error}"
    );
    let before = balances::balances_as_of(&mut tx, date("2026-01-01"))
        .await
        .unwrap_or_else(|error| panic!("the balances before the second posting: {error}"));
    assert_eq!(entry(&before, "Assets:Checking").own, money(i64::MAX));
    common::roll_back(tx, "an overflowing sum").await;
    storage.close().await;
}
