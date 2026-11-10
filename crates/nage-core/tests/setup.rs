//! Setup's core half (SPEC W1 step 3, INV-5's Core half, INV-8's tree half, INV-11's Core half,
//! decision 96) and the user and book operations beneath it (SPEC 5.1, INV-26's Core half).

mod common;

use nage_core::accounts::{self, Kind, SystemKey};
use nage_core::book::{self, Book, BookError};
use nage_core::fold::NameError;
use nage_core::money::MinorDigits;
use nage_core::setup::{self, Setup, SetupError};
use nage_core::test_support::open_migrated;
use nage_core::users::{self, NewUser, UserError, UserId};
use sqlx::AssertSqlSafe;

use common::{
    LATER_TEXT, begin, commit, corrupt, currency, later, now, reader, roll_back, seeded,
    standard_setup,
};

/// A non-admin user with the opaque hash `hash`.
fn member(username: &str, display_name: &str) -> NewUser {
    NewUser {
        username: username.to_owned(),
        display_name: display_name.to_owned(),
        password_hash: String::from("hash"),
        is_admin: false,
    }
}

/// An admin user with the opaque hash `hash`.
fn admin(username: &str) -> NewUser {
    NewUser {
        is_admin: true,
        ..member(username, username)
    }
}

/// Asserts that `result` is the refusal whose `Debug` text is `expected` (the error enums hold
/// `sqlx::Error` and are not `PartialEq`).
#[track_caller]
fn assert_refused<T: std::fmt::Debug, E: std::fmt::Debug>(
    result: Result<T, E>,
    expected: &str,
    label: &str,
) {
    match result {
        Ok(value) => panic!("{label} was accepted: {value:?}"),
        Err(error) => assert_eq!(format!("{error:?}"), expected, "{label}"),
    }
}

/// What `conn` sees: the number of accounts, the number of users and the book's name, if any.
async fn held(conn: &mut sqlx::SqliteConnection) -> (usize, i64, Option<String>) {
    let accounts = accounts::accounts(conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"))
        .len();
    let users = users::count_users(conn)
        .await
        .unwrap_or_else(|error| panic!("cannot count users: {error}"));
    let book = book::book(conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"))
        .map(|book| book.name);
    (accounts, users, book)
}

/// `initialize` with the starter chart writes the book (name, currency code, minor digits and
/// creation stamp), one user (the admin: id 1, no creator, admin, never edited or deactivated),
/// the five roots with their keys, names and kinds, the three other system accounts under the
/// root of their kind with their keys and names, and the starter chart of SPEC 6.2 under each
/// root in order (compared with 6.2's list written out here, never with the constant setup
/// writes from), 27 accounts in all, every one created and last updated by the admin at the
/// setup instant, joint, not balance-only, not archived; full names join the path with a colon.
///
/// Proves: W1 step 3, INV-5 (Core half: the currency and digits set at setup), INV-8 (Core
/// half: the five keyed roots), INV-11 (Core half), decisions 92 and 96.
#[tokio::test]
async fn initialize_creates_the_book_the_admin_the_system_accounts_and_the_starter_chart() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut conn = reader(&storage).await;

    let book = book::book(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"));
    let expected_book = Book {
        name: String::from("Household"),
        currency_code: String::from("CAD"),
        minor_digits: MinorDigits::TWO,
        created_at: now(),
    };
    assert_eq!(book.as_ref(), Some(&expected_book), "the book row");
    assert_eq!(initialized.book, expected_book, "the book setup returned");

    let count = users::count_users(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot count users: {error}"));
    assert_eq!(count, 1, "users after setup");
    let admin = &initialized.admin;
    assert_eq!(admin.id, UserId(1));
    assert_eq!(admin.username, "admin");
    assert_eq!(admin.display_name, "Admin");
    assert!(admin.is_admin, "the first user is an admin");
    assert_eq!(admin.created_by, None, "the first admin has no creator");
    assert_eq!(admin.created_at, now());
    assert_eq!((admin.updated_by, admin.updated_at), (None, None));
    assert_eq!((admin.disabled_by, admin.disabled_at), (None, None));
    assert!(admin.is_enabled());
    let stored = users::user(&mut conn, admin.id)
        .await
        .unwrap_or_else(|error| panic!("cannot read the admin: {error}"));
    assert_eq!(stored.as_ref(), Some(admin), "the admin as stored");

    let all = accounts::accounts(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"));
    assert_eq!(all.len(), 27, "accounts after setup with the starter chart");
    for kind in Kind::ALL {
        let root = accounts::system_account(&mut conn, kind.root_key())
            .await
            .unwrap_or_else(|error| panic!("no {kind} root: {error}"));
        assert!(root.is_root(), "{kind}'s root has a parent");
        assert!(root.is_system());
        assert_eq!(root.kind, kind, "the kind of the {kind} root");
        assert_eq!(root.name, kind.root_name(), "the name of the {kind} root");
        assert_eq!(root.system_key, Some(kind.root_key()));
    }
    for key in [
        SystemKey::OpeningBalances,
        SystemKey::ValuationChanges,
        SystemKey::Uncategorized,
    ] {
        let account = accounts::system_account(&mut conn, key)
            .await
            .unwrap_or_else(|error| panic!("no {key} account: {error}"));
        let parent_key = key
            .parent_key()
            .unwrap_or_else(|| panic!("{key} has no parent key"));
        let root = accounts::system_account(&mut conn, parent_key)
            .await
            .unwrap_or_else(|error| panic!("no {parent_key} root: {error}"));
        assert_eq!(account.name, key.name(), "the name of {key}");
        assert_eq!(account.kind, key.kind(), "the kind of {key}");
        assert_eq!(account.parent_id, Some(root.id), "the parent of {key}");
        assert!(!account.is_root());
        assert!(account.is_system());
    }
    let spec_6_2: [(Kind, &[&str]); 4] = [
        (Kind::Asset, &["Checking", "Savings", "Cash"]),
        (Kind::Liability, &["Credit Card"]),
        (Kind::Income, &["Salary", "Other"]),
        (
            Kind::Expense,
            &[
                "Groceries",
                "Dining",
                "Housing",
                "Utilities",
                "Transport",
                "Health",
                "Entertainment",
                "Shopping",
                "Subscriptions",
                "Travel",
                "Gifts",
                "Fees",
                "Other",
            ],
        ),
    ];
    for (kind, names) in spec_6_2 {
        let root = accounts::system_account(&mut conn, kind.root_key())
            .await
            .unwrap_or_else(|error| panic!("no {kind} root: {error}"));
        let children = accounts::children(&mut conn, root.id)
            .await
            .unwrap_or_else(|error| panic!("cannot read the children of {kind}: {error}"));
        let starter: Vec<&str> = children
            .iter()
            .filter(|child| child.system_key.is_none())
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(starter, names, "the starter chart under {kind}");
        for child in &children {
            assert_eq!(child.kind, kind, "the kind of {}", child.name);
            assert_eq!(
                child.parent_id,
                Some(root.id),
                "the parent of {}",
                child.name
            );
        }
    }
    let equity = accounts::system_account(&mut conn, SystemKey::Equity)
        .await
        .unwrap_or_else(|error| panic!("no equity root: {error}"))
        .id;
    let equity_children = accounts::children(&mut conn, equity)
        .await
        .unwrap_or_else(|error| panic!("cannot read the equity children: {error}"));
    assert_eq!(
        equity_children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["Opening Balances", "Valuation Changes"],
        "Equity holds its two system accounts and nothing else"
    );
    for account in &all {
        let name = &account.name;
        assert_eq!(account.created_by, admin.id, "created_by of {name}");
        assert_eq!(account.created_at, now(), "created_at of {name}");
        assert_eq!(account.updated_by, admin.id, "updated_by of {name}");
        assert_eq!(account.updated_at, now(), "updated_at of {name}");
        assert_eq!(account.owner_user_id, None, "the owner of {name}");
        assert!(!account.is_balance_only, "{name} is balance-only");
        assert!(!account.is_archived(), "{name} is archived");
    }
    let opening = accounts::system_account(&mut conn, SystemKey::OpeningBalances)
        .await
        .unwrap_or_else(|error| panic!("no opening balances account: {error}"));
    let full = accounts::full_name(&mut conn, opening.id)
        .await
        .unwrap_or_else(|error| panic!("cannot name {}: {error}", opening.id));
    assert_eq!(full, "Equity:Opening Balances");
    drop(conn);
    storage.close().await;
}

/// Declining the starter chart leaves the eight system accounts and nothing else; the first
/// user is made an admin even when the setup names a non-admin.
///
/// Proves: SPEC 6.2 (the starter chart is optional), W1 step 3, decision 96 (the first user is
/// an admin).
#[tokio::test]
async fn initialize_without_the_starter_chart_leaves_the_eight_system_accounts() {
    let (_dir, storage) = open_migrated().await;
    let without_chart = Setup {
        admin: NewUser {
            is_admin: false,
            ..standard_setup(false).admin
        },
        ..standard_setup(false)
    };
    let mut tx = begin(&storage, "setup without the starter chart").await;
    let initialized = setup::initialize(&mut tx, now(), &without_chart)
        .await
        .unwrap_or_else(|error| panic!("cannot set up without the starter chart: {error}"));
    commit(tx, "setup without the starter chart").await;
    assert!(
        initialized.admin.is_admin,
        "the first user is made an admin"
    );
    let mut conn = reader(&storage).await;
    let all = accounts::accounts(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"));
    assert_eq!(
        all.len(),
        8,
        "accounts after setup without the starter chart"
    );
    assert!(
        all.iter().all(accounts::Account::is_system),
        "a non-system account exists: {all:#?}"
    );
    let keys: Vec<SystemKey> = all
        .iter()
        .filter_map(|account| account.system_key)
        .collect();
    assert_eq!(keys, SystemKey::ALL, "the eight keys in setup order");
    drop(conn);
    storage.close().await;
}

/// A second setup is refused as already initialized: after a complete setup, after a book row
/// alone and after a user alone. Counted inside each refusing transaction, before the roll-back,
/// the refused attempt has written nothing: 27 accounts, the one user and the book in the
/// complete case, no account beside the lone book or the lone user. The committed chart keeps
/// its 27 accounts after the roll-back.
///
/// Proves: INV-5 (Core half: set once), W1 step 3 (the second submission), decision 96.
#[tokio::test]
async fn initialize_refuses_a_second_setup_and_any_partial_state() {
    let (_dir, storage, _initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a second setup").await;
    assert_refused(
        setup::initialize(&mut tx, later(), &standard_setup(false)).await,
        "AlreadyInitialized",
        "a second setup after a complete one",
    );
    assert_eq!(
        held(&mut tx).await,
        (27, 1, Some(String::from("Household"))),
        "inside the refused second setup, before the roll-back"
    );
    roll_back(tx, "a second setup").await;
    let mut conn = reader(&storage).await;
    let count = accounts::accounts(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"))
        .len();
    assert_eq!(count, 27, "accounts after the refused second setup");
    drop(conn);
    storage.close().await;

    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a book alone").await;
    book::create_book(&mut tx, now(), "Books", currency("CAD"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the book: {error}"));
    assert_refused(
        setup::initialize(&mut tx, now(), &standard_setup(true)).await,
        "AlreadyInitialized",
        "setup after a book row alone",
    );
    assert_eq!(
        held(&mut tx).await,
        (0, 0, Some(String::from("Books"))),
        "inside the refused setup on a book alone, before the roll-back"
    );
    roll_back(tx, "a book alone").await;
    storage.close().await;

    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a user alone").await;
    users::create_user(&mut tx, None, now(), &admin("root"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the first admin: {error}"));
    assert_refused(
        setup::initialize(&mut tx, now(), &standard_setup(true)).await,
        "AlreadyInitialized",
        "setup after a user alone",
    );
    assert_eq!(
        held(&mut tx).await,
        (0, 1, None),
        "inside the refused setup on a user alone, before the roll-back"
    );
    roll_back(tx, "a user alone").await;
    storage.close().await;
}

/// `create_user`: the first user needs no creator and must be an admin; every later user
/// names an existing, enabled admin (a missing creator, an unknown one, a non-admin and a
/// deactivated admin are refused); the username and display name are normalized and checked
/// (a colon is allowed in both); the username is unique by the fold; the hash must not be
/// empty; a created row carries its creator and instant, no edit or deactivation stamps, and
/// is read back by id.
///
/// Proves: SPEC 5.1, INV-26 (Core half), decisions 92 and 96.
#[tokio::test]
async fn create_user_names_an_active_admin_and_checks_the_names() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "users").await;
    assert_refused(
        users::create_user(&mut tx, None, now(), &member("ann", "Ann")).await,
        "FirstUserMustBeAdmin",
        "a first user who is not an admin",
    );
    let first = users::create_user(&mut tx, None, now(), &admin("  Root  "))
        .await
        .unwrap_or_else(|error| panic!("cannot create the first admin: {error}"));
    assert_eq!(first.id, UserId(1));
    assert_eq!(first.username, "Root", "the username is trimmed");
    assert_eq!(first.created_by, None);
    assert!(first.is_admin);

    assert_refused(
        users::create_user(&mut tx, None, later(), &member("ann", "Ann")).await,
        "CreatorRequired",
        "a second user without a creator",
    );
    assert_refused(
        users::create_user(&mut tx, Some(UserId(99)), later(), &member("ann", "Ann")).await,
        "NoSuchCreator",
        "an unknown creator",
    );
    let ann = users::create_user(
        &mut tx,
        Some(first.id),
        later(),
        &member(" Ann\u{00A0} Lee ", "  Ann   Lee "),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create ann: {error}"));
    assert_eq!(ann.id, UserId(2));
    assert_eq!(ann.username, "Ann Lee", "the username is normalized");
    assert_eq!(
        ann.display_name, "Ann Lee",
        "the display name is normalized"
    );
    assert!(!ann.is_admin);
    assert_eq!(ann.created_by, Some(first.id));
    assert_eq!(ann.created_at, later());
    assert_eq!((ann.updated_by, ann.updated_at), (None, None));
    assert_eq!((ann.disabled_by, ann.disabled_at), (None, None));
    let read = users::user(&mut tx, ann.id)
        .await
        .unwrap_or_else(|error| panic!("cannot read ann: {error}"));
    assert_eq!(read.as_ref(), Some(&ann));
    let missing = users::user(&mut tx, UserId(99))
        .await
        .unwrap_or_else(|error| panic!("cannot look up user 99: {error}"));
    assert_eq!(missing, None);
    let count = users::count_users(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("cannot count users: {error}"));
    assert_eq!(count, 2);

    assert_refused(
        users::create_user(&mut tx, Some(ann.id), later(), &member("bob", "Bob")).await,
        "CreatorNotAdmin",
        "a creator who is not an admin",
    );
    for (username, label) in [
        ("ANN LEE", "the same username in capitals"),
        ("ann\u{00A0}lee", "the same username with a no-break space"),
        (
            "  Ann Lee  ",
            "the same username with white space around it",
        ),
    ] {
        assert_refused(
            users::create_user(&mut tx, Some(first.id), later(), &member(username, "X")).await,
            "UsernameTaken",
            label,
        );
    }
    let long = "x".repeat(65);
    for (new, expected, label) in [
        (member("", "X"), "Username(Empty)", "an empty username"),
        (member("   ", "X"), "Username(Empty)", "a blank username"),
        (
            member(&long, "X"),
            "Username(TooLong)",
            "a 65-character username",
        ),
        (
            member("a\tb", "X"),
            "Username(Control)",
            "a tab in a username",
        ),
        (
            member("cal", ""),
            "DisplayName(Empty)",
            "an empty display name",
        ),
        (
            member("cal", &long),
            "DisplayName(TooLong)",
            "a 65-character display name",
        ),
        (
            member("cal", "a\nb"),
            "DisplayName(Control)",
            "a line break in a display name",
        ),
        (
            NewUser {
                password_hash: String::new(),
                ..member("cal", "Cal")
            },
            "PasswordHashEmpty",
            "an empty password hash",
        ),
    ] {
        assert_refused(
            users::create_user(&mut tx, Some(first.id), later(), &new).await,
            expected,
            label,
        );
    }
    let colon = users::create_user(&mut tx, Some(first.id), later(), &member("a:b", "c:d"))
        .await
        .unwrap_or_else(|error| panic!("a colon in a username was refused: {error}"));
    assert_eq!(
        (colon.username.as_str(), colon.display_name.as_str()),
        ("a:b", "c:d")
    );

    let second_admin = users::create_user(&mut tx, Some(first.id), later(), &admin("deputy"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the second admin: {error}"));
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE users SET disabled_by = 1, disabled_at = '{LATER_TEXT}' WHERE id = {}",
        second_admin.id.0
    )))
    .execute(&mut *tx)
    .await
    .unwrap_or_else(|error| panic!("cannot deactivate the second admin: {error}"));
    assert_refused(
        users::create_user(
            &mut tx,
            Some(second_admin.id),
            later(),
            &member("eve", "Eve"),
        )
        .await,
        "CreatorNotAdmin",
        "a deactivated admin as the creator",
    );
    roll_back(tx, "users").await;
    storage.close().await;
}

/// `create_book`: the name is normalized and checked (a colon allowed), the currency's code and
/// digits are stored and read back (JPY's 0), the book returned for KWD has 3 digits, and a
/// second book is refused; an uninitialized database reads back as `None`; a committed book
/// row whose digits are planted outside 0 to 3 (through a plain connection, check constraints
/// ignored) reads back as the refusal `Corrupt`, never as a book.
///
/// Proves: INV-5 (Core half), SPEC 6.10 (the digits from the code chosen at setup), decisions 87
/// and 96 (the digits read back through a checked conversion).
#[tokio::test]
async fn create_book_stores_the_currency_once_and_checks_the_name() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "the book").await;
    let none = book::book(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the empty book: {error}"));
    assert_eq!(none, None, "the book before setup");
    let long = "x".repeat(65);
    for (name, expected, label) in [
        ("", "Name(Empty)", "an empty book name"),
        (" \u{3000} ", "Name(Empty)", "a blank book name"),
        (long.as_str(), "Name(TooLong)", "a 65-character book name"),
        (
            "a\u{0007}b",
            "Name(Control)",
            "a control character in the book name",
        ),
    ] {
        assert_refused(
            book::create_book(&mut tx, now(), name, currency("CAD")).await,
            expected,
            label,
        );
    }
    let book = book::create_book(&mut tx, now(), "  Our:  Books ", currency("JPY"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the book: {error}"));
    assert_eq!(
        book,
        Book {
            name: String::from("Our: Books"),
            currency_code: String::from("JPY"),
            minor_digits: MinorDigits::ZERO,
            created_at: now(),
        }
    );
    let read = book::book(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"));
    assert_eq!(read.as_ref(), Some(&book));
    assert_refused(
        book::create_book(&mut tx, later(), "Another", currency("KWD")).await,
        "AlreadyInitialized",
        "a second book",
    );
    roll_back(tx, "the book").await;

    let mut tx = begin(&storage, "a three-digit book").await;
    let book = book::create_book(&mut tx, now(), "Dinars", currency("KWD"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the KWD book: {error}"));
    assert_eq!(book.minor_digits, MinorDigits::THREE);
    roll_back(tx, "a three-digit book").await;
    storage.close().await;

    let (dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a book to corrupt").await;
    book::create_book(&mut tx, now(), "Books", currency("CAD"))
        .await
        .unwrap_or_else(|error| panic!("cannot create the book: {error}"));
    commit(tx, "a book to corrupt").await;
    corrupt(
        &dir.db_path(),
        false,
        &[
            "PRAGMA ignore_check_constraints = ON",
            "UPDATE book SET minor_digits = 7 WHERE id = 1",
        ],
    )
    .await;
    let mut conn = reader(&storage).await;
    assert_refused(
        book::book(&mut conn).await,
        "Corrupt(\"minor_digits is 7\")",
        "a book row with minor digits outside 0 to 3",
    );
    drop(conn);
    storage.close().await;
}

/// Every refusal of setup, of the user operation and of the book operation has its own message
/// that names the subject (W2: every refusal explains itself).
///
/// Proves: W2, decision 96.
#[test]
fn setup_user_and_book_refusals_explain_themselves() {
    let user_messages: Vec<String> = [
        UserError::Username(NameError::Empty),
        UserError::DisplayName(NameError::TooLong),
        UserError::PasswordHashEmpty,
        UserError::FirstUserMustBeAdmin,
        UserError::CreatorRequired,
        UserError::NoSuchCreator,
        UserError::CreatorNotAdmin,
        UserError::UsernameTaken,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        user_messages,
        [
            "the username is required",
            "the display name has at most 64 characters",
            "a password is required",
            "the first user must be an admin",
            "a user is created by an admin",
            "the creating admin does not exist",
            "only an active admin can add users",
            "this username is already taken",
        ]
    );
    let book_messages: Vec<String> = [
        BookError::AlreadyInitialized,
        BookError::Name(NameError::Colon),
        BookError::Corrupt(String::from("minor_digits is 7")),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        book_messages,
        [
            "the book is already set up",
            "the book name cannot contain a colon",
            "the book row is corrupt: minor_digits is 7",
        ]
    );
    assert_eq!(
        SetupError::AlreadyInitialized.to_string(),
        "the book is already set up"
    );
    assert_eq!(
        SetupError::User(UserError::UsernameTaken).to_string(),
        "this username is already taken"
    );
}
