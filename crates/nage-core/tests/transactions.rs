//! The general transaction create and the read by id (SPEC 6.4, the Core halves of INV-1, INV-2
//! and INV-26, the posting halves of INV-8 and INV-12, decisions 99, 102 and 104): the row and
//! the postings a create writes, each refusal with what it carries and the proof that it wrote
//! no row, the book that is not set up, the book's minor digits in the refusals, and the
//! messages. Every scenario is built through the core's operations, and `nage check` runs over
//! each of them but the extreme amounts, on which SQLite's own sums can stop (decision 99), and
//! the database without a book.

mod common;

use nage_core::accounts::{self, AccountError, AccountId, NewAccount};
use nage_core::balances;
use nage_core::book::BookError;
use nage_core::date::Date;
use nage_core::money::{MinorDigits, Money};
use nage_core::setup::{Setup, initialize};
use nage_core::test_support::open_migrated;
use nage_core::transactions::{
    self, NewOpeningBalance, OpeningBalanceEdit, OpeningBalanceError, Origin, PostingId,
    TransactionError, TransactionId, create_opening_balance, create_transaction,
    edit_opening_balance, remove_opening_balance,
};
use nage_core::users::UserId;

use common::{
    account_named, assert_sound, begin, check_report, commit, currency, date, later, ledger_rows,
    member, money, new_transaction, now, open_balance, reader, refusal, roll_back, seeded,
    standard_setup, today,
};

/// The postings of a test transaction: each an account and its amount in stored sign as minor
/// units.
type Postings<'a> = &'a [(AccountId, i64)];

/// A balanced set is written as one transaction of origin `manual` with empty payee and memo,
/// version 1, the created and updated pairs naming the acting user (a member, not the admin who
/// set the book up) at the instant and no deleted pair, then its postings in the order given
/// (a split of three is neither sorted by account nor by amount), under ids that rise in that
/// order; the read by id gives the same transaction back, `None` for an unknown id; the first
/// and the last day of `Date`'s range are accepted; the balances hold the postings and the
/// five kinds sum to zero; `nage check` passes inside the write transaction and over the
/// committed ledger, which the read pool reads back equal.
///
/// Proves: SPEC 6.4, INV-1 and INV-2 (Core halves: a balanced set of two or more postings is
/// written whole), INV-4, INV-26 (Core half for transactions), decision 99 (the row, the
/// stamps, the order of the postings, no date rule), decision 104 (check over the scenario).
#[tokio::test]
async fn create_transaction_writes_the_row_its_stamps_and_its_postings_in_order() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the first transactions").await;
    let editor = member(&mut tx, admin, "editor").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let dining = account_named(&mut tx, "Expenses:Dining").await;
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a new book holds no transaction"
    );

    let paycheck = create_transaction(
        &mut tx,
        editor,
        later(),
        &new_transaction("2026-03-01", &[(checking, 300_000), (salary, -300_000)]),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the paycheck: {error}"));
    assert_eq!(paycheck.date, date("2026-03-01"));
    assert_eq!(
        (paycheck.payee.as_str(), paycheck.memo.as_str()),
        ("", ""),
        "payee and memo are the empty string"
    );
    assert_eq!(paycheck.origin, Origin::Manual);
    assert_eq!(paycheck.version, 1);
    assert_eq!(
        (paycheck.created_by, paycheck.created_at),
        (editor, later())
    );
    assert_eq!(
        (paycheck.updated_by, paycheck.updated_at),
        (editor, later())
    );
    assert_eq!((paycheck.deleted_by, paycheck.deleted_at), (None, None));
    assert!(!paycheck.is_deleted());
    assert_eq!(
        paycheck
            .postings
            .iter()
            .map(|posting| (posting.account, posting.amount))
            .collect::<Vec<_>>(),
        [(checking, money(300_000)), (salary, money(-300_000))]
    );

    let split = create_transaction(
        &mut tx,
        admin,
        now(),
        &new_transaction(
            "2026-03-10",
            &[(groceries, 4_500), (card, -10_500), (dining, 6_000)],
        ),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the split: {error}"));
    assert_eq!(
        split
            .postings
            .iter()
            .map(|posting| (posting.account, posting.amount))
            .collect::<Vec<_>>(),
        [
            (groceries, money(4_500)),
            (card, money(-10_500)),
            (dining, money(6_000))
        ],
        "the postings in the order given"
    );
    assert_eq!((split.created_by, split.created_at), (admin, now()));
    assert_eq!((split.updated_by, split.updated_at), (admin, now()));
    assert!(
        split.id > paycheck.id,
        "a later transaction has a higher id"
    );
    let posting_ids: Vec<PostingId> = paycheck
        .postings
        .iter()
        .chain(&split.postings)
        .map(|posting| posting.id)
        .collect();
    assert!(
        posting_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the posting ids do not rise in the order given: {posting_ids:?}"
    );

    for (id, written) in [(paycheck.id, &paycheck), (split.id, &split)] {
        let read = transactions::transaction(&mut tx, id)
            .await
            .unwrap_or_else(|error| panic!("cannot read transaction {id}: {error}"));
        assert_eq!(read.as_ref(), Some(written), "transaction {id} read back");
    }
    let unknown = transactions::transaction(&mut tx, TransactionId(999))
        .await
        .unwrap_or_else(|error| panic!("cannot read an unknown id: {error}"));
    assert_eq!(unknown, None);

    for end in [Date::MIN, Date::MAX] {
        let dated = create_transaction(
            &mut tx,
            admin,
            now(),
            &new_transaction(&end.to_string(), &[(checking, 1), (salary, -1)]),
        )
        .await
        .unwrap_or_else(|error| panic!("a transaction dated {end} was refused: {error}"));
        assert_eq!(dated.date, end);
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        (4, 9),
        "four transactions and their nine postings"
    );

    let all = balances::balances_as_of(&mut tx, date("2026-03-31"))
        .await
        .unwrap_or_else(|error| panic!("cannot compute the balances: {error}"));
    let own = |account: AccountId| {
        all.accounts
            .iter()
            .find(|balance| balance.account.id == account)
            .unwrap_or_else(|| panic!("no balance for account {account}"))
            .own
    };
    assert_eq!(
        own(checking),
        money(300_001),
        "the paycheck and the transaction dated 1900-01-01; the one dated 2199-12-31 is later"
    );
    assert_eq!(own(salary), money(-300_001));
    assert_eq!(own(card), money(-10_500));
    assert_eq!(own(groceries), money(4_500));
    assert_eq!(own(dining), money(6_000));
    assert_eq!(
        Money::checked_sum(all.kinds.iter().map(|kind| kind.stored)),
        Some(money(0)),
        "INV-4"
    );
    assert_sound(&mut tx, "the first transactions").await;
    commit(tx, "the first transactions").await;

    let mut conn = reader(&storage).await;
    let committed = transactions::transaction(&mut conn, split.id)
        .await
        .unwrap_or_else(|error| panic!("cannot read the committed split: {error}"));
    assert_eq!(committed, Some(split), "the split through the read pool");
    drop(conn);
    let report = check_report(&storage).await;
    assert!(
        report.is_sound(),
        "check over the committed ledger: {:?}",
        report.violations()
    );
    storage.close().await;
}

/// A set of no postings and a set of one are refused with the number given; a posting whose
/// amount is zero is refused naming its account by its full name, whether it comes first or
/// last in an otherwise balanced set; none of the refused calls writes a row, and the next
/// transaction is written as if they had never been made.
///
/// Proves: INV-2 (Core half: at least two postings, no zero amount), decision 99 (a refusal
/// is decided before the first write).
#[tokio::test]
async fn create_transaction_refuses_fewer_than_two_postings_and_zero_amounts() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "too few and zero").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;

    let error = refusal(
        create_transaction(&mut tx, admin, now(), &new_transaction("2026-03-01", &[])).await,
        "a transaction without postings",
    );
    assert!(
        matches!(error, TransactionError::TooFewPostings { given: 0 }),
        "no postings: {error:?}"
    );
    let error = refusal(
        create_transaction(
            &mut tx,
            admin,
            now(),
            &new_transaction("2026-03-01", &[(checking, 100)]),
        )
        .await,
        "a transaction with one posting",
    );
    assert!(
        matches!(error, TransactionError::TooFewPostings { given: 1 }),
        "one posting: {error:?}"
    );

    for (postings, account) in [
        (
            [(checking, 0), (salary, -100), (groceries, 100)],
            "Assets:Checking",
        ),
        (
            [(checking, -100), (salary, 100), (groceries, 0)],
            "Expenses:Groceries",
        ),
    ] {
        let error = refusal(
            create_transaction(
                &mut tx,
                admin,
                now(),
                &new_transaction("2026-03-01", &postings),
            )
            .await,
            "a zero amount",
        );
        assert!(
            matches!(&error, TransactionError::ZeroAmount { account: named } if named == account),
            "a zero amount on {account}: {error:?}"
        );
    }
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a refused call wrote a row"
    );

    let accepted = create_transaction(
        &mut tx,
        admin,
        now(),
        &new_transaction("2026-03-01", &[(checking, 100), (salary, -100)]),
    )
    .await
    .unwrap_or_else(|error| panic!("the balanced pair was refused: {error}"));
    assert_eq!(accepted.postings.len(), 2);
    assert_eq!(ledger_rows(&mut tx).await, (1, 2));
    assert_sound(&mut tx, "the refused sets and the accepted pair").await;
    roll_back(tx, "too few and zero").await;
    storage.close().await;
}

/// Postings that do not sum to zero are refused with their sum in stored sign and the book's
/// minor digits: a pair that is off in either direction, a split one unit short, and a set
/// whose running total leaves 64 bits on the way to a sum that fits; a sum that itself does
/// not fit in 64 bits is refused with no sum; a sum of exactly the most negative amount is
/// carried as it is, and its message writes the difference between the two sides exactly,
/// without the sign; none of them writes a row. The sum is exact whatever the order: sets
/// that balance only beyond 64 bits of running total (the largest amount, 1 and the most
/// negative amount in that order; twice the largest and twice the most negative amount with
/// 2) are accepted and stored as given.
///
/// Proves: INV-1 (Core half: the postings sum to zero), SPEC 6.10 and decision 85 (the exact
/// sum), decision 99 (the difference carried, no row written, the sum whatever the order),
/// decision 102 (the most negative sum's difference written exactly, without the sign).
#[tokio::test]
async fn create_transaction_refuses_postings_that_do_not_sum_to_zero() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "unbalanced").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let other = account_named(&mut tx, "Income:Other").await;

    let unbalanced: [(Postings, Option<i64>); 6] = [
        (&[(checking, 100), (salary, -50)], Some(50)),
        (&[(checking, 100), (salary, -150)], Some(-50)),
        (
            &[(checking, 300_000), (salary, -250_000), (other, -49_999)],
            Some(1),
        ),
        (
            &[
                (checking, i64::MAX),
                (savings, i64::MAX),
                (salary, i64::MIN),
            ],
            Some(i64::MAX - 1),
        ),
        (&[(checking, i64::MAX), (savings, i64::MAX)], None),
        (&[(salary, i64::MIN), (other, i64::MIN)], None),
    ];
    for (postings, sum) in unbalanced {
        let error = refusal(
            create_transaction(
                &mut tx,
                admin,
                now(),
                &new_transaction("2026-03-01", postings),
            )
            .await,
            "an unbalanced set",
        );
        assert!(
            matches!(
                &error,
                TransactionError::Unbalanced { sum: carried, digits }
                    if *carried == sum.map(money) && *digits == MinorDigits::TWO
            ),
            "the refusal of {postings:?} does not carry {sum:?}: {error:?}"
        );
    }
    let error = refusal(
        create_transaction(
            &mut tx,
            admin,
            now(),
            &new_transaction("2026-03-01", &[(salary, -i64::MAX), (other, -1)]),
        )
        .await,
        "a set whose sum is the most negative amount",
    );
    assert!(
        matches!(
            &error,
            TransactionError::Unbalanced { sum, digits }
                if *sum == Some(money(i64::MIN)) && *digits == MinorDigits::TWO
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by 92,233,720,368,547,758.08. Change the amounts until both sides are equal, then save again.",
        "the difference of the most negative sum is not written exactly"
    );
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a refused call wrote a row"
    );
    assert_sound(&mut tx, "the refused unbalanced sets").await;

    let balanced: [Postings; 2] = [
        &[(checking, i64::MAX), (salary, 1), (card, i64::MIN)],
        &[
            (checking, i64::MAX),
            (savings, i64::MAX),
            (salary, i64::MIN),
            (other, i64::MIN),
            (card, 2),
        ],
    ];
    for postings in balanced {
        let written = create_transaction(
            &mut tx,
            admin,
            now(),
            &new_transaction("2026-03-01", postings),
        )
        .await
        .unwrap_or_else(|error| panic!("the balanced set {postings:?} was refused: {error}"));
        assert_eq!(
            written
                .postings
                .iter()
                .map(|posting| (posting.account, posting.amount.minor()))
                .collect::<Vec<_>>(),
            postings,
            "the extreme amounts as given"
        );
    }
    assert_eq!(ledger_rows(&mut tx).await, (2, 8));
    roll_back(tx, "unbalanced").await;
    storage.close().await;
}

/// A posting on an account id that no account has is refused as a refusal that carries the
/// id, never as the foreign key's database error, in the first position or the second; a
/// posting on a root is refused naming the root; a posting on an archived account is refused
/// naming it by its full name, in the first position or the second, and accepted once the
/// account is unarchived, as the message advises; none of the refused calls writes a row.
///
/// Proves: INV-8 (the posting half: no posting on a root), INV-12 (the posting half: an
/// archived account takes no new posting), decision 99 (an unknown account is a refusal; no
/// row written).
#[tokio::test]
async fn create_transaction_refuses_unknown_root_and_archived_accounts() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "accounts refused").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let assets = account_named(&mut tx, "Assets").await;
    let income = account_named(&mut tx, "Income").await;

    for postings in [
        [(AccountId(999), 100), (salary, -100)],
        [(checking, 100), (AccountId(999), -100)],
    ] {
        let error = refusal(
            create_transaction(
                &mut tx,
                admin,
                now(),
                &new_transaction("2026-03-01", &postings),
            )
            .await,
            "an unknown account",
        );
        assert!(
            matches!(
                error,
                TransactionError::NoSuchAccount {
                    account: AccountId(999)
                }
            ),
            "an unknown account in {postings:?}: {error:?}"
        );
    }

    for (postings, root) in [
        ([(assets, 100), (salary, -100)], "Assets"),
        ([(checking, 100), (income, -100)], "Income"),
    ] {
        let error = refusal(
            create_transaction(
                &mut tx,
                admin,
                now(),
                &new_transaction("2026-03-01", &postings),
            )
            .await,
            "a posting on a root",
        );
        assert!(
            matches!(&error, TransactionError::RootAccount { account } if account == root),
            "a posting on {root}: {error:?}"
        );
    }

    let old = accounts::create_account(
        &mut tx,
        admin,
        now(),
        &NewAccount {
            name: String::from("Old"),
            parent: savings,
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create Old: {error}"))
    .id;
    accounts::archive_account(&mut tx, admin, later(), old)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Old: {error}"));
    let on_archived = new_transaction("2026-03-01", &[(old, 100), (salary, -100)]);
    let error = refusal(
        create_transaction(&mut tx, admin, later(), &on_archived).await,
        "a posting on an archived account",
    );
    assert!(
        matches!(
            &error,
            TransactionError::ArchivedAccount { account } if account == "Assets:Savings:Old"
        ),
        "a posting on an archived account: {error:?}"
    );
    let error = refusal(
        create_transaction(
            &mut tx,
            admin,
            later(),
            &new_transaction("2026-03-01", &[(salary, -100), (old, 100)]),
        )
        .await,
        "a posting on an archived account in the second position",
    );
    assert!(
        matches!(
            &error,
            TransactionError::ArchivedAccount { account } if account == "Assets:Savings:Old"
        ),
        "an archived account in the second position: {error:?}"
    );
    assert_eq!(
        ledger_rows(&mut tx).await,
        (0, 0),
        "a refused call wrote a row"
    );

    accounts::unarchive_account(&mut tx, admin, later(), old)
        .await
        .unwrap_or_else(|error| panic!("cannot unarchive Old: {error}"));
    create_transaction(&mut tx, admin, later(), &on_archived)
        .await
        .unwrap_or_else(|error| panic!("the unarchived account was refused: {error}"));
    assert_eq!(ledger_rows(&mut tx).await, (1, 2));
    assert_sound(&mut tx, "the refused accounts and the unarchived one").await;
    roll_back(tx, "accounts refused").await;
    storage.close().await;
}

/// On a migrated database without a book row, each of the four writes is refused as not set
/// up and writes nothing, the general create also when it is given no postings at all (the
/// book comes before the count rule), and the two reads answer `None`.
///
/// Proves: decision 102 (every write reads the book's minor digits before it builds a
/// refusal, so a book that is not set up is refused first, before the count rule too),
/// decision 99 (no row written).
#[tokio::test]
async fn the_writes_refuse_a_book_that_is_not_set_up() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "no book").await;
    let nobody = UserId(1);
    let error = refusal(
        create_transaction(
            &mut tx,
            nobody,
            now(),
            &new_transaction("2026-03-01", &[(AccountId(1), 100), (AccountId(2), -100)]),
        )
        .await,
        "a transaction without a book",
    );
    assert!(
        matches!(error, TransactionError::NotInitialized),
        "the general create: {error:?}"
    );
    let error = refusal(
        create_transaction(&mut tx, nobody, now(), &new_transaction("2026-03-01", &[])).await,
        "a transaction without postings and without a book",
    );
    assert!(
        matches!(error, TransactionError::NotInitialized),
        "the general create with no postings: {error:?}"
    );
    let error = refusal(
        create_opening_balance(
            &mut tx,
            nobody,
            now(),
            today(),
            &NewOpeningBalance {
                account: AccountId(1),
                date: date("2025-12-31"),
                amount: money(100),
            },
        )
        .await,
        "an opening balance without a book",
    );
    assert!(
        matches!(error, OpeningBalanceError::NotInitialized),
        "the opening-balance create: {error:?}"
    );
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            nobody,
            now(),
            today(),
            &OpeningBalanceEdit {
                id: TransactionId(1),
                expected_version: 1,
                date: date("2025-12-31"),
                amount: money(100),
            },
        )
        .await,
        "an edit without a book",
    );
    assert!(
        matches!(error, OpeningBalanceError::NotInitialized),
        "the edit: {error:?}"
    );
    let error = refusal(
        remove_opening_balance(&mut tx, nobody, now(), TransactionId(1), 1).await,
        "a removal without a book",
    );
    assert!(
        matches!(error, OpeningBalanceError::NotInitialized),
        "the removal: {error:?}"
    );
    let read = transactions::transaction(&mut tx, TransactionId(1))
        .await
        .unwrap_or_else(|error| panic!("cannot read on an empty database: {error}"));
    assert_eq!(read, None);
    let read = transactions::opening_balance(&mut tx, AccountId(1))
        .await
        .unwrap_or_else(|error| panic!("cannot read on an empty database: {error}"));
    assert_eq!(read, None);
    assert_eq!(ledger_rows(&mut tx).await, (0, 0));
    roll_back(tx, "no book").await;
    storage.close().await;
}

/// In a book whose currency has no minor unit (JPY), the refusals carry that book's digits
/// and their messages write the amounts without decimals: the difference of an unbalanced
/// set, the range of amounts the book can hold when the create or the edit is given an amount
/// too large to store, the existing opening balance a second one is refused with, and the
/// saved entry of a stale edit and of a stale removal.
///
/// Proves: decision 102 (amounts in messages with the book's minor digits), SPEC 6.10 (the
/// display form).
#[tokio::test]
async fn refusals_name_amounts_with_the_books_minor_digits() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a yen book").await;
    let setup = Setup {
        currency: currency("JPY"),
        ..standard_setup(true)
    };
    let admin = initialize(&mut tx, now(), &setup)
        .await
        .unwrap_or_else(|error| panic!("cannot set up the yen book: {error}"))
        .admin
        .id;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let salary = account_named(&mut tx, "Income:Salary").await;

    let error = refusal(
        create_transaction(
            &mut tx,
            admin,
            now(),
            &new_transaction("2026-03-01", &[(checking, 1_500), (salary, -1_000)]),
        )
        .await,
        "an unbalanced set in yen",
    );
    assert!(
        matches!(
            error,
            TransactionError::Unbalanced {
                sum: Some(_),
                digits: MinorDigits::ZERO
            }
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("the two sides differ by 500. "),
        "the difference is not written in yen: {error}"
    );

    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            now(),
            today(),
            &NewOpeningBalance {
                account: checking,
                date: date("2025-12-31"),
                amount: money(i64::MIN),
            },
        )
        .await,
        "an amount too large to store in yen",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::TooLarge {
                digits: MinorDigits::ZERO,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "The opening balance of Assets:Checking was not saved because the amount is beyond what Nage can store: it holds amounts from -9,223,372,036,854,775,807 to 9,223,372,036,854,775,807. Check the amount for a typing mistake and enter one within that range.",
        "the range is not written in yen"
    );

    let entry = open_balance(&mut tx, admin, now(), checking, "2025-12-31", 125_000).await;
    let error = refusal(
        create_opening_balance(
            &mut tx,
            admin,
            now(),
            today(),
            &NewOpeningBalance {
                account: checking,
                date: date("2026-01-31"),
                amount: money(90_000),
            },
        )
        .await,
        "a second opening balance in yen",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::AlreadyExists {
                digits: MinorDigits::ZERO,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("already has one, 125,000 on 2025-12-31,"),
        "the existing amount is not written in yen: {error}"
    );

    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &OpeningBalanceEdit {
                id: entry.id,
                expected_version: 4,
                date: date("2025-12-31"),
                amount: money(130_000),
            },
        )
        .await,
        "a stale edit in yen",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::Stale {
                digits: MinorDigits::ZERO,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("it is now 125,000 on 2025-12-31."),
        "the saved amount is not written in yen: {error}"
    );
    let error = refusal(
        edit_opening_balance(
            &mut tx,
            admin,
            later(),
            today(),
            &OpeningBalanceEdit {
                id: entry.id,
                expected_version: entry.version,
                date: date("2025-12-31"),
                amount: money(i64::MIN),
            },
        )
        .await,
        "an edit to an amount too large to store in yen",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::TooLarge {
                digits: MinorDigits::ZERO,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("from -9,223,372,036,854,775,807 to 9,223,372,036,854,775,807."),
        "the range of an edit is not written in yen: {error}"
    );
    let error = refusal(
        remove_opening_balance(&mut tx, admin, later(), entry.id, 4).await,
        "a stale removal in yen",
    );
    assert!(
        matches!(
            error,
            OpeningBalanceError::Stale {
                digits: MinorDigits::ZERO,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("it is now 125,000 on 2025-12-31."),
        "the saved amount of a stale removal is not written in yen: {error}"
    );
    assert_sound(&mut tx, "the yen book").await;
    roll_back(tx, "a yen book").await;
    storage.close().await;
}

/// Every refusal of the general create says what was refused, why, and what to do instead, in
/// plain words, naming the account and writing the sum as SPEC 6.10 displays it; no two
/// refusals share a message; the failures of the chart, of the book row and of the database
/// pass the inner error's message on whole, a database failure named as one. The test prints
/// each moment with its message
/// (`cargo test -p nage-core --test transactions transaction_refusals -- --nocapture` shows
/// them).
///
/// Proves: decision 102 (every message of the general create, in its three parts; a failure
/// of the database, of the book row or of the chart passes the inner error's message on).
#[test]
fn transaction_refusals_explain_themselves() {
    let cases: [(&str, &str, TransactionError, &str); 10] = [
        (
            "NotInitialized",
            "A transaction is saved before the book has been set up.",
            TransactionError::NotInitialized,
            "The transaction was not saved because this book is not set up yet. Finish the setup first, then try again.",
        ),
        (
            "TooFewPostings",
            "A transaction is saved with no lines at all.",
            TransactionError::TooFewPostings { given: 0 },
            "The transaction was not saved because it has no lines, and a transaction moves money between at least two accounts. Give it at least two lines, each with an account and an amount, then save again.",
        ),
        (
            "TooFewPostings",
            "A transaction is saved with a single line, 45.00 on Expenses:Groceries.",
            TransactionError::TooFewPostings { given: 1 },
            "The transaction was not saved because it has only one line, and a transaction moves money between at least two accounts. Give it at least two lines, each with an account and an amount, then save again.",
        ),
        (
            "NoSuchAccount",
            "A line of a transaction names account number 999, which no account has.",
            TransactionError::NoSuchAccount {
                account: AccountId(999),
            },
            "The transaction was not saved because one of its lines names an account that does not exist (account number 999). Choose an account from the chart of accounts for that line, then save again.",
        ),
        (
            "RootAccount",
            "A line of a transaction is put on Assets itself, not on an account under it.",
            TransactionError::RootAccount {
                account: String::from("Assets"),
            },
            "The transaction was not saved because one of its lines is on Assets, one of the five top-level accounts, which only group the accounts under them and cannot be used on a line. Choose an account under Assets for that line, then save again.",
        ),
        (
            "ArchivedAccount",
            "A line of a transaction is put on Expenses:Gifts, which has been archived.",
            TransactionError::ArchivedAccount {
                account: String::from("Expenses:Gifts"),
            },
            "The transaction was not saved because one of its lines is on Expenses:Gifts, which is archived, and an archived account cannot be used on a new line. Unarchive Expenses:Gifts first, or choose another account for that line, then save again.",
        ),
        (
            "ZeroAmount",
            "A transaction's line on Assets:Checking is left at 0.00.",
            TransactionError::ZeroAmount {
                account: String::from("Assets:Checking"),
            },
            "The transaction was not saved because the amount of its line on Assets:Checking is zero, and every line must move some money. Enter an amount other than zero for that line, or leave the line out, then save again.",
        ),
        (
            "Unbalanced",
            "A transaction takes 300.00 out of Assets:Checking and puts only 250.00 on Expenses:Groceries.",
            TransactionError::Unbalanced {
                sum: Some(money(-5_000)),
                digits: MinorDigits::TWO,
            },
            "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by 50.00. Change the amounts until both sides are equal, then save again.",
        ),
        (
            "Unbalanced",
            "In a book kept in a currency with three decimals, a transaction's lines add up to 1,234.567.",
            TransactionError::Unbalanced {
                sum: Some(money(1_234_567)),
                digits: MinorDigits::THREE,
            },
            "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by 1,234.567. Change the amounts until both sides are equal, then save again.",
        ),
        (
            "Unbalanced",
            "A transaction's amounts are so large that their sum is beyond what Nage can count.",
            TransactionError::Unbalanced {
                sum: None,
                digits: MinorDigits::TWO,
            },
            "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by more than Nage can count. Check the amounts for a typing mistake, then save again.",
        ),
    ];
    let mut messages = Vec::new();
    for (variant, moment, error, expected) in &cases {
        let message = error.to_string();
        println!("[TransactionError::{variant}] {moment} => {message}");
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
        TransactionError::Accounts(AccountError::NoSuchAccount).to_string(),
        AccountError::NoSuchAccount.to_string(),
        "a failure of the chart passes its message on"
    );
    assert_eq!(
        TransactionError::Book(BookError::Corrupt(String::from("minor_digits is 7"))).to_string(),
        "the book row is corrupt: minor_digits is 7",
        "a failure of the book row passes its message on"
    );
    let database = TransactionError::Database(sqlx::Error::RowNotFound).to_string();
    assert_eq!(
        database,
        format!("database error: {}", sqlx::Error::RowNotFound),
        "a database failure does not pass its message on"
    );
}

/// The four origins print and parse as the texts of schema 0002's CHECK, in SPEC 6.4's order,
/// and another text is refused naming it; the ledger's ids print as their numbers.
///
/// Proves: SPEC 6.4 (the four origins), decision 92 (the spellings); the ids print as their
/// numbers.
#[test]
fn origins_and_ledger_ids_print_as_stored() {
    assert_eq!(
        Origin::ALL.map(Origin::as_str),
        ["manual", "import", "opening_balance", "balance_update"]
    );
    for origin in Origin::ALL {
        assert_eq!(origin.to_string(), origin.as_str());
        assert_eq!(origin.as_str().parse::<Origin>(), Ok(origin));
    }
    let unknown = match "opening balance".parse::<Origin>() {
        Ok(origin) => panic!("a text with a space parsed as {origin}"),
        Err(error) => error,
    };
    assert_eq!(
        unknown.to_string(),
        "unknown transaction origin \"opening balance\""
    );
    assert!(
        "Manual".parse::<Origin>().is_err(),
        "the capitalized word is not an origin"
    );
    assert_eq!(TransactionId(12).to_string(), "12");
    assert_eq!(PostingId(34).to_string(), "34");
}
