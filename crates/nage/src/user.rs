//! `nage user add` and `nage user reset-password` (SPEC W1 step 4, 5.2 and 5.3, decisions 93,
//! 96, 112 and 113): another user, and a new password for a user who forgot theirs, from the
//! server's terminal, each in the name of the admin `--by` names. Whoever can run commands on
//! the server can therefore act as any admin, as they can already read and change the data
//! file; `--by` records who acted. The screens for both arrive with milestone 8.

use std::path::PathBuf;

use crate::messages::{Done, Stop};
use crate::prompt::Source;

/// What `nage user add` was given.
pub(crate) struct Add {
    /// The data directory, resolved (`--data-dir`, `NAGE_DATA_DIR`, `./data`).
    pub(crate) data_dir: PathBuf,
    /// `--by`: the username of the admin who adds the user, as typed.
    pub(crate) by: String,
    /// `--username` of the new user, as typed.
    pub(crate) username: String,
    /// `--display-name` of the new user, as typed.
    pub(crate) display_name: String,
}

/// What `nage user reset-password` was given.
pub(crate) struct Reset {
    /// The data directory, resolved (`--data-dir`, `NAGE_DATA_DIR`, `./data`).
    pub(crate) data_dir: PathBuf,
    /// `--by`: the username of the admin who resets the password, as typed.
    pub(crate) by: String,
    /// `--username` of the user whose password is reset, as typed.
    pub(crate) username: String,
}

/// Adds a user who is not an admin (W1 step 4, decision 113) and answers [`Done::UserAdded`]
/// with the names as the core stored them. It goes in this order, and stops at the first
/// refusal:
///
/// 1. The names. The username and the display name are checked by the core's name rules and
///    refused in the core's words (`UserError::Username`, `UserError::DisplayName`).
/// 2. The database. It is opened by `database::open_existing`, by `check`'s rules: never
///    created, never migrated, and refused when it is missing, behind or ahead of this binary
///    (`Stop::database`: `Refusal::NoDatabase`, `Refusal::DatabaseBehind`,
///    `Refusal::DatabaseNewer`). One that holds no user is `Refusal::NotSetUp`.
/// 3. Who is named. `--by` is looked up by the fold of what was typed
///    (`users::user_by_username`): nobody is `Refusal::AddedByNobody`; a user who is not an
///    admin, or is deactivated, is refused in the core's words (`UserError::CreatorNotAdmin`).
///    A `--username` that somebody already has, by the fold, is refused in the core's words
///    (`UserError::UsernameTaken`).
/// 4. The password, only now: `prompt::new_password` for `Task::AddUser` and the username as
///    typed, then `nage_core::password::hash` with SPEC 5.3's parameters
///    (`HashParams::SPEC`), outside any transaction.
/// 5. The write. One write transaction runs `users::create_user` in the admin's name at the
///    system clock's instant, cut to the second, with `is_admin` false, and commits; the core
///    checks step 3 again there.
///
/// So everything that can be refused without the password is refused before the password is
/// asked for or read, and a command that is refused writes nothing. The storage is closed
/// before the function returns, whatever the outcome (decision 53).
pub(crate) async fn add(add: Add, source: &mut Source<'_>) -> Result<Done, Stop> {
    fold::checked_name(&add.username).map_err(UserError::Username)?;
    fold::checked_name(&add.display_name).map_err(UserError::DisplayName)?;
    let storage = open(&add.data_dir, Task::AddUser).await?;
    let done = add_in(&storage, &add, source).await;
    storage.close().await;
    done
}

/// Gives a user a new password in an admin's name (SPEC 5.3, decisions 112 and 113) and
/// answers [`Done::PasswordReset`] with both usernames as stored. It goes in this order, and
/// stops at the first refusal:
///
/// 1. The database, as [`add`] opens it; one that holds no user is `Refusal::NotSetUp`.
/// 2. Who is named, each looked up by the fold of what was typed. `--by`: nobody is
///    `Refusal::ResetByNobody`; a user who is not an admin, or is deactivated, is refused in
///    the core's words (`PasswordChangeError::NotAnAdmin`). `--username`: nobody is
///    `Refusal::UnknownUser`. The two may name the same user, so that the only admin, having
///    forgotten their password, can reset it here.
/// 3. The password, only now: `prompt::new_password` for `Task::ResetPassword` and the user's
///    username as stored, then `nage_core::password::hash` with SPEC 5.3's parameters,
///    outside any transaction.
/// 4. The write. One write transaction runs `users::reset_password` at the system clock's
///    instant, cut to the second, and commits: the new hash, who reset it and when, and every
///    session of the user ended. Nothing forces a change at the next sign-in.
///
/// A command that is refused changes nothing: the old password still signs in and every
/// session stays. The storage is closed before the function returns (decision 53).
pub(crate) async fn reset_password(reset: Reset, source: &mut Source<'_>) -> Result<Done, Stop> {
    let storage = open(&reset.data_dir, Task::ResetPassword).await?;
    let done = reset_in(&storage, &reset, source).await;
    storage.close().await;
    done
}

use std::path::Path;

use nage_core::fold;
use nage_core::password::{self, HashParams};
use nage_core::storage::{self, ReadTransaction, Storage};
use nage_core::timestamp::Timestamp;
use nage_core::users::{self, NewUser, User, UserError};

use crate::database;
use crate::messages::{Refusal, Task};
use crate::prompt;

/// The rest of [`add`] on the database it opened: whether it holds a user, then steps 3 to 5.
async fn add_in(storage: &Storage, add: &Add, source: &mut Source<'_>) -> Result<Done, Stop> {
    let mut read = read_set_up(storage, Task::AddUser).await?;
    let by = named(&mut read, &add.by)
        .await?
        .ok_or_else(|| Refusal::AddedByNobody {
            typed: add.by.clone(),
        })?;
    if !is_active_admin(&by) {
        return Err(UserError::CreatorNotAdmin {
            username: by.username,
        }
        .into());
    }
    if named(&mut read, &add.username).await?.is_some() {
        return Err(UserError::UsernameTaken.into());
    }
    // The read ends before the password is asked for, which may take a while.
    read.rollback().await.map_err(storage::Error::Sqlx)?;
    let password_hash = hashed_password(source, Task::AddUser, &add.username)?;
    let mut tx = storage.begin_write().await?;
    let now = Timestamp::from_jiff(jiff::Timestamp::now())?;
    let new = NewUser {
        username: add.username.clone(),
        display_name: add.display_name.clone(),
        password_hash,
        is_admin: false,
    };
    let user = users::create_user(&mut tx, Some(by.id), now, &new).await?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    Ok(Done::UserAdded {
        username: user.username,
        display_name: user.display_name,
        by: by.username,
    })
}

/// The database of `data_dir`, opened by `check`'s rules (`database::open_existing`) for
/// `task`: a database that is missing, behind or ahead is refused in the command's words.
async fn open(data_dir: &Path, task: Task) -> Result<Storage, Stop> {
    database::open_existing(data_dir)
        .await
        .map_err(|error| Stop::database(task, error))
}

/// A read of `storage` for `task`, refused as `Refusal::NotSetUp` while the database holds no
/// user.
async fn read_set_up(storage: &Storage, task: Task) -> Result<ReadTransaction, Stop> {
    let mut read = storage.begin_read().await?;
    let users = users::count_users(&mut read)
        .await
        .map_err(storage::Error::Sqlx)?;
    if users == 0 {
        return Err(Refusal::NotSetUp { task }.into());
    }
    Ok(read)
}

/// The user whose username has the fold of `typed`, or `None`.
async fn named(read: &mut ReadTransaction, typed: &str) -> Result<Option<User>, Stop> {
    let found = users::user_by_username(read, typed)
        .await
        .map_err(storage::Error::Sqlx)?;
    Ok(found.map(|credentials| credentials.user))
}

/// Whether `user` may act for `--by`: an admin who is not deactivated (decision 96).
fn is_active_admin(user: &User) -> bool {
    user.is_admin && user.is_enabled()
}

/// The password `task` sets for `username`, asked for or read, then hashed with SPEC 5.3's
/// parameters.
fn hashed_password(source: &mut Source<'_>, task: Task, username: &str) -> Result<String, Stop> {
    let password = prompt::new_password(source, task, username)?;
    Ok(password::hash(HashParams::SPEC, &password)?)
}

use nage_core::users::PasswordChangeError;

/// The rest of [`reset_password`] on the database it opened: whether it holds a user, then
/// steps 2 to 4.
async fn reset_in(storage: &Storage, reset: &Reset, source: &mut Source<'_>) -> Result<Done, Stop> {
    let mut read = read_set_up(storage, Task::ResetPassword).await?;
    let by = named(&mut read, &reset.by)
        .await?
        .ok_or_else(|| Refusal::ResetByNobody {
            typed: reset.by.clone(),
        })?;
    if !is_active_admin(&by) {
        return Err(PasswordChangeError::NotAnAdmin {
            username: by.username,
        }
        .into());
    }
    let user = named(&mut read, &reset.username)
        .await?
        .ok_or_else(|| Refusal::UnknownUser {
            typed: reset.username.clone(),
        })?;
    // The read ends before the password is asked for, which may take a while.
    read.rollback().await.map_err(storage::Error::Sqlx)?;
    let password_hash = hashed_password(source, Task::ResetPassword, &user.username)?;
    let mut tx = storage.begin_write().await?;
    let now = Timestamp::from_jiff(jiff::Timestamp::now())?;
    let user = users::reset_password(&mut tx, by.id, now, user.id, &password_hash).await?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    Ok(Done::PasswordReset {
        username: user.username,
        by: by.username,
    })
}
