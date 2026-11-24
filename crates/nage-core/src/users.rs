//! Users (SPEC 5.1, 5.3, INV-26's Core half, decisions 92, 93, 96, 106, 112 and 114): the first
//! admin, created at setup with no creator, and every later user, created by an existing,
//! enabled admin; usernames unique by the fold of decision 95 and found by it at sign-in; one's
//! own password change and an admin's reset, each of which writes who changed the user and
//! when and ends the sessions decision 112 names. Passwords are hashed by the caller
//! (`crate::password`), outside the transaction; the core stores the hash as opaque text and
//! hands it back only through [`user_by_username`], for the caller to verify against.

use std::fmt;

use crate::fold::NameError;
use crate::sessions::SessionId;
use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;

/// A user's id: `users.id`, a plain integer primary key that is never reused, since users are
/// never deleted (SPEC 5.1 deactivates them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]
#[sqlx(transparent)]
pub struct UserId(pub i64);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A row of `users` as the core reads it (the password hash stays in the database).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    /// `None` for the first admin only (decision 92).
    pub created_by: Option<UserId>,
    pub created_at: Timestamp,
    /// Set together with `updated_at` at the first edit: a password change or reset
    /// (decision 112), and milestone 8's edits.
    pub updated_by: Option<UserId>,
    pub updated_at: Option<Timestamp>,
    /// Set together with `disabled_at` by a deactivation, cleared by a reactivation.
    pub disabled_by: Option<UserId>,
    pub disabled_at: Option<Timestamp>,
}

impl User {
    /// Whether the user is active: not deactivated (SPEC 5.1).
    pub fn is_enabled(&self) -> bool {
        self.disabled_at.is_none()
    }
}

/// What a new user is made of (W1, W14): the username and display name as typed, which
/// [`create_user`] normalizes, the password hash the caller computed, and whether the user is
/// an admin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewUser {
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub is_admin: bool,
}

/// Creates a user at `now` (SPEC 5.1, decision 96). `by` is `None` only for the first admin,
/// while the users table is empty (then `new.is_admin` must be true: `FirstUserMustBeAdmin`);
/// every later user names its creator, an existing, enabled admin (`CreatorRequired`,
/// `NoSuchCreator`, `CreatorNotAdmin`). The username and display name are normalized and
/// checked (`fold::checked_name` with a colon allowed), the password hash must not be empty, and
/// the username must be new by the fold (`UsernameTaken`). The row's `updated_*` and
/// `disabled_*` pairs start empty (decision 92). Returns the row as stored.
pub async fn create_user(
    tx: &mut WriteTransaction,
    by: Option<UserId>,
    now: Timestamp,
    new: &NewUser,
) -> Result<User, UserError> {
    let username =
        fold::checked_name(&new.username, Colon::Allowed).map_err(UserError::Username)?;
    let display_name =
        fold::checked_name(&new.display_name, Colon::Allowed).map_err(UserError::DisplayName)?;
    if new.password_hash.is_empty() {
        return Err(UserError::PasswordHashEmpty);
    }
    let existing = count_users(tx).await?;
    match by {
        None if existing > 0 => return Err(UserError::CreatorRequired),
        None if !new.is_admin => return Err(UserError::FirstUserMustBeAdmin),
        None => {}
        Some(creator) => match user(tx, creator).await? {
            None => return Err(UserError::NoSuchCreator),
            Some(creator) if !creator.is_admin || !creator.is_enabled() => {
                return Err(UserError::CreatorNotAdmin {
                    username: creator.username,
                });
            }
            Some(_) => {}
        },
    }
    let folded = fold::fold(&username);
    let taken = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM users WHERE username_folded = ?1"#,
        folded
    )
    .fetch_one(&mut **tx)
    .await?;
    if taken > 0 {
        return Err(UserError::UsernameTaken);
    }
    let id = sqlx::query!(
        "INSERT INTO users (username, username_folded, display_name, password_hash, is_admin, created_by, created_at, updated_by, updated_at, disabled_by, disabled_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, NULL)",
        username,
        folded,
        display_name,
        new.password_hash,
        new.is_admin,
        by,
        now
    )
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    user(tx, UserId(id))
        .await?
        .ok_or(UserError::Database(sqlx::Error::RowNotFound))
}

/// A user with the stored password hash, as a sign-in and the commands read it by username
/// (decision 106). Its `Debug` form never shows the hash.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub user: User,
    /// `users.password_hash` as stored: a PHC string `password::verify` reads.
    pub password_hash: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("user", &self.user)
            .field("password_hash", &"<hidden>")
            .finish()
    }
}

/// The user whose username has the fold of `typed` (decisions 95 and 106), with the stored
/// hash, or `None`: a sign-in and the commands' `--by` and `--username` find a user by what was
/// typed, in any case and any spelling the fold makes equal. A deactivated user is found like
/// any other (`user.is_enabled()` says so). A text that is no user's username, an empty one
/// included, finds nobody and is never an error.
pub async fn user_by_username(
    conn: &mut sqlx::SqliteConnection,
    typed: &str,
) -> Result<Option<Credentials>, sqlx::Error> {
    let folded = fold::fold(typed);
    let row = sqlx::query_as!(
        CredentialsRow,
        r#"SELECT id AS "id!: UserId", username, display_name, is_admin AS "is_admin!: bool", created_by AS "created_by?: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by?: UserId", updated_at AS "updated_at?: Timestamp", disabled_by AS "disabled_by?: UserId", disabled_at AS "disabled_at?: Timestamp", password_hash FROM users WHERE username_folded = ?1"#,
        folded
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(Credentials::from))
}

/// One's own password change at `now` (SPEC 5.3, decisions 93 and 112), after the caller
/// verified the current password against `verified.password_hash` and hashed the new one, both
/// outside the transaction. Reads the user again and refuses unless the user still exists
/// (`NoSuchUser`), is still active (`Deactivated`) and still holds the hash that was verified
/// (`ChangedMeanwhile`: a reset or another change committed in between wins, as it does at a
/// sign-in); an empty new hash is refused too (`PasswordHashEmpty`). Then stores
/// `password_hash`, writes `updated_by` (the user) and `updated_at`, and ends every other session
/// of the user while keeping `keep`, the session in use. A refusal writes nothing. Returns the
/// row as stored.
pub async fn change_own_password(
    tx: &mut WriteTransaction,
    verified: &Credentials,
    now: Timestamp,
    password_hash: &str,
    keep: SessionId,
) -> Result<User, PasswordChangeError> {
    let id = verified.user.id;
    let Some(current) = credentials(tx, id).await? else {
        return Err(PasswordChangeError::NoSuchUser { user: id });
    };
    if !current.user.is_enabled() {
        return Err(PasswordChangeError::Deactivated {
            username: current.user.username,
        });
    }
    if current.password_hash != verified.password_hash {
        return Err(PasswordChangeError::ChangedMeanwhile {
            username: current.user.username,
        });
    }
    if password_hash.is_empty() {
        return Err(PasswordChangeError::PasswordHashEmpty);
    }
    store_password(tx, id, password_hash, id, now).await?;
    sessions::end_other_sessions(tx, id, keep).await?;
    reload(tx, id).await
}

/// An admin's reset of `user`'s password at `now` (SPEC 5.3, decisions 93, 96 and 112), after
/// the caller hashed the new one outside the transaction: stores `password_hash`, writes
/// `updated_by` (the admin) and `updated_at`, and ends every session of the user. `by` must be
/// an existing (`NoSuchAdmin`), enabled admin (`NotAnAdmin`) and may be the user being reset,
/// so that an admin who forgot their password recovers at the server's terminal; `user` must
/// exist (`NoSuchUser`); the hash must not be empty (`PasswordHashEmpty`); a refusal writes
/// nothing. Nothing forces a change at the next sign-in. Returns the row as stored.
pub async fn reset_password(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    user: UserId,
    password_hash: &str,
) -> Result<User, PasswordChangeError> {
    let Some(admin) = self::user(tx, by).await? else {
        return Err(PasswordChangeError::NoSuchAdmin { admin: by });
    };
    if !admin.is_admin || !admin.is_enabled() {
        return Err(PasswordChangeError::NotAnAdmin {
            username: admin.username,
        });
    }
    if self::user(tx, user).await?.is_none() {
        return Err(PasswordChangeError::NoSuchUser { user });
    }
    if password_hash.is_empty() {
        return Err(PasswordChangeError::PasswordHashEmpty);
    }
    store_password(tx, user, password_hash, by, now).await?;
    sessions::end_user_sessions(tx, user).await?;
    reload(tx, user).await
}

/// How many users exist.
pub async fn count_users(conn: &mut sqlx::SqliteConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM users"#)
        .fetch_one(conn)
        .await
}

/// The user `id`, or `None`.
pub async fn user(
    conn: &mut sqlx::SqliteConnection,
    id: UserId,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        r#"SELECT id AS "id!: UserId", username, display_name, is_admin AS "is_admin!: bool", created_by AS "created_by?: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by?: UserId", updated_at AS "updated_at?: Timestamp", disabled_by AS "disabled_by?: UserId", disabled_at AS "disabled_at?: Timestamp" FROM users WHERE id = ?1"#,
        id
    )
    .fetch_optional(conn)
    .await
}

/// Every user, active and deactivated, in the order they were created (by id: users are never
/// deleted and ids never reused): what the account pages' owner lists and colours read
/// (decisions 132 and 138).
pub async fn users(conn: &mut sqlx::SqliteConnection) -> Result<Vec<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        r#"SELECT id AS "id!: UserId", username, display_name, is_admin AS "is_admin!: bool", created_by AS "created_by?: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by?: UserId", updated_at AS "updated_at?: Timestamp", disabled_by AS "disabled_by?: UserId", disabled_at AS "disabled_at?: Timestamp" FROM users ORDER BY id"#
    )
    .fetch_all(conn)
    .await
}

/// Why a user could not be created (decisions 96 and 114). Each message says what was refused,
/// why and what to do (decision 102) and can be shown beside a form field (W2) or on a command's
/// error line as it stands; a database failure is not a refusal and passes its message on.
#[derive(Debug)]
pub enum UserError {
    /// The username is refused by the name rules.
    Username(NameError),
    /// The display name is refused by the name rules.
    DisplayName(NameError),
    /// The password hash is empty.
    PasswordHashEmpty,
    /// The first user of the book must be an admin.
    FirstUserMustBeAdmin,
    /// A user exists, so the new one must name its creator.
    CreatorRequired,
    /// The creator does not exist.
    NoSuchCreator,
    /// The creator is not an admin, or is deactivated: `username` is the creator's, as stored.
    CreatorNotAdmin { username: String },
    /// Another user has this username, ignoring case.
    UsernameTaken,
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Username(error) => write!(
                f,
                "The user was not created because the username {}. {}",
                name_reason(*error),
                name_remedy("username", *error)
            ),
            Self::DisplayName(error) => write!(
                f,
                "The user was not created because the display name {}. {}",
                name_reason(*error),
                name_remedy("display name", *error)
            ),
            Self::PasswordHashEmpty => f.write_str(
                "The user was not created because no password was given, and every user needs one to sign in. Enter a password, then try again.",
            ),
            Self::FirstUserMustBeAdmin => f.write_str(
                "The user was not created because the first user of a book must be an admin, since only an admin can add the other users. Create the first user as an admin.",
            ),
            Self::CreatorRequired => f.write_str(
                "The user was not created because no admin was named as the one adding them, and every user after the first is added by an admin. Name the admin who is adding this user, then try again.",
            ),
            Self::NoSuchCreator => f.write_str(
                "The user was not created because the admin named as the one adding them does not exist. Name an existing admin, then try again.",
            ),
            Self::CreatorNotAdmin { username } => write!(
                f,
                "The user was not created because {username}, who was named as the one adding them, is not an active admin, and only an active admin can add users. Name an active admin, then try again."
            ),
            Self::UsernameTaken => f.write_str(
                "The user was not created because another user already has this username; usernames that differ only in capitals or extra spaces count as the same. Choose a different username, then try again.",
            ),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl UserError {
    /// The refusal's three parts (decision 150), which join into the sentence `Display` writes
    /// above (`Display` keeps it as a literal of its own, and the words tests hold the two equal);
    /// `None` for a database failure, which is not a refusal.
    pub fn parts(&self) -> Option<Parts> {
        const WHAT: &str = "The user was not created";
        Some(match self {
            Self::Username(error) => Parts::new(
                WHAT,
                format!("the username {}", name_reason(*error)),
                name_remedy("username", *error),
            ),
            Self::DisplayName(error) => Parts::new(
                WHAT,
                format!("the display name {}", name_reason(*error)),
                name_remedy("display name", *error),
            ),
            Self::PasswordHashEmpty => Parts::new(
                WHAT,
                "no password was given, and every user needs one to sign in",
                "Enter a password, then try again.",
            ),
            Self::FirstUserMustBeAdmin => Parts::new(
                WHAT,
                "the first user of a book must be an admin, since only an admin can add the other users",
                "Create the first user as an admin.",
            ),
            Self::CreatorRequired => Parts::new(
                WHAT,
                "no admin was named as the one adding them, and every user after the first is added by an admin",
                "Name the admin who is adding this user, then try again.",
            ),
            Self::NoSuchCreator => Parts::new(
                WHAT,
                "the admin named as the one adding them does not exist",
                "Name an existing admin, then try again.",
            ),
            Self::CreatorNotAdmin { username } => Parts::new(
                WHAT,
                format!(
                    "{username}, who was named as the one adding them, is not an active admin, and only an active admin can add users"
                ),
                "Name an active admin, then try again.",
            ),
            Self::UsernameTaken => Parts::new(
                WHAT,
                "another user already has this username; usernames that differ only in capitals or extra spaces count as the same",
                "Choose a different username, then try again.",
            ),
            Self::Database(_) => return None,
        })
    }

    /// The refusal as a command says it at the terminal (decision 150 as the pause amended it),
    /// beside the page's parts above: `None` for a database failure, and for the refusals no
    /// command meets, since `nage init` and `nage user add` find the admin and hash the password
    /// before they create a user (`PasswordHashEmpty`, `FirstUserMustBeAdmin`, `CreatorRequired`,
    /// `NoSuchCreator`).
    pub fn at_the_terminal(&self) -> Option<AtTheTerminal> {
        Some(match self {
            Self::Username(error) => name_at_the_terminal("username", "--username", *error),
            Self::DisplayName(error) => {
                name_at_the_terminal("display name", "--display-name", *error)
            }
            Self::CreatorNotAdmin { username } => AtTheTerminal::new(
                format!(
                    "{username}, who was named with --by, is not an active admin, and only an active admin can add users."
                ),
                "Name an active admin with --by, then run the command again.",
            ),
            Self::UsernameTaken => AtTheTerminal::new(
                "Another user already has this username; usernames that differ only in capitals or extra spaces count as the same.",
                "Give a different username with --username, then run the command again.",
            ),
            Self::PasswordHashEmpty
            | Self::FirstUserMustBeAdmin
            | Self::CreatorRequired
            | Self::NoSuchCreator
            | Self::Database(_) => return None,
        })
    }
}

impl std::error::Error for UserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Username(error) | Self::DisplayName(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for UserError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Why a name rule refused a username, a display name or a book name: the middle part of the
/// message (decision 102), which completes "the username ...". It is `NameError`'s own
/// wording, which account names share: what is wrong with what was typed (decision 136,
/// amending 114).
pub(crate) fn name_reason(error: NameError) -> String {
    error.to_string()
}

/// What to do about a `subject` ("username", "display name", "book name") that a name rule
/// refused: the last part of the message (decision 102), in the words decision 136 gives the
/// name rules.
pub(crate) fn name_remedy(subject: &str, error: NameError) -> String {
    match error {
        NameError::Empty => format!("Enter a {subject}, then try again."),
        NameError::TooLong { .. } => format!("Shorten the {subject}, then try again."),
        NameError::Control => format!("Type the {subject} again on one line, then try again."),
        NameError::Colon => format!("Remove the colon from the {subject}, then try again."),
    }
}

/// A name rule's refusal of a `subject` ("username", "display name", "book name") as a command
/// says it at the terminal, naming the `option` that gives it ("--username", "--display-name",
/// "--book-name"): the reason a whole sentence, and what now ending "then run the command again"
/// (decision 150 as the pause amended it).
pub(crate) fn name_at_the_terminal(subject: &str, option: &str, error: NameError) -> AtTheTerminal {
    let now = match error {
        NameError::Empty => format!("Give a {subject} with {option}, then run the command again."),
        NameError::TooLong { .. } => {
            format!("Give a shorter {subject} with {option}, then run the command again.")
        }
        NameError::Control => format!(
            "Give the {subject} with {option} on one line, without a tab or another character that cannot be shown, then run the command again."
        ),
        NameError::Colon => format!(
            "Give the {subject} with {option} without the colon, then run the command again."
        ),
    };
    AtTheTerminal::new(format!("The {subject} {}.", name_reason(error)), now)
}

/// Why a password was not changed or reset (decision 112). Each message says what was
/// refused, why and what to do (decision 102), naming the user, where it names one, by username
/// where the user is known and by number where it is not; a database failure is not a refusal
/// and passes its message on.
#[derive(Debug)]
pub enum PasswordChangeError {
    /// The user whose password was to change does not exist.
    NoSuchUser { user: UserId },
    /// One's own change: the user is deactivated.
    Deactivated { username: String },
    /// One's own change: the stored hash is no longer the one the current password was verified
    /// against, since the password was changed or reset in between.
    ChangedMeanwhile { username: String },
    /// A reset: the acting admin does not exist.
    NoSuchAdmin { admin: UserId },
    /// A reset: the acting user is not an admin, or is deactivated.
    NotAnAdmin { username: String },
    /// The new password hash is empty.
    PasswordHashEmpty,
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for PasswordChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchUser { user } => write!(
                f,
                "The password was not changed because the user it belongs to does not exist (user number {user}). Check which user was meant, then try again."
            ),
            Self::Deactivated { username } => write!(
                f,
                "The password was not changed because {username} has been deactivated, and a deactivated user cannot sign in or change a password. Ask an admin to reactivate {username} first."
            ),
            Self::ChangedMeanwhile { username } => write!(
                f,
                "The password was not changed because {username}'s password was changed or reset somewhere else a moment ago, after the current password had been checked. Sign in again with the password that is valid now, then change it if you still want to."
            ),
            Self::NoSuchAdmin { admin } => write!(
                f,
                "The password was not changed because the admin named as the one resetting it does not exist (user number {admin}). Name an existing admin, then try again."
            ),
            Self::NotAnAdmin { username } => write!(
                f,
                "The password was not changed because {username}, who was named as the one resetting it, is not an active admin, and only an active admin can reset a password. Name an active admin, then try again."
            ),
            Self::PasswordHashEmpty => f.write_str(
                "The password was not changed because no new password was given, and every user needs one to sign in. Enter a new password, then try again.",
            ),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl PasswordChangeError {
    /// The refusal's three parts (decision 150), which join into the sentence `Display` writes
    /// above (`Display` keeps it as a literal of its own, and the words tests hold the two equal);
    /// `None` for a database failure, which is not a refusal.
    pub fn parts(&self) -> Option<Parts> {
        const WHAT: &str = "The password was not changed";
        Some(match self {
            Self::NoSuchUser { user } => Parts::new(
                WHAT,
                format!("the user it belongs to does not exist (user number {user})"),
                "Check which user was meant, then try again.",
            ),
            Self::Deactivated { username } => Parts::new(
                WHAT,
                format!(
                    "{username} has been deactivated, and a deactivated user cannot sign in or change a password"
                ),
                format!("Ask an admin to reactivate {username} first."),
            ),
            Self::ChangedMeanwhile { username } => Parts::new(
                WHAT,
                format!(
                    "{username}'s password was changed or reset somewhere else a moment ago, after the current password had been checked"
                ),
                "Sign in again with the password that is valid now, then change it if you still want to.",
            ),
            Self::NoSuchAdmin { admin } => Parts::new(
                WHAT,
                format!(
                    "the admin named as the one resetting it does not exist (user number {admin})"
                ),
                "Name an existing admin, then try again.",
            ),
            Self::NotAnAdmin { username } => Parts::new(
                WHAT,
                format!(
                    "{username}, who was named as the one resetting it, is not an active admin, and only an active admin can reset a password"
                ),
                "Name an active admin, then try again.",
            ),
            Self::PasswordHashEmpty => Parts::new(
                WHAT,
                "no new password was given, and every user needs one to sign in",
                "Enter a new password, then try again.",
            ),
            Self::Database(_) => return None,
        })
    }

    /// The refusal as `nage user reset-password` says it at the terminal (decision 150 as the
    /// pause amended it), beside the page's parts above: `None` for a database failure, and for the
    /// refusals the command never meets, since it finds both users and hashes the password before
    /// the reset (`NoSuchUser`, `NoSuchAdmin`, `PasswordHashEmpty`), and one's own change is the
    /// page's alone (`Deactivated`, `ChangedMeanwhile`).
    pub fn at_the_terminal(&self) -> Option<AtTheTerminal> {
        match self {
            Self::NotAnAdmin { username } => Some(AtTheTerminal::new(
                format!(
                    "{username}, who was named with --by, is not an active admin, and only an active admin can reset a password."
                ),
                "Name an active admin with --by, then run the command again.",
            )),
            Self::NoSuchUser { .. }
            | Self::Deactivated { .. }
            | Self::ChangedMeanwhile { .. }
            | Self::NoSuchAdmin { .. }
            | Self::PasswordHashEmpty
            | Self::Database(_) => None,
        }
    }
}

impl std::error::Error for PasswordChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for PasswordChangeError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use crate::fold::{self, Colon};
use crate::words::{AtTheTerminal, Parts};

/// A row of `users` with its stored hash, as [`user_by_username`] and [`credentials`] select it.
struct CredentialsRow {
    id: UserId,
    username: String,
    display_name: String,
    is_admin: bool,
    created_by: Option<UserId>,
    created_at: Timestamp,
    updated_by: Option<UserId>,
    updated_at: Option<Timestamp>,
    disabled_by: Option<UserId>,
    disabled_at: Option<Timestamp>,
    password_hash: String,
}

impl From<CredentialsRow> for Credentials {
    fn from(row: CredentialsRow) -> Self {
        Credentials {
            user: User {
                id: row.id,
                username: row.username,
                display_name: row.display_name,
                is_admin: row.is_admin,
                created_by: row.created_by,
                created_at: row.created_at,
                updated_by: row.updated_by,
                updated_at: row.updated_at,
                disabled_by: row.disabled_by,
                disabled_at: row.disabled_at,
            },
            password_hash: row.password_hash,
        }
    }
}

/// The user `id` with the stored hash, or `None`: what the write half of a sign-in and one's own
/// change read again inside their transaction, to compare with the credentials the password was
/// verified against (decisions 106 and 112).
pub(crate) async fn credentials(
    conn: &mut sqlx::SqliteConnection,
    id: UserId,
) -> Result<Option<Credentials>, sqlx::Error> {
    let row = sqlx::query_as!(
        CredentialsRow,
        r#"SELECT id AS "id!: UserId", username, display_name, is_admin AS "is_admin!: bool", created_by AS "created_by?: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by?: UserId", updated_at AS "updated_at?: Timestamp", disabled_by AS "disabled_by?: UserId", disabled_at AS "disabled_at?: Timestamp", password_hash FROM users WHERE id = ?1"#,
        id
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(Credentials::from))
}

use crate::sessions;

/// Stores `password_hash` as the password of `user` and writes `by` and `now` as who changed
/// the row and when (decision 93): one's own change names the user, a reset the admin. The
/// caller has read the user in the same transaction.
async fn store_password(
    tx: &mut WriteTransaction,
    user: UserId,
    password_hash: &str,
    by: UserId,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users SET password_hash = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        password_hash,
        by,
        now,
        user
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The user `id` as stored after a password change or reset, which must exist.
async fn reload(tx: &mut WriteTransaction, id: UserId) -> Result<User, PasswordChangeError> {
    user(tx, id)
        .await?
        .ok_or(PasswordChangeError::Database(sqlx::Error::RowNotFound))
}
