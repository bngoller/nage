//! Sessions (SPEC 5.3, decisions 92, 106 and 108): the rows behind the `nage_session` cookie.
//! The web layer makes a session's token (32 random bytes) and its SHA-256; the core stores and
//! finds only that hash, as a [`TokenHash`], so no randomness and no hashing of tokens happens
//! here. A session lasts [`SESSION_IDLE_SECONDS`] without use and [`SESSION_MAX_SECONDS`] in
//! all: `expires_at` is stored, and rewritten whenever `last_seen_at` is, as the earlier of
//! the last use plus 30 days and the creation plus 90 days (decision 92). Every operation takes
//! the write transaction, or a connection for a read, and, where time matters, the instant from its
//! caller: the core never reads the clock (decision 88). None of them refuses with a message: a
//! session that is unknown, expired or a deactivated user's is simply not found.

use std::fmt;
use std::fmt::Write as _;

use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;
use crate::users::{Credentials, User, UserId};

/// How long a session lasts without use (SPEC 5.3: 30 days, sliding), in seconds.
pub const SESSION_IDLE_SECONDS: i64 = 2_592_000;

/// How long a session lasts in all, however much it is used (SPEC 5.3: 90 days), in seconds.
pub const SESSION_MAX_SECONDS: i64 = 7_776_000;

/// A session's id: `sessions.id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]
#[sqlx(transparent)]
pub struct SessionId(pub i64);

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The SHA-256 of a session's token as `sessions.token_hash` holds it: exactly 64 lowercase
/// hexadecimal characters (decision 92). Built from the 32 bytes of the digest, never from
/// text, so the token itself, which is 64 hexadecimal characters too, cannot be passed in its
/// place.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TokenHash(String);

impl TokenHash {
    /// The hash whose 32 bytes are `digest`, written as 64 lowercase hexadecimal characters.
    pub fn from_digest(digest: [u8; 32]) -> TokenHash {
        let mut hex = String::with_capacity(64);
        for byte in digest {
            // Writing to a `String` cannot fail.
            let _ = write!(hex, "{byte:02x}");
        }
        TokenHash(hex)
    }

    /// The 64 characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A row of `sessions` as the core reads it; the token's hash stays in the database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub created_at: Timestamp,
    /// Equal to `created_at` until the first [`touch_session`].
    pub last_seen_at: Timestamp,
    /// The first instant at which the session is no longer valid: the earlier of
    /// `last_seen_at` plus 30 days and `created_at` plus 90 days.
    pub expires_at: Timestamp,
}

/// A valid session and its user, as the session rule reads them (decision 108).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    pub session: Session,
    pub user: User,
}

/// Creates a session for `user` at `now` under `token_hash` (decisions 92 and 108):
/// `last_seen_at` equal to `created_at`, `expires_at` [`SESSION_IDLE_SECONDS`] later (an expiry
/// past [`Timestamp::MAX`] is that instant). The caller has read the user in the same
/// transaction: set-up has just created it, and a sign-in goes through [`sign_in`]. A user
/// that does not exist and a token hash already stored are database errors (the foreign key
/// and the unique index). Returns the row as stored.
pub async fn create_session(
    tx: &mut WriteTransaction,
    user: UserId,
    now: Timestamp,
    token_hash: &TokenHash,
) -> Result<Session, sqlx::Error> {
    let expires_at = expiry(now, SESSION_IDLE_SECONDS);
    let hash = token_hash.as_str();
    let id = sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, created_at, last_seen_at, expires_at) VALUES (?1, ?2, ?3, ?3, ?4)",
        hash,
        user,
        now,
        expires_at
    )
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    // The row holds exactly the values bound: nothing is computed by SQLite.
    Ok(Session {
        id: SessionId(id),
        user_id: user,
        created_at: now,
        last_seen_at: now,
        expires_at,
    })
}

/// The write half of a sign-in (SPEC 5.3, decision 106), after the password in `verified` was
/// verified outside any transaction: reads the user again and answers `None`, writing nothing,
/// unless the user still exists, is still active and still holds the hash that was verified
/// (a deactivation, a password change or a reset that was committed in between wins); then
/// deletes every expired session, every user's, as [`delete_expired_sessions`] does, and creates
/// the session as [`create_session`] does.
pub async fn sign_in(
    tx: &mut WriteTransaction,
    verified: &Credentials,
    now: Timestamp,
    token_hash: &TokenHash,
) -> Result<Option<Session>, sqlx::Error> {
    let current = users::credentials(tx, verified.user.id).await?;
    let unchanged = current.is_some_and(|current| {
        current.user.is_enabled() && current.password_hash == verified.password_hash
    });
    if !unchanged {
        return Ok(None);
    }
    delete_expired_sessions(tx, now).await?;
    create_session(tx, verified.user.id, now, token_hash)
        .await
        .map(Some)
}

/// The valid session stored under `token_hash`, with its user, or `None` (SPEC 5.3, decision
/// 108): the hash is known, the session has not expired (`now` is before `expires_at`; at
/// `expires_at` itself it has) and its user is active. A read: nothing is written, an expired
/// row stays until a sign-in, or a reset or an own change of its user's password, deletes it.
pub async fn valid_session(
    conn: &mut sqlx::SqliteConnection,
    token_hash: &TokenHash,
    now: Timestamp,
) -> Result<Option<SignedIn>, sqlx::Error> {
    let hash = token_hash.as_str();
    // `expires_at > now` compares the stored text, whose one fixed form (decision 56's CHECK)
    // sorts as the instants do.
    let Some(session) = sqlx::query_as!(
        Session,
        r#"SELECT id AS "id!: SessionId", user_id AS "user_id!: UserId", created_at AS "created_at!: Timestamp", last_seen_at AS "last_seen_at!: Timestamp", expires_at AS "expires_at!: Timestamp" FROM sessions WHERE token_hash = ?1 AND expires_at > ?2"#,
        hash,
        now
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let user = users::user(conn, session.user_id).await?;
    Ok(user
        .filter(User::is_enabled)
        .map(|user| SignedIn { session, user }))
}

/// Records a use of the session `id` at `now` (SPEC 5.3's sliding refresh, decision 92):
/// rewrites `last_seen_at` to `now` and `expires_at` to the earlier of `now` plus
/// [`SESSION_IDLE_SECONDS`] and `created_at` plus [`SESSION_MAX_SECONDS`] (an expiry past
/// [`Timestamp::MAX`] is that instant), and returns the row as stored. A session that does not
/// exist or has expired at `now` is `None` and is left as it is: a touch never revives a
/// session. How often a session is touched (at most once an hour, on a GET or HEAD) is the
/// caller's rule (decision 108).
pub async fn touch_session(
    tx: &mut WriteTransaction,
    id: SessionId,
    now: Timestamp,
) -> Result<Option<Session>, sqlx::Error> {
    let Some(session) = sqlx::query_as!(
        Session,
        r#"SELECT id AS "id!: SessionId", user_id AS "user_id!: UserId", created_at AS "created_at!: Timestamp", last_seen_at AS "last_seen_at!: Timestamp", expires_at AS "expires_at!: Timestamp" FROM sessions WHERE id = ?1 AND expires_at > ?2"#,
        id,
        now
    )
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };
    let expires_at =
        expiry(now, SESSION_IDLE_SECONDS).min(expiry(session.created_at, SESSION_MAX_SECONDS));
    sqlx::query!(
        "UPDATE sessions SET last_seen_at = ?1, expires_at = ?2 WHERE id = ?3",
        now,
        expires_at,
        id
    )
    .execute(&mut **tx)
    .await?;
    Ok(Some(Session {
        last_seen_at: now,
        expires_at,
        ..session
    }))
}

/// Deletes the session `id` (sign-out, decision 106) and says whether a row was deleted.
pub async fn delete_session(tx: &mut WriteTransaction, id: SessionId) -> Result<bool, sqlx::Error> {
    let deleted = sqlx::query!("DELETE FROM sessions WHERE id = ?1", id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

/// Deletes every session that has expired at `now` (`expires_at` at or before `now`), whoever
/// its user is (SPEC 5.3: expired sessions are deleted at each login), and returns how many.
pub async fn delete_expired_sessions(
    tx: &mut WriteTransaction,
    now: Timestamp,
) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?1", now)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(deleted)
}

/// Deletes every session of `user`, expired or not (an admin's reset, SPEC 5.3 and decision
/// 112), and returns how many.
pub async fn end_user_sessions(
    tx: &mut WriteTransaction,
    user: UserId,
) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query!("DELETE FROM sessions WHERE user_id = ?1", user)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(deleted)
}

/// Deletes every session of `user` except `keep`, expired or not (one's own password change,
/// decision 112), and returns how many. `keep` naming another user's session, or none, keeps
/// nothing of `user`'s and deletes nothing of anyone else's.
pub async fn end_other_sessions(
    tx: &mut WriteTransaction,
    user: UserId,
    keep: SessionId,
) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query!(
        "DELETE FROM sessions WHERE user_id = ?1 AND id <> ?2",
        user,
        keep
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(deleted)
}

use crate::users;

/// `from` plus `seconds` (a positive count), or [`Timestamp::MAX`] when that lies past the last
/// instant a timestamp can hold (decision 98), so that an expiry is never out of range.
fn expiry(from: Timestamp, seconds: i64) -> Timestamp {
    from.as_second()
        .checked_add(seconds)
        .and_then(|second| Timestamp::from_second(second).ok())
        .unwrap_or(Timestamp::MAX)
}
