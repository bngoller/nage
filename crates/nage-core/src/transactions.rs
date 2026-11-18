//! Transactions (SPEC 6.3, 6.4, 6.6, W2, the Core halves of INV-1, INV-2, INV-7, INV-13 and
//! INV-26, the posting halves of INV-8 and INV-12, decisions 99 to 102): the one write path for
//! transactions until milestone 3. The general create writes a balanced transaction of origin
//! `manual`; the opening-balance operations read, create, edit and remove the single opening
//! balance of an Assets or Liabilities account, which alone carries the origin
//! `opening_balance`. Every write takes the caller's write transaction, the acting user and the
//! instant, reads the book's minor digits (they format the amounts its refusals name), decides
//! every refusal before its first write, and refuses with a message that says what was refused,
//! why and what to do instead (decision 102). The core reads no clock: the opening-balance
//! create and edit are handed today's date.

use std::fmt;
use std::str::FromStr;

use crate::accounts::{AccountError, AccountId, Kind};
use crate::book::BookError;
use crate::date::Date;
use crate::money::{MinorDigits, Money};
use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;
use crate::users::UserId;

/// A transaction's id: `transactions.id`, never given to another row (decision 92's
/// AUTOINCREMENT), so an id read by a form names the same transaction for ever.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]
#[sqlx(transparent)]
pub struct TransactionId(pub i64);

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A posting's id: `postings.id`, never given to another row (decision 92).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]
#[sqlx(transparent)]
pub struct PostingId(pub i64);

impl fmt::Display for PostingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where a transaction came from (SPEC 6.4), stored as the four texts of schema 0002's CHECK.
/// This slice writes `manual` and `opening_balance`; `import` arrives with milestone 3 and
/// `balance_update` with milestone 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Manual,
    Import,
    OpeningBalance,
    BalanceUpdate,
}

impl Origin {
    /// The four origins in SPEC 6.4's order.
    pub const ALL: [Origin; 4] = [
        Origin::Manual,
        Origin::Import,
        Origin::OpeningBalance,
        Origin::BalanceUpdate,
    ];

    /// The stored text: `manual`, `import`, `opening_balance`, `balance_update`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Manual => "manual",
            Origin::Import => "import",
            Origin::OpeningBalance => "opening_balance",
            Origin::BalanceUpdate => "balance_update",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Origin {
    type Err = UnknownOrigin;

    /// Exactly one of the four stored texts.
    fn from_str(text: &str) -> Result<Origin, UnknownOrigin> {
        Origin::ALL
            .into_iter()
            .find(|origin| origin.as_str() == text)
            .ok_or_else(|| UnknownOrigin(text.to_owned()))
    }
}

/// A text that is not one of the four origins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownOrigin(pub String);

impl fmt::Display for UnknownOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown transaction origin {:?}", self.0)
    }
}

impl std::error::Error for UnknownOrigin {}

/// A row of `postings` as the core reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Posting {
    pub id: PostingId,
    pub account: AccountId,
    /// In stored sign (SPEC 6.3): the change in the account's stored balance.
    pub amount: Money,
}

/// A row of `transactions` with its postings, as the core reads it, live or deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub id: TransactionId,
    /// The accounting date (SPEC 6.4).
    pub date: Date,
    /// The empty string means none (decision 92); nothing in this slice writes another.
    pub payee: String,
    /// The empty string means none.
    pub memo: String,
    pub origin: Origin,
    /// 1 at creation, one more after every write to the transaction or its postings (INV-7).
    pub version: i64,
    pub created_by: UserId,
    pub created_at: Timestamp,
    /// Equal to the created pair until the first edit.
    pub updated_by: UserId,
    pub updated_at: Timestamp,
    /// Set together with `deleted_at` by a removal; a deleted transaction is in no balance
    /// (INV-25).
    pub deleted_by: Option<UserId>,
    pub deleted_at: Option<Timestamp>,
    /// The postings by id, which is the order they were given in.
    pub postings: Vec<Posting>,
}

impl Transaction {
    /// Whether the transaction is deleted (soft delete: it stays stored and readable).
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// One posting of a new transaction: the account and the amount in stored sign (SPEC 6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewPosting {
    pub account: AccountId,
    pub amount: Money,
}

/// What a new transaction is made of in this slice (decision 99): its date and its postings.
/// Payee and memo inputs arrive with milestone 3's import and milestone 4's entry form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTransaction {
    pub date: Date,
    pub postings: Vec<NewPosting>,
}

/// Creates a transaction dated `new.date` with `new.postings` at `now` by `by` (SPEC 6.4,
/// decision 99): origin `manual`, payee and memo the empty string, version 1, the updated pair
/// equal to the created pair, no deleted pair, then the postings in the order given. A book
/// that is not set up is refused first (`NotInitialized`). Then refused, each before anything
/// is written: fewer than two postings (`TooFewPostings`, INV-2); a posting whose account does
/// not exist (`NoSuchAccount`, never the foreign key's error), is a root (`RootAccount`,
/// INV-8) or is archived (`ArchivedAccount`, INV-12); a posting whose amount is zero
/// (`ZeroAmount`, INV-2); postings that do not sum to zero (`Unbalanced` with their sum,
/// INV-1), summed exactly whatever their order. When several of these apply, which of them is
/// reported is not specified. There is no date rule beyond `Date`'s range. Returns the
/// transaction as [`transaction`] reads it back.
pub async fn create_transaction(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    new: &NewTransaction,
) -> Result<Transaction, TransactionError> {
    let digits = book_digits(tx)
        .await?
        .ok_or(TransactionError::NotInitialized)?;
    if new.postings.len() < 2 {
        return Err(TransactionError::TooFewPostings {
            given: new.postings.len(),
        });
    }
    for posting in &new.postings {
        let account = accounts::account(tx, posting.account).await?.ok_or(
            TransactionError::NoSuchAccount {
                account: posting.account,
            },
        )?;
        if account.is_root() {
            return Err(TransactionError::RootAccount {
                account: account.name,
            });
        }
        if account.is_archived() {
            return Err(TransactionError::ArchivedAccount {
                account: accounts::full_name(tx, account.id).await?,
            });
        }
        if posting.amount == Money::ZERO {
            return Err(TransactionError::ZeroAmount {
                account: accounts::full_name(tx, account.id).await?,
            });
        }
    }
    let sum = Money::checked_sum(new.postings.iter().map(|posting| posting.amount));
    if sum != Some(Money::ZERO) {
        return Err(TransactionError::Unbalanced { sum, digits });
    }
    let id = insert(tx, by, now, Origin::Manual, new.date, &new.postings).await?;
    Ok(reload(tx, id).await?)
}

/// The transaction `id` with its postings by id, live or deleted, or `None`.
pub async fn transaction(
    conn: &mut sqlx::SqliteConnection,
    id: TransactionId,
) -> Result<Option<Transaction>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"SELECT id AS "id!: TransactionId", date AS "date!: Date", payee, memo, origin AS "origin!: Origin", version AS "version!: i64", created_by AS "created_by!: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by!: UserId", updated_at AS "updated_at!: Timestamp", deleted_by AS "deleted_by?: UserId", deleted_at AS "deleted_at?: Timestamp" FROM transactions WHERE id = ?1"#,
        id
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let postings = sqlx::query_as!(
        Posting,
        r#"SELECT id AS "id!: PostingId", account_id AS "account!: AccountId", amount AS "amount!: Money" FROM postings WHERE transaction_id = ?1 ORDER BY id"#,
        id
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(Some(Transaction {
        id: row.id,
        date: row.date,
        payee: row.payee,
        memo: row.memo,
        origin: row.origin,
        version: row.version,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_by: row.updated_by,
        updated_at: row.updated_at,
        deleted_by: row.deleted_by,
        deleted_at: row.deleted_at,
        postings,
    }))
}

/// An account's live opening balance as its form shows it (SPEC 6.6, decision 100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpeningBalance {
    /// The opening-balance transaction, which an edit or a removal names.
    pub id: TransactionId,
    pub account: AccountId,
    pub date: Date,
    /// In natural sign (SPEC 6.3): what an Assets account holds, what a Liabilities account
    /// owes; negative for an overdrawn account or a card in credit.
    pub amount: Money,
    /// The version an edit or a removal names as the one it expects (INV-7).
    pub version: i64,
}

/// What a new opening balance is made of (SPEC 6.6, W2): the account, the date (no default)
/// and the amount in natural sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewOpeningBalance {
    pub account: AccountId,
    pub date: Date,
    pub amount: Money,
}

/// An edit of an opening balance (decision 100): the entry's transaction id and the version
/// the form read, with the date and the amount in natural sign as the form now holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpeningBalanceEdit {
    pub id: TransactionId,
    pub expected_version: i64,
    pub date: Date,
    pub amount: Money,
}

/// The live opening balance of `account` (decision 100): the transaction's id, its date, the
/// amount in natural sign and the version; `None` when the account has none, which includes a
/// removed one, an account that does not exist and every account outside Assets and
/// Liabilities (`Equity:Opening Balances` itself among them). `Corrupt` when the stored amount
/// has no natural sign.
pub async fn opening_balance(
    conn: &mut sqlx::SqliteConnection,
    account: AccountId,
) -> Result<Option<OpeningBalance>, OpeningBalanceError> {
    let kind = accounts::account(&mut *conn, account)
        .await?
        .map(|found| found.kind);
    if !kind.is_some_and(takes_opening_balance) {
        return Ok(None);
    }
    let id = sqlx::query_scalar!(
        r#"SELECT t.id AS "id!: TransactionId" FROM transactions t JOIN postings p ON p.transaction_id = t.id WHERE p.account_id = ?1 AND t.origin = ?2 AND t.deleted_at IS NULL ORDER BY t.id LIMIT 1"#,
        account,
        Origin::OpeningBalance
    )
    .fetch_optional(&mut *conn)
    .await?;
    match id {
        Some(id) => entry(conn, id).await,
        None => Ok(None),
    }
}

/// Creates the opening balance of `new.account` at `now` by `by` (SPEC 6.6, INV-13's Core
/// half, decision 100): a transaction of origin `opening_balance` dated `new.date`, payee and
/// memo the empty string, version 1, the updated pair equal to the created pair, with two
/// postings: the account's, `new.amount` turned from natural into stored sign (SPEC 6.3), and
/// its negation on `Equity:Opening Balances`, found by its system key whatever its name. A
/// book that is not set up is refused first (`NotInitialized`). Then refused, each before
/// anything is written: an account that does not exist (`NoSuchAccount`), lies outside Assets
/// and Liabilities (`NotAssetOrLiability`), is a root (`RootAccount`), is archived
/// (`ArchivedAccount`) or has a live opening balance already (`AlreadyExists`, which carries
/// it); an amount of zero (`ZeroAmount`); a date after `today` (`DateAfterToday`; `today`
/// itself is accepted); the most negative amount, whose stored form or counter posting does
/// not fit (`TooLarge`). When several of these apply, which of them is reported is not
/// specified. Any other Assets or Liabilities account may have one, a parent of sub-accounts
/// included. Returns the entry as [`opening_balance`] reads it.
pub async fn create_opening_balance(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    today: Date,
    new: &NewOpeningBalance,
) -> Result<OpeningBalance, OpeningBalanceError> {
    let digits = book_digits(tx)
        .await?
        .ok_or(OpeningBalanceError::NotInitialized)?;
    let account =
        accounts::account(tx, new.account)
            .await?
            .ok_or(OpeningBalanceError::NoSuchAccount {
                account: new.account,
            })?;
    let name = accounts::full_name(tx, account.id).await?;
    if !takes_opening_balance(account.kind) {
        return Err(OpeningBalanceError::NotAssetOrLiability {
            account: name,
            kind: account.kind,
        });
    }
    if account.is_root() {
        return Err(OpeningBalanceError::RootAccount { account: name });
    }
    if account.is_archived() {
        return Err(OpeningBalanceError::ArchivedAccount { account: name });
    }
    if let Some(existing) = opening_balance(tx, account.id).await? {
        return Err(OpeningBalanceError::AlreadyExists {
            account: name,
            existing,
            digits,
        });
    }
    let (stored, counter) = legs(&name, account.kind, new.date, new.amount, today, digits)?;
    let opening = accounts::system_account(tx, SystemKey::OpeningBalances).await?;
    let postings = [
        NewPosting {
            account: opening.id,
            amount: counter,
        },
        NewPosting {
            account: account.id,
            amount: stored,
        },
    ];
    insert(tx, by, now, Origin::OpeningBalance, new.date, &postings).await?;
    reread(tx, account.id).await
}

/// Changes the date and the amount of the opening balance `edit.id` at `now` by `by` (SPEC
/// 6.6, INV-7, decisions 100 and 101). A book that is not set up is refused first
/// (`NotInitialized`); then the save is decided in this order:
///
/// 1. `edit.id` must name a live opening balance, else `NoSuchOpeningBalance` (also for the id
///    of a removed entry and of any other transaction).
/// 2. When `edit.date` and `edit.amount` both equal what is saved, the save is not an edit:
///    what is saved is returned and nothing is written, whatever `edit.expected_version` is
///    and even on an archived account.
/// 3. An `edit.expected_version` other than the stored version is refused as `Stale`, which
///    carries the entry as saved now.
/// 4. An archived account refuses every change (`ArchivedAccount`), and the new values meet
///    the create's rules (`ZeroAmount`, `DateAfterToday` against `today`, `TooLarge`); when
///    several of these apply, which of them is reported is not specified.
///
/// The write then updates in place the transaction's date, both postings' amounts (the
/// account's in stored sign and its negation), the version by exactly one and the updated
/// pair; no id and no created pair changes. Returns the entry as [`opening_balance`] reads it.
pub async fn edit_opening_balance(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    today: Date,
    edit: &OpeningBalanceEdit,
) -> Result<OpeningBalance, OpeningBalanceError> {
    let digits = book_digits(tx)
        .await?
        .ok_or(OpeningBalanceError::NotInitialized)?;
    let saved = entry(tx, edit.id)
        .await?
        .ok_or(OpeningBalanceError::NoSuchOpeningBalance { id: edit.id })?;
    if edit.date == saved.date && edit.amount == saved.amount {
        return Ok(saved);
    }
    let (account, name) = writable(tx, saved, edit.expected_version, digits).await?;
    let (stored, counter) = legs(&name, account.kind, edit.date, edit.amount, today, digits)?;
    sqlx::query!(
        "UPDATE transactions SET date = ?1, version = version + 1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        edit.date,
        by,
        now,
        edit.id
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query!(
        "UPDATE postings SET amount = ?1 WHERE transaction_id = ?2 AND account_id = ?3",
        stored,
        edit.id,
        account.id
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query!(
        "UPDATE postings SET amount = ?1 WHERE transaction_id = ?2 AND account_id <> ?3",
        counter,
        edit.id,
        account.id
    )
    .execute(&mut **tx)
    .await?;
    reread(tx, account.id).await
}

/// Removes the opening balance `id` at `now` by `by` (decision 100, amending SPEC 6.6): a soft
/// delete that sets the deleted pair and raises the version by one, leaving the updated pair
/// and the postings as they are. The entry then leaves every balance (INV-25) and its account
/// may be given a new opening balance (INV-13 counts live ones). A book that is not set up is
/// refused first (`NotInitialized`). Then refused, each before anything is written and in the
/// edit's order: an `id` that names no live opening balance (`NoSuchOpeningBalance`, also for
/// the id of a removed entry and of any other transaction); an `expected_version` other than
/// the stored version (`Stale`, which carries the entry as saved now); an archived account
/// (`ArchivedAccount`). Returns the removed transaction as [`transaction`] reads it back.
pub async fn remove_opening_balance(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    id: TransactionId,
    expected_version: i64,
) -> Result<Transaction, OpeningBalanceError> {
    let digits = book_digits(tx)
        .await?
        .ok_or(OpeningBalanceError::NotInitialized)?;
    let saved = entry(tx, id)
        .await?
        .ok_or(OpeningBalanceError::NoSuchOpeningBalance { id })?;
    writable(tx, saved, expected_version, digits).await?;
    sqlx::query!(
        "UPDATE transactions SET version = version + 1, deleted_by = ?1, deleted_at = ?2 WHERE id = ?3",
        by,
        now,
        id
    )
    .execute(&mut **tx)
    .await?;
    Ok(reload(tx, id).await?)
}

/// Why [`create_transaction`] refused (decisions 99 and 102). Each message says what was
/// refused, why, and what to do instead, and names an account by its full name (SPEC 6.2) and
/// an amount as SPEC 6.10 displays it with the book's minor digits; a form shows it as it
/// stands.
#[derive(Debug)]
pub enum TransactionError {
    /// The book is not set up: there is no book row.
    NotInitialized,
    /// Fewer than two postings were given (INV-2): `given` is 0 or 1.
    TooFewPostings { given: usize },
    /// A posting names an account id that no account has.
    NoSuchAccount { account: AccountId },
    /// A posting is on a root (INV-8); `account` is the root's name.
    RootAccount { account: String },
    /// A posting is on an archived account (INV-12); `account` is its full name.
    ArchivedAccount { account: String },
    /// The posting on `account` (its full name) has an amount of zero (INV-2).
    ZeroAmount { account: String },
    /// The postings do not sum to zero (INV-1): `sum` is their sum in stored sign, or `None`
    /// when it does not fit in 64 bits; `digits` are the book's.
    Unbalanced {
        sum: Option<Money>,
        digits: MinorDigits,
    },
    /// The chart of accounts could not be read.
    Accounts(AccountError),
    /// The book row could not be read.
    Book(BookError),
    /// Any other database failure.
    Database(sqlx::Error),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => f.write_str(
                "The transaction was not saved because this book is not set up yet. Finish the setup first, then try again.",
            ),
            Self::TooFewPostings { given } => {
                let has = if *given == 0 {
                    "no lines"
                } else {
                    "only one line"
                };
                write!(
                    f,
                    "The transaction was not saved because it has {has}, and a transaction moves money between at least two accounts. Give it at least two lines, each with an account and an amount, then save again."
                )
            }
            Self::NoSuchAccount { account } => write!(
                f,
                "The transaction was not saved because one of its lines names an account that does not exist (account number {account}). Choose an account from the chart of accounts for that line, then save again."
            ),
            Self::RootAccount { account } => write!(
                f,
                "The transaction was not saved because one of its lines is on {account}, one of the five top-level accounts, which only group the accounts under them and cannot be used on a line. Choose an account under {account} for that line, then save again."
            ),
            Self::ArchivedAccount { account } => write!(
                f,
                "The transaction was not saved because one of its lines is on {account}, which is archived, and an archived account cannot be used on a new line. Unarchive {account} first, or choose another account for that line, then save again."
            ),
            Self::ZeroAmount { account } => write!(
                f,
                "The transaction was not saved because the amount of its line on {account} is zero, and every line must move some money. Enter an amount other than zero for that line, or leave the line out, then save again."
            ),
            Self::Unbalanced {
                sum: Some(sum),
                digits,
            } => {
                // The two sides differ by the sum without its sign: its display with the minus
                // dropped, which writes the most negative sum exactly, with no arithmetic.
                let shown = sum.display(*digits).to_string();
                write!(
                    f,
                    "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by {}. Change the amounts until both sides are equal, then save again.",
                    shown.trim_start_matches('-')
                )
            }
            Self::Unbalanced { sum: None, .. } => f.write_str(
                "The transaction was not saved because its lines do not balance: what leaves one account must arrive in another, and here the two sides differ by more than Nage can count. Check the amounts for a typing mistake, then save again.",
            ),
            Self::Accounts(error) => write!(f, "{error}"),
            Self::Book(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for TransactionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Accounts(error) => Some(error),
            Self::Book(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<AccountError> for TransactionError {
    fn from(error: AccountError) -> Self {
        Self::Accounts(error)
    }
}

impl From<BookError> for TransactionError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<sqlx::Error> for TransactionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Why an opening-balance operation refused (decisions 100 to 102). Each message says what was
/// refused, why, and what to do instead, and names the account by its full name (SPEC 6.2) and
/// an amount in natural sign as SPEC 6.10 displays it with the book's minor digits; a form
/// shows it as it stands.
#[derive(Debug)]
pub enum OpeningBalanceError {
    /// The book is not set up: there is no book row.
    NotInitialized,
    /// No account has this id.
    NoSuchAccount { account: AccountId },
    /// The account (its full name) is of a kind other than Assets and Liabilities (INV-13).
    NotAssetOrLiability { account: String, kind: Kind },
    /// The account is a root (INV-8, INV-13); `account` is the root's name.
    RootAccount { account: String },
    /// The account (its full name) is archived: its opening balance is not created, edited or
    /// removed (INV-12).
    ArchivedAccount { account: String },
    /// The amount is zero (SPEC 6.6).
    ZeroAmount { account: String },
    /// The account has a live opening balance already (INV-13): `existing` is that entry,
    /// `digits` are the book's.
    AlreadyExists {
        account: String,
        existing: OpeningBalance,
        digits: MinorDigits,
    },
    /// `date` is after `today`.
    DateAfterToday {
        account: String,
        date: Date,
        today: Date,
    },
    /// The amount cannot be stored: the most negative amount, whose stored form on a
    /// Liabilities account, or whose counter posting on an Assets account, does not fit;
    /// `digits` are the book's, which write the range of amounts the book can hold.
    TooLarge {
        account: String,
        digits: MinorDigits,
    },
    /// `id` names no live opening balance: no transaction, another kind of transaction, or a
    /// removed entry.
    NoSuchOpeningBalance { id: TransactionId },
    /// The version named is not the stored one (INV-7, decision 101): `saved` is the entry as
    /// saved now, as [`opening_balance`] reads it; `digits` are the book's.
    Stale {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
    /// A stored opening balance cannot be shown: its amount has no natural sign (the most
    /// negative amount on a Liabilities account), which these operations never write.
    Corrupt(String),
    /// The chart of accounts could not be read, or `Equity:Opening Balances` is missing
    /// (INV-11 broken).
    Accounts(AccountError),
    /// The book row could not be read.
    Book(BookError),
    /// Any other database failure.
    Database(sqlx::Error),
}

impl fmt::Display for OpeningBalanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => f.write_str(
                "The opening balance was not changed because this book is not set up yet. Finish the setup first, then try again.",
            ),
            Self::NoSuchAccount { account } => write!(
                f,
                "The opening balance was not saved because its account does not exist (account number {account}). Open the account from the chart of accounts and enter the opening balance there."
            ),
            Self::NotAssetOrLiability { account, kind } => write!(
                f,
                "The opening balance was not saved because {account} is an account under {}, and only accounts under Assets or Liabilities, which hold what the household has and what it owes, start with a balance. Leave {account} without an opening balance: income, expense and equity accounts start at zero.",
                kind.root_name()
            ),
            Self::RootAccount { account } => write!(
                f,
                "The opening balance was not saved because {account} is one of the five top-level accounts, which only group the accounts under them and hold nothing of their own. Enter the opening balance on an account under {account} instead."
            ),
            Self::ArchivedAccount { account } => write!(
                f,
                "The opening balance of {account} was not changed because the account is archived, and an archived account's opening balance cannot be entered, edited or removed. Unarchive {account} first, then try again."
            ),
            Self::ZeroAmount { account } => write!(
                f,
                "The opening balance of {account} was not saved because the amount is zero, and an account that starts at zero needs no opening balance. Enter the amount the account started with; if it started at zero, leave it without an opening balance, and remove the one it has, if any."
            ),
            Self::AlreadyExists {
                account,
                existing,
                digits,
            } => write!(
                f,
                "The opening balance was not saved because {account} already has one, {} on {}, and an account has only one opening balance. Change that one instead, or remove it first.",
                existing.amount.display(*digits),
                existing.date
            ),
            Self::DateAfterToday {
                account,
                date,
                today,
            } => write!(
                f,
                "The opening balance of {account} was not saved because its date, {date}, is after today, {today}: an opening balance is what the account held at the end of its day, and that day cannot lie in the future. Enter today's date or an earlier one."
            ),
            Self::TooLarge { account, digits } => {
                // From the largest amount negated to the largest: only the most negative amount,
                // one unit below that range, cannot be stored.
                let largest = Money::from_minor(i64::MAX).display(*digits);
                write!(
                    f,
                    "The opening balance of {account} was not saved because the amount is beyond what Nage can store: it holds amounts from -{largest} to {largest}. Check the amount for a typing mistake and enter one within that range."
                )
            }
            Self::NoSuchOpeningBalance { id } => write!(
                f,
                "The opening balance was not changed because it is no longer there (entry number {id}): it may have been removed while this page was open. Open the account again to see what is saved now."
            ),
            Self::Stale {
                account,
                saved,
                digits,
            } => write!(
                f,
                "Your change to the opening balance of {account} was not saved because the opening balance was changed while you were working, by another user or in another window: it is now {} on {}. Look at what is saved now, then make your change again or keep what is saved.",
                saved.amount.display(*digits),
                saved.date
            ),
            Self::Corrupt(what) => write!(
                f,
                "The opening balance could not be read because what is stored for it is damaged: {what}. Ask whoever looks after Nage to run `nage check` and to restore a backup if it reports a problem."
            ),
            Self::Accounts(error) => write!(f, "{error}"),
            Self::Book(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for OpeningBalanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Accounts(error) => Some(error),
            Self::Book(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<AccountError> for OpeningBalanceError {
    fn from(error: AccountError) -> Self {
        Self::Accounts(error)
    }
}

impl From<BookError> for OpeningBalanceError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<sqlx::Error> for OpeningBalanceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use crate::{accounts, book};

/// The book's minor digits, which format the amounts a refusal names (decision 102); `None`
/// while the book is not set up, which every write refuses first.
async fn book_digits(conn: &mut sqlx::SqliteConnection) -> Result<Option<MinorDigits>, BookError> {
    Ok(book::book(conn).await?.map(|book| book.minor_digits))
}

/// Writes a transaction of `origin` dated `date` by `by` at `now`, then `postings` in the order
/// given, so that their ids rise in that order (decision 99): payee and memo the empty string,
/// version 1, the updated pair equal to the created pair, no deleted pair. The one insert of
/// both creates, which have decided every refusal before they call it; only
/// [`create_opening_balance`] passes the origin `opening_balance`.
async fn insert(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    origin: Origin,
    date: Date,
    postings: &[NewPosting],
) -> Result<TransactionId, sqlx::Error> {
    let id = sqlx::query!(
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES (?1, '', '', ?2, 1, ?3, ?4, ?3, ?4, NULL, NULL)",
        date,
        origin,
        by,
        now
    )
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    let id = TransactionId(id);
    for posting in postings {
        sqlx::query!(
            "INSERT INTO postings (transaction_id, account_id, amount) VALUES (?1, ?2, ?3)",
            id,
            posting.account,
            posting.amount
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(id)
}

/// The transaction `id` as stored after a write, which must exist.
async fn reload(tx: &mut WriteTransaction, id: TransactionId) -> Result<Transaction, sqlx::Error> {
    transaction(tx, id).await?.ok_or(sqlx::Error::RowNotFound)
}

use crate::accounts::SystemKey;
use crate::balances;

/// Whether an account of `kind` may have an opening balance (SPEC 6.6, INV-13): Assets and
/// Liabilities only.
fn takes_opening_balance(kind: Kind) -> bool {
    matches!(kind, Kind::Asset | Kind::Liability)
}

/// The live opening balance `id` (decision 100), built from the transaction and its posting on
/// an Assets or Liabilities account (the other posting is the counter posting on Equity): the
/// read gives it by account, the edit and the removal find it by id. `None` when `id` names no
/// transaction, a transaction of another origin, a removed one, or one without such a posting;
/// `Corrupt` when that posting's amount has no natural sign.
async fn entry(
    conn: &mut sqlx::SqliteConnection,
    id: TransactionId,
) -> Result<Option<OpeningBalance>, OpeningBalanceError> {
    let Some(found) = transaction(&mut *conn, id).await? else {
        return Ok(None);
    };
    if found.origin != Origin::OpeningBalance || found.is_deleted() {
        return Ok(None);
    }
    for posting in &found.postings {
        let kind = accounts::account(&mut *conn, posting.account)
            .await?
            .ok_or(AccountError::NoSuchAccount)?
            .kind;
        if !takes_opening_balance(kind) {
            continue;
        }
        let amount = balances::natural(posting.amount, kind).ok_or_else(|| {
            OpeningBalanceError::Corrupt(format!(
                "entry number {id} holds an amount Nage cannot show"
            ))
        })?;
        return Ok(Some(OpeningBalance {
            id,
            account: posting.account,
            date: found.date,
            amount,
            version: found.version,
        }));
    }
    Ok(None)
}

/// The rules a new date and amount meet in the create and the edit (SPEC 6.6, decision 100):
/// not zero (`ZeroAmount`), not after `today` (`DateAfterToday`), and storable (`TooLarge`: the
/// most negative amount, whose stored form on a Liabilities account or whose counter posting
/// does not fit); then the two stored amounts, the account's and its negation on `Equity:Opening
/// Balances`. The stored sign is the natural sign times the kind's sign (SPEC 6.3), the flip
/// [`balances::natural`] makes in the other direction. `TooLarge` carries the book's `digits`.
fn legs(
    account: &str,
    kind: Kind,
    date: Date,
    amount: Money,
    today: Date,
    digits: MinorDigits,
) -> Result<(Money, Money), OpeningBalanceError> {
    if amount == Money::ZERO {
        return Err(OpeningBalanceError::ZeroAmount {
            account: account.to_owned(),
        });
    }
    if date > today {
        return Err(OpeningBalanceError::DateAfterToday {
            account: account.to_owned(),
            date,
            today,
        });
    }
    let too_large = || OpeningBalanceError::TooLarge {
        account: account.to_owned(),
        digits,
    };
    let stored = balances::natural(amount, kind).ok_or_else(too_large)?;
    let counter = stored.checked_neg().ok_or_else(too_large)?;
    Ok((stored, counter))
}

/// The live opening balance of `account` as the read gives it after a write, which must exist.
async fn reread(
    tx: &mut WriteTransaction,
    account: AccountId,
) -> Result<OpeningBalance, OpeningBalanceError> {
    opening_balance(tx, account)
        .await?
        .ok_or(OpeningBalanceError::Database(sqlx::Error::RowNotFound))
}

use crate::accounts::Account;

/// The refusals the edit and the removal share once the entry is found, in their order
/// (decisions 100 and 101): a version other than the stored one (`Stale`, which carries the
/// entry as saved now), then an archived account (`ArchivedAccount`, INV-12). Returns the
/// entry's account and its full name.
async fn writable(
    tx: &mut WriteTransaction,
    saved: OpeningBalance,
    expected_version: i64,
    digits: MinorDigits,
) -> Result<(Account, String), OpeningBalanceError> {
    let account = accounts::account(tx, saved.account)
        .await?
        .ok_or(AccountError::NoSuchAccount)?;
    let name = accounts::full_name(tx, account.id).await?;
    if expected_version != saved.version {
        return Err(OpeningBalanceError::Stale {
            account: name,
            saved,
            digits,
        });
    }
    if account.is_archived() {
        return Err(OpeningBalanceError::ArchivedAccount { account: name });
    }
    Ok((account, name))
}
