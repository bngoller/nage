//! The household `cargo xtask seed` writes for milestone 2, the book `fixtures/books/v0.2.0.sqlite`
//! (PLAN M2 slice 8, decision 161, the maintainer's pick "With the awkward corners"): made-up names
//! only, written through nage-core's operations alone (setup, users, accounts, opening balances),
//! never by raw SQL, at fixed instants, so that two runs write the same rows but for sqlx's
//! bookkeeping times and the salts of the password hashes. The clock is never read. Every text
//! value passes `scripts/fixture-guard.sh`: no digit run of seven or more, no eight-digit date and
//! no period-decimal amount in a text.
//!
//! ```text
//! Book "Household", CAD, starter accounts
//! Users: sam (Sam, admin), alex (Alex)
//!
//! Accounts
//!   Chequing             joint     2,450.00
//!   Savings              joint
//!     Holiday fund       joint     1,200.00
//!     Emergency fund     Sam       8,000.00
//!   Wallet               Alex      (none entered)
//!   Overdrawn account    Sam        -310.40
//!   Old account          Alex      archived
//!   Credit card          Alex        640.25 owed
//!   Car loan             joint    12,000.00 owed
//! Categories: one added, one renamed, one archived, one with a 64-character name
//! Opening balances dated 2026-01-01
//! ```

use std::fmt;

use nage_core::accounts::{self, AccountId, NewAccount};
use nage_core::currency::Currency;
use nage_core::date::Date;
use nage_core::money::Money;
use nage_core::password::{self, HashParams};
use nage_core::setup::{self, Setup};
use nage_core::storage::{Storage, WriteTransaction};
use nage_core::timestamp::Timestamp;
use nage_core::transactions::{self, NewOpeningBalance};
use nage_core::users::{self, NewUser, UserId};

/// The instant every row is stamped with: setup, every user and every account change.
pub(crate) const NOW: &str = "2026-01-02T12:00:00Z";
/// The day the operations take for today, in UTC: the opening balances are dated before it.
pub(crate) const TODAY: &str = "2026-01-02";
/// The date of every opening balance.
pub(crate) const OPENING_DATE: &str = "2026-01-01";
/// The book's name.
pub(crate) const BOOK_NAME: &str = "Household";
/// The book's currency, with the starter accounts.
pub(crate) const CURRENCY: &str = "CAD";
/// The test password of both users, hashed with SPEC 5.3's parameters and written down in
/// `fixtures/README.md` (assumption A15 of PLAN M2 slice 8).
pub(crate) const PASSWORD: &str = "household test password";

/// A user of the household.
pub(crate) struct Person {
    pub(crate) username: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) admin: bool,
}

/// The two users: sam, the admin who sets the book up, and alex, added by sam.
pub(crate) const PEOPLE: [Person; 2] = [
    Person {
        username: "sam",
        display_name: "Sam",
        admin: true,
    },
    Person {
        username: "alex",
        display_name: "Alex",
        admin: false,
    },
];

/// One change to the chart, in the order the seed makes it, by the user who makes it.
pub(crate) enum Change {
    /// Renames the account at `full_name`.
    Rename {
        full_name: &'static str,
        to: &'static str,
        by: &'static str,
    },
    /// Creates `name` under the account at `under`, owned by `owner` (`None`: joint).
    Create {
        name: &'static str,
        under: &'static str,
        owner: Option<&'static str>,
        by: &'static str,
    },
    /// Sets the owner of the account at `full_name`.
    Own {
        full_name: &'static str,
        owner: &'static str,
        by: &'static str,
    },
    /// Enters the opening balance of the account at `full_name`, dated [`OPENING_DATE`]: the
    /// amount as a person types it, in the natural sign (an asset below zero is a negative
    /// amount; a debt is what is owed).
    Open {
        full_name: &'static str,
        amount: &'static str,
        by: &'static str,
    },
    /// Archives the account at `full_name`.
    Archive {
        full_name: &'static str,
        by: &'static str,
    },
}

/// Every change after the set-up, in order (decision 161).
pub(crate) const CHANGES: &[Change] = &[
    Change::Rename {
        full_name: "Assets:Checking",
        to: "Chequing",
        by: "sam",
    },
    Change::Open {
        full_name: "Assets:Chequing",
        amount: "2450.00",
        by: "sam",
    },
    Change::Create {
        name: "Holiday fund",
        under: "Assets:Savings",
        owner: None,
        by: "sam",
    },
    Change::Open {
        full_name: "Assets:Savings:Holiday fund",
        amount: "1200.00",
        by: "sam",
    },
    Change::Create {
        name: "Emergency fund",
        under: "Assets:Savings",
        owner: Some("sam"),
        by: "sam",
    },
    Change::Open {
        full_name: "Assets:Savings:Emergency fund",
        amount: "8000.00",
        by: "sam",
    },
    Change::Rename {
        full_name: "Assets:Cash",
        to: "Wallet",
        by: "alex",
    },
    Change::Own {
        full_name: "Assets:Wallet",
        owner: "alex",
        by: "alex",
    },
    Change::Create {
        name: "Overdrawn account",
        under: "Assets",
        owner: Some("sam"),
        by: "sam",
    },
    Change::Open {
        full_name: "Assets:Overdrawn account",
        amount: "-310.40",
        by: "sam",
    },
    Change::Create {
        name: "Old account",
        under: "Assets",
        owner: Some("alex"),
        by: "alex",
    },
    Change::Archive {
        full_name: "Assets:Old account",
        by: "alex",
    },
    Change::Rename {
        full_name: "Liabilities:Credit Card",
        to: "Credit card",
        by: "alex",
    },
    Change::Own {
        full_name: "Liabilities:Credit card",
        owner: "alex",
        by: "alex",
    },
    Change::Open {
        full_name: "Liabilities:Credit card",
        amount: "640.25",
        by: "alex",
    },
    Change::Create {
        name: "Car loan",
        under: "Liabilities",
        owner: None,
        by: "sam",
    },
    Change::Open {
        full_name: "Liabilities:Car loan",
        amount: "12000.00",
        by: "sam",
    },
    Change::Create {
        name: "Pets",
        under: "Expenses",
        owner: None,
        by: "alex",
    },
    Change::Rename {
        full_name: "Expenses:Dining",
        to: "Eating out",
        by: "sam",
    },
    Change::Archive {
        full_name: "Expenses:Gifts",
        by: "sam",
    },
    Change::Create {
        name: "Repairs to the old farmhouse roof, gutters, porch and back fence",
        under: "Expenses",
        owner: None,
        by: "sam",
    },
];

/// Writes the household into `storage`, a freshly migrated database (decision 161): the set-up
/// with the starter accounts in [`CURRENCY`] by sam, alex added by sam, then every one of
/// [`CHANGES`] in its own write transaction, each through the core operation a page or a command
/// calls, at [`NOW`] and with today [`TODAY`]. Fails with the core's words.
pub(crate) async fn seed(storage: &Storage) -> Result<(), String> {
    let now: Timestamp = NOW
        .parse()
        .map_err(|error| format!("the instant {NOW} is not a timestamp: {error:?}"))?;
    let today: Date = TODAY
        .parse()
        .map_err(|error| format!("the day {TODAY} is not a date: {error:?}"))?;
    let opening_date: Date = OPENING_DATE
        .parse()
        .map_err(|error| format!("the day {OPENING_DATE} is not a date: {error:?}"))?;
    let currency = Currency::from_code(CURRENCY)
        .ok_or_else(|| format!("{CURRENCY} is not in the currency table"))?;

    let mut people: Vec<(&'static str, UserId)> = Vec::with_capacity(PEOPLE.len());
    let admin = PEOPLE
        .iter()
        .find(|person| person.admin)
        .ok_or("the household has no admin")?;
    let setup = Setup {
        book_name: String::from(BOOK_NAME),
        currency,
        admin: new_user(admin)?,
        starter_chart: true,
    };
    let mut tx = begin(storage).await?;
    let initialized = setup::initialize(&mut tx, now, &setup)
        .await
        .map_err(|error| failed("set the book up", error))?;
    commit(tx).await?;
    people.push((admin.username, initialized.admin.id));

    for person in PEOPLE
        .iter()
        .filter(|person| person.username != admin.username)
    {
        let new = new_user(person)?;
        let mut tx = begin(storage).await?;
        let user = users::create_user(&mut tx, Some(initialized.admin.id), now, &new)
            .await
            .map_err(|error| failed(&format!("add {}", person.username), error))?;
        commit(tx).await?;
        people.push((person.username, user.id));
    }
    let user = |username: &str| {
        people
            .iter()
            .find(|(name, _)| *name == username)
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("the household has no user {username}"))
    };

    for change in CHANGES {
        let mut tx = begin(storage).await?;
        match change {
            Change::Rename { full_name, to, by } => {
                let account = account_at(&mut tx, full_name).await?;
                accounts::rename_account(&mut tx, user(by)?, now, account, to)
                    .await
                    .map_err(|error| failed(&format!("rename {full_name} to {to}"), error))?;
            }
            Change::Create {
                name,
                under,
                owner,
                by,
            } => {
                let parent = account_at(&mut tx, under).await?;
                let owner = owner.map(user).transpose()?;
                let new = NewAccount {
                    name: String::from(*name),
                    parent,
                    owner,
                };
                accounts::create_account(&mut tx, user(by)?, now, &new)
                    .await
                    .map_err(|error| failed(&format!("create {name} under {under}"), error))?;
            }
            Change::Own {
                full_name,
                owner,
                by,
            } => {
                let account = account_at(&mut tx, full_name).await?;
                accounts::set_owner(&mut tx, user(by)?, now, account, Some(user(owner)?))
                    .await
                    .map_err(|error| failed(&format!("give {full_name} to {owner}"), error))?;
            }
            Change::Open {
                full_name,
                amount,
                by,
            } => {
                let account = account_at(&mut tx, full_name).await?;
                let amount = Money::parse(amount, currency.minor_digits())
                    .map_err(|error| format!("the amount {amount} is refused: {error:?}"))?;
                let new = NewOpeningBalance {
                    account,
                    date: opening_date,
                    amount,
                };
                transactions::create_opening_balance(&mut tx, user(by)?, now, today, &new)
                    .await
                    .map_err(|error| {
                        failed(&format!("enter the opening balance of {full_name}"), error)
                    })?;
            }
            Change::Archive { full_name, by } => {
                let account = account_at(&mut tx, full_name).await?;
                accounts::archive_account(&mut tx, user(by)?, now, account)
                    .await
                    .map_err(|error| failed(&format!("archive {full_name}"), error))?;
            }
        }
        commit(tx).await?;
    }
    Ok(())
}

/// The new user `person` is, with the test password hashed with SPEC 5.3's parameters, outside
/// any transaction.
fn new_user(person: &Person) -> Result<NewUser, String> {
    let password_hash = password::hash(HashParams::SPEC, PASSWORD)
        .map_err(|error| failed(&format!("hash the password of {}", person.username), error))?;
    Ok(NewUser {
        username: String::from(person.username),
        display_name: String::from(person.display_name),
        password_hash,
        is_admin: person.admin,
    })
}

/// The account at `full_name`, which must exist.
async fn account_at(tx: &mut WriteTransaction, full_name: &str) -> Result<AccountId, String> {
    accounts::resolve(tx, full_name)
        .await
        .map_err(|error| failed(&format!("find {full_name}"), error))?
        .map(|account| account.id)
        .ok_or_else(|| format!("the book has no account {full_name}"))
}

/// A write transaction on the writer.
async fn begin(storage: &Storage) -> Result<WriteTransaction, String> {
    storage
        .begin_write()
        .await
        .map_err(|error| failed("begin a write", error))
}

/// Commits `tx`.
async fn commit(tx: WriteTransaction) -> Result<(), String> {
    tx.commit()
        .await
        .map_err(|error| failed("commit a write", error))
}

/// The message of a step that failed: what could not be done, then the core's words.
fn failed(what: &str, error: impl fmt::Display) -> String {
    format!("cannot {what}: {error}")
}
