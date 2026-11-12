//! Users (SPEC 5.1, INV-26's Core half, decisions 92, 93 and 96): the first admin, created at
//! setup with no creator, and every later user, created by an existing, enabled admin; usernames
//! unique by the fold of decision 95. Passwords are hashed by the caller (slice 5); the core
//! stores the hash as opaque text and never reads it back.

use std::fmt;

use crate::fold::NameError;
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
    /// Set together with `updated_at` at the first edit (slice 5 and milestone 8 write them).
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
                return Err(UserError::CreatorNotAdmin);
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

/// Why a user could not be created (decision 96); each message can be shown beside a form field
/// (W2).
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
    /// The creator is not an admin, or is deactivated.
    CreatorNotAdmin,
    /// Another user has this username, ignoring case.
    UsernameTaken,
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Username(error) => write!(f, "the username {error}"),
            Self::DisplayName(error) => write!(f, "the display name {error}"),
            Self::PasswordHashEmpty => f.write_str("a password is required"),
            Self::FirstUserMustBeAdmin => f.write_str("the first user must be an admin"),
            Self::CreatorRequired => f.write_str("a user is created by an admin"),
            Self::NoSuchCreator => f.write_str("the creating admin does not exist"),
            Self::CreatorNotAdmin => f.write_str("only an active admin can add users"),
            Self::UsernameTaken => f.write_str("this username is already taken"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
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

use crate::fold::{self, Colon};
