//! The opening-balance operations (SPEC 6.3, 6.6, W2, 10.3's opening-balance row, INV-7 and
//! INV-13's Core halves, INV-12, INV-25, decisions 100 to 102 and 104): the read, the create in
//! natural sign with each refusal, the edit in place by id and version, the save that changes
//! nothing, the stale save with what is saved now, the removal and the new entry after it, and
//! the messages. Every scenario is built through the core's operations, except the two
//! corruptions named where they are planted, and `nage check` runs over each of them but the
//! corruptions, the largest amounts, on which SQLite's own sums can stop (decision 99), and the
//! stale edit on an archived account.

mod common;

use nage_core::accounts::{
    self, AccountError, AccountId, Kind, NewAccount, SystemKey, archive_account, rename_account,
    unarchive_account,
};
use nage_core::balances;
use nage_core::book::BookError;
use nage_core::date::{Date, DateError};
use nage_core::money::{MinorDigits, Money, ParseMoneyError};
use nage_core::setup::{Setup, initialize};
use nage_core::storage::WriteTransaction;
use nage_core::test_support::open_migrated;
use nage_core::timestamp::Timestamp;
use nage_core::transactions::{
    self, NewOpeningBalance, OpeningBalance, OpeningBalanceEdit, OpeningBalanceError,
    OpeningBoxRefusal, OpeningClash, OpeningForm, OpeningFormError, OpeningNamed, OpeningRefusals,
    OpeningRemoved, OpeningSaved, Origin, Transaction, TransactionId, create_opening_balance,
    edit_opening_balance, remove_opening_balance, remove_opening_balance_form,
    save_opening_balance_form,
};
use nage_core::users::UserId;

use common::{
    LATER_SECOND, NOW_TEXT, TOMORROW_TEXT, account_named, assert_sound, begin, check_report,
    commit, currency, date, insert_transaction, later, ledger_rows, member, money, now,
    open_balance, post, refusal, roll_back, seeded, stamp, standard_setup, today,
};

/// What `create_opening_balance` is given: `natural` minor units in natural sign dated `on`.
fn new(account: AccountId, on: &str, natural: i64) -> NewOpeningBalance {
    NewOpeningBalance {
        account,
        date: date(on),
        amount: money(natural),
    }
}

/// What `edit_opening_balance` is given: the entry, the version the form read, and the date
/// and the amount in natural sign as the form now holds them.
fn edit(id: TransactionId, expected_version: i64, on: &str, natural: i64) -> OpeningBalanceEdit {
    OpeningBalanceEdit {
        id,
        expected_version,
        date: date(on),
        amount: money(natural),
    }
}

/// The live opening balance of `account` as the read gives it.
async fn read(tx: &mut WriteTransaction, account: AccountId) -> Option<OpeningBalance> {
    transactions::opening_balance(tx, account)
        .await
        .unwrap_or_else(|error| panic!("cannot read the opening balance of {account}: {error}"))
}

/// The transaction `id`, which must exist.
async fn row(tx: &mut WriteTransaction, id: TransactionId) -> Transaction {
    transactions::transaction(tx, id)
        .await
        .unwrap_or_else(|error| panic!("cannot read transaction {id}: {error}"))
        .unwrap_or_else(|| panic!("no transaction {id}"))
}

/// The postings of `transaction` as (account, stored minor units), by account id, so that a
/// test does not depend on which of the two an operation writes first.
fn legs(transaction: &Transaction) -> Vec<(AccountId, i64)> {
    let mut legs: Vec<(AccountId, i64)> = transaction
        .postings
        .iter()
        .map(|posting| (posting.account, posting.amount.minor()))
        .collect();
    legs.sort();
    legs
}

/// `legs` of the two postings an opening balance of `stored` minor units on `account` has:
/// the account's and its negation on `opening`, by account id.
fn two_legs(account: AccountId, stored: i64, opening: AccountId) -> Vec<(AccountId, i64)> {
    let mut legs = vec![(account, stored), (opening, -stored)];
    legs.sort();
    legs
}

/// The id of `Equity:Opening Balances`, found by its system key.
async fn opening_account(tx: &mut WriteTransaction) -> AccountId {
    accounts::system_account(tx, SystemKey::OpeningBalances)
        .await
        .unwrap_or_else(|error| panic!("cannot find Equity:Opening Balances: {error}"))
        .id
}

/// The own balance of `account` over all dates, in stored sign.
async fn own(tx: &mut WriteTransaction, account: AccountId) -> Money {
    balances::own_balance(tx, account)
        .await
        .unwrap_or_else(|error| panic!("cannot sum account {account}: {error}"))
}

/// An archived account with an opening balance, which only arises once the account's postings
/// cancel out: `Assets:Savings` opens at 500.00 on 2025-12-31, the 500.00 moves to
/// `Assets:Checking` on 2026-01-05, and Savings is archived at `later`. Returns Savings and
/// its entry.
async fn archived_savings(tx: &mut WriteTransaction, admin: UserId) -> (AccountId, OpeningBalance) {
    let savings = account_named(tx, "Assets:Savings").await;
    let checking = account_named(tx, "Assets:Checking").await;
    let entry = open_balance(tx, admin, now(), savings, "2025-12-31", 50_000).await;
    post(
        tx,
        admin,
        now(),
        "2026-01-05",
        &[(savings, -50_000), (checking, 50_000)],
    )
    .await;
    archive_account(tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Savings: {error}"));
    (savings, entry)
}

/// The largest amount a form takes in a book of two minor digits, 999,999,999,999.99, in minor
/// units (decision 142).
const LARGEST: i64 = 99_999_999_999_999;

/// The opening-balance form as a request sends it: the date box and the amount box as typed, the
/// tick box, and what the form names.
fn form(date_box: &str, amount_box: &str, below_zero: bool, named: OpeningNamed) -> OpeningForm {
    OpeningForm {
        date: date_box.to_owned(),
        amount: amount_box.to_owned(),
        below_zero,
        named,
    }
}

/// What a change form or a confirmation showing `entry` names: its entry and its version.
fn naming(entry: &OpeningBalance) -> OpeningNamed {
    OpeningNamed::Entry {
        id: Some(entry.id),
        version: Some(entry.version),
    }
}

/// Saves `sent` on `account` through the form-shaped save by `by` at `at`, today being
/// [`today`].
async fn save(
    tx: &mut WriteTransaction,
    by: UserId,
    at: Timestamp,
    account: AccountId,
    sent: &OpeningForm,
) -> Result<OpeningSaved, OpeningFormError> {
    save_opening_balance_form(tx, by, at, today(), account, sent).await
}

/// The entry `result` saved, panicking on anything else.
#[track_caller]
fn saved(result: Result<OpeningSaved, OpeningFormError>, label: &str) -> OpeningBalance {
    match result {
        Ok(OpeningSaved::Saved(entry)) => entry,
        other => panic!("{label}: not saved: {other:?}"),
    }
}

/// The refusals of `result`, panicking on anything else.
#[track_caller]
fn box_refusals(result: Result<OpeningSaved, OpeningFormError>, label: &str) -> OpeningRefusals {
    match result {
        Err(OpeningFormError::Refused(refusals)) => *refusals,
        other => panic!("{label}: not refused: {other:?}"),
    }
}

/// The clash of `result`, panicking on anything else.
#[track_caller]
fn clash<T: std::fmt::Debug>(result: Result<T, OpeningFormError>, label: &str) -> OpeningClash {
    match result {
        Err(OpeningFormError::Clash(clash)) => *clash,
        other => panic!("{label}: no clash: {other:?}"),
    }
}

/// The failure of `result`, panicking on anything else.
#[track_caller]
fn failure<T: std::fmt::Debug>(
    result: Result<T, OpeningFormError>,
    label: &str,
) -> OpeningBalanceError {
    match result {
        Err(OpeningFormError::Failed(error)) => error,
        other => panic!("{label}: not failed: {other:?}"),
    }
}

/// Asserts that `refusals` holds nothing at the top, `date` under the date box and `amount` under
/// the amount box.
#[track_caller]
fn assert_boxes(
    refusals: &OpeningRefusals,
    date: Option<OpeningBoxRefusal>,
    amount: Option<OpeningBoxRefusal>,
    label: &str,
) {
    assert!(refusals.top.is_none(), "{label}: {refusals:?}");
    assert_eq!(refusals.date, date, "{label}: the date box");
    assert_eq!(refusals.amount, amount, "{label}: the amount box");
}

/// The refusal of an amount `error` in a book of two minor digits.
fn amount_refusal(error: ParseMoneyError) -> OpeningBoxRefusal {
    OpeningBoxRefusal::Amount {
        error,
        digits: MinorDigits::TWO,
    }
}

/// The create writes one transaction of origin `opening_balance` with empty payee and memo,
/// version 1 and the created and updated pairs of the acting user (a member) at the instant,
/// and two postings: the account's in stored sign and its negation on `Equity:Opening
/// Balances`. The amount is given and read back in natural sign: an Assets account's as it
/// is, a card's as what is owed (a plain positive number, stored negative), an overdrawn
/// Assets account's negative, a card in credit's negative (stored positive). A parent of
/// sub-accounts may have one, and a date equal to today is accepted. The read gives the
/// entry's id, account, date, natural amount and version, and `None` for an account without
/// one, an unknown account and `Equity:Opening Balances` itself; the balances hold the
/// postings, the five kinds sum to zero and `nage check` passes, inside the write transaction
/// and over the committed ledger.
///
/// Proves: SPEC 6.3 (natural sign in, stored sign written), SPEC 6.6, INV-13 (Core half: two
/// postings, the account against `Equity:Opening Balances`), INV-4, INV-26 (Core half for
/// transactions), decision 100 (the create and the read; negative amounts; a parent account;
/// today accepted; no stored label), decision 104 (check over the scenario).
#[tokio::test]
async fn create_opening_balance_writes_the_entry_in_natural_sign() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "opening balances").await;
    let editor = member(&mut tx, admin, "editor").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let sub_account = |name: &str, parent: AccountId| NewAccount {
        name: name.to_owned(),
        parent,
        owner: None,
    };
    accounts::create_account(&mut tx, admin, now(), &sub_account("Vacation", savings))
        .await
        .unwrap_or_else(|error| panic!("cannot create Vacation under Savings: {error}"));
    let line = accounts::create_account(&mut tx, admin, now(), &sub_account("Line", liabilities))
        .await
        .unwrap_or_else(|error| panic!("cannot create Liabilities:Line: {error}"))
        .id;
    assert_eq!(read(&mut tx, checking).await, None, "no entry yet");

    let entry = create_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &new(checking, "2025-12-31", 125_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create Checking's opening balance: {error}"));
    assert_eq!(
        entry,
        OpeningBalance {
            id: entry.id,
            account: checking,
            date: date("2025-12-31"),
            amount: money(125_000),
            version: 1,
        }
    );
    assert_eq!(read(&mut tx, checking).await, Some(entry));
    let written = row(&mut tx, entry.id).await;
    assert_eq!(written.origin, Origin::OpeningBalance);
    assert_eq!(
        (written.payee.as_str(), written.memo.as_str()),
        ("", ""),
        "the words \"Opening balance\" are not stored"
    );
    assert_eq!(written.date, date("2025-12-31"));
    assert_eq!(written.version, 1);
    assert_eq!((written.created_by, written.created_at), (editor, later()));
    assert_eq!((written.updated_by, written.updated_at), (editor, later()));
    assert_eq!((written.deleted_by, written.deleted_at), (None, None));
    assert_eq!(legs(&written), two_legs(checking, 125_000, opening));

    for (account, on, natural, stored, label) in [
        (card, "2025-12-31", 80_000, -80_000, "a card: what is owed"),
        (savings, "2026-01-15", -2_500, -2_500, "an overdrawn parent"),
        (line, "2026-01-15", -1_000, 1_000, "a liability in credit"),
        (cash, common::TODAY_TEXT, 4_000, 4_000, "dated today"),
    ] {
        let created = open_balance(&mut tx, admin, now(), account, on, natural).await;
        assert_eq!(
            (
                created.account,
                created.date,
                created.amount,
                created.version
            ),
            (account, date(on), money(natural), 1),
            "{label}"
        );
        assert_eq!(read(&mut tx, account).await, Some(created), "{label}");
        let written = row(&mut tx, created.id).await;
        assert_eq!(written.origin, Origin::OpeningBalance, "{label}");
        assert_eq!(
            legs(&written),
            two_legs(account, stored, opening),
            "{label}"
        );
        assert_eq!(own(&mut tx, account).await, money(stored), "{label}");
    }

    for (account, label) in [
        (salary, "an account that cannot have one"),
        (AccountId(999), "an unknown account"),
        (opening, "Equity:Opening Balances, a posting of every entry"),
    ] {
        assert_eq!(read(&mut tx, account).await, None, "{label}");
    }
    assert_eq!(ledger_rows(&mut tx).await, (5, 10));
    assert_eq!(
        own(&mut tx, opening).await,
        money(-125_000 + 80_000 + 2_500 - 1_000 - 4_000),
        "Equity:Opening Balances holds the negation of every entry"
    );
    let all = balances::balances_as_of(&mut tx, today())
        .await
        .unwrap_or_else(|error| panic!("cannot compute the balances: {error}"));
    let natural = |kind: Kind| {
        all.kinds
            .iter()
            .find(|balance| balance.kind == kind)
            .unwrap_or_else(|| panic!("no balance for {kind}"))
            .natural
    };
    assert_eq!(natural(Kind::Asset), money(125_000 - 2_500 + 4_000));
    assert_eq!(
        natural(Kind::Liability),
        money(80_000 - 1_000),
        "what is owed"
    );
    assert_eq!(
        Money::checked_sum(all.kinds.iter().map(|kind| kind.stored)),
        Some(money(0)),
        "INV-4"
    );
    assert_sound(&mut tx, "the opening balances").await;
    commit(tx, "opening balances").await;
    let report = check_report(&storage).await;
    assert!(
        report.is_sound(),
        "check over the committed entries: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// The counter posting goes to the account that holds the system key `opening_balances`,
/// whatever it is called: after a rename to "Starting Balances" the entry is still written
/// against it. On a book that has lost that account (deleted with unchecked SQL, a corruption)
/// the create fails naming the key (`MissingSystemAccount`), not with the foreign key's error,
/// and writes nothing.
///
/// Proves: SPEC 6.2 (code finds the system accounts by their keys), INV-13 (Core half: the
/// account against `Equity:Opening Balances`), decision 100 (found by its system key).
#[tokio::test]
async fn the_counter_posting_finds_equity_opening_balances_by_its_key() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a renamed counter account").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    rename_account(&mut tx, admin, later(), opening, "Starting Balances")
        .await
        .unwrap_or_else(|error| panic!("cannot rename Equity:Opening Balances: {error}"));
    let entry = open_balance(&mut tx, admin, later(), checking, "2025-12-31", 500).await;
    let written = row(&mut tx, entry.id).await;
    assert_eq!(legs(&written), two_legs(checking, 500, opening));
    assert_sound(&mut tx, "the renamed counter account").await;
    roll_back(tx, "a renamed counter account").await;

    let mut tx = begin(&storage, "a lost counter account").await;
    sqlx::query("DELETE FROM accounts WHERE system_key = 'opening_balances'")
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot delete Equity:Opening Balances: {error}"));
    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &new(checking, "2025-12-31", 500),
        )
        .await,
        "an opening balance without its counter account",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::Accounts(AccountError::MissingSystemAccount(
                SystemKey::OpeningBalances
            ))
        ),
        "{error:?}"
    );
    assert_eq!(ledger_rows(&mut tx).await, (0, 0));
    roll_back(tx, "a lost counter account").await;
    storage.close().await;
}

/// The create refuses an account id that no account has; an expense, an income and an equity
/// account, each named with its kind (`Equity:Opening Balances` itself among them); the
/// Assets and the Liabilities root; an archived Assets account; and a second opening balance
/// for an account that has a live one, on an Assets and on a Liabilities account, carrying the
/// existing entry as the read gives it. None of the refused calls writes a row.
///
/// Proves: INV-13 (Core half: at most one live opening balance per account, only on a
/// non-root Assets or Liabilities account), INV-8 (no posting on a root), INV-12 (an archived
/// account takes no new posting), SPEC 10.3 (second refused, income account refused),
/// decision 100 (the create's account refusals; no row written).
#[tokio::test]
async fn create_opening_balance_refuses_accounts_that_cannot_have_one() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "accounts refused").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;

    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            now(),
            today(),
            &new(AccountId(999), "2025-12-31", 100),
        )
        .await,
        "an unknown account",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::NoSuchAccount {
                account: AccountId(999)
            }
        ),
        "an unknown account: {error:?}"
    );

    for (full_name, kind) in [
        ("Expenses:Groceries", Kind::Expense),
        ("Income:Salary", Kind::Income),
        ("Equity:Valuation Changes", Kind::Equity),
        ("Equity:Opening Balances", Kind::Equity),
    ] {
        let account = account_named(&mut tx, full_name).await;
        let error = refusal(
            create_opening_balance(
                &mut tx,
                admin,
                now(),
                today(),
                &new(account, "2025-12-31", 100),
            )
            .await,
            full_name,
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::NotAssetOrLiability { account, kind: named }
                    if account == full_name && *named == kind
            ),
            "{full_name}: {error:?}"
        );
    }

    for root in ["Assets", "Liabilities"] {
        let account = account_named(&mut tx, root).await;
        let error = refusal(
            create_opening_balance(
                &mut tx,
                admin,
                now(),
                today(),
                &new(account, "2025-12-31", 100),
            )
            .await,
            root,
        );
        assert!(
            matches!(&error, OpeningBalanceError::RootAccount { account } if account == root),
            "{root}: {error:?}"
        );
    }

    archive_account(&mut tx, admin, later(), cash)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Cash: {error}"));
    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &new(cash, "2025-12-31", 100),
        )
        .await,
        "an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ArchivedAccount { account } if account == "Assets:Cash"
        ),
        "an archived account: {error:?}"
    );
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a refused call wrote a row"
    );

    for (account, full_name, natural) in [
        (checking, "Assets:Checking", 125_000),
        (card, "Liabilities:Credit Card", 80_000),
    ] {
        let first = open_balance(&mut tx, admin, now(), account, "2025-12-31", natural).await;
        let error = refusal(
            create_opening_balance(
                &mut tx,
                admin,
                later(),
                today(),
                &new(account, "2026-01-31", 90_000),
            )
            .await,
            "a second opening balance",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::AlreadyExists { account, existing, digits }
                    if account == full_name && *existing == first && *digits == MinorDigits::TWO
            ),
            "a second opening balance for {full_name}: {error:?}"
        );
        assert_eq!(
            read(&mut tx, account).await,
            Some(first),
            "the first entry of {full_name} stays as it was"
        );
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        (2, 4),
        "only the two first entries were written"
    );
    assert_sound(&mut tx, "the refused accounts and the two entries").await;
    roll_back(tx, "accounts refused").await;
    storage.close().await;
}

/// The create refuses an amount of zero; a date after the today it is handed, the day after
/// and the last day of `Date`'s range alike, carrying the date and that today (an earlier
/// today refuses a date that a later today accepts: the core reads no clock); and the most
/// negative amount, on an Assets and on a Liabilities account. None of the refused calls
/// writes a row. The largest amount is stored as the opening balance of an Assets and of a
/// Liabilities account, its negation as an Assets account's, and each is read back.
///
/// Proves: SPEC 6.6 (nonzero), SPEC 6.4 and decision 88 (today comes from the caller), SPEC
/// 10.3 (zero refused; a date after today and an amount too large to store refused), decision
/// 100 (the create's value refusals; the day after refused; no row written).
#[tokio::test]
async fn create_opening_balance_refuses_zero_a_date_after_today_and_an_amount_too_large() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "values refused").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;

    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            now(),
            today(),
            &new(checking, "2025-12-31", 0),
        )
        .await,
        "an amount of zero",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ZeroAmount { account } if account == "Assets:Checking"
        ),
        "an amount of zero: {error:?}"
    );

    for (on, handed) in [
        (TOMORROW_TEXT, today()),
        ("2199-12-31", today()),
        ("2025-07-01", date("2025-06-30")),
    ] {
        let error = refusal(
            create_opening_balance(&mut tx, admin, now(), handed, &new(checking, on, 100)).await,
            "a date after today",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::DateAfterToday { account, date: refused, today }
                    if account == "Assets:Checking" && *refused == date(on) && *today == handed
            ),
            "{on} with today {handed}: {error:?}"
        );
    }

    for (account, full_name) in [
        (checking, "Assets:Checking"),
        (card, "Liabilities:Credit Card"),
    ] {
        let error = refusal(
            create_opening_balance(
                &mut tx,
                admin,
                now(),
                today(),
                &new(account, "2025-12-31", i64::MIN),
            )
            .await,
            "the most negative amount",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::TooLarge { account, digits }
                    if account == full_name && *digits == MinorDigits::TWO
            ),
            "the most negative amount on {full_name}: {error:?}"
        );
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a refused call wrote a row"
    );
    assert_sound(&mut tx, "the refused values").await;

    for (account, natural, stored) in [
        (checking, LARGEST, LARGEST),
        (savings, -LARGEST, -LARGEST),
        (card, LARGEST, -LARGEST),
    ] {
        let created = open_balance(&mut tx, admin, now(), account, "2025-07-01", natural).await;
        assert_eq!(created.amount, money(natural));
        assert_eq!(read(&mut tx, account).await, Some(created));
        let written = row(&mut tx, created.id).await;
        assert_eq!(legs(&written), two_legs(account, stored, opening));
    }
    assert_eq!(ledger_rows(&mut tx).await, (3, 6));
    roll_back(tx, "values refused").await;
    storage.close().await;
}

/// An edit names the entry's id and the version its form read and changes the date and the
/// amount together, the date alone or the amount alone, each in place: the transaction's id,
/// its posting ids and accounts, its origin and its created pair stay, the date and both
/// postings' amounts follow (a card's amount in natural sign again), the version rises by
/// exactly one per edit and the updated pair names the editor and the instant of each; a new
/// date equal to today is accepted; no edit adds a row, and the edit of the card's entry
/// leaves Checking's as it was; the balances follow the new amounts; a change of the sign
/// alone is an edit too, never taken for a save that changes nothing; and `nage check` passes.
///
/// Proves: INV-7 (each write bumps the version by one; the edit names the version it
/// expects), INV-26 (Core half: the updated pair names the acting user), SPEC 6.6 and W2 (the
/// edit carries the version), decision 100 (the edit in place, of the entry it names alone; a
/// change of the sign alone is an edit), decision 103 (the amount and the date changed
/// together).
#[tokio::test]
async fn edit_opening_balance_changes_the_date_and_the_amount_in_place() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "edits").await;
    let editor = member(&mut tx, admin, "editor").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let created = row(&mut tx, entry.id).await;
    let posting_ids = |transaction: &Transaction| {
        transaction
            .postings
            .iter()
            .map(|posting| (posting.id, posting.account))
            .collect::<Vec<_>>()
    };

    let both = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-30", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the date and the amount: {error}"));
    assert_eq!(
        both,
        OpeningBalance {
            id: entry.id,
            account: checking,
            date: date("2025-12-30"),
            amount: money(130_000),
            version: 2,
        }
    );
    assert_eq!(read(&mut tx, checking).await, Some(both));
    let edited = row(&mut tx, entry.id).await;
    assert_eq!(edited.date, date("2025-12-30"));
    assert_eq!(edited.version, 2);
    assert_eq!(legs(&edited), two_legs(checking, 130_000, opening));
    assert_eq!(
        posting_ids(&edited),
        posting_ids(&created),
        "the postings keep their ids and accounts"
    );
    assert_eq!((edited.created_by, edited.created_at), (admin, now()));
    assert_eq!((edited.updated_by, edited.updated_at), (editor, later()));
    assert_eq!((edited.deleted_by, edited.deleted_at), (None, None));
    assert_eq!(edited.origin, Origin::OpeningBalance);
    assert_eq!((edited.payee.as_str(), edited.memo.as_str()), ("", ""));

    let after = stamp(LATER_SECOND + 60);
    let date_alone = edit_opening_balance(
        &mut tx,
        admin,
        after,
        today(),
        &edit(entry.id, 2, "2025-12-29", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the date alone: {error}"));
    assert_eq!(
        (date_alone.date, date_alone.amount, date_alone.version),
        (date("2025-12-29"), money(130_000), 3)
    );
    let edited = row(&mut tx, entry.id).await;
    assert_eq!((edited.updated_by, edited.updated_at), (admin, after));
    assert_eq!((edited.created_by, edited.created_at), (admin, now()));

    let amount_alone = edit_opening_balance(
        &mut tx,
        editor,
        after,
        today(),
        &edit(entry.id, 3, "2025-12-29", -500),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the amount alone: {error}"));
    assert_eq!(
        (amount_alone.date, amount_alone.amount, amount_alone.version),
        (date("2025-12-29"), money(-500), 4)
    );
    let edited = row(&mut tx, entry.id).await;
    assert_eq!(legs(&edited), two_legs(checking, -500, opening));
    assert_eq!(posting_ids(&edited), posting_ids(&created));

    let owed = open_balance(&mut tx, admin, now(), card, "2025-12-31", 80_000).await;
    let other = row(&mut tx, entry.id).await;
    let less = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(owed.id, 1, common::TODAY_TEXT, 75_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot edit the card's opening balance: {error}"));
    assert_eq!(
        (less.date, less.amount, less.version),
        (today(), money(75_000), 2),
        "a card's amount in natural sign, dated today"
    );
    assert_eq!(
        legs(&row(&mut tx, owed.id).await),
        two_legs(card, -75_000, opening)
    );
    assert_eq!(
        row(&mut tx, entry.id).await,
        other,
        "the edit of the card's entry changed Checking's"
    );

    assert_eq!(ledger_rows(&mut tx).await, (2, 4), "an edit adds no row");
    assert_eq!(own(&mut tx, checking).await, money(-500));
    assert_eq!(own(&mut tx, card).await, money(-75_000));
    assert_eq!(own(&mut tx, opening).await, money(75_500));
    let flipped = edit_opening_balance(
        &mut tx,
        editor,
        after,
        today(),
        &edit(entry.id, 4, "2025-12-29", 500),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the sign alone: {error}"));
    assert_eq!(
        (flipped.amount, flipped.version),
        (money(500), 5),
        "a save that only changes the sign is an edit"
    );
    assert_sound(&mut tx, "the edited entries").await;
    roll_back(tx, "edits").await;
    storage.close().await;
}

/// A save whose date and amount both equal what is saved is not an edit: it returns the entry
/// as saved and leaves the transaction exactly as it was (version, updated pair, postings),
/// whether it names the stored version, an older one, one that never existed or none that
/// could; whatever today it is handed; for a card, whose amount is compared in natural sign;
/// and on an archived account too.
///
/// Proves: decision 100 (a save that changes nothing writes nothing, keeps the version and
/// records nobody, whatever version it names), INV-7 (no write, no bump), SPEC 10.3 (an
/// unchanged save writing nothing).
#[tokio::test]
async fn a_save_that_changes_nothing_writes_nothing_whatever_version_it_names() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "unchanged saves").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let saved = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-30", 125_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot edit the entry once: {error}"));
    assert_eq!(saved.version, 2);
    let before = row(&mut tx, entry.id).await;
    let rows = ledger_rows(&mut tx).await;

    let after = stamp(LATER_SECOND + 60);
    for (expected_version, handed) in [
        (2, today()),
        (1, today()),
        (7, today()),
        (0, today()),
        (2, date("2025-01-01")),
    ] {
        let result = edit_opening_balance(
            &mut tx,
            admin,
            after,
            handed,
            &edit(entry.id, expected_version, "2025-12-30", 125_000),
        )
        .await
        .unwrap_or_else(|error| {
            panic!("an unchanged save naming version {expected_version} was refused: {error}")
        });
        assert_eq!(
            result, saved,
            "an unchanged save naming version {expected_version}"
        );
        assert_eq!(
            row(&mut tx, entry.id).await,
            before,
            "an unchanged save naming version {expected_version} wrote something"
        );
    }
    assert_eq!(ledger_rows(&mut tx).await, rows);

    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let owed = open_balance(&mut tx, admin, now(), card, "2025-12-31", 80_000).await;
    let before = row(&mut tx, owed.id).await;
    let result = edit_opening_balance(
        &mut tx,
        editor,
        after,
        today(),
        &edit(owed.id, 4, "2025-12-31", 80_000),
    )
    .await
    .unwrap_or_else(|error| panic!("an unchanged save of what a card owes was refused: {error}"));
    assert_eq!(result, owed);
    assert_eq!(row(&mut tx, owed.id).await, before);
    assert_sound(&mut tx, "the unchanged saves").await;
    roll_back(tx, "unchanged saves").await;

    let mut tx = begin(&storage, "an unchanged save on an archived account").await;
    let (_savings, archived) = archived_savings(&mut tx, admin).await;
    let before = row(&mut tx, archived.id).await;
    let result = edit_opening_balance(
        &mut tx,
        admin,
        after,
        today(),
        &edit(archived.id, 5, "2025-12-31", 50_000),
    )
    .await
    .unwrap_or_else(|error| panic!("an unchanged save on an archived account: {error}"));
    assert_eq!(result, archived);
    assert_eq!(row(&mut tx, archived.id).await, before);
    assert_sound(&mut tx, "the archived account and its entry").await;
    roll_back(tx, "an unchanged save on an archived account").await;
    storage.close().await;
}

/// When two people change the same entry, the second save is refused as stale and nothing of
/// it is written: the refusal carries the account, the book's digits and the entry as saved
/// now, exactly as the read gives it. A version ahead of the stored one is stale too. The
/// stale refusal comes before the refusals of the new values (a zero amount, a date after
/// today) and before the archived refusal. "Save mine", the same values sent again naming the
/// version now saved, goes through.
///
/// Proves: INV-7 (the edit names the version it expects), SPEC 5.4 (a mismatch is refused
/// with the current data; no silent overwrite), decision 100 (the edit's order), decision 101
/// (what the stale refusal carries; nothing written; save mine).
#[tokio::test]
async fn a_stale_edit_is_refused_with_the_entry_as_saved_now() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "two editors").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let first = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-31", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("the first save was refused: {error}"));
    assert_eq!(first.version, 2);
    let before = row(&mut tx, entry.id).await;

    let after = stamp(LATER_SECOND + 60);
    for (stale, label) in [
        (
            edit(entry.id, 1, "2025-12-28", 120_000),
            "the version the form read",
        ),
        (
            edit(entry.id, 3, "2025-12-28", 120_000),
            "a version ahead of the stored one",
        ),
        (edit(entry.id, 1, "2025-12-31", 0), "a stale zero amount"),
        (
            edit(entry.id, 1, TOMORROW_TEXT, 130_000),
            "a stale date after today",
        ),
    ] {
        let error = refusal(
            edit_opening_balance(&mut tx, admin, after, today(), &stale).await,
            label,
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::Stale { account, saved, digits }
                    if account == "Assets:Checking" && *saved == first
                        && *digits == MinorDigits::TWO
            ),
            "{label}: {error:?}"
        );
        assert_eq!(
            read(&mut tx, checking).await,
            Some(first),
            "{label}: what the refusal carries is what the read gives"
        );
        assert_eq!(
            row(&mut tx, entry.id).await,
            before,
            "{label}: the refused save wrote something"
        );
    }

    let mine = edit_opening_balance(
        &mut tx,
        admin,
        after,
        today(),
        &edit(entry.id, first.version, "2025-12-28", 120_000),
    )
    .await
    .unwrap_or_else(|error| panic!("save mine was refused: {error}"));
    assert_eq!(
        (mine.date, mine.amount, mine.version),
        (date("2025-12-28"), money(120_000), 3)
    );
    assert_sound(&mut tx, "the two editors' saves").await;
    roll_back(tx, "two editors").await;

    let mut tx = begin(&storage, "a stale edit on an archived account").await;
    let (_savings, archived) = archived_savings(&mut tx, admin).await;
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            after,
            today(),
            &edit(archived.id, 2, "2025-12-31", 60_000),
        )
        .await,
        "a stale edit on an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::Stale { account, saved, .. }
                if account == "Assets:Savings" && *saved == archived
        ),
        "a stale edit on an archived account: {error:?}"
    );
    roll_back(tx, "a stale edit on an archived account").await;
    storage.close().await;
}

/// The edit refuses an id that names no live opening balance (no transaction, or a
/// transaction of another origin, which it leaves untouched); a new amount of zero; a new date
/// after today, and a new amount alone when the saved date is after the today it is handed;
/// the most negative amount; and every change on an archived account (a date alone and an
/// amount alone); the date goes through once the account is unarchived, as the message
/// advises. None of the refused calls writes anything.
///
/// Proves: INV-12 (an archived account's live postings keep their sum), SPEC 10.3 (a date
/// after today, an archived account and an amount too large refused), decision 100 (the edit's
/// refusals; the id must name a live opening balance).
#[tokio::test]
async fn edit_opening_balance_refuses_what_the_create_refuses_and_an_archived_account() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "edits refused").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let manual = post(
        &mut tx,
        admin,
        now(),
        "2026-03-01",
        &[(checking, 300_000), (salary, -300_000)],
    )
    .await;
    for id in [TransactionId(999), manual.id] {
        let error = refusal(
            edit_opening_balance(
                &mut tx,
                admin,
                later(),
                today(),
                &edit(id, 1, "2026-03-02", 300_000),
            )
            .await,
            "an id that names no opening balance",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::NoSuchOpeningBalance { id: named } if *named == id
            ),
            "transaction {id}: {error:?}"
        );
    }
    assert_eq!(
        row(&mut tx, manual.id).await,
        manual,
        "the manual transaction was touched"
    );

    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let before = row(&mut tx, entry.id).await;
    let rows = ledger_rows(&mut tx).await;
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &edit(entry.id, 1, "2025-12-31", 0),
        )
        .await,
        "a new amount of zero",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ZeroAmount { account } if account == "Assets:Checking"
        ),
        "a new amount of zero: {error:?}"
    );
    for (on, natural, handed) in [
        (TOMORROW_TEXT, 125_000, today()),
        ("2025-12-31", 1, date("2025-06-30")),
    ] {
        let error = refusal(
            edit_opening_balance(
                &mut tx,
                admin,
                later(),
                handed,
                &edit(entry.id, 1, on, natural),
            )
            .await,
            "a date after today",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::DateAfterToday { account, date: refused, today }
                    if account == "Assets:Checking" && *refused == date(on) && *today == handed
            ),
            "{on} with today {handed}: {error:?}"
        );
    }
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &edit(entry.id, 1, "2025-12-31", i64::MIN),
        )
        .await,
        "the most negative amount",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::TooLarge { account, digits }
                if account == "Assets:Checking" && *digits == MinorDigits::TWO
        ),
        "the most negative amount: {error:?}"
    );
    assert_eq!(
        row(&mut tx, entry.id).await,
        before,
        "a refused edit wrote something"
    );
    assert_eq!(ledger_rows(&mut tx).await, rows);
    assert_sound(&mut tx, "the refused edits").await;
    roll_back(tx, "edits refused").await;

    let mut tx = begin(&storage, "an edit on an archived account").await;
    let (savings, archived) = archived_savings(&mut tx, admin).await;
    let before = row(&mut tx, archived.id).await;
    let change = edit(archived.id, 1, "2025-12-30", 50_000);
    let error = refusal(
        edit_opening_balance(&mut tx, admin, later(), today(), &change).await,
        "an edit on an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ArchivedAccount { account } if account == "Assets:Savings"
        ),
        "an edit on an archived account: {error:?}"
    );
    assert_eq!(row(&mut tx, archived.id).await, before);
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &edit(archived.id, 1, "2025-12-31", 60_000),
        )
        .await,
        "an amount edit on an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ArchivedAccount { account } if account == "Assets:Savings"
        ),
        "an amount edit on an archived account: {error:?}"
    );
    assert_eq!(row(&mut tx, archived.id).await, before);
    unarchive_account(&mut tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive Savings: {error}"));
    let changed = edit_opening_balance(&mut tx, admin, later(), today(), &change)
        .await
        .unwrap_or_else(|error| panic!("the edit after the unarchive was refused: {error}"));
    assert_eq!((changed.date, changed.version), (date("2025-12-30"), 2));
    assert_sound(&mut tx, "the edit after the unarchive").await;
    roll_back(tx, "an edit on an archived account").await;
    storage.close().await;
}

/// The removal names the entry's id and the version and deletes softly: the deleted pair
/// names the remover, a member who neither created nor last edited the entry, and the
/// instant, the version rises by one, and the updated pair (an earlier editor's) and the
/// postings stay as they were; the transaction is still read by id.
/// The entry leaves the read and every balance, and the account takes a new opening balance
/// under a new id at version 1. A form opened before the removal cannot reach the new entry:
/// an edit naming the old id is refused as not found and a removal as already removed, in its
/// own sentence, whatever version it names.
/// `nage check` passes over the removed and the new entry.
///
/// Proves: INV-25 (a deleted transaction leaves every balance and stays stored), INV-13
/// (Core half: the count is of live entries, so a new one is accepted), INV-7 (the removal
/// bumps the version by one), W8 (a delete is soft and records who and when), SPEC 10.3
/// (removal, then a new one accepted), decision 100 (the removal; the id never given again
/// keeps an old form from the new entry), decision 104 (check over the scenario).
#[tokio::test]
async fn remove_opening_balance_deletes_softly_and_frees_the_account() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a removal").await;
    let editor = member(&mut tx, admin, "editor").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-31", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot edit the entry: {error}"));
    let before = row(&mut tx, entry.id).await;
    let remover = member(&mut tx, admin, "remover").await;

    let after = stamp(LATER_SECOND + 60);
    let removed = remove_opening_balance(&mut tx, remover, after, entry.id, 2)
        .await
        .unwrap_or_else(|error| panic!("cannot remove the entry: {error}"));
    assert_eq!(removed.id, entry.id);
    assert!(removed.is_deleted());
    assert_eq!(
        (removed.deleted_by, removed.deleted_at),
        (Some(remover), Some(after))
    );
    assert_eq!(removed.version, 3);
    assert_eq!(
        (removed.updated_by, removed.updated_at),
        (editor, later()),
        "the updated pair stays the last edit's"
    );
    assert_eq!((removed.created_by, removed.created_at), (admin, now()));
    assert_eq!(removed.postings, before.postings, "the postings stay");
    assert_eq!((removed.date, removed.origin), (before.date, before.origin));
    assert_eq!(row(&mut tx, entry.id).await, removed);
    assert_eq!(
        read(&mut tx, checking).await,
        None,
        "the entry left the read"
    );
    assert_eq!(own(&mut tx, checking).await, money(0));
    assert_eq!(own(&mut tx, opening).await, money(0));
    let all = balances::balances_as_of(&mut tx, today())
        .await
        .unwrap_or_else(|error| panic!("cannot compute the balances: {error}"));
    assert!(
        all.accounts
            .iter()
            .all(|balance| balance.own == money(0) && balance.total == money(0)),
        "the removed entry is in a balance"
    );

    let again = open_balance(&mut tx, editor, after, checking, "2026-01-31", 90_000).await;
    assert!(again.id > entry.id, "the new entry has a new id");
    assert_eq!(
        (again.date, again.amount, again.version),
        (date("2026-01-31"), money(90_000), 1)
    );
    assert_eq!(read(&mut tx, checking).await, Some(again));

    for expected_version in [1, 2, 3] {
        let error = refusal(
            edit_opening_balance(
                &mut tx,
                admin,
                after,
                today(),
                &edit(entry.id, expected_version, "2026-01-31", 95_000),
            )
            .await,
            "an edit from a form opened before the removal",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::NoSuchOpeningBalance { id } if *id == entry.id
            ),
            "an edit naming the removed entry at version {expected_version}: {error:?}"
        );
        let error = refusal(
            remove_opening_balance(&mut tx, admin, after, entry.id, expected_version).await,
            "a removal from a form opened before the removal",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::AlreadyRemoved { id } if *id == entry.id
            ),
            "a removal naming the removed entry at version {expected_version}: {error:?}"
        );
        assert!(
            error
                .to_string()
                .starts_with("The opening balance was not removed because it was already removed"),
            "a removal is told it was not removed: {error}"
        );
    }
    assert_eq!(read(&mut tx, checking).await, Some(again));
    assert_eq!(row(&mut tx, entry.id).await, removed);
    assert_eq!(ledger_rows(&mut tx).await, (2, 4));
    assert_eq!(own(&mut tx, checking).await, money(90_000));
    commit(tx, "a removal").await;
    let report = check_report(&storage).await;
    assert!(
        report.is_sound(),
        "check over the removed and the new entry: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// The removal refuses an id that names no live opening balance (no transaction, or a
/// transaction of another origin, which stays live) as already removed; a version other than the stored one,
/// older or ahead, carrying the entry as saved now; and an archived account, where a stale
/// version is still refused as stale first, as in the edit, and where the removal goes through
/// once the account is unarchived. None of the refused calls writes anything. A removal
/// deletes the entry it names and no other: the entry, the book's second transaction, is
/// removed naming the stored version while the manual transaction and the card's opening
/// balance stay live and as they were, and once Savings' entry is removed, Checking still
/// holds the 500.00 of the transfer.
///
/// Proves: INV-12 (an archived account's live postings keep their sum), INV-7 (the removal
/// names the version it expects), decision 100 (the removal's refusals; the id must name a live
/// opening balance; the removal deletes the entry it names and no other), decision 101 (the
/// stale refusal's data).
#[tokio::test]
async fn remove_opening_balance_refuses_unknown_stale_and_archived() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "removals refused").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let manual = post(
        &mut tx,
        admin,
        now(),
        "2026-03-01",
        &[(checking, 300_000), (salary, -300_000)],
    )
    .await;
    for id in [TransactionId(999), manual.id] {
        let error = refusal(
            remove_opening_balance(&mut tx, admin, later(), id, 1).await,
            "an id that names no opening balance",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::AlreadyRemoved { id: named } if *named == id
            ),
            "transaction {id}: {error:?}"
        );
    }
    assert_eq!(
        row(&mut tx, manual.id).await,
        manual,
        "the manual transaction was touched"
    );

    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let saved = edit_opening_balance(
        &mut tx,
        admin,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-31", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot edit the entry: {error}"));
    let before = row(&mut tx, entry.id).await;
    for expected_version in [1, 3] {
        let error = refusal(
            remove_opening_balance(&mut tx, admin, later(), entry.id, expected_version).await,
            "a stale removal",
        );
        assert!(
            matches!(
                &error,
                OpeningBalanceError::RemoveStale { account, saved: carried, digits }
                    if account == "Assets:Checking" && *carried == saved
                        && *digits == MinorDigits::TWO
            ),
            "a removal naming version {expected_version}: {error:?}"
        );
        assert!(
            error.to_string().starts_with(
                "The opening balance of Assets:Checking was not removed because it was changed"
            ),
            "a removal is told it was not removed: {error}"
        );
    }
    assert_eq!(
        row(&mut tx, entry.id).await,
        before,
        "a refused removal wrote something"
    );
    assert_eq!(read(&mut tx, checking).await, Some(saved));
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let owed = open_balance(&mut tx, admin, now(), card, "2025-12-31", 80_000).await;
    let other = row(&mut tx, owed.id).await;
    let removed = remove_opening_balance(&mut tx, admin, later(), entry.id, saved.version)
        .await
        .unwrap_or_else(|error| {
            panic!("the removal naming the stored version was refused: {error}")
        });
    assert_eq!(removed.id, entry.id);
    assert!(removed.is_deleted(), "the entry named was not removed");
    assert_eq!(
        row(&mut tx, manual.id).await,
        manual,
        "the removal touched the manual transaction"
    );
    assert_eq!(
        row(&mut tx, owed.id).await,
        other,
        "the removal touched the card's opening balance"
    );
    assert_sound(&mut tx, "the refused removals").await;
    roll_back(tx, "removals refused").await;

    let mut tx = begin(&storage, "a removal on an archived account").await;
    let (savings, archived) = archived_savings(&mut tx, admin).await;
    let before = row(&mut tx, archived.id).await;
    let error = refusal(
        remove_opening_balance(&mut tx, admin, later(), archived.id, 2).await,
        "a stale removal on an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::RemoveStale { account, saved, .. }
                if account == "Assets:Savings" && *saved == archived
        ),
        "a stale removal on an archived account: {error:?}"
    );
    let error = refusal(
        remove_opening_balance(&mut tx, admin, later(), archived.id, 1).await,
        "a removal on an archived account",
    );
    assert!(
        matches!(
            &error,
            OpeningBalanceError::ArchivedAccount { account } if account == "Assets:Savings"
        ),
        "a removal on an archived account: {error:?}"
    );
    assert_eq!(row(&mut tx, archived.id).await, before);
    assert_eq!(read(&mut tx, savings).await, Some(archived));
    unarchive_account(&mut tx, admin, later(), savings)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive Savings: {error}"));
    let removed = remove_opening_balance(&mut tx, admin, later(), archived.id, 1)
        .await
        .unwrap_or_else(|error| panic!("the removal after the unarchive was refused: {error}"));
    assert!(removed.is_deleted());
    assert_eq!(read(&mut tx, savings).await, None);
    assert_eq!(
        own(&mut tx, checking).await,
        money(50_000),
        "the removal deleted the transfer too"
    );
    assert_sound(&mut tx, "the removal after the unarchive").await;
    roll_back(tx, "a removal on an archived account").await;
    storage.close().await;
}

/// A stored opening balance whose amount has no natural sign, the most negative amount on a
/// Liabilities account, planted with unchecked SQL as a corruption since no operation writes
/// it, is read as corrupt and never as a wrong amount.
///
/// Proves: SPEC 6.3 and decision 96 (the natural sign of the most negative amount is refused,
/// never wrapped), decision 100 (the read gives the amount in natural sign).
#[tokio::test]
async fn a_stored_amount_without_a_natural_sign_is_read_as_corrupt() {
    let (_dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a planted amount").await;
    let opening = opening_account(&mut tx).await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    insert_transaction(
        &mut tx,
        "2025-12-31",
        "opening_balance",
        false,
        NOW_TEXT,
        &[(card, i64::MIN), (opening, i64::MAX)],
    )
    .await;
    let error = refusal(
        transactions::opening_balance(&mut tx, card).await,
        "an amount without a natural sign",
    );
    assert!(matches!(error, OpeningBalanceError::Corrupt), "{error:?}");
    assert_eq!(
        error.to_string(),
        "The opening balance could not be read because what is stored for it is damaged: it holds an amount Nage cannot show. Ask whoever looks after Nage to run nage check and to restore a backup if it reports a problem.",
        "no entry number (decision 145)"
    );
    roll_back(tx, "a planted amount").await;
    storage.close().await;
}

/// Every refusal of the opening-balance operations says what was not done, why, and what to do
/// instead, in plain words, but a clash's, which ends after the why (decision 145); one that can
/// stand at the top of a page names the account by its full name, one that can stand under a box
/// names none, and none prints a number a request chose; the amounts are in natural sign as SPEC
/// 6.10 displays them, with the dates involved; no two refusals share a message; the
/// failures of the chart, of the book row and of the database pass the inner error's message
/// on whole, a database failure named as one. The test prints each moment with its message
/// (`cargo test -p nage-core --test opening_balance opening_balance_refusals -- --nocapture`
/// shows them).
///
/// Proves: decision 102 (every message of the opening-balance operations, in its three
/// parts; a failure of the database, of the book row or of the chart passes the inner error's
/// message on), decision 101 (the stale message says what is saved now), W2 (every refusal
/// explains itself).
#[test]
fn opening_balance_refusals_explain_themselves() {
    let saved = |on: &str, natural: i64, version: i64| OpeningBalance {
        id: TransactionId(12),
        account: AccountId(9),
        date: date(on),
        amount: money(natural),
        version,
    };
    let cases: [(&str, &str, OpeningBalanceError, &str); 17] = [
        (
            "NotInitialized",
            "An opening balance is saved before the book has been set up.",
            OpeningBalanceError::NotInitialized,
            "The opening balance was not saved because this book is not set up yet. Finish the setup first, then try again.",
        ),
        (
            "NoSuchAccount",
            "An opening balance is saved for account number 999, which no account has.",
            OpeningBalanceError::NoSuchAccount {
                account: AccountId(999),
            },
            "The opening balance was not saved because its account does not exist. Go to Accounts and choose the account from the list.",
        ),
        (
            "NotAssetOrLiability",
            "An opening balance of 45.00 is entered for the expense category Expenses:Groceries.",
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Expenses:Groceries"),
                kind: Kind::Expense,
            },
            "The opening balance was not saved because Expenses:Groceries is a category under Expenses, and only accounts under Assets and Liabilities, which hold what the household has and what it owes, start with a balance. Leave it without one: everything under Income, Expenses and Equity starts at zero.",
        ),
        (
            "NotAssetOrLiability",
            "An opening balance is entered for Income:Salary.",
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Income:Salary"),
                kind: Kind::Income,
            },
            "The opening balance was not saved because Income:Salary is a category under Income, and only accounts under Assets and Liabilities, which hold what the household has and what it owes, start with a balance. Leave it without one: everything under Income, Expenses and Equity starts at zero.",
        ),
        (
            "RootAccount",
            "An opening balance is entered for Liabilities itself, not for an account under it.",
            OpeningBalanceError::RootAccount {
                account: String::from("Liabilities"),
            },
            "The opening balance was not saved because Liabilities is one of Nage's five sections, which only group the accounts in them and hold nothing of their own. Enter it on an account under Liabilities instead.",
        ),
        (
            "ArchivedAccount",
            "The opening balance of Assets:Savings is entered, changed or removed after the account was archived.",
            OpeningBalanceError::ArchivedAccount {
                account: String::from("Assets:Savings"),
            },
            "The opening balance of Assets:Savings was left as it is because the account is archived, and an archived account's opening balance cannot be entered, changed or removed. Unarchive it first, then try again.",
        ),
        (
            "ZeroAmount",
            "The opening balance of Assets:Checking is saved with an amount of 0.00.",
            OpeningBalanceError::ZeroAmount {
                account: String::from("Assets:Checking"),
            },
            "The opening balance was not saved because the amount is zero, and an account that starts at zero needs no opening balance. There is nothing to enter: leave it without one.",
        ),
        (
            "AlreadyExists",
            "An opening balance is entered for Assets:Checking, which already has one of 1,250.00 dated 2025-12-31.",
            OpeningBalanceError::AlreadyExists {
                account: String::from("Assets:Checking"),
                existing: saved("2025-12-31", 125_000, 1),
                digits: MinorDigits::TWO,
            },
            "Your opening balance for Assets:Checking was not saved because one was entered while you were working, by another user or in another window: it is 1,250.00 on 2025-12-31.",
        ),
        (
            "AlreadyExists",
            "An opening balance is entered for Liabilities:Credit Card, which already has one of 800.00 owed, dated 2025-12-31.",
            OpeningBalanceError::AlreadyExists {
                account: String::from("Liabilities:Credit Card"),
                existing: saved("2025-12-31", 80_000, 1),
                digits: MinorDigits::TWO,
            },
            "Your opening balance for Liabilities:Credit Card was not saved because one was entered while you were working, by another user or in another window: it is 800.00 on 2025-12-31.",
        ),
        (
            "DateAfterToday",
            "On 2026-11-09 the opening balance of Assets:Checking is saved with the date 2026-11-10.",
            OpeningBalanceError::DateAfterToday {
                account: String::from("Assets:Checking"),
                date: date("2026-11-10"),
                today: date("2026-11-09"),
            },
            "The opening balance was not saved because the date is after today, 2026-11-09, and an opening balance cannot be for a day that has not come. Pick today or an earlier day, then save again.",
        ),
        (
            "TooLarge",
            "The opening balance of Assets:Checking is saved with an amount of -92,233,720,368,547,758.08, which is beyond what Nage can store.",
            OpeningBalanceError::TooLarge {
                account: String::from("Assets:Checking"),
                digits: MinorDigits::TWO,
            },
            "The opening balance was not saved because the amount is more than Nage takes for one amount: at most 999,999,999,999.99 either way. Check it for a typing mistake, then save again.",
        ),
        (
            "NoSuchOpeningBalance",
            "A form still open on an opening balance is saved after another user removed that opening balance.",
            OpeningBalanceError::NoSuchOpeningBalance {
                id: TransactionId(12),
            },
            "Your change to the opening balance was not saved because it was removed while you were working, by another user or in another window.",
        ),
        (
            "Stale",
            "Two people have the opening balance of Assets:Checking open; the first saves 1,300.00 dated 2025-12-30, then the second saves.",
            OpeningBalanceError::Stale {
                account: String::from("Assets:Checking"),
                saved: saved("2025-12-30", 130_000, 2),
                digits: MinorDigits::TWO,
            },
            "Your change to the opening balance of Assets:Checking was not saved because it was changed while you were working, by another user or in another window: it is now 1,300.00 on 2025-12-30.",
        ),
        (
            "Stale",
            "The same, in a book kept in a currency with three decimals, for an overdrawn account that now starts at -1.500.",
            OpeningBalanceError::Stale {
                account: String::from("Assets:Checking"),
                saved: saved("2025-12-30", -1_500, 2),
                digits: MinorDigits::THREE,
            },
            "Your change to the opening balance of Assets:Checking was not saved because it was changed while you were working, by another user or in another window: it is now -1.500 on 2025-12-30.",
        ),
        (
            "Stale",
            "The same, in a book kept in a currency without decimals, for a card that now owes 80,000.",
            OpeningBalanceError::Stale {
                account: String::from("Liabilities:Credit Card"),
                saved: saved("2025-12-31", 80_000, 3),
                digits: MinorDigits::ZERO,
            },
            "Your change to the opening balance of Liabilities:Credit Card was not saved because it was changed while you were working, by another user or in another window: it is now 80,000 on 2025-12-31.",
        ),
        (
            "RemoveStale",
            "Two people have the opening balance of Assets:Checking open; the first saves 1,300.00 dated 2025-12-30, then the second removes it.",
            OpeningBalanceError::RemoveStale {
                account: String::from("Assets:Checking"),
                saved: saved("2025-12-30", 130_000, 2),
                digits: MinorDigits::TWO,
            },
            "The opening balance of Assets:Checking was not removed because it was changed while you were working, by another user or in another window: it is now 1,300.00 on 2025-12-30.",
        ),
        (
            "Corrupt",
            "The database was changed outside Nage and holds an opening balance that cannot be shown.",
            OpeningBalanceError::Corrupt,
            "The opening balance could not be read because what is stored for it is damaged: it holds an amount Nage cannot show. Ask whoever looks after Nage to run nage check and to restore a backup if it reports a problem.",
        ),
    ];
    let mut messages = Vec::new();
    for (variant, moment, error, expected) in &cases {
        let message = error.to_string();
        println!("[OpeningBalanceError::{variant}] {moment} => {message}");
        assert_eq!(&message, expected, "the message of {error:?}");
        messages.push(message);
    }
    let mut distinct = messages.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        messages.len(),
        "two refusals share a message"
    );

    assert_eq!(
        OpeningBalanceError::Accounts(AccountError::MissingSystemAccount(
            SystemKey::OpeningBalances
        ))
        .to_string(),
        "the system account opening_balances is missing",
        "a failure of the chart passes its message on"
    );
    assert_eq!(
        OpeningBalanceError::Book(BookError::Corrupt(String::from("minor_digits is 7")))
            .to_string(),
        "the book row is corrupt: minor_digits is 7",
        "a failure of the book row passes its message on"
    );
    let database = OpeningBalanceError::Database(sqlx::Error::RowNotFound).to_string();
    assert_eq!(
        database,
        format!("database error: {}", sqlx::Error::RowNotFound),
        "a database failure does not pass its message on"
    );
}

/// Every sentence of the opening balance, written out with the moment that leads to it: under
/// the date box, under the amount box (the empty-amount, the sign, the comma and the point
/// sentences in books of none, two and three decimals, the example and the limit following the book's
/// digits), at the top of the account's page, on the clash pages and of the operations alone.
/// Each refusal says what was not saved, why and what to do, but a clash sentence, which ends
/// after the why; a sentence at the top names the account by its full place and one under a box
/// names none; none prints a number a request chose. Pinned as one text snapshot.
///
/// Proves: Decisions 102, 142, 143, 144 and 145 with the pause's P6: every sentence of the form's boxes, the top, the clashes and the operations; each refusal in three parts but a clash sentence, which ends after the why; a sentence at the top naming the account by its full place and one under a box naming none; no number a request chose; the limit and the example figure in the book's digits, the empty-amount, the sign and the comma sentences at none, two and three decimals; pinned as one text snapshot.
#[test]
fn the_opening_balance_sentences() {
    let jpy = MinorDigits::ZERO;
    let cad = MinorDigits::TWO;
    let kwd = MinorDigits::THREE;
    let parse_error = |text: &str, digits: MinorDigits| match Money::parse(text, digits) {
        Ok(amount) => panic!("{text:?} was read as {amount:?}"),
        Err(error) => error,
    };
    let date_error = |text: &str| match text.parse::<Date>() {
        Ok(date) => panic!("{text:?} was read as {date}"),
        Err(error) => error,
    };
    let amount = |text: &str, digits: MinorDigits| OpeningBoxRefusal::Amount {
        error: parse_error(text, digits),
        digits,
    };
    let decimals = |text: &str, digits: MinorDigits, currency: &str| {
        assert_eq!(
            parse_error(text, digits),
            ParseMoneyError::TooManyDecimals {
                allowed: digits.get()
            },
            "{text:?}"
        );
        OpeningBoxRefusal::Decimals {
            currency: currency.to_owned(),
            allowed: digits.get(),
        }
    };
    let too_large = |text: &str, digits: MinorDigits| {
        let typed = Money::parse(text, digits)
            .unwrap_or_else(|error| panic!("{text:?} is no amount: {error}"));
        assert!(!typed.is_taken(digits), "{text:?} is past the limit");
        OpeningBoxRefusal::TooLarge { digits }
    };
    let entry = |on: &str, natural: i64| OpeningBalance {
        id: TransactionId(4321),
        account: AccountId(12),
        date: date(on),
        amount: money(natural),
        version: 3,
    };
    let checking = || String::from("Assets:Checking");

    let under_the_date_box: Vec<(&str, String, OpeningBoxRefusal)> = vec![
        (
            "Date(Empty)",
            String::from("The date box is left empty."),
            OpeningBoxRefusal::Date(DateError::Empty),
        ),
        (
            "Date(Format)",
            String::from("A crafted form sends 30/09/2026."),
            OpeningBoxRefusal::Date(date_error("30/09/2026")),
        ),
        (
            "Date(Invalid)",
            String::from("A crafted form sends 2026-02-30."),
            OpeningBoxRefusal::Date(date_error("2026-02-30")),
        ),
        (
            "Date(OutOfRange)",
            String::from("A crafted form sends 1899-12-31."),
            OpeningBoxRefusal::Date(date_error("1899-12-31")),
        ),
        (
            "DateAfterToday",
            String::from("On 2026-11-16 the date 2026-11-17 is picked."),
            OpeningBoxRefusal::DateAfterToday {
                today: date("2026-11-16"),
            },
        ),
    ];
    let under_the_amount_box: Vec<(&str, String, OpeningBoxRefusal)> = vec![
        (
            "Amount(Empty)",
            String::from("The amount box is left empty."),
            amount("", cad),
        ),
        (
            "Amount(Empty)",
            String::from("In a book kept in JPY the amount box is left empty."),
            amount("", jpy),
        ),
        (
            "Amount(Empty)",
            String::from("In a book kept in KWD the amount box is left empty."),
            amount("", kwd),
        ),
        (
            "Amount(Empty)",
            String::from("The amount box holds only a minus, with or without the tick."),
            amount("-", cad),
        ),
        (
            "Amount(UnexpectedCharacter)",
            String::from("The amount is typed as 1 234,56 EUR."),
            amount("1 234,56 EUR", cad),
        ),
        (
            "Amount(UnexpectedCharacter)",
            String::from("In a book kept in JPY the amount is typed as 1 234 JPY."),
            amount("1 234 JPY", jpy),
        ),
        (
            "Amount(UnexpectedCharacter)",
            String::from("In a book kept in KWD the amount is typed as 1 234,567 KWD."),
            amount("1 234,567 KWD", kwd),
        ),
        (
            "Amount(MisplacedComma)",
            String::from("The amount is typed as 1.234,56."),
            amount("1.234,56", cad),
        ),
        (
            "Amount(MisplacedComma)",
            String::from("In a book kept in JPY the amount is typed as 12,34."),
            amount("12,34", jpy),
        ),
        (
            "Amount(MisplacedComma)",
            String::from("In a book kept in KWD the amount is typed as 1.234,567."),
            amount("1.234,567", kwd),
        ),
        (
            "Amount(MisplacedPoint)",
            String::from("The amount is typed as 1.234.56."),
            amount("1.234.56", cad),
        ),
        (
            "Amount(MisplacedPoint)",
            String::from("In a book kept in JPY the amount is typed as 1.234.567."),
            amount("1.234.567", jpy),
        ),
        (
            "Amount(MisplacedPoint)",
            String::from("In a book kept in KWD the amount is typed as 1.234.567.890."),
            amount("1.234.567.890", kwd),
        ),
        (
            "Decimals",
            String::from("In a book kept in CAD the amount is typed as 12.345."),
            decimals("12.345", cad, "CAD"),
        ),
        (
            "Decimals",
            String::from("In a book kept in KWD the amount is typed as 12.3456."),
            decimals("12.3456", kwd, "KWD"),
        ),
        (
            "Decimals",
            String::from("In a book kept in JPY the amount is typed as 1,500.5."),
            decimals("1,500.5", jpy, "JPY"),
        ),
        (
            "TwoSigns",
            String::from(
                "The amount is typed as -35.20 and the box The account was overdrawn is ticked.",
            ),
            OpeningBoxRefusal::TwoSigns,
        ),
        (
            "TooLarge",
            String::from("In a book kept in CAD the amount is typed as 50,000,000,000,000,000.00."),
            too_large("50,000,000,000,000,000.00", cad),
        ),
        (
            "TooLarge",
            String::from("In a book kept in JPY the amount is typed as 1,000,000,000,000."),
            too_large("1,000,000,000,000", jpy),
        ),
        (
            "TooLarge",
            String::from("In a book kept in KWD the amount is typed as 1,000,000,000,000.000."),
            too_large("1,000,000,000,000.000", kwd),
        ),
        (
            "Zero",
            String::from("The amount 0 is typed for an account that has no opening balance."),
            OpeningBoxRefusal::Zero,
        ),
    ];
    let at_the_top: Vec<(&str, String, OpeningBalanceError)> = vec![(
        "ArchivedMeanwhile",
        String::from("Another user archived Assets:Checking while its form was open."),
        OpeningBalanceError::ArchivedMeanwhile {
            account: checking(),
        },
    )];
    let clashes: Vec<(&str, String, OpeningClash)> = vec![
        (
            "Changed",
            String::from("Another user saved 2,500.00 on 2026-09-30 while this form was open."),
            OpeningClash::Changed {
                account: checking(),
                saved: entry("2026-09-30", 250_000),
                digits: cad,
            },
        ),
        (
            "Removed",
            String::from("Another user removed the opening balance while this form was open."),
            OpeningClash::Removed {
                account: checking(),
            },
        ),
        (
            "Entered",
            String::from("Another user entered 2,480.15 on 2026-09-30 while this form was open."),
            OpeningClash::Entered {
                account: checking(),
                saved: entry("2026-09-30", 248_015),
                digits: cad,
            },
        ),
        (
            "RemoveChanged",
            String::from(
                "Another user saved 2,500.00 on 2026-09-30 while the confirmation was open.",
            ),
            OpeningClash::RemoveChanged {
                account: checking(),
                saved: entry("2026-09-30", 250_000),
                digits: cad,
            },
        ),
    ];
    let operations: Vec<(&str, String, OpeningBalanceError)> = vec![
        (
            "NotInitialized",
            String::from("An opening balance is saved before the book has been set up."),
            OpeningBalanceError::NotInitialized,
        ),
        (
            "NoSuchAccount",
            String::from("An opening balance is saved for an account number no account has."),
            OpeningBalanceError::NoSuchAccount {
                account: AccountId(987_654),
            },
        ),
        (
            "NotAssetOrLiability",
            String::from("An opening balance is entered for the category Expenses:Groceries."),
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Expenses:Groceries"),
                kind: Kind::Expense,
            },
        ),
        (
            "NotAssetOrLiability",
            String::from(
                "An opening balance is entered for the built-in account Equity:Valuation Changes.",
            ),
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Equity:Valuation Changes"),
                kind: Kind::Equity,
            },
        ),
        (
            "RootAccount",
            String::from("An opening balance is entered for the section Liabilities itself."),
            OpeningBalanceError::RootAccount {
                account: String::from("Liabilities"),
            },
        ),
        (
            "ArchivedAccount",
            String::from(
                "The opening balance of the archived Assets:Savings is entered, changed or removed.",
            ),
            OpeningBalanceError::ArchivedAccount {
                account: String::from("Assets:Savings"),
            },
        ),
        (
            "ZeroAmount",
            String::from("The opening balance of Assets:Checking is saved with an amount of 0.00."),
            OpeningBalanceError::ZeroAmount {
                account: checking(),
            },
        ),
        (
            "AlreadyExists",
            String::from(
                "An opening balance is entered for Assets:Checking, which has one of 2,480.15 on 2026-09-30.",
            ),
            OpeningBalanceError::AlreadyExists {
                account: checking(),
                existing: entry("2026-09-30", 248_015),
                digits: cad,
            },
        ),
        (
            "DateAfterToday",
            String::from(
                "On 2026-11-16 the opening balance of Assets:Checking is saved with the date 2026-11-17.",
            ),
            OpeningBalanceError::DateAfterToday {
                account: checking(),
                date: date("2026-11-17"),
                today: date("2026-11-16"),
            },
        ),
        (
            "TooLarge",
            String::from(
                "The opening balance of Assets:Savings is saved with an amount past the limit.",
            ),
            OpeningBalanceError::TooLarge {
                account: String::from("Assets:Savings"),
                digits: cad,
            },
        ),
        (
            "NoSuchOpeningBalance",
            String::from("A change names an entry that was removed meanwhile."),
            OpeningBalanceError::NoSuchOpeningBalance {
                id: TransactionId(4321),
            },
        ),
        (
            "AlreadyRemoved",
            String::from("A removal names an entry that was removed meanwhile."),
            OpeningBalanceError::AlreadyRemoved {
                id: TransactionId(4321),
            },
        ),
        (
            "Stale",
            String::from(
                "A change names a version older than the one saved, 2,500.00 on 2026-09-30.",
            ),
            OpeningBalanceError::Stale {
                account: checking(),
                saved: entry("2026-09-30", 250_000),
                digits: cad,
            },
        ),
        (
            "RemoveStale",
            String::from(
                "A removal names a version older than the one saved, 2,500.00 on 2026-09-30.",
            ),
            OpeningBalanceError::RemoveStale {
                account: checking(),
                saved: entry("2026-09-30", 250_000),
                digits: cad,
            },
        ),
        (
            "Corrupt",
            String::from(
                "The database was changed outside Nage and holds an opening balance that cannot be shown.",
            ),
            OpeningBalanceError::Corrupt,
        ),
    ];

    let mut lines = Vec::new();
    let write = |lines: &mut Vec<String>, label: &str, moment: &str, sentence: &str| {
        lines.push(format!("[{label}] {moment}"));
        lines.push(format!("=> {sentence}"));
        lines.push(String::new());
    };
    let three_parts = |sentence: &str, label: &str| {
        assert!(
            sentence.contains(" because ") && sentence.matches(". ").count() >= 1,
            "{label}: why and what to do: {sentence}"
        );
    };
    let chosen = ["987654", "987,654", "4321", "4,321"];
    let names_no_chosen_number = |sentence: &str, label: &str| {
        for number in chosen {
            assert!(
                !sentence.contains(number),
                "{label} prints the number {number}: {sentence}"
            );
        }
    };

    lines.push(String::from(
        "The opening-balance form, under the date box:",
    ));
    lines.push(String::new());
    for (label, moment, refusal) in &under_the_date_box {
        let sentence = refusal.to_string();
        three_parts(&sentence, label);
        assert!(!sentence.contains("Assets:"), "{label} names an account");
        write(&mut lines, label, moment, &sentence);
    }
    lines.push(String::from(
        "The opening-balance form, under the amount box:",
    ));
    lines.push(String::new());
    for (label, moment, refusal) in &under_the_amount_box {
        let sentence = refusal.to_string();
        three_parts(&sentence, label);
        assert!(!sentence.contains("Assets:"), "{label} names an account");
        write(&mut lines, label, moment, &sentence);
    }
    lines.push(String::from("At the top of the account's page:"));
    lines.push(String::new());
    for (label, moment, error) in &at_the_top {
        let sentence = error.to_string();
        assert!(
            sentence.starts_with("Assets:Checking "),
            "{label} names the account"
        );
        write(&mut lines, label, moment, &sentence);
    }
    lines.push(String::from("The clash pages:"));
    lines.push(String::new());
    for (label, moment, clash) in &clashes {
        let sentence = clash.to_string();
        assert!(
            sentence.contains("Assets:Checking"),
            "{label} names the account"
        );
        assert!(
            !sentence.contains("Save mine") && !sentence.contains("try again"),
            "{label} ends after the why: {sentence}"
        );
        names_no_chosen_number(&sentence, label);
        write(&mut lines, label, moment, &sentence);
    }
    lines.push(String::from("The operations' own refusals:"));
    lines.push(String::new());
    for (label, moment, error) in &operations {
        let sentence = error.to_string();
        names_no_chosen_number(&sentence, label);
        write(&mut lines, label, moment, &sentence);
    }
    // A removal's own refusal says what was not done, not that a change was not saved (the hand
    // check's H5), and the change keeps its sentence.
    let removed = OpeningBalanceError::AlreadyRemoved {
        id: TransactionId(4321),
    }
    .to_string();
    assert_eq!(
        removed,
        "The opening balance was not removed because it was already removed, by another user or in another window. There is nothing left to remove.",
        "a removal of an entry no longer live"
    );
    assert!(
        OpeningBalanceError::NoSuchOpeningBalance {
            id: TransactionId(4321),
        }
        .to_string()
        .starts_with("Your change to the opening balance was not saved because it was removed"),
        "the change keeps its sentence"
    );
    let sentences = lines.join("\n").trim_end().to_owned();
    insta::assert_snapshot!(sentences);
}

/// The form-shaped save enters an opening balance at version 1 against `Equity:Opening
/// Balances`, from the boxes as typed (white space and commas as the money module takes them),
/// by the acting user at the instant; an overdrawn account's amount and a card's amount owed
/// in natural sign; then the change form naming the entry and its version changes it in place,
/// the version raised by exactly one, the updated pair the second user's and the created pair
/// kept, no row added; a save of what is saved writes nothing; and a change of the amount alone,
/// then of the date alone to today itself, each changes it in place again. `nage check` passes
/// inside the transaction and over the committed book.
///
/// Proves: Decisions 140 and 141 with INV-7, INV-13 and INV-26: the form enters one at version 1 against Equity:Opening Balances, an amount below zero in natural sign, changes it in place with the version raised by exactly one and the stamps, and a save of what is saved writes nothing.
#[tokio::test]
async fn the_form_enters_an_opening_balance_and_changes_it_in_place() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the form").await;
    let editor = member(&mut tx, admin, "editor").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;

    let entry = saved(
        save(
            &mut tx,
            editor,
            later(),
            checking,
            &form(" 2025-12-31 ", " 1,250.00 ", false, OpeningNamed::New),
        )
        .await,
        "the enter form",
    );
    assert_eq!(
        entry,
        OpeningBalance {
            id: entry.id,
            account: checking,
            date: date("2025-12-31"),
            amount: money(125_000),
            version: 1,
        }
    );
    assert_eq!(read(&mut tx, checking).await, Some(entry));
    let created = row(&mut tx, entry.id).await;
    assert_eq!(created.origin, Origin::OpeningBalance);
    assert_eq!((created.created_by, created.created_at), (editor, later()));
    assert_eq!((created.updated_by, created.updated_at), (editor, later()));
    assert_eq!(legs(&created), two_legs(checking, 125_000, opening));

    for (account, amount_box, ticked, natural, stored, label) in [
        (
            savings,
            "25.00",
            true,
            -2_500,
            -2_500,
            "an overdrawn account",
        ),
        (card, "800", false, 80_000, -80_000, "a card's amount owed"),
    ] {
        let other = saved(
            save(
                &mut tx,
                admin,
                now(),
                account,
                &form("2026-01-15", amount_box, ticked, OpeningNamed::New),
            )
            .await,
            label,
        );
        assert_eq!(
            (other.account, other.amount, other.version),
            (account, money(natural), 1),
            "{label}"
        );
        assert_eq!(
            legs(&row(&mut tx, other.id).await),
            two_legs(account, stored, opening),
            "{label}"
        );
    }
    assert_eq!(ledger_rows(&mut tx).await, (3, 6));

    let after = stamp(LATER_SECOND + 60);
    let changed = saved(
        save(
            &mut tx,
            admin,
            after,
            checking,
            &form("2025-12-30", "1300", false, naming(&entry)),
        )
        .await,
        "the change form",
    );
    assert_eq!(
        changed,
        OpeningBalance {
            id: entry.id,
            account: checking,
            date: date("2025-12-30"),
            amount: money(130_000),
            version: 2,
        }
    );
    let written = row(&mut tx, entry.id).await;
    assert_eq!(written.version, 2, "the version rose by exactly one");
    assert_eq!((written.updated_by, written.updated_at), (admin, after));
    assert_eq!((written.created_by, written.created_at), (editor, later()));
    assert_eq!(legs(&written), two_legs(checking, 130_000, opening));
    assert_eq!(ledger_rows(&mut tx).await, (3, 6), "a change adds no row");

    let before = row(&mut tx, entry.id).await;
    let result = save(
        &mut tx,
        editor,
        stamp(LATER_SECOND + 120),
        checking,
        &form("2025-12-30", "1,300.00", false, naming(&changed)),
    )
    .await;
    assert!(
        matches!(result, Ok(OpeningSaved::Nothing)),
        "a save of what is saved: {result:?}"
    );
    assert_eq!(row(&mut tx, entry.id).await, before, "nothing was written");

    let amount_only = saved(
        save(
            &mut tx,
            admin,
            stamp(LATER_SECOND + 180),
            checking,
            &form("2025-12-30", "1,350.00", false, naming(&changed)),
        )
        .await,
        "a change of the amount alone",
    );
    assert_eq!(
        (amount_only.date, amount_only.amount, amount_only.version),
        (date("2025-12-30"), money(135_000), 3),
        "a change of the amount alone"
    );
    let today_box = today().to_string();
    let date_only = saved(
        save(
            &mut tx,
            admin,
            stamp(LATER_SECOND + 240),
            checking,
            &form(&today_box, "1,350.00", false, naming(&amount_only)),
        )
        .await,
        "a change of the date alone, to today",
    );
    assert_eq!(
        (date_only.date, date_only.amount, date_only.version),
        (today(), money(135_000), 4),
        "a change of the date alone, to today"
    );
    assert_eq!(
        legs(&row(&mut tx, entry.id).await),
        two_legs(checking, 135_000, opening)
    );
    assert_eq!(ledger_rows(&mut tx).await, (3, 6), "a change adds no row");
    assert_sound(&mut tx, "the form's entries").await;
    commit(tx, "the form").await;
    let report = check_report(&storage).await;
    assert!(report.is_sound(), "{:?}", report.violations());
    storage.close().await;
}

/// A form refused in both boxes gets every refusal at once, one per box, the date's beside the
/// amount's: an empty, malformed, nonexistent or out-of-range date and a date after the today
/// handed in under the date box; an empty amount, a sign an amount cannot have, a comma and a
/// point out of place and too many decimals (naming the book's currency) under the amount box;
/// a box with no problem has none; a box that holds no digit, only a minus, only the dollar sign
/// or both, ticked or not, gets the one sentence of a box that holds no amount. Nothing is
/// written, on the enter form and on the change form.
///
/// Proves: Decision 145 with decision 99 and the hand check's H2: an empty, malformed or out-of-range date and a malformed amount are refused at once, one refusal per box, a date after the today handed in under the date box, too many decimals naming the currency, a lone minus and a lone dollar sign, ticked and unticked, told the amount box holds no amount, and nothing is written.
#[tokio::test]
async fn a_form_refused_in_both_boxes_writes_nothing() {
    use ParseMoneyError::{Empty, MisplacedComma, MisplacedPoint, UnexpectedCharacter};
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "refused boxes").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let after_today = OpeningBoxRefusal::DateAfterToday { today: today() };
    let cases = [
        (
            "",
            "",
            Some(OpeningBoxRefusal::Date(DateError::Empty)),
            Some(amount_refusal(Empty)),
        ),
        (
            "30/09/2026",
            "1 234,56 EUR",
            Some(OpeningBoxRefusal::Date(DateError::Format)),
            Some(amount_refusal(UnexpectedCharacter)),
        ),
        (
            "2026-02-30",
            "1.234,56",
            Some(OpeningBoxRefusal::Date(DateError::Invalid)),
            Some(amount_refusal(MisplacedComma)),
        ),
        (
            "1899-12-31",
            "1.234.56",
            Some(OpeningBoxRefusal::Date(DateError::OutOfRange)),
            Some(amount_refusal(MisplacedPoint)),
        ),
        (
            TOMORROW_TEXT,
            "12.345",
            Some(after_today.clone()),
            Some(OpeningBoxRefusal::Decimals {
                currency: String::from("CAD"),
                allowed: 2,
            }),
        ),
        ("2199-12-31", "100.00", Some(after_today.clone()), None),
        ("2025-12-31", "$", None, Some(amount_refusal(Empty))),
        (
            "   ",
            "1,250.00",
            Some(OpeningBoxRefusal::Date(DateError::Empty)),
            None,
        ),
    ];
    for (date_box, amount_box, date_refusal, amount_box_refusal) in cases.clone() {
        let label = format!("{date_box:?} and {amount_box:?}");
        let refusals = box_refusals(
            save(
                &mut tx,
                admin,
                now(),
                checking,
                &form(date_box, amount_box, false, OpeningNamed::New),
            )
            .await,
            &label,
        );
        assert_boxes(&refusals, date_refusal, amount_box_refusal, &label);
        assert_eq!(ledger_rows(&mut tx).await, (0, 0), "{label} wrote a row");
    }
    // A box that holds no digit, only a minus, only the dollar sign or both, ticked or not, is
    // refused as an empty one, in the one sentence of an amount box that holds no amount (the
    // hand check's H2).
    for amount_box in ["-", "$", "-$", "$-", " - "] {
        for ticked in [false, true] {
            let label = format!("{amount_box:?} ticked {ticked}");
            let refusals = box_refusals(
                save(
                    &mut tx,
                    admin,
                    now(),
                    checking,
                    &form("2025-12-31", amount_box, ticked, OpeningNamed::New),
                )
                .await,
                &label,
            );
            assert_boxes(&refusals, None, Some(amount_refusal(Empty)), &label);
            assert_eq!(
                refusals.amount.as_ref().map(ToString::to_string).as_deref(),
                Some(
                    "The opening balance was not saved because the amount box holds no amount, and an opening balance needs its amount: Nage never guesses it. Type it as 1,234.56, then save again."
                ),
                "{label}"
            );
            assert_eq!(ledger_rows(&mut tx).await, (0, 0), "{label} wrote a row");
        }
    }
    let refusals = box_refusals(
        save_opening_balance_form(
            &mut tx,
            admin,
            now(),
            date("2025-06-30"),
            checking,
            &form("2025-07-01", "100.00", false, OpeningNamed::New),
        )
        .await,
        "a date after an earlier today",
    );
    assert_boxes(
        &refusals,
        Some(OpeningBoxRefusal::DateAfterToday {
            today: date("2025-06-30"),
        }),
        None,
        "a date after an earlier today",
    );

    let entry = open_balance(&mut tx, admin, now(), card, "2025-12-31", 80_000).await;
    let before = row(&mut tx, entry.id).await;
    for (date_box, amount_box, date_refusal, amount_box_refusal) in cases {
        let label = format!("the change form with {date_box:?} and {amount_box:?}");
        let refusals = box_refusals(
            save(
                &mut tx,
                admin,
                later(),
                card,
                &form(date_box, amount_box, false, naming(&entry)),
            )
            .await,
            &label,
        );
        assert_boxes(&refusals, date_refusal, amount_box_refusal, &label);
        assert_eq!(row(&mut tx, entry.id).await, before, "{label} wrote");
    }
    assert_eq!(ledger_rows(&mut tx).await, (1, 2));
    assert_sound(&mut tx, "the refused forms").await;
    roll_back(tx, "refused boxes").await;
    storage.close().await;
}

/// The limit holds whatever a form sends: the create and the edit take the largest amount and
/// its negation, on an Assets and a Liabilities account, and refuse one minor unit more either
/// way and both 64-bit extremes as `TooLarge`, writing nothing; the form refuses the same and an
/// amount past 64 bits under the amount box, ticked or not, and saves the largest amount ticked.
/// In a book kept in JPY, without decimals, and one kept in KWD, with three, the form reads the
/// typed amount with the book's digits, takes the largest amount and refuses one more under the
/// amount box, and a clash writes its figure with the book's digits.
///
/// Proves: Decision 142: create_opening_balance and edit_opening_balance first take the largest amount and its negation and refuse one minor unit more and both 64-bit extremes as TooLarge, nothing written; then the form refuses them, and an amount past 64 bits, under the amount box.
#[tokio::test]
async fn the_limit_holds_whatever_a_form_sends() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the limit").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let largest = Money::largest_taken(MinorDigits::TWO);
    assert_eq!(largest, money(LARGEST));

    let mut entries = Vec::new();
    for (account, natural, stored) in [
        (checking, LARGEST, LARGEST),
        (savings, -LARGEST, -LARGEST),
        (card, LARGEST, -LARGEST),
    ] {
        let entry = open_balance(&mut tx, admin, now(), account, "2025-12-31", natural).await;
        assert_eq!(entry.amount, money(natural));
        assert_eq!(
            legs(&row(&mut tx, entry.id).await),
            two_legs(account, stored, opening)
        );
        entries.push(entry);
    }
    let rows = ledger_rows(&mut tx).await;
    let past = [LARGEST + 1, -LARGEST - 1, i64::MAX, i64::MIN];
    for natural in past {
        for (account, full_name) in [
            (cash, "Assets:Cash"),
            (checking, "Assets:Checking"),
            (card, "Liabilities:Credit Card"),
        ] {
            let error = refusal(
                create_opening_balance(
                    &mut tx,
                    admin,
                    later(),
                    today(),
                    &new(account, "2025-12-31", natural),
                )
                .await,
                "an amount past the limit",
            );
            let expected_refusal = if account == cash {
                matches!(
                    &error,
                    OpeningBalanceError::TooLarge { account, digits }
                        if account == full_name && *digits == MinorDigits::TWO
                )
            } else {
                matches!(&error, OpeningBalanceError::AlreadyExists { .. })
            };
            assert!(expected_refusal, "{natural} on {full_name}: {error:?}");
        }
        for entry in &entries {
            let before = row(&mut tx, entry.id).await;
            let error = refusal(
                edit_opening_balance(
                    &mut tx,
                    admin,
                    later(),
                    today(),
                    &edit(entry.id, entry.version, "2025-12-31", natural),
                )
                .await,
                "an edit past the limit",
            );
            assert!(
                matches!(&error, OpeningBalanceError::TooLarge { digits, .. } if *digits == MinorDigits::TWO),
                "{natural} on entry {}: {error:?}",
                entry.id
            );
            assert_eq!(row(&mut tx, entry.id).await, before);
        }
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        rows,
        "a refused call wrote a row"
    );

    let too_large = Some(OpeningBoxRefusal::TooLarge {
        digits: MinorDigits::TWO,
    });
    for (amount_box, ticked) in [
        ("1,000,000,000,000.00", false),
        ("-1,000,000,000,000.00", false),
        ("1,000,000,000,000", true),
        ("999,999,999,999.991", false),
        ("92,233,720,368,547,758.07", false),
        ("92,233,720,368,547,758.08", false),
        ("-92,233,720,368,547,758.09", false),
        ("99999999999999999999999", true),
    ] {
        let label = format!("{amount_box:?}, ticked {ticked}");
        let expected = if amount_box == "999,999,999,999.991" {
            Some(OpeningBoxRefusal::Decimals {
                currency: String::from("CAD"),
                allowed: 2,
            })
        } else {
            too_large.clone()
        };
        let refusals = box_refusals(
            save(
                &mut tx,
                admin,
                later(),
                cash,
                &form("2025-12-31", amount_box, ticked, OpeningNamed::New),
            )
            .await,
            &label,
        );
        assert_boxes(&refusals, None, expected, &label);
        for entry in &entries {
            let refusals = box_refusals(
                save(
                    &mut tx,
                    admin,
                    later(),
                    entry.account,
                    &form("2025-12-31", amount_box, ticked, naming(entry)),
                )
                .await,
                &label,
            );
            assert!(
                refusals.amount.is_some() && refusals.date.is_none() && refusals.top.is_none(),
                "{label} on entry {}: {refusals:?}",
                entry.id
            );
        }
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        rows,
        "a refused form wrote a row"
    );

    let taken = saved(
        save(
            &mut tx,
            admin,
            later(),
            cash,
            &form("2025-12-31", "999,999,999,999.99", true, OpeningNamed::New),
        )
        .await,
        "the largest amount ticked",
    );
    assert_eq!(taken.amount, money(-LARGEST));
    assert_sound(&mut tx, "the largest amounts").await;
    commit(tx, "the limit").await;
    let report = check_report(&storage).await;
    assert!(report.is_sound(), "{:?}", report.violations());
    storage.close().await;

    for (code, digits, typed, minor, largest_box, past_box, figure) in [
        (
            "JPY",
            MinorDigits::ZERO,
            "1,500",
            1_500,
            "999,999,999,999",
            "1,000,000,000,000",
            "it is now 1,500 on 2025-12-31.",
        ),
        (
            "KWD",
            MinorDigits::THREE,
            "1,500.250",
            1_500_250,
            "999,999,999,999.999",
            "1,000,000,000,000.000",
            "it is now 1,500.250 on 2025-12-31.",
        ),
    ] {
        in_a_book_of(code, digits, typed, minor, largest_box, past_box, figure).await;
    }
}

/// The second half of [`the_limit_holds_whatever_a_form_sends`]: in a book kept in `code`, of
/// `digits` decimals, the form reads `typed` as `minor` minor units, takes `largest_box` ticked
/// as the largest amount below zero, refuses `past_box` under the amount box with the limit in
/// the book's digits, and writes a clash's figure with them, ending in `figure`.
async fn in_a_book_of(
    code: &str,
    digits: MinorDigits,
    typed: &str,
    minor: i64,
    largest_box: &str,
    past_box: &str,
    figure: &str,
) {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, code).await;
    let initialized = initialize(
        &mut tx,
        now(),
        &Setup {
            currency: currency(code),
            ..standard_setup(true)
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot set the book up in {code}: {error}"));
    let admin = initialized.admin.id;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;

    let entry = saved(
        save(
            &mut tx,
            admin,
            now(),
            checking,
            &form("2025-12-31", typed, false, OpeningNamed::New),
        )
        .await,
        code,
    );
    assert_eq!(entry.amount, money(minor), "{code}: {typed} read");
    let largest = saved(
        save(
            &mut tx,
            admin,
            now(),
            cash,
            &form("2025-12-31", largest_box, true, OpeningNamed::New),
        )
        .await,
        code,
    );
    assert_eq!(
        largest.amount,
        money(-Money::largest_taken(digits).minor()),
        "{code}: {largest_box} ticked is the largest amount below zero"
    );
    let refusals = box_refusals(
        save(
            &mut tx,
            admin,
            now(),
            card,
            &form("2025-12-31", past_box, false, OpeningNamed::New),
        )
        .await,
        code,
    );
    assert_boxes(
        &refusals,
        None,
        Some(OpeningBoxRefusal::TooLarge { digits }),
        &format!("{code}: {past_box}"),
    );
    let found = clash(
        save(
            &mut tx,
            admin,
            later(),
            checking,
            &form(
                "2025-12-30",
                "2",
                false,
                OpeningNamed::Entry {
                    id: Some(entry.id),
                    version: Some(9),
                },
            ),
        )
        .await,
        code,
    );
    assert_eq!(
        found,
        OpeningClash::Changed {
            account: String::from("Assets:Checking"),
            saved: entry,
            digits,
        },
        "{code}: the clash"
    );
    assert!(
        found.to_string().ends_with(figure),
        "{code}: the clash's figure is not in the book's digits: {found}"
    );
    assert_sound(&mut tx, code).await;
    roll_back(tx, code).await;
    storage.close().await;
}

/// A typed zero on an account that has an opening balance writes nothing and answers the
/// removal's question with the live entry, whatever the date box holds (an empty or a malformed
/// date shows no problem) and whatever the tick; on an account without one the zero is refused
/// under the amount box, beside the date's own problem when it has one. A zero is a zero
/// whatever sign it carries and whatever the tick: a minus on a zero with the box ticked asks to
/// remove on an account that has one and is refused as a zero on one without, never as two
/// signs. No zero is ever stored.
///
/// Proves: Decision 143 with the pause's P2 and the hand check's H6: a zero on an account with an opening balance writes nothing and answers AskRemove with the live one, also with an empty date box and as a minus on a zero with the box ticked; on one without, it is refused under the amount box as Zero, also as a minus on a zero with the box ticked, never as two signs, and with an empty date box in both boxes; no zero is ever stored.
#[tokio::test]
async fn a_typed_zero_asks_to_remove_or_is_refused() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a typed zero").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    for (date_box, ticked, date_refusal) in [
        ("2025-12-31", false, None),
        ("2025-12-31", true, None),
        ("", false, Some(OpeningBoxRefusal::Date(DateError::Empty))),
        (
            TOMORROW_TEXT,
            false,
            Some(OpeningBoxRefusal::DateAfterToday { today: today() }),
        ),
    ] {
        let label = format!("a zero on Cash dated {date_box:?}, ticked {ticked}");
        let refusals = box_refusals(
            save(
                &mut tx,
                admin,
                later(),
                cash,
                &form(date_box, "0", ticked, OpeningNamed::New),
            )
            .await,
            &label,
        );
        assert_boxes(
            &refusals,
            date_refusal,
            Some(OpeningBoxRefusal::Zero),
            &label,
        );
    }
    // A zero is a zero whatever sign it carries and whatever the tick: a minus on a zero with the
    // box ticked is refused as a zero, never as two signs (the hand check's H6).
    for amount_box in ["-0", "-0.00", "-$0", "$-0.00"] {
        for ticked in [true, false] {
            let label = format!("{amount_box:?} on Cash, ticked {ticked}");
            let refusals = box_refusals(
                save(
                    &mut tx,
                    admin,
                    later(),
                    cash,
                    &form("2025-12-31", amount_box, ticked, OpeningNamed::New),
                )
                .await,
                &label,
            );
            assert_boxes(&refusals, None, Some(OpeningBoxRefusal::Zero), &label);
        }
    }
    assert_eq!(read(&mut tx, cash).await, None);
    assert_eq!(ledger_rows(&mut tx).await, (0, 0), "a zero wrote a row");

    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let before = row(&mut tx, entry.id).await;
    for (date_box, amount_box, ticked) in [
        ("2025-12-31", "0", false),
        ("2026-01-15", "0.00", false),
        ("", "0", false),
        ("2026-13-01", "$0", true),
        (TOMORROW_TEXT, " -0.00 ", false),
        ("2025-12-31", "-0", true),
        ("2025-12-31", "-$0.00", true),
    ] {
        let label = format!("{date_box:?} and {amount_box:?}, ticked {ticked}");
        let result = save(
            &mut tx,
            admin,
            later(),
            checking,
            &form(date_box, amount_box, ticked, naming(&entry)),
        )
        .await;
        assert!(
            matches!(&result, Ok(OpeningSaved::AskRemove(live)) if *live == entry),
            "{label}: {result:?}"
        );
        assert_eq!(row(&mut tx, entry.id).await, before, "{label} wrote");
    }
    assert_eq!(ledger_rows(&mut tx).await, (1, 2), "a zero wrote a row");
    assert!(
        row(&mut tx, entry.id)
            .await
            .postings
            .iter()
            .all(|posting| posting.amount != Money::ZERO),
        "a zero was stored"
    );
    assert_sound(&mut tx, "the typed zeros").await;
    roll_back(tx, "a typed zero").await;
    storage.close().await;
}

/// The core applies the tick: a typed minus with the box ticked, also after or before the dollar
/// sign, is the two-signs refusal under the amount box with nothing written; a ticked amount, also
/// typed after a space or after the dollar sign, and a typed minus without the tick are saved
/// below zero in natural sign, an overdrawn account's stored below zero and a card in credit's
/// stored above it.
///
/// Proves: Decision 141 with the pause's P3: the core applies the tick; a typed minus with the box ticked, also after the dollar sign, is refused as TwoSigns under the amount box with nothing written; a ticked amount, also typed after a space and after the dollar sign, and a typed minus unticked are saved below zero in natural sign, a card in credit stored positive; a zero stays a zero whatever the tick.
#[tokio::test]
async fn a_minus_and_the_tick_box_are_two_signs() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "two signs").await;
    let opening = opening_account(&mut tx).await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    for amount_box in ["-35.20", "$-35.20", "-$35.20", " -35.20 "] {
        let refusals = box_refusals(
            save(
                &mut tx,
                admin,
                later(),
                checking,
                &form("2025-12-31", amount_box, true, OpeningNamed::New),
            )
            .await,
            amount_box,
        );
        assert_boxes(
            &refusals,
            None,
            Some(OpeningBoxRefusal::TwoSigns),
            amount_box,
        );
    }
    let refusals = box_refusals(
        save(
            &mut tx,
            admin,
            later(),
            checking,
            &form("", "-35.20", true, OpeningNamed::New),
        )
        .await,
        "two signs and no date",
    );
    assert_boxes(
        &refusals,
        Some(OpeningBoxRefusal::Date(DateError::Empty)),
        Some(OpeningBoxRefusal::TwoSigns),
        "two signs and no date",
    );
    assert_eq!(ledger_rows(&mut tx).await, (0, 0), "two signs wrote a row");

    for (account, amount_box, ticked, natural, stored, label) in [
        (
            checking,
            " 35.20",
            true,
            -3_520,
            -3_520,
            "ticked after a space",
        ),
        (savings, "$35.20", true, -3_520, -3_520, "ticked after a $"),
        (
            cash,
            "-35.20",
            false,
            -3_520,
            -3_520,
            "a typed minus unticked",
        ),
        (card, "12.00", true, -1_200, 1_200, "a card in credit"),
    ] {
        let entry = saved(
            save(
                &mut tx,
                admin,
                later(),
                account,
                &form("2025-12-31", amount_box, ticked, OpeningNamed::New),
            )
            .await,
            label,
        );
        assert_eq!(entry.amount, money(natural), "{label}");
        assert_eq!(
            legs(&row(&mut tx, entry.id).await),
            two_legs(account, stored, opening),
            "{label}"
        );
    }
    assert_sound(&mut tx, "the ticked entries").await;
    roll_back(tx, "two signs").await;
    storage.close().await;
}

/// A save naming a version no longer saved, one ahead of it, or no number, or no entry number,
/// whatever its boxes hold (other values, a zero, boxes with problems), is the clash Changed
/// carrying the entry as saved now, judged before the boxes' problems; nothing is written. Save
/// mine, the same boxes naming the live version, goes through.
///
/// Proves: Decision 144 with INV-7, decision 101 and the pause's P2: a save naming a version no longer saved, ahead of it, or no number, with other boxes, is the clash Changed carrying the live one, judged before the boxes' problems, nothing written; Save mine naming the live version goes through.
#[tokio::test]
async fn a_stale_save_is_a_clash_with_what_is_saved_now() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a stale save").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let entry = saved(
        save(
            &mut tx,
            admin,
            now(),
            checking,
            &form("2025-12-31", "1,250.00", false, OpeningNamed::New),
        )
        .await,
        "the entry",
    );
    let first = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-31", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("the first save was refused: {error}"));
    let before = row(&mut tx, entry.id).await;
    let stale = |version: Option<i64>| OpeningNamed::Entry {
        id: Some(entry.id),
        version,
    };
    for (sent, label) in [
        (
            form("2025-12-28", "1,200.00", false, stale(Some(1))),
            "the version the form read",
        ),
        (
            form("2025-12-28", "1,200.00", false, stale(Some(3))),
            "a version ahead",
        ),
        (
            form("2025-12-28", "1,200.00", false, stale(None)),
            "no version number",
        ),
        (
            form(
                "2025-12-28",
                "1,200.00",
                false,
                OpeningNamed::Entry {
                    id: None,
                    version: Some(2),
                },
            ),
            "no entry number",
        ),
        (form("", "abc", true, stale(Some(1))), "boxes with problems"),
        (
            form("2025-12-31", "0", false, stale(Some(1))),
            "a stale zero",
        ),
        (
            form(TOMORROW_TEXT, "1,300.00", false, stale(Some(1))),
            "a stale date after today",
        ),
    ] {
        let found = clash(
            save(&mut tx, admin, stamp(LATER_SECOND + 60), checking, &sent).await,
            label,
        );
        assert_eq!(
            found,
            OpeningClash::Changed {
                account: String::from("Assets:Checking"),
                saved: first,
                digits: MinorDigits::TWO,
            },
            "{label}"
        );
        assert_eq!(found.saved(), Some(&first), "{label}");
        assert_eq!(row(&mut tx, entry.id).await, before, "{label} wrote");
    }
    let mine = saved(
        save(
            &mut tx,
            admin,
            stamp(LATER_SECOND + 60),
            checking,
            &form("2025-12-28", "1,200.00", false, naming(&first)),
        )
        .await,
        "save mine",
    );
    assert_eq!(
        (mine.id, mine.date, mine.amount, mine.version),
        (entry.id, date("2025-12-28"), money(120_000), 3)
    );
    assert_sound(&mut tx, "the stale saves").await;
    roll_back(tx, "a stale save").await;
    storage.close().await;
}

/// A save and a removal are judged against the live opening balance of the account in the
/// address: naming an entry that was removed and entered anew meanwhile (even at the same version
/// number), they meet the new one as Changed and RemoveChanged with its figure and version;
/// naming another account's entry, they do the same while the account has one, and are Removed
/// and nothing to do while it has none, Removed judged before the boxes' problems (a box with a
/// problem is no refusal there). The other account's entry stays as it was and nothing is
/// written.
///
/// Proves: Decision 144 with the pause's P2: a save and a removal naming an entry removed and entered anew meanwhile meet the new one as Changed and RemoveChanged with its figure and version; naming another account's entry they do the same while the account has one, and are Removed and Nothing while it has none; the other account's entry stays as it was and nothing is written.
#[tokio::test]
async fn a_save_or_a_removal_naming_another_entry_meets_the_live_one() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "another entry").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let old = saved(
        save(
            &mut tx,
            admin,
            now(),
            checking,
            &form("2025-12-31", "1,250.00", false, OpeningNamed::New),
        )
        .await,
        "the first entry",
    );
    remove_opening_balance(&mut tx, admin, now(), old.id, old.version)
        .await
        .unwrap_or_else(|error| panic!("cannot remove the first entry: {error}"));
    let anew = open_balance(&mut tx, admin, later(), checking, "2026-01-31", 90_000).await;
    assert_eq!(anew.version, 1);
    let theirs = open_balance(&mut tx, admin, now(), card, "2025-12-31", 80_000).await;
    let before = (
        row(&mut tx, anew.id).await,
        row(&mut tx, theirs.id).await,
        ledger_rows(&mut tx).await,
    );
    let changed_on_checking = OpeningClash::Changed {
        account: String::from("Assets:Checking"),
        saved: anew,
        digits: MinorDigits::TWO,
    };
    let remove_changed_on_checking = OpeningClash::RemoveChanged {
        account: String::from("Assets:Checking"),
        saved: anew,
        digits: MinorDigits::TWO,
    };
    for (named, label) in [
        (
            OpeningNamed::Entry {
                id: Some(old.id),
                version: Some(1),
            },
            "the removed entry at the new one's version",
        ),
        (
            OpeningNamed::Entry {
                id: Some(old.id),
                version: Some(2),
            },
            "the removed entry at its own last version",
        ),
        (naming(&theirs), "the card's entry"),
    ] {
        let found = clash(
            save(
                &mut tx,
                admin,
                later(),
                checking,
                &form("2025-12-30", "1,000.00", false, named),
            )
            .await,
            label,
        );
        assert_eq!(found, changed_on_checking, "the save naming {label}");
        let found = clash(
            remove_opening_balance_form(&mut tx, admin, later(), checking, named).await,
            label,
        );
        assert_eq!(
            found, remove_changed_on_checking,
            "the removal naming {label}"
        );
    }
    for (date_box, amount_box, label) in [
        ("2025-12-30", "1,000.00", "the card's entry on Cash"),
        (
            "",
            "1.000,00",
            "the card's entry on Cash, with a problem in each box",
        ),
        (
            "2025-12-30",
            "abc",
            "the card's entry on Cash, with a problem in the amount box",
        ),
    ] {
        let found = clash(
            save(
                &mut tx,
                admin,
                later(),
                cash,
                &form(date_box, amount_box, false, naming(&theirs)),
            )
            .await,
            label,
        );
        assert_eq!(
            found,
            OpeningClash::Removed {
                account: String::from("Assets:Cash"),
            },
            "{label}: removed meanwhile is judged before the boxes' problems"
        );
        assert_eq!(found.saved(), None, "{label}");
    }
    let result = remove_opening_balance_form(&mut tx, admin, later(), cash, naming(&theirs)).await;
    assert!(
        matches!(result, Ok(OpeningRemoved::Nothing)),
        "the removal on Cash naming the card's entry: {result:?}"
    );
    assert_eq!(
        (
            row(&mut tx, anew.id).await,
            row(&mut tx, theirs.id).await,
            ledger_rows(&mut tx).await,
        ),
        before,
        "something was written"
    );
    assert_eq!(read(&mut tx, card).await, Some(theirs));
    assert_sound(&mut tx, "the entries named elsewhere").await;
    roll_back(tx, "another entry").await;
    storage.close().await;
}

/// A save whose date and amount, the tick applied, equal what is saved writes nothing, whatever
/// entry and version it names or does not name: the enter form sent twice leaves one entry at
/// version 1; a change naming a stale version, no version, no entry or an entry that was replaced
/// by one with the same values, an amount written another way, and a ticked amount equal to an
/// overdrawn account's, all end there.
///
/// Proves: Decision 100 with the pause's P2 and INV-13: the enter form sent twice leaves one entry at version 1, the second save Nothing; a save naming a stale version, no entry or a replaced entry with the saved date and amount, the tick applied, is Nothing; nothing is written.
#[tokio::test]
async fn a_save_of_what_is_saved_writes_nothing_whatever_it_names() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "what is saved").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let enter = form("2025-12-31", "1,250.00", false, OpeningNamed::New);
    let first = saved(
        save(&mut tx, admin, now(), checking, &enter).await,
        "the enter form",
    );
    let result = save(&mut tx, admin, later(), checking, &enter).await;
    assert!(
        matches!(result, Ok(OpeningSaved::Nothing)),
        "the enter form sent twice: {result:?}"
    );
    assert_eq!(ledger_rows(&mut tx).await, (1, 2));
    assert_eq!(read(&mut tx, checking).await, Some(first));
    assert_eq!(first.version, 1);

    old_then_replaced(&mut tx, admin, checking, first).await;

    let overdrawn = saved(
        save(
            &mut tx,
            admin,
            now(),
            savings,
            &form("2026-01-15", "25.00", true, OpeningNamed::New),
        )
        .await,
        "an overdrawn account",
    );
    let before = row(&mut tx, overdrawn.id).await;
    for (amount_box, ticked) in [("25.00", true), ("-25", false), ("$-25.00", false)] {
        let result = save(
            &mut tx,
            admin,
            later(),
            savings,
            &form("2026-01-15", amount_box, ticked, naming(&overdrawn)),
        )
        .await;
        assert!(
            matches!(result, Ok(OpeningSaved::Nothing)),
            "{amount_box:?}, ticked {ticked}: {result:?}"
        );
        assert_eq!(row(&mut tx, overdrawn.id).await, before);
    }
    assert_sound(&mut tx, "the saves of what is saved").await;
    roll_back(tx, "what is saved").await;
    storage.close().await;
}

/// The second half of [`a_save_of_what_is_saved_writes_nothing_whatever_it_names`]: on
/// `checking`, whose live entry `first` holds 1,250.00 on 2025-12-31, every save of those values
/// names something else and writes nothing; then the entry is removed and one with the same
/// values entered anew, and a save naming the removed one with those values writes nothing too.
async fn old_then_replaced(
    tx: &mut WriteTransaction,
    admin: UserId,
    checking: AccountId,
    first: OpeningBalance,
) {
    let before = row(tx, first.id).await;
    for named in [
        OpeningNamed::Entry {
            id: Some(first.id),
            version: Some(7),
        },
        OpeningNamed::Entry {
            id: Some(first.id),
            version: None,
        },
        OpeningNamed::Entry {
            id: None,
            version: None,
        },
        OpeningNamed::New,
    ] {
        for amount_box in ["1,250.00", " $1250 ", "1250.0"] {
            let result = save(
                tx,
                admin,
                later(),
                checking,
                &form(" 2025-12-31", amount_box, false, named),
            )
            .await;
            assert!(
                matches!(result, Ok(OpeningSaved::Nothing)),
                "{named:?} with {amount_box:?}: {result:?}"
            );
            assert_eq!(row(tx, first.id).await, before);
        }
    }
    remove_opening_balance(tx, admin, later(), first.id, first.version)
        .await
        .unwrap_or_else(|error| panic!("cannot remove the entry: {error}"));
    let anew = open_balance(tx, admin, later(), checking, "2025-12-31", 125_000).await;
    let rows = ledger_rows(tx).await;
    let result = save(
        tx,
        admin,
        later(),
        checking,
        &form("2025-12-31", "1,250.00", false, naming(&first)),
    )
    .await;
    assert!(
        matches!(result, Ok(OpeningSaved::Nothing)),
        "a form naming the replaced entry: {result:?}"
    );
    assert_eq!(read(tx, checking).await, Some(anew));
    assert_eq!(ledger_rows(tx).await, rows);
}

/// Two creates at once leave one opening balance: the second enter form, holding other boxes
/// (or boxes with problems), meets the first as the clash Entered carrying it, nothing written;
/// one live opening balance is left, and `nage check` passes over the committed book.
///
/// Proves: INV-13 with decision 144: a second save that enters other boxes meets the first as the clash Entered carrying it, nothing written, one live opening balance left, and nage check passes over the committed book.
#[tokio::test]
async fn two_creates_leave_one_opening_balance() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "two creates").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let first = saved(
        save(
            &mut tx,
            admin,
            now(),
            checking,
            &form("2025-12-31", "1,250.00", false, OpeningNamed::New),
        )
        .await,
        "the first create",
    );
    for (date_box, amount_box) in [
        ("2025-12-31", "1,300.00"),
        ("2025-12-30", "1,250.00"),
        ("", "abc"),
        ("2025-12-31", "0"),
    ] {
        let label = format!("a second create with {date_box:?} and {amount_box:?}");
        let found = clash(
            save(
                &mut tx,
                editor,
                later(),
                checking,
                &form(date_box, amount_box, false, OpeningNamed::New),
            )
            .await,
            &label,
        );
        assert_eq!(
            found,
            OpeningClash::Entered {
                account: String::from("Assets:Checking"),
                saved: first,
                digits: MinorDigits::TWO,
            },
            "{label}"
        );
    }
    assert_eq!(read(&mut tx, checking).await, Some(first));
    assert_eq!(ledger_rows(&mut tx).await, (1, 2), "one opening balance");
    commit(tx, "two creates").await;
    let report = check_report(&storage).await;
    assert!(report.is_sound(), "{:?}", report.violations());
    storage.close().await;
}

/// The confirmation removes the live entry named at its version: the deleted pair, the version
/// raised by one, the postings kept and the entry out of the read. While one is live, a stale,
/// ahead, malformed or missing version or entry is the clash RemoveChanged with the live one and
/// writes nothing; with none live, whatever is named is nothing to do.
///
/// Proves: Decision 143 with INV-7, INV-25 and the pause's P2: the removal of the live entry at its version sets the deleted pair and raises the version by one; while one is live, a stale, malformed or missing version or entry is RemoveChanged with the live one; with none live, whatever is named is Nothing; nothing written but the removal.
#[tokio::test]
async fn the_confirmation_removes_names_its_version_and_finds_nothing_to_do() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the confirmation").await;
    let editor = member(&mut tx, admin, "editor").await;
    let remover = member(&mut tx, admin, "remover").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let result = remove_opening_balance_form(
        &mut tx,
        remover,
        later(),
        cash,
        OpeningNamed::Entry {
            id: Some(TransactionId(1)),
            version: Some(1),
        },
    )
    .await;
    assert!(
        matches!(result, Ok(OpeningRemoved::Nothing)),
        "a removal on an account that never had one: {result:?}"
    );
    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let live = edit_opening_balance(
        &mut tx,
        editor,
        later(),
        today(),
        &edit(entry.id, 1, "2025-12-31", 130_000),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot change the entry: {error}"));
    let before = row(&mut tx, entry.id).await;
    let anything = [
        OpeningNamed::Entry {
            id: Some(entry.id),
            version: Some(1),
        },
        OpeningNamed::Entry {
            id: Some(entry.id),
            version: Some(3),
        },
        OpeningNamed::Entry {
            id: Some(entry.id),
            version: None,
        },
        OpeningNamed::Entry {
            id: None,
            version: Some(2),
        },
        OpeningNamed::Entry {
            id: Some(TransactionId(999)),
            version: Some(2),
        },
        OpeningNamed::New,
    ];
    for named in anything {
        let found = clash(
            remove_opening_balance_form(&mut tx, remover, later(), checking, named).await,
            "a removal naming something else",
        );
        assert_eq!(
            found,
            OpeningClash::RemoveChanged {
                account: String::from("Assets:Checking"),
                saved: live,
                digits: MinorDigits::TWO,
            },
            "{named:?}"
        );
        assert_eq!(row(&mut tx, entry.id).await, before, "{named:?} wrote");
    }

    let after = stamp(LATER_SECOND + 60);
    let result =
        remove_opening_balance_form(&mut tx, remover, after, checking, naming(&live)).await;
    let Ok(OpeningRemoved::Removed(removed)) = result else {
        panic!("the removal of the live entry: {result:?}");
    };
    assert_eq!(removed.id, entry.id);
    assert_eq!(
        (removed.deleted_by, removed.deleted_at),
        (Some(remover), Some(after))
    );
    assert_eq!(removed.version, 3, "the version rose by exactly one");
    assert_eq!((removed.updated_by, removed.updated_at), (editor, later()));
    assert_eq!(removed.postings, before.postings);
    assert_eq!(row(&mut tx, entry.id).await, removed);
    assert_eq!(read(&mut tx, checking).await, None);
    let rows = ledger_rows(&mut tx).await;

    for named in anything.into_iter().chain([naming(&live)]) {
        for account in [checking, cash] {
            let result = remove_opening_balance_form(
                &mut tx,
                remover,
                stamp(LATER_SECOND + 120),
                account,
                named,
            )
            .await;
            assert!(
                matches!(result, Ok(OpeningRemoved::Nothing)),
                "{named:?} on account {account}: {result:?}"
            );
        }
    }
    assert_eq!(
        row(&mut tx, entry.id).await,
        removed,
        "a second removal wrote"
    );
    assert_eq!(ledger_rows(&mut tx).await, rows);
    assert_sound(&mut tx, "the removal").await;
    roll_back(tx, "the confirmation").await;
    storage.close().await;
}

/// A save or a removal on an account archived meanwhile is refused with `ArchivedMeanwhile` at
/// the top, naming the account by its full place, no box judged (boxes with problems included),
/// whatever the form names, with or without an opening balance, a save of what is saved included
/// (the account is judged before nothing to do); nothing is written.
///
/// Proves: Decisions 140 and 145 with INV-12: a save or a removal on an account archived meanwhile is refused with ArchivedMeanwhile at the top, no box judged, nothing written.
#[tokio::test]
async fn an_account_archived_meanwhile_is_refused_at_the_top() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "archived meanwhile").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    archive_account(&mut tx, admin, later(), cash)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Cash: {error}"));
    let first = save(
        &mut tx,
        admin,
        later(),
        cash,
        &form("2025-12-31", "1,250.00", false, OpeningNamed::New),
    )
    .await;
    assert!(
        matches!(
            &first,
            Err(OpeningFormError::Refused(refusals))
                if matches!(&refusals.top, Some(OpeningBalanceError::ArchivedMeanwhile { .. }))
        ),
        "an enter form on an account archived meanwhile: {first:?}"
    );
    let (savings, archived) = archived_savings(&mut tx, admin).await;
    let before = (row(&mut tx, archived.id).await, ledger_rows(&mut tx).await);
    let at_the_top = |refusals: &OpeningRefusals, full_name: &str, label: &str| {
        assert!(
            matches!(
                &refusals.top,
                Some(OpeningBalanceError::ArchivedMeanwhile { account }) if account == full_name
            ),
            "{label}: {refusals:?}"
        );
        assert_eq!(
            (&refusals.date, &refusals.amount),
            (&None, &None),
            "{label}: no box judged"
        );
    };
    for (account, full_name, named) in [
        (savings, "Assets:Savings", naming(&archived)),
        (savings, "Assets:Savings", OpeningNamed::New),
        (cash, "Assets:Cash", OpeningNamed::New),
    ] {
        // 500.00 on 2025-12-31 is what Savings holds saved: the account is judged before nothing
        // to do (decision 140), so even a save of what is saved is refused at the top.
        for (date_box, amount_box) in [
            ("2025-12-30", "600.00"),
            ("", "abc"),
            ("2025-12-31", "0"),
            ("2025-12-31", "500.00"),
        ] {
            let label = format!("a save on {full_name} naming {named:?} with {amount_box:?}");
            let refusals = box_refusals(
                save(
                    &mut tx,
                    admin,
                    later(),
                    account,
                    &form(date_box, amount_box, false, named),
                )
                .await,
                &label,
            );
            at_the_top(&refusals, full_name, &label);
        }
        let label = format!("a removal on {full_name} naming {named:?}");
        let result = remove_opening_balance_form(&mut tx, admin, later(), account, named).await;
        let Err(OpeningFormError::Refused(refusals)) = result else {
            panic!("{label}: {result:?}");
        };
        at_the_top(&refusals, full_name, &label);
    }
    assert_eq!(
        (row(&mut tx, archived.id).await, ledger_rows(&mut tx).await),
        before,
        "something was written"
    );
    assert_sound(&mut tx, "the archived accounts").await;
    roll_back(tx, "archived meanwhile").await;
    storage.close().await;
}

/// The form-shaped save and removal fail, as no refusal, for an account that takes no opening
/// balance: a section, a category, a built-in account and an unknown account, each with the
/// core's own refusal; an archived category fails as a category, that judgement coming before
/// archived; nothing is written.
///
/// Proves: A10 with INV-8 and INV-13 and the hand check's H4: a section, a category, an archived category, a built-in account and an unknown account are Failed for the save and the removal, the archived category as a category before archived, nothing written, the core keeping its refusals for accounts no page reaches.
#[tokio::test]
async fn an_account_that_takes_no_opening_balance_fails_the_form() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "no opening balance").await;
    let assets = account_named(&mut tx, "Assets").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let opening = opening_account(&mut tx).await;
    for (account, label) in [
        (assets, "a section"),
        (groceries, "a category"),
        (opening, "a built-in account"),
        (AccountId(999), "an unknown account"),
    ] {
        let sent = form("2025-12-31", "1,250.00", false, OpeningNamed::New);
        let save_error = failure(save(&mut tx, admin, later(), account, &sent).await, label);
        let remove_error = failure(
            remove_opening_balance_form(
                &mut tx,
                admin,
                later(),
                account,
                OpeningNamed::Entry {
                    id: Some(TransactionId(1)),
                    version: Some(1),
                },
            )
            .await,
            label,
        );
        for error in [&save_error, &remove_error] {
            let expected = match label {
                "a section" => matches!(
                    error,
                    OpeningBalanceError::RootAccount { account } if account == "Assets"
                ),
                "a category" => matches!(
                    error,
                    OpeningBalanceError::NotAssetOrLiability { account, kind: Kind::Expense }
                        if account == "Expenses:Groceries"
                ),
                "a built-in account" => matches!(
                    error,
                    OpeningBalanceError::NotAssetOrLiability { account, kind: Kind::Equity }
                        if account == "Equity:Opening Balances"
                ),
                _ => matches!(
                    error,
                    OpeningBalanceError::NoSuchAccount {
                        account: AccountId(999)
                    }
                ),
            };
            assert!(expected, "{label}: {error:?}");
        }
    }
    // An archived category fails as an account that takes no opening balance: that judgement
    // comes before archived, so it is never refused at the top as archived meanwhile (the hand
    // check's H4).
    archive_account(&mut tx, admin, later(), groceries)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Groceries: {error}"));
    let label = "an archived category";
    let sent = form("2025-12-31", "1,250.00", false, OpeningNamed::New);
    let save_error = failure(save(&mut tx, admin, later(), groceries, &sent).await, label);
    let remove_error = failure(
        remove_opening_balance_form(
            &mut tx,
            admin,
            later(),
            groceries,
            OpeningNamed::Entry {
                id: Some(TransactionId(1)),
                version: Some(1),
            },
        )
        .await,
        label,
    );
    for error in [&save_error, &remove_error] {
        assert!(
            matches!(
                error,
                OpeningBalanceError::NotAssetOrLiability { account, kind: Kind::Expense }
                    if account == "Expenses:Groceries"
            ),
            "{label}: {error:?}"
        );
    }
    assert_eq!(ledger_rows(&mut tx).await, (0, 0), "a failed form wrote");
    roll_back(tx, "no opening balance").await;
    storage.close().await;
}
