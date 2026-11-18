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
use nage_core::money::{MinorDigits, Money};
use nage_core::storage::WriteTransaction;
use nage_core::transactions::{
    self, NewOpeningBalance, OpeningBalance, OpeningBalanceEdit, OpeningBalanceError, Origin,
    Transaction, TransactionId, create_opening_balance, edit_opening_balance,
    remove_opening_balance,
};
use nage_core::users::UserId;

use common::{
    LATER_SECOND, NOW_TEXT, TOMORROW_TEXT, account_named, assert_sound, begin, check_report,
    commit, date, insert_transaction, later, ledger_rows, member, money, now, open_balance, post,
    refusal, roll_back, seeded, stamp, today,
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
        (checking, i64::MAX, i64::MAX),
        (savings, -i64::MAX, -i64::MAX),
        (card, i64::MAX, -i64::MAX),
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
/// an edit or a removal naming the old id is refused as not found, whatever version it names.
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
                OpeningBalanceError::NoSuchOpeningBalance { id } if *id == entry.id
            ),
            "a removal naming the removed entry at version {expected_version}: {error:?}"
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
/// transaction of another origin, which stays live); a version other than the stored one,
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
                OpeningBalanceError::Stale { account, saved: carried, digits }
                    if account == "Assets:Checking" && *carried == saved
                        && *digits == MinorDigits::TWO
            ),
            "a removal naming version {expected_version}: {error:?}"
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
            OpeningBalanceError::Stale { account, saved, .. }
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
    let planted = insert_transaction(
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
    assert!(
        matches!(error, OpeningBalanceError::Corrupt(_)),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        format!(
            "The opening balance could not be read because what is stored for it is damaged: entry number {planted} holds an amount Nage cannot show. Ask whoever looks after Nage to run `nage check` and to restore a backup if it reports a problem."
        )
    );
    roll_back(tx, "a planted amount").await;
    storage.close().await;
}

/// Every refusal of the opening-balance operations says what was refused, why, and what to do
/// instead, in plain words, naming the account by its full name, the amounts in natural sign
/// as SPEC 6.10 displays them and the dates involved; no two refusals share a message; the
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
    let cases: [(&str, &str, OpeningBalanceError, &str); 16] = [
        (
            "NotInitialized",
            "An opening balance is saved before the book has been set up.",
            OpeningBalanceError::NotInitialized,
            "The opening balance was not changed because this book is not set up yet. Finish the setup first, then try again.",
        ),
        (
            "NoSuchAccount",
            "An opening balance is saved for account number 999, which no account has.",
            OpeningBalanceError::NoSuchAccount {
                account: AccountId(999),
            },
            "The opening balance was not saved because its account does not exist (account number 999). Open the account from the chart of accounts and enter the opening balance there.",
        ),
        (
            "NotAssetOrLiability",
            "An opening balance of 45.00 is entered for the expense category Expenses:Groceries.",
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Expenses:Groceries"),
                kind: Kind::Expense,
            },
            "The opening balance was not saved because Expenses:Groceries is an account under Expenses, and only accounts under Assets or Liabilities, which hold what the household has and what it owes, start with a balance. Leave Expenses:Groceries without an opening balance: income, expense and equity accounts start at zero.",
        ),
        (
            "NotAssetOrLiability",
            "An opening balance is entered for Income:Salary.",
            OpeningBalanceError::NotAssetOrLiability {
                account: String::from("Income:Salary"),
                kind: Kind::Income,
            },
            "The opening balance was not saved because Income:Salary is an account under Income, and only accounts under Assets or Liabilities, which hold what the household has and what it owes, start with a balance. Leave Income:Salary without an opening balance: income, expense and equity accounts start at zero.",
        ),
        (
            "RootAccount",
            "An opening balance is entered for Liabilities itself, not for an account under it.",
            OpeningBalanceError::RootAccount {
                account: String::from("Liabilities"),
            },
            "The opening balance was not saved because Liabilities is one of the five top-level accounts, which only group the accounts under them and hold nothing of their own. Enter the opening balance on an account under Liabilities instead.",
        ),
        (
            "ArchivedAccount",
            "The opening balance of Assets:Savings is entered, changed or removed after the account was archived.",
            OpeningBalanceError::ArchivedAccount {
                account: String::from("Assets:Savings"),
            },
            "The opening balance of Assets:Savings was not changed because the account is archived, and an archived account's opening balance cannot be entered, edited or removed. Unarchive Assets:Savings first, then try again.",
        ),
        (
            "ZeroAmount",
            "The opening balance of Assets:Checking is saved with an amount of 0.00.",
            OpeningBalanceError::ZeroAmount {
                account: String::from("Assets:Checking"),
            },
            "The opening balance of Assets:Checking was not saved because the amount is zero, and an account that starts at zero needs no opening balance. Enter the amount the account started with; if it started at zero, leave it without an opening balance, and remove the one it has, if any.",
        ),
        (
            "AlreadyExists",
            "An opening balance is entered for Assets:Checking, which already has one of 1,250.00 dated 2025-12-31.",
            OpeningBalanceError::AlreadyExists {
                account: String::from("Assets:Checking"),
                existing: saved("2025-12-31", 125_000, 1),
                digits: MinorDigits::TWO,
            },
            "The opening balance was not saved because Assets:Checking already has one, 1,250.00 on 2025-12-31, and an account has only one opening balance. Change that one instead, or remove it first.",
        ),
        (
            "AlreadyExists",
            "An opening balance is entered for Liabilities:Credit Card, which already has one of 800.00 owed, dated 2025-12-31.",
            OpeningBalanceError::AlreadyExists {
                account: String::from("Liabilities:Credit Card"),
                existing: saved("2025-12-31", 80_000, 1),
                digits: MinorDigits::TWO,
            },
            "The opening balance was not saved because Liabilities:Credit Card already has one, 800.00 on 2025-12-31, and an account has only one opening balance. Change that one instead, or remove it first.",
        ),
        (
            "DateAfterToday",
            "On 2026-11-09 the opening balance of Assets:Checking is saved with the date 2026-11-10.",
            OpeningBalanceError::DateAfterToday {
                account: String::from("Assets:Checking"),
                date: date("2026-11-10"),
                today: date("2026-11-09"),
            },
            "The opening balance of Assets:Checking was not saved because its date, 2026-11-10, is after today, 2026-11-09: an opening balance is what the account held at the end of its day, and that day cannot lie in the future. Enter today's date or an earlier one.",
        ),
        (
            "TooLarge",
            "The opening balance of Assets:Checking is saved with an amount of -92,233,720,368,547,758.08, which is beyond what Nage can store.",
            OpeningBalanceError::TooLarge {
                account: String::from("Assets:Checking"),
                digits: MinorDigits::TWO,
            },
            "The opening balance of Assets:Checking was not saved because the amount is beyond what Nage can store: it holds amounts from -92,233,720,368,547,758.07 to 92,233,720,368,547,758.07. Check the amount for a typing mistake and enter one within that range.",
        ),
        (
            "NoSuchOpeningBalance",
            "A form still open on an opening balance is saved after another user removed that opening balance.",
            OpeningBalanceError::NoSuchOpeningBalance {
                id: TransactionId(12),
            },
            "The opening balance was not changed because it is no longer there (entry number 12): it may have been removed while this page was open. Open the account again to see what is saved now.",
        ),
        (
            "Stale",
            "Two people have the opening balance of Assets:Checking open; the first saves 1,300.00 dated 2025-12-30, then the second saves.",
            OpeningBalanceError::Stale {
                account: String::from("Assets:Checking"),
                saved: saved("2025-12-30", 130_000, 2),
                digits: MinorDigits::TWO,
            },
            "Your change to the opening balance of Assets:Checking was not saved because the opening balance was changed while you were working, by another user or in another window: it is now 1,300.00 on 2025-12-30. Look at what is saved now, then make your change again or keep what is saved.",
        ),
        (
            "Stale",
            "The same, in a book kept in a currency with three decimals, for an overdrawn account that now starts at -1.500.",
            OpeningBalanceError::Stale {
                account: String::from("Assets:Checking"),
                saved: saved("2025-12-30", -1_500, 2),
                digits: MinorDigits::THREE,
            },
            "Your change to the opening balance of Assets:Checking was not saved because the opening balance was changed while you were working, by another user or in another window: it is now -1.500 on 2025-12-30. Look at what is saved now, then make your change again or keep what is saved.",
        ),
        (
            "Stale",
            "The same, in a book kept in a currency without decimals, for a card that now owes 80,000.",
            OpeningBalanceError::Stale {
                account: String::from("Liabilities:Credit Card"),
                saved: saved("2025-12-31", 80_000, 3),
                digits: MinorDigits::ZERO,
            },
            "Your change to the opening balance of Liabilities:Credit Card was not saved because the opening balance was changed while you were working, by another user or in another window: it is now 80,000 on 2025-12-31. Look at what is saved now, then make your change again or keep what is saved.",
        ),
        (
            "Corrupt",
            "The database was changed outside Nage and holds an opening balance that cannot be shown.",
            OpeningBalanceError::Corrupt(String::from(
                "entry number 12 holds an amount Nage cannot show",
            )),
            "The opening balance could not be read because what is stored for it is damaged: entry number 12 holds an amount Nage cannot show. Ask whoever looks after Nage to run `nage check` and to restore a backup if it reports a problem.",
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
