//! Setup's core half (SPEC W1 step 3, INV-5's Core half, decisions 93 and 96): the book row, the
//! first admin, the five roots, the three other system accounts and the optional starter chart,
//! written in that order in one write transaction, by the one core operation without an acting
//! user (none exists yet). The `/setup` form (slice 5) and `nage init` call it.

use std::fmt;

use crate::accounts::{AccountError, Kind};
use crate::book::{Book, BookError};
use crate::currency::Currency;
use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;
use crate::users::{NewUser, User, UserError};

/// The starter chart of SPEC 6.2, by kind: the names created under each root when setup is
/// asked for it. Declining it leaves the roots and the three system accounts.
pub const STARTER_CHART: &[(Kind, &[&str])] = &[
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

/// What setup is given (W1 step 2): the book name, the currency chosen from the table, the admin
/// (whose `is_admin` is made true) and whether to create the starter chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub book_name: String,
    pub currency: Currency,
    pub admin: NewUser,
    pub starter_chart: bool,
}

/// What setup created: the book and the admin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initialized {
    pub book: Book,
    pub admin: User,
}

/// Initializes the book at `now` (W1 step 3, decision 96): refused as `AlreadyInitialized` when
/// a book row or any user exists (the second of two concurrent submissions, serialized by the
/// writer connection); then the book, the admin, the roots with their keys, `Equity:Opening
/// Balances`, `Equity:Valuation Changes` and `Expenses:Uncategorized` with theirs, and the
/// starter chart when `setup.starter_chart` is true, every account created by the admin at
/// `now`. The caller commits.
pub async fn initialize(
    tx: &mut WriteTransaction,
    now: Timestamp,
    setup: &Setup,
) -> Result<Initialized, SetupError> {
    if book::book(tx).await?.is_some() || users::count_users(tx).await? > 0 {
        return Err(SetupError::AlreadyInitialized);
    }
    let book = book::create_book(tx, now, &setup.book_name, setup.currency).await?;
    let admin = users::create_user(
        tx,
        None,
        now,
        &NewUser {
            is_admin: true,
            ..setup.admin.clone()
        },
    )
    .await?;
    let mut roots = Vec::with_capacity(Kind::ALL.len());
    for kind in Kind::ALL {
        let root =
            accounts::insert_system_account(tx, admin.id, now, kind.root_key(), None).await?;
        roots.push((kind, root.id));
    }
    let root_of = |kind: Kind| {
        roots
            .iter()
            .find(|(root_kind, _)| *root_kind == kind)
            .map(|(_, id)| *id)
            .ok_or(AccountError::MissingSystemAccount(kind.root_key()))
    };
    for key in [
        SystemKey::OpeningBalances,
        SystemKey::ValuationChanges,
        SystemKey::Uncategorized,
    ] {
        let parent = root_of(key.kind())?;
        accounts::insert_system_account(tx, admin.id, now, key, Some(parent)).await?;
    }
    if setup.starter_chart {
        for (kind, names) in STARTER_CHART {
            let parent = root_of(*kind)?;
            for name in *names {
                accounts::insert_starter_account(tx, admin.id, now, name, parent, *kind).await?;
            }
        }
    }
    Ok(Initialized { book, admin })
}

/// Why setup was refused (decision 96).
#[derive(Debug)]
pub enum SetupError {
    /// A book row or a user exists already.
    AlreadyInitialized,
    /// The book could not be written.
    Book(BookError),
    /// The admin could not be created.
    User(UserError),
    /// An account could not be created.
    Account(AccountError),
    /// Any other database failure.
    Database(sqlx::Error),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => f.write_str("the book is already set up"),
            Self::Book(error) => write!(f, "{error}"),
            Self::User(error) => write!(f, "{error}"),
            Self::Account(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for SetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Book(error) => Some(error),
            Self::User(error) => Some(error),
            Self::Account(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::AlreadyInitialized => None,
        }
    }
}

impl From<BookError> for SetupError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<UserError> for SetupError {
    fn from(error: UserError) -> Self {
        Self::User(error)
    }
}

impl From<AccountError> for SetupError {
    fn from(error: AccountError) -> Self {
        Self::Account(error)
    }
}

impl From<sqlx::Error> for SetupError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use crate::accounts::{self, SystemKey};
use crate::book;
use crate::users;
