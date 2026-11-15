//! `nage init` (SPEC W1 step 4, 5.2, decision 113): sets Nage up from the terminal as the
//! set-up page does, through the same core operation, before the server is ever started or
//! beside a server that has not been set up.

use std::path::PathBuf;

use crate::messages::{Done, Stop};
use crate::prompt::Source;

/// What `nage init` was given.
pub(crate) struct Init {
    /// The data directory, resolved (`--data-dir`, `NAGE_DATA_DIR`, `./data`).
    pub(crate) data_dir: PathBuf,
    /// `--book-name`, as typed.
    pub(crate) book_name: String,
    /// `--currency`, as typed; `None` when the flag was left out.
    pub(crate) currency: Option<String>,
    /// `--username` of the first user, as typed.
    pub(crate) username: String,
    /// `--display-name` of the first user, as typed.
    pub(crate) display_name: String,
    /// False with `--no-starter-accounts`.
    pub(crate) starter_chart: bool,
}

/// Sets Nage up (decision 113) and answers [`Done::SetUp`], with the names as the core stored
/// them and the database's absolute path. It goes in this order, and stops at the first
/// refusal:
///
/// 1. The currency. Without `--currency` nothing else is looked at:
///    `Refusal::CurrencyMissing`, a usage error. A code that is not in the currency table,
///    compared exactly as typed, is `Refusal::UnknownCurrency`.
/// 2. The names. The book name, the username and the display name are checked by the core's
///    name rules (`fold::checked_name` with a colon allowed) and refused in the core's words
///    (`BookError::Name`, `UserError::Username`, `UserError::DisplayName`).
/// 3. The database, when its file is there. It is opened by `database::open_existing`, by
///    `check`'s rules, so one behind or ahead of this binary is refused and is not migrated
///    (`Stop::database`: `Refusal::DatabaseBehind`, `Refusal::DatabaseNewer`); one that holds
///    a book or a user is refused as already set up (`SetupError::AlreadyInitialized`, the
///    book's message). A missing file is not created yet.
/// 4. The password, only now: `prompt::new_password` for `Task::SetUp` and the username as
///    typed, then `nage_core::password::hash` with SPEC 5.3's parameters
///    (`HashParams::SPEC`), outside any transaction.
/// 5. The write. A missing database is created and migrated (`database::create`); then one
///    write transaction runs `nage_core::setup::initialize` at the system clock's instant,
///    cut to the second, with the first user as an admin, and commits. Of two commands at once,
///    the second is refused there by the core as already set up when it found a database at
///    this binary's version; when it found the file the first is still creating, it stops at
///    step 3; and when it found none, `database::create` refuses the file the first created
///    (`OpenError::Create`).
///
/// So everything that can be refused without the password is refused before the password is
/// asked for or read, and a command that is refused or fails before step 5 leaves no trace:
/// no data directory, no database file, no row. The first user gets no session: they sign in
/// on the sign-in page. The storage is closed before the function returns, whatever the
/// outcome, so no `-wal` or `-shm` file stays behind (decision 53).
pub(crate) async fn run(init: Init, source: &mut Source<'_>) -> Result<Done, Stop> {
    let Some(code) = init.currency.as_deref() else {
        return Err(Refusal::CurrencyMissing.into());
    };
    let currency = Currency::from_code(code).ok_or_else(|| Refusal::UnknownCurrency {
        typed: code.to_owned(),
    })?;
    fold::checked_name(&init.book_name, Colon::Allowed).map_err(BookError::Name)?;
    fold::checked_name(&init.username, Colon::Allowed).map_err(UserError::Username)?;
    fold::checked_name(&init.display_name, Colon::Allowed).map_err(UserError::DisplayName)?;
    match database::open_existing(&init.data_dir).await {
        Ok(storage) => {
            let done = set_up_found(&storage, &init, currency, source).await;
            storage.close().await;
            done
        }
        Err(OpenError::Missing(_)) => {
            let password_hash = hashed_password(source, &init.username)?;
            let storage = database::create(&init.data_dir)
                .await
                .map_err(|error| Stop::database(Task::SetUp, error))?;
            let done = write(&storage, &init, currency, password_hash).await;
            storage.close().await;
            done
        }
        Err(error) => Err(Stop::database(Task::SetUp, error)),
    }
}

use nage_core::book::{self, BookError};
use nage_core::currency::Currency;
use nage_core::fold::{self, Colon};
use nage_core::password::{self, HashParams};
use nage_core::setup::{self, Setup, SetupError};
use nage_core::storage::{self, Storage};
use nage_core::timestamp::Timestamp;
use nage_core::users::{self, NewUser, UserError};

use crate::database::{self, OpenError};
use crate::messages::{Refusal, Task};
use crate::prompt;

/// Steps 3 to 5 of [`run`] on a database that is there and at this binary's version: refused
/// when it holds a book or a user, set up otherwise.
async fn set_up_found(
    storage: &Storage,
    init: &Init,
    currency: Currency,
    source: &mut Source<'_>,
) -> Result<Done, Stop> {
    let mut read = storage.begin_read().await?;
    let book = book::book(&mut read).await?;
    let users = users::count_users(&mut read)
        .await
        .map_err(storage::Error::Sqlx)?;
    // The read ends before the password is asked for, which may take a while.
    read.rollback().await.map_err(storage::Error::Sqlx)?;
    if book.is_some() || users > 0 {
        return Err(SetupError::AlreadyInitialized.into());
    }
    let password_hash = hashed_password(source, &init.username)?;
    write(storage, init, currency, password_hash).await
}

/// Step 4 of [`run`]: the first user's password, asked for or read, then hashed with SPEC
/// 5.3's parameters.
fn hashed_password(source: &mut Source<'_>, username: &str) -> Result<String, Stop> {
    let password = prompt::new_password(source, Task::SetUp, username)?;
    Ok(password::hash(HashParams::SPEC, &password)?)
}

/// Step 5 of [`run`], once the database exists: the set-up in one write transaction, stamped
/// with the system clock cut to the second.
async fn write(
    storage: &Storage,
    init: &Init,
    currency: Currency,
    password_hash: String,
) -> Result<Done, Stop> {
    let mut tx = storage.begin_write().await?;
    let now = Timestamp::from_jiff(jiff::Timestamp::now())?;
    let setup = Setup {
        book_name: init.book_name.clone(),
        currency,
        admin: NewUser {
            username: init.username.clone(),
            display_name: init.display_name.clone(),
            password_hash,
            is_admin: true,
        },
        starter_chart: init.starter_chart,
    };
    let initialized = setup::initialize(&mut tx, now, &setup).await?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    Ok(Done::SetUp {
        book_name: initialized.book.name,
        currency: currency.code(),
        starter_chart: init.starter_chart,
        database: database::path(&init.data_dir),
        username: initialized.admin.username,
        display_name: initialized.admin.display_name,
    })
}
