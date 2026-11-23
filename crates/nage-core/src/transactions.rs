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

use crate::accounts::{AccountError, AccountId, Kind, Word};
use crate::book::BookError;
use crate::date::{Date, DateError};
use crate::money::{MinorDigits, Money, ParseMoneyError};
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
/// itself is accepted); an amount past [`Money::largest_taken`] above or below zero
/// (`TooLarge`, decision 142, which also covers every amount whose stored form or counter
/// posting does not fit). When several of these apply, which of them is reported is not
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
/// edit's order: an `id` that names no live opening balance (`AlreadyRemoved`, a removal's own
/// sentence, also for the id of a removed entry and of any other transaction); an
/// `expected_version` other than the stored version (`RemoveStale`, which carries the entry as saved now and words the
/// confirmation's clash, decision 145); an archived account (`ArchivedAccount`). Returns the
/// removed transaction as [`transaction`] reads it back.
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
        .ok_or(OpeningBalanceError::AlreadyRemoved { id })?;
    writable(tx, saved, expected_version, digits)
        .await
        .map_err(|error| match error {
            OpeningBalanceError::Stale {
                account,
                saved,
                digits,
            } => OpeningBalanceError::RemoveStale {
                account,
                saved,
                digits,
            },
            other => other,
        })?;
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

/// Why an opening-balance operation refused (decisions 100 to 102 and 145). Each message says
/// what was not done, why, and what to do instead, but a clash's (`AlreadyExists`, `Stale`,
/// `RemoveStale`, `NoSuchOpeningBalance`), which ends after the why since the page offers the
/// choices; one that can stand at the top of a page names the account by its full name (SPEC
/// 6.2), one that can stand under a box (`ZeroAmount`, `DateAfterToday`, `TooLarge`) names
/// none, and none prints a number a request chose; an amount is in natural sign as SPEC 6.10
/// displays it with the book's minor digits. A form shows it as it stands.
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
    /// The account (its full name) was archived after the page that sent the form or the
    /// removal was shown (decision 145): the pages' word for [`Self::ArchivedAccount`], at the
    /// top of the account's page.
    ArchivedMeanwhile { account: String },
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
    /// The amount is more than a form takes, above or below zero ([`Money::is_taken`], decision
    /// 142), which also covers every amount whose stored form or counter posting does not fit;
    /// `digits` are the book's, which write the limit.
    TooLarge {
        account: String,
        digits: MinorDigits,
    },
    /// `id` names no live opening balance: no transaction, another kind of transaction, or a
    /// removed entry. The change's refusal; a removal's is [`Self::AlreadyRemoved`].
    NoSuchOpeningBalance { id: TransactionId },
    /// A removal named `id`, which names no live opening balance (no transaction, another kind
    /// of transaction, or a removed entry): there is nothing left to remove (the hand check's
    /// H5). Its sentence names no entry number and no page reaches it.
    AlreadyRemoved { id: TransactionId },
    /// The version named is not the stored one (INV-7, decision 101): `saved` is the entry as
    /// saved now, as [`opening_balance`] reads it; `digits` are the book's.
    Stale {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
    /// A removal named a version that is not the stored one (INV-7, decision 143): `saved` is
    /// the entry as saved now; `digits` are the book's. Its sentence is the confirmation's clash
    /// sentence ([`OpeningClash::RemoveChanged`]), so that one refusal has one sentence whether a
    /// form or the operation alone meets it (decision 145).
    RemoveStale {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
    /// A stored opening balance cannot be shown: its amount has no natural sign (the most
    /// negative amount on a Liabilities account), which these operations never write. Its
    /// sentence names no entry number (decision 145).
    Corrupt,
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
                "The opening balance was not saved because this book is not set up yet. Finish the setup first, then try again.",
            ),
            Self::NoSuchAccount { .. } => f.write_str(
                "The opening balance was not saved because its account does not exist. Go to Accounts and choose the account from the list.",
            ),
            Self::NotAssetOrLiability { account, kind } => write!(
                f,
                "The opening balance was not saved because {account} is {} under {}, and only accounts under Assets and Liabilities, which hold what the household has and what it owes, start with a balance. Leave it without one: everything under Income, Expenses and Equity starts at zero.",
                Word::of(*kind).with_article(),
                kind.root_name()
            ),
            Self::RootAccount { account } => write!(
                f,
                "The opening balance was not saved because {account} is one of Nage's five sections, which only group the accounts in them and hold nothing of their own. Enter it on an account under {account} instead."
            ),
            Self::ArchivedAccount { account } => write!(
                f,
                "The opening balance of {account} was left as it is because the account is archived, and an archived account's opening balance cannot be entered, changed or removed. Unarchive it first, then try again."
            ),
            Self::ArchivedMeanwhile { account } => write!(
                f,
                "{account} was archived while you were working, by another user or in another window, so its opening balance cannot be entered, changed or removed now. Unarchive it first, then try again."
            ),
            // One refusal has one sentence, whether a form or the operation alone meets it, and
            // a sentence that can stand under a box names no account (the pause's P6).
            Self::ZeroAmount { .. } => f.write_str(
                "The opening balance was not saved because the amount is zero, and an account that starts at zero needs no opening balance. There is nothing to enter: leave it without one.",
            ),
            Self::AlreadyExists {
                account,
                existing,
                digits,
            } => write!(
                f,
                "Your opening balance for {account} was not saved because one was entered while you were working, by another user or in another window: it is {} on {}.",
                existing.amount.display(*digits),
                existing.date
            ),
            Self::DateAfterToday { today, .. } => write!(
                f,
                "The opening balance was not saved because the date is after today, {today}, and an opening balance cannot be for a day that has not come. Pick today or an earlier day, then save again."
            ),
            Self::TooLarge { digits, .. } => write!(
                f,
                "The opening balance was not saved because the amount is more than Nage takes for one amount: at most {} either way. Check it for a typing mistake, then save again.",
                Money::largest_taken(*digits).display(*digits)
            ),
            Self::NoSuchOpeningBalance { .. } => f.write_str(
                "Your change to the opening balance was not saved because it was removed while you were working, by another user or in another window.",
            ),
            Self::AlreadyRemoved { .. } => f.write_str(
                "The opening balance was not removed because it was already removed, by another user or in another window. There is nothing left to remove.",
            ),
            Self::Stale {
                account,
                saved,
                digits,
            } => write!(
                f,
                "Your change to the opening balance of {account} was not saved because it was changed while you were working, by another user or in another window: it is now {} on {}.",
                saved.amount.display(*digits),
                saved.date
            ),
            Self::RemoveStale {
                account,
                saved,
                digits,
            } => write!(
                f,
                "The opening balance of {account} was not removed because it was changed while you were working, by another user or in another window: it is now {} on {}.",
                saved.amount.display(*digits),
                saved.date
            ),
            // No entry number: the id is one a change or a removal names in its request
            // (decision 145, the pause's P6).
            Self::Corrupt => f.write_str(
                "The opening balance could not be read because what is stored for it is damaged: it holds an amount Nage cannot show. Ask whoever looks after Nage to run nage check and to restore a backup if it reports a problem.",
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

/// Whether `account` takes an opening balance (SPEC 6.6, INV-8, INV-13): an account under Assets
/// or Liabilities that is neither a section nor built in. The opening-balance pages read this rule
/// from the core ([`crate::chart::opening_balance_account`]) and never rebuild it.
pub(crate) fn account_takes_opening_balance(account: &Account) -> bool {
    takes_opening_balance(account.kind) && !account.is_root() && !account.is_system()
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
        let amount = balances::natural(posting.amount, kind).ok_or(OpeningBalanceError::Corrupt)?;
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

/// The rules a new date and amount meet in the create and the edit (SPEC 6.6, decisions 100 and
/// 142): not zero (`ZeroAmount`), not after `today` (`DateAfterToday`), and taken by a form
/// (`TooLarge`: more than [`Money::largest_taken`] above or below zero, whatever a form sends,
/// which also covers the most negative amount, whose stored form on a Liabilities account or
/// whose counter posting does not fit); then the two stored amounts, the account's and its negation on `Equity:Opening
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
    if !amount.is_taken(digits) {
        return Err(too_large());
    }
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

/// What a save or a removal from the opening-balance pages names (decision 140): nothing, on
/// the form that enters one; or the entry and the version the change form or the confirmation
/// showed, each `None` when the request sent a value that is not a number as Nage prints one
/// (an entry that is no number names no entry; a version that is no number is no version saved).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningNamed {
    New,
    Entry {
        id: Option<TransactionId>,
        version: Option<i64>,
    },
}

/// The opening-balance form as a request sends it (decisions 140 and 141, the pause's P3): the
/// date box and the amount box as typed, the tick box as sent, and what the form names. The web
/// hands the boxes on untouched; the core applies the tick ([`save_opening_balance_form`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpeningForm {
    pub date: String,
    pub amount: String,
    /// The tick box under the amount box ("The account was overdrawn", "The account was in
    /// credit"): ticked, a good amount above zero is saved below zero; with a typed minus it is
    /// the two-signs refusal; a zero stays a zero.
    pub below_zero: bool,
    pub named: OpeningNamed,
}

/// What a save from the opening-balance form did (decisions 140 and 143).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningSaved {
    /// Created (version 1) or changed in place (one version more): the entry as saved now.
    Saved(OpeningBalance),
    /// The account has a live opening balance and the form sent its date and its amount, the
    /// tick applied: nothing was written, whatever entry and version the form named or did not
    /// name (decision 100, the pause's P2): a second tap on Save, a stale change holding the
    /// saved values, a form naming a replaced entry with the saved values.
    Nothing,
    /// A zero typed on an account that has an opening balance: nothing was written, and the page
    /// asks whether to remove this entry, as saved now (decision 143).
    AskRemove(OpeningBalance),
}

/// What a removal from the confirmation did (decision 143).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpeningRemoved {
    /// The entry was removed: the transaction as stored now, deleted.
    Removed(Transaction),
    /// Nothing to remove: the account has no live opening balance, whatever the removal named;
    /// nothing was written and no message is set (the pause's P2).
    Nothing,
}

/// Why one box of the opening-balance form was refused (decisions 142 and 145): each says what
/// was not saved, why and what to do, never repeating what was typed, never a number a request
/// chose, and never the account's name, which the page shows under its heading (the pause's P6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpeningBoxRefusal {
    /// The date box as [`Date::from_box`] refused it.
    Date(DateError),
    /// A date after `today`, the server's day in the configured zone.
    DateAfterToday { today: Date },
    /// The amount as [`Money::parse`] refused it, but for too many decimals and an amount past
    /// 64 bits, which have their own sentences; `digits` are the book's, whose example the
    /// sentence gives ([`ParseMoneyError::because`]).
    Amount {
        error: ParseMoneyError,
        digits: MinorDigits,
    },
    /// More decimals than the book's currency, named by its code, has.
    Decimals { currency: String, allowed: u8 },
    /// A minus and the tick box together.
    TwoSigns,
    /// More than a form takes ([`Money::is_taken`]), an amount past 64 bits included.
    TooLarge { digits: MinorDigits },
    /// A zero on an account that has no opening balance yet (decision 143).
    Zero,
}

impl fmt::Display for OpeningBoxRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(DateError::Empty) => f.write_str(
                "The opening balance was not saved because the date box is empty, and an opening balance needs the day it is for: Nage never guesses it. Pick the day, then save again.",
            ),
            Self::Date(error) => write!(f, "The opening balance was not saved because {error}"),
            Self::DateAfterToday { today } => write!(
                f,
                "The opening balance was not saved because the date is after today, {today}, and an opening balance cannot be for a day that has not come. Pick today or an earlier day, then save again."
            ),
            Self::Amount {
                error: ParseMoneyError::Empty,
                digits,
            } => write!(
                f,
                "The opening balance was not saved because the amount box holds no amount, and an opening balance needs its amount: Nage never guesses it. Type it as {}, then save again.",
                ParseMoneyError::example(*digits)
            ),
            Self::Amount { error, digits } => write!(
                f,
                "The opening balance was not saved because {}",
                error.because(*digits)
            ),
            Self::Decimals {
                currency,
                allowed: 0,
            } => write!(
                f,
                "The opening balance was not saved because the amount has decimals, and amounts in {currency} have none: Nage never rounds. Type it without decimals, then save again."
            ),
            Self::Decimals { currency, allowed } => write!(
                f,
                "The opening balance was not saved because the amount has more decimals than amounts in {currency}, which have {allowed}: Nage never rounds. Type it with at most {allowed}, then save again."
            ),
            Self::TwoSigns => f.write_str(
                "The opening balance was not saved because the amount has a minus and the box under it is ticked, and one of the two is enough: each makes the amount less than nothing. Take away the minus or untick the box, then save again.",
            ),
            Self::TooLarge { digits } => write!(
                f,
                "The opening balance was not saved because the amount is more than Nage takes for one amount: at most {} either way. Check it for a typing mistake, then save again.",
                Money::largest_taken(*digits).display(*digits)
            ),
            Self::Zero => f.write_str(
                "The opening balance was not saved because the amount is zero, and an account that starts at zero needs no opening balance. There is nothing to enter: leave it without one.",
            ),
        }
    }
}

/// Every refusal of one opening-balance form at once (decision 145, the maintainer's ruling
/// "Every problem at once"): at most one per box, the date's then the amount's, each shown
/// under its box; a refusal that belongs to no box (the account archived meanwhile,
/// [`OpeningBalanceError::ArchivedMeanwhile`]) is `top`, shown at the top of the account's
/// page. Nothing is written when any is present.
#[derive(Debug, Default)]
pub struct OpeningRefusals {
    pub top: Option<OpeningBalanceError>,
    pub date: Option<OpeningBoxRefusal>,
    pub amount: Option<OpeningBoxRefusal>,
}

impl OpeningRefusals {
    /// Whether no box was refused.
    pub fn is_empty(&self) -> bool {
        self.top.is_none() && self.date.is_none() && self.amount.is_none()
    }
}

/// A save or a removal that met another save or removal made while the page was open
/// (decision 144): 409, nothing written, what is saved now carried for the page. Each is judged
/// against the live opening balance of the account in the address, never against the entry a
/// form names (the pause's P2): an entry of another account is never shown, changed or removed.
/// Its sentence stands at the top of the page and names the account by its full place; it ends
/// after the why, since the page offers the choices (decision 145).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpeningClash {
    /// A save named an entry, well formed or not, while the account's live opening balance is
    /// another entry or stands at another version: `saved` is the live one, whose entry and
    /// version Save mine names. Also the answer when the entry named was removed and one entered
    /// anew meanwhile, and when a crafted form names another account's entry.
    Changed {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
    /// A save named an entry while the account has no live opening balance; never answered
    /// while the account has one.
    Removed { account: String },
    /// A save that names no entry met an account that has a live opening balance now (INV-13):
    /// `saved` is that one.
    Entered {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
    /// A removal named anything but the live entry at its version (another entry, another
    /// version, a malformed or missing entry or version) while the account has one: `saved` is
    /// the live one, whose entry and version the confirmation shown again names.
    RemoveChanged {
        account: String,
        saved: OpeningBalance,
        digits: MinorDigits,
    },
}

impl OpeningClash {
    /// The entry saved now, which the page shows under Saved now; `None` after a removal.
    pub fn saved(&self) -> Option<&OpeningBalance> {
        match self {
            Self::Changed { saved, .. }
            | Self::Entered { saved, .. }
            | Self::RemoveChanged { saved, .. } => Some(saved),
            Self::Removed { .. } => None,
        }
    }
}

impl fmt::Display for OpeningClash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed {
                account,
                saved,
                digits,
            } => write!(
                f,
                "Your change to the opening balance of {account} was not saved because it was changed while you were working, by another user or in another window: it is now {} on {}.",
                saved.amount.display(*digits),
                saved.date
            ),
            Self::Removed { account } => write!(
                f,
                "Your change to the opening balance of {account} was not saved because it was removed while you were working, by another user or in another window."
            ),
            Self::Entered {
                account,
                saved,
                digits,
            } => write!(
                f,
                "Your opening balance for {account} was not saved because one was entered while you were working, by another user or in another window: it is {} on {}.",
                saved.amount.display(*digits),
                saved.date
            ),
            Self::RemoveChanged {
                account,
                saved,
                digits,
            } => write!(
                f,
                "The opening balance of {account} was not removed because it was changed while you were working, by another user or in another window: it is now {} on {}.",
                saved.amount.display(*digits),
                saved.date
            ),
        }
    }
}

/// Why a save or a removal from the opening-balance pages was not done.
#[derive(Debug)]
pub enum OpeningFormError {
    /// Every refusal of the form at once (decision 145): 422.
    Refused(Box<OpeningRefusals>),
    /// Another save or removal came first (decision 144): 409.
    Clash(Box<OpeningClash>),
    /// Not a refusal: an account that does not exist or cannot have an opening balance (the
    /// page for an unknown address answers it first), a book not set up, a missing built-in
    /// account or a database failure (decision 102).
    Failed(OpeningBalanceError),
}

/// Saves the opening-balance form of `account` by `by` at `now`, `today` being the server's day
/// in the configured zone (decisions 140 to 145, the pause's P2 and P3), in `tx`, one write all
/// or none. It looks up the live opening balance of the account in the address (the live one)
/// and compares it with what the form names; it never fetches the entry a form names by its id.
/// The amount box is parsed by [`Money::parse`] as it stands (white space and the dollar sign
/// where that module allows them); the tick then applies: ticked with a typed minus, wherever
/// the module takes one, it is the two-signs refusal; ticked with a good amount above zero, that
/// amount below zero (checked negation); a zero stays a zero whatever the tick. Judged in this
/// order, each before anything is written:
///
/// 1. The account must exist and take an opening balance (not a section, under Assets or
///    Liabilities), else `Failed`; archived, it is `Refused` with
///    [`OpeningBalanceError::ArchivedMeanwhile`] at the top and no box judged.
/// 2. Nothing to do: the account has a live one and both boxes are good and, the tick applied,
///    equal its date and its amount: `Nothing`, nothing written, whatever entry and version the
///    form names or does not name (decision 100 made whole: a second tap on Save opening
///    balance or on Save as a new opening balance, a stale change holding the saved values, a
///    form naming a replaced entry with the saved values).
/// 3. A clash, judged before the boxes' problems and carrying what is saved now: no entry named
///    and a live one, `Entered`; an entry named, well formed or not, and a live one that is
///    another entry or stands at another version, `Changed` with the live one (also when the
///    entry named was removed and one entered anew, and when a crafted form names another
///    account's entry); an entry named and no live one, `Removed`.
/// 4. Otherwise every box at once ([`OpeningRefusals`]): the date box by [`Date::from_box`],
///    then not after `today`; the amount box by [`Money::parse`] with the book's digits, two
///    signs, more than a form takes ([`Money::is_taken`], an amount past 64 bits included). A
///    zero in the amount box on an account with a live one is no refusal but `AskRemove` with
///    the live one, whatever the date box holds (a removal needs no date, so the date shows no
///    problem); on an account without one, the zero's refusal under the amount box, beside the
///    date's own problem when it has one (decision 143).
/// 5. Then no entry named and none live creates it (version 1), and the live entry named at its
///    version is changed in place (the version raised by exactly one, INV-7), as
///    [`create_opening_balance`] and [`edit_opening_balance`] do, with their stamps (INV-26).
///
/// Two creates at once leave one opening balance: the second meets the first in step 2 when it
/// holds the same boxes, else in step 3 (INV-13).
pub async fn save_opening_balance_form(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    today: Date,
    account: AccountId,
    form: &OpeningForm,
) -> Result<OpeningSaved, OpeningFormError> {
    let book = form_account(tx, account).await?;
    let digits = book.digits;
    let live = opening_balance(tx, account)
        .await
        .map_err(OpeningFormError::Failed)?;
    let date = Date::from_box(&form.date);
    let amount = form_amount(&form.amount, form.below_zero, digits, &book.currency);
    if let (Some(saved), Ok(date), Ok(amount)) = (live, &date, &amount)
        && *date == saved.date
        && *amount == saved.amount
    {
        return Ok(OpeningSaved::Nothing);
    }
    let clash = match (form.named, live) {
        (OpeningNamed::New, Some(saved)) => Some(OpeningClash::Entered {
            account: book.name.clone(),
            saved,
            digits,
        }),
        (OpeningNamed::Entry { id, version }, Some(saved))
            if id != Some(saved.id) || version != Some(saved.version) =>
        {
            Some(OpeningClash::Changed {
                account: book.name.clone(),
                saved,
                digits,
            })
        }
        (OpeningNamed::Entry { .. }, None) => Some(OpeningClash::Removed {
            account: book.name.clone(),
        }),
        _ => None,
    };
    if let Some(clash) = clash {
        return Err(OpeningFormError::Clash(Box::new(clash)));
    }
    let mut refusals = OpeningRefusals::default();
    match &amount {
        Ok(zero) if *zero == Money::ZERO => match live {
            Some(saved) => return Ok(OpeningSaved::AskRemove(saved)),
            None => refusals.amount = Some(OpeningBoxRefusal::Zero),
        },
        Ok(_) => {}
        Err(refusal) => refusals.amount = Some(refusal.clone()),
    }
    match date {
        Err(error) => refusals.date = Some(OpeningBoxRefusal::Date(error)),
        Ok(date) if date > today => {
            refusals.date = Some(OpeningBoxRefusal::DateAfterToday { today })
        }
        Ok(_) => {}
    }
    let (Ok(date), Ok(amount), true) = (date, amount, refusals.is_empty()) else {
        return Err(OpeningFormError::Refused(Box::new(refusals)));
    };
    let saved = match live {
        None => {
            create_opening_balance(
                tx,
                by,
                now,
                today,
                &NewOpeningBalance {
                    account,
                    date,
                    amount,
                },
            )
            .await
        }
        Some(saved) => {
            edit_opening_balance(
                tx,
                by,
                now,
                today,
                &OpeningBalanceEdit {
                    id: saved.id,
                    expected_version: saved.version,
                    date,
                    amount,
                },
            )
            .await
        }
    };
    saved
        .map(OpeningSaved::Saved)
        .map_err(OpeningFormError::Failed)
}

/// The account in the address of an opening-balance page as the form-shaped operations judge it
/// (decision 140): the book's digits and currency code, and the account's full place.
struct FormAccount {
    digits: MinorDigits,
    currency: String,
    name: String,
}

/// Judges the account `id` for the form-shaped operations, before anything else (decision 140):
/// a book that is not set up, an account that does not exist or takes no opening balance (a
/// section, an account outside Assets and Liabilities, a built-in account) is `Failed` with the
/// core's own refusal; an archived one is `Refused` with [`OpeningBalanceError::ArchivedMeanwhile`]
/// at the top and no box judged.
async fn form_account(
    tx: &mut WriteTransaction,
    id: AccountId,
) -> Result<FormAccount, OpeningFormError> {
    let failed = |error: OpeningBalanceError| OpeningFormError::Failed(error);
    let book = book::book(tx)
        .await
        .map_err(|error| failed(error.into()))?
        .ok_or(failed(OpeningBalanceError::NotInitialized))?;
    let account = accounts::account(tx, id)
        .await
        .map_err(|error| failed(error.into()))?
        .ok_or(failed(OpeningBalanceError::NoSuchAccount { account: id }))?;
    let name = accounts::full_name(tx, account.id)
        .await
        .map_err(|error| failed(error.into()))?;
    if account.is_root() {
        return Err(failed(OpeningBalanceError::RootAccount { account: name }));
    }
    if !account_takes_opening_balance(&account) {
        return Err(failed(OpeningBalanceError::NotAssetOrLiability {
            account: name,
            kind: account.kind,
        }));
    }
    if account.is_archived() {
        return Err(OpeningFormError::Refused(Box::new(OpeningRefusals {
            top: Some(OpeningBalanceError::ArchivedMeanwhile { account: name }),
            ..OpeningRefusals::default()
        })));
    }
    Ok(FormAccount {
        digits: book.minor_digits,
        currency: book.currency_code,
        name,
    })
}

/// The amount box as typed, read by [`Money::parse`] with the book's `digits`, the tick then
/// applied (decision 141, the pause's P3): ticked with a typed minus, wherever the money module
/// takes one, is `TwoSigns`; ticked, a good amount above zero is that amount below zero; a zero
/// stays a zero. Too many decimals name the book's `currency`; an amount past 64 bits and one past
/// [`Money::is_taken`] are `TooLarge` (decision 142).
fn form_amount(
    text: &str,
    below_zero: bool,
    digits: MinorDigits,
    currency: &str,
) -> Result<Money, OpeningBoxRefusal> {
    let typed = Money::parse(text, digits).map_err(|error| match error {
        ParseMoneyError::TooManyDecimals { allowed } => OpeningBoxRefusal::Decimals {
            currency: currency.to_owned(),
            allowed,
        },
        ParseMoneyError::Overflow => OpeningBoxRefusal::TooLarge { digits },
        error => OpeningBoxRefusal::Amount { error, digits },
    })?;
    let amount = if below_zero {
        if typed < Money::ZERO {
            return Err(OpeningBoxRefusal::TwoSigns);
        }
        typed
            .checked_neg()
            .ok_or(OpeningBoxRefusal::TooLarge { digits })?
    } else {
        typed
    };
    if amount.is_taken(digits) {
        Ok(amount)
    } else {
        Err(OpeningBoxRefusal::TooLarge { digits })
    }
}

/// Removes the opening balance of `account` from the confirmation by `by` at `now` (decisions
/// 140 and 143, the pause's P2), in `tx`, one write all or none, judged against the account's
/// live opening balance, never against the entry named: the account must exist and take an
/// opening balance, else `Failed`; archived, `Refused` with
/// [`OpeningBalanceError::ArchivedMeanwhile`] at the top; no live one is `Nothing`, nothing
/// written, whatever is named; the live entry named at its version is removed as
/// [`remove_opening_balance`] removes it (the deleted pair, the version raised by exactly one);
/// anything else while one is live (another entry, another version, a malformed or missing
/// entry or version) is the clash `RemoveChanged` with the live one, nothing written. A removal
/// never passes silently while the account keeps an opening balance.
pub async fn remove_opening_balance_form(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    named: OpeningNamed,
) -> Result<OpeningRemoved, OpeningFormError> {
    let book = form_account(tx, account).await?;
    let Some(saved) = opening_balance(tx, account)
        .await
        .map_err(OpeningFormError::Failed)?
    else {
        return Ok(OpeningRemoved::Nothing);
    };
    match named {
        OpeningNamed::Entry {
            id: Some(id),
            version: Some(version),
        } if id == saved.id && version == saved.version => {
            remove_opening_balance(tx, by, now, id, version)
                .await
                .map(OpeningRemoved::Removed)
                .map_err(OpeningFormError::Failed)
        }
        _ => Err(OpeningFormError::Clash(Box::new(
            OpeningClash::RemoveChanged {
                account: book.name,
                saved,
                digits: book.digits,
            },
        ))),
    }
}
