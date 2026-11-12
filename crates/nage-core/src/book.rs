//! The book row (SPEC 5.1, 6.10, INV-5's Core half, decisions 33, 56, 87 and 96): one row,
//! written once at setup with the currency chosen and its minor digits, and never changed by any
//! core function; the digits are kept as set up even if a later currency table changes them.

use std::fmt;

use crate::currency::Currency;
use crate::fold::NameError;
use crate::money::MinorDigits;
use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;

/// The book as stored: its name, its currency code and the minor digits set at setup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub name: String,
    pub currency_code: String,
    pub minor_digits: MinorDigits,
    pub created_at: Timestamp,
}

/// Writes the book row at `now` (W1 step 3, INV-5's Core half): refused as `AlreadyInitialized`
/// when a row exists; the name is normalized and checked (`fold::checked_name` with a colon
/// allowed); the currency's code and digits come from the table entry the caller chose.
pub async fn create_book(
    tx: &mut WriteTransaction,
    now: Timestamp,
    name: &str,
    currency: Currency,
) -> Result<Book, BookError> {
    let name = fold::checked_name(name, Colon::Allowed).map_err(BookError::Name)?;
    if book(tx).await?.is_some() {
        return Err(BookError::AlreadyInitialized);
    }
    let code = currency.code();
    let digits = i64::from(currency.minor_digits().get());
    sqlx::query!(
        "INSERT INTO book (id, name, currency, minor_digits, created_at) VALUES (1, ?1, ?2, ?3, ?4)",
        name,
        code,
        digits,
        now
    )
    .execute(&mut **tx)
    .await?;
    Ok(Book {
        name,
        currency_code: code.to_owned(),
        minor_digits: currency.minor_digits(),
        created_at: now,
    })
}

/// The book, or `None` while the database is not initialized (SPEC 5.2's "not initialized"
/// is this `None`). The digits are read back through `MinorDigits::new`, never from the
/// currency table (decision 87).
pub async fn book(conn: &mut sqlx::SqliteConnection) -> Result<Option<Book>, BookError> {
    let row = sqlx::query!(
        r#"SELECT name, currency, minor_digits AS "minor_digits!: i64", created_at AS "created_at!: Timestamp" FROM book WHERE id = 1"#
    )
    .fetch_optional(conn)
    .await?;
    row.map(|row| {
        let minor_digits = u8::try_from(row.minor_digits)
            .ok()
            .and_then(MinorDigits::new)
            .ok_or_else(|| BookError::Corrupt(format!("minor_digits is {}", row.minor_digits)))?;
        Ok(Book {
            name: row.name,
            currency_code: row.currency,
            minor_digits,
            created_at: row.created_at,
        })
    })
    .transpose()
}

/// Why the book could not be written or read (decision 96).
#[derive(Debug)]
pub enum BookError {
    /// The book row exists already (INV-5: the currency is set once).
    AlreadyInitialized,
    /// The book name is refused by the name rules.
    Name(NameError),
    /// A stored value the schema should have refused, such as minor digits outside 0 to 3.
    Corrupt(String),
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => f.write_str("the book is already set up"),
            Self::Name(error) => write!(f, "the book name {error}"),
            Self::Corrupt(what) => write!(f, "the book row is corrupt: {what}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for BookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::AlreadyInitialized | Self::Corrupt(_) => None,
        }
    }
}

impl From<sqlx::Error> for BookError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use crate::fold::{self, Colon};
