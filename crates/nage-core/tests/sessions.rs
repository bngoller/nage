//! Sessions (SPEC 5.3, decisions 92, 106 and 108): the token hash, a session's creation, the
//! valid session with its user, the touch and the expiry at 30 days of use and at 90 in all,
//! sign-out, the deletion of expired sessions, the ending of a user's sessions, and the write
//! half of a sign-in. The instants are counted from the tests' "now"; no test waits.

mod common;

use nage_core::sessions::{
    self, SESSION_IDLE_SECONDS, SESSION_MAX_SECONDS, Session, SessionId, SignedIn, TokenHash,
};
use nage_core::test_support::{self, session_rows};
use nage_core::timestamp::Timestamp;
use nage_core::users::{self, Credentials, User, UserId};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use common::{
    DAY, NOW_SECOND, after, assert_sound, begin, block_on, member, refusal, roll_back, seeded,
    session, sqlite_code, stamp, token, valid,
};

/// The offset from "now" at which most tests create their first session: the day after the
/// test book's set-up, after its second user was added.
const START: i64 = DAY;

/// The session part of what [`valid`] finds.
async fn valid_session(
    conn: &mut sqlx::SqliteConnection,
    byte: u8,
    at: Timestamp,
) -> Option<Session> {
    valid(conn, byte, at)
        .await
        .map(|signed_in| signed_in.session)
}

/// The user `id` as `conn` sees it, who must exist.
async fn stored(conn: &mut sqlx::SqliteConnection, id: UserId) -> User {
    users::user(conn, id)
        .await
        .unwrap_or_else(|error| panic!("cannot read user {id}: {error}"))
        .unwrap_or_else(|| panic!("user {id} does not exist"))
}

/// The credentials of the user typed as `typed`, who must exist.
async fn credentials(conn: &mut sqlx::SqliteConnection, typed: &str) -> Credentials {
    users::user_by_username(conn, typed)
        .await
        .unwrap_or_else(|error| panic!("cannot look {typed:?} up: {error}"))
        .unwrap_or_else(|| panic!("{typed:?} finds nobody"))
}

/// Asserts that the sign-in named `label` was refused: no session, and no database error.
#[track_caller]
fn assert_not_signed_in(outcome: Result<Option<Session>, sqlx::Error>, label: &str) {
    match outcome {
        Ok(None) => {}
        Ok(Some(session)) => panic!("{label} was accepted: {session:?}"),
        Err(error) => panic!("{label} failed: {error}"),
    }
}

/// Touches the session `id` at `at`, panicking on a database error.
async fn touch(
    tx: &mut nage_core::storage::WriteTransaction,
    id: SessionId,
    at: Timestamp,
) -> Option<Session> {
    sessions::touch_session(tx, id, at)
        .await
        .unwrap_or_else(|error| panic!("cannot touch session {id}: {error}"))
}

/// A token hash is the 32 bytes of a digest written as 64 lowercase hexadecimal characters,
/// two per byte in order, the form `sessions.token_hash` accepts; equal digests give equal
/// hashes and different digests different ones.
///
/// Proves: decision 92 (`token_hash` is exactly 64 lowercase hexadecimal characters), decision
/// 108 (the core is handed the token's hash, never the token).
#[test]
fn a_token_hash_is_sixty_four_lowercase_hexadecimal_characters() {
    assert_eq!(TokenHash::from_digest([0; 32]).as_str(), "0".repeat(64));
    assert_eq!(TokenHash::from_digest([0xAB; 32]).as_str(), "ab".repeat(32));
    assert_eq!(TokenHash::from_digest([0xFF; 32]).as_str(), "ff".repeat(32));
    let mut counting = [0_u8; 32];
    for (byte, value) in counting.iter_mut().zip(0_u8..) {
        *byte = value;
    }
    assert_eq!(
        TokenHash::from_digest(counting).as_str(),
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    );
    assert_eq!(token(7), TokenHash::from_digest([7; 32]));
    assert_ne!(token(7), token(8));
    assert_eq!(token(7).as_str().len(), 64);
}

/// `create_session` writes a session of the user that starts at the instant given: created
/// and last seen then, expiring 30 days later to the second. It is returned as stored and is
/// found, with its user's row, under its token hash and under no other; a second session of
/// the same user at the same instant is a row of its own. The two lifetimes are 30 and 90
/// days in seconds.
///
/// Proves: decision 92 (`last_seen_at` equal to `created_at` at creation), decision 108
/// (create: `expires_at` 30 days later; the session found by its token's hash).
#[tokio::test]
async fn create_session_starts_a_session_at_the_instant_given_for_thirty_days() {
    assert_eq!(SESSION_IDLE_SECONDS, 30 * DAY);
    assert_eq!(SESSION_MAX_SECONDS, 90 * DAY);
    let (_dir, storage, initialized) = seeded(false).await;
    let mut tx = begin(&storage, "a session").await;
    let sam = member(&mut tx, initialized.admin.id, "sam").await;
    let at = after(START);
    let created = sessions::create_session(&mut tx, sam, at, &token(1))
        .await
        .unwrap_or_else(|error| panic!("cannot create the session: {error}"));
    assert_eq!(
        created,
        Session {
            id: created.id,
            user_id: sam,
            created_at: at,
            last_seen_at: at,
            expires_at: after(START + 30 * DAY),
        }
    );
    assert_eq!(
        valid(&mut tx, 1, at).await,
        Some(SignedIn {
            session: created.clone(),
            user: stored(&mut tx, sam).await,
        }),
        "the session found under its token hash"
    );
    assert_eq!(valid(&mut tx, 2, at).await, None, "another token hash");

    let second = session(&mut tx, sam, at, 2).await;
    assert_ne!(second.id, created.id, "two sessions share an id");
    assert_eq!(valid_session(&mut tx, 2, at).await, Some(second));
    assert_eq!(valid_session(&mut tx, 1, at).await, Some(created));
    assert_eq!(session_rows(&mut tx).await, 2);
    assert_sound(&mut tx, "two sessions").await;
    roll_back(tx, "a session").await;
    storage.close().await;
}

/// A session for a user who does not exist and a second session under a token hash already
/// stored are database errors, the foreign key's (787) and the unique index's (2067), and
/// write no row.
///
/// Proves: decision 92 (`sessions.user_id` names a user; the unique index on `token_hash`),
/// decision 108 (create).
#[tokio::test]
async fn create_session_fails_for_an_unknown_user_and_a_token_hash_in_use() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "failing sessions").await;
    let at = after(START);
    let error = refusal(
        sessions::create_session(&mut tx, UserId(99), at, &token(1)).await,
        "a session for a user who does not exist",
    );
    assert_eq!(sqlite_code(&error), Some(787), "{error}");
    assert_eq!(session_rows(&mut tx).await, 0);

    let first = session(&mut tx, admin, at, 1).await;
    let error = refusal(
        sessions::create_session(&mut tx, admin, after(START + 1), &token(1)).await,
        "a second session under one token hash",
    );
    assert_eq!(sqlite_code(&error), Some(2067), "{error}");
    assert_eq!(session_rows(&mut tx).await, 1);
    assert_eq!(valid_session(&mut tx, 1, at).await, Some(first));
    roll_back(tx, "failing sessions").await;
    storage.close().await;
}

/// A session is valid from its creation until one second before `expires_at` and no longer
/// at `expires_at` itself or after it; an unknown token hash finds nothing; a read writes
/// nothing, so an expired row stays stored. The session of a deactivated user (planted with
/// an unchecked update) is not valid although it has not expired, while another user's
/// session stays valid.
///
/// Proves: SPEC 5.3 (a session expires; a valid session is an active user's), decision 108
/// (a valid session: its hash known, not yet expired, its user active), decision 115 (the
/// deactivation by an unchecked update).
#[tokio::test]
async fn a_session_is_valid_until_it_expires_and_only_for_an_active_user() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "validity").await;
    let sam = member(&mut tx, admin, "sam").await;
    let created = session(&mut tx, sam, after(START), 1).await;
    let admins = session(&mut tx, admin, after(START), 2).await;
    let expiry = START + 30 * DAY;
    assert_eq!(created.expires_at, after(expiry));

    let sam_row = stored(&mut tx, sam).await;
    for (offset, label) in [
        (START, "at its creation"),
        (START + 1, "a second later"),
        (START + 29 * DAY, "after 29 days"),
        (expiry - 1, "one second before it expires"),
    ] {
        assert_eq!(
            valid(&mut tx, 1, after(offset)).await,
            Some(SignedIn {
                session: created.clone(),
                user: sam_row.clone(),
            }),
            "{label}"
        );
    }
    for (offset, label) in [
        (expiry, "at the instant it expires"),
        (expiry + 1, "a second after"),
        (expiry + 100 * DAY, "a hundred days after"),
    ] {
        assert_eq!(valid(&mut tx, 1, after(offset)).await, None, "{label}");
    }
    assert_eq!(
        valid(&mut tx, 3, after(START)).await,
        None,
        "an unknown token hash"
    );
    assert_eq!(
        session_rows(&mut tx).await,
        2,
        "the reads deleted nothing, the expired row included"
    );

    test_support::deactivate_unchecked(&mut tx, sam, admin, after(START + 1)).await;
    assert_eq!(
        valid(&mut tx, 1, after(START + 2)).await,
        None,
        "the session of a deactivated user"
    );
    assert_eq!(
        valid_session(&mut tx, 2, after(START + 2)).await,
        Some(admins),
        "another user's session"
    );
    assert_eq!(session_rows(&mut tx).await, 2);
    roll_back(tx, "validity").await;
    storage.close().await;
}

/// A touch records a use: it rewrites `last_seen_at` to the instant given and `expires_at` to
/// 30 days after it, until that would pass 90 days after the creation; from then on the expiry
/// stays at 90 days after the creation however late the touch, and at that instant the session
/// has expired and a touch finds nothing. Each touch returns the row as stored, the creation
/// stamp and the user unchanged; a touch at the instant of the last one changes nothing; the
/// user's other session is never touched.
///
/// Proves: SPEC 5.3 (expiry is the earlier of 30 days after last use and 90 days after
/// creation), decision 92 (`expires_at` rewritten whenever `last_seen_at` is), decision 108
/// (touch; the expiry at 30 days of use and at 90 in all).
#[tokio::test]
async fn touch_session_moves_the_expiry_thirty_days_on_and_never_past_ninety_days() {
    let (_dir, storage, initialized) = seeded(false).await;
    let mut tx = begin(&storage, "touches").await;
    let sam = member(&mut tx, initialized.admin.id, "sam").await;
    let created = session(&mut tx, sam, after(START), 1).await;
    let other = session(&mut tx, sam, after(START), 2).await;
    let cap = START + 90 * DAY;

    for (seen, expires) in [
        (START + 20 * DAY, START + 50 * DAY),
        (START + 45 * DAY, START + 75 * DAY),
        (START + 45 * DAY, START + 75 * DAY),
        (START + 60 * DAY - 1, cap - 1),
        (START + 60 * DAY, cap),
        (START + 70 * DAY, cap),
        (START + 89 * DAY, cap),
        (cap - 1, cap),
    ] {
        let expected = Session {
            last_seen_at: after(seen),
            expires_at: after(expires),
            ..created.clone()
        };
        assert_eq!(
            touch(&mut tx, created.id, after(seen)).await,
            Some(expected.clone()),
            "the touch {seen} seconds after now"
        );
        assert_eq!(
            valid_session(&mut tx, 1, after(seen)).await,
            Some(expected),
            "the row after the touch {seen} seconds after now"
        );
    }
    assert_eq!(
        touch(&mut tx, created.id, after(cap)).await,
        None,
        "a touch 90 days after the creation"
    );
    assert_eq!(valid(&mut tx, 1, after(cap)).await, None);
    assert_eq!(
        valid_session(&mut tx, 2, after(START + 1)).await,
        Some(other),
        "the user's other session was never touched"
    );
    assert_eq!(session_rows(&mut tx).await, 2);
    roll_back(tx, "touches").await;
    storage.close().await;
}

/// A touch never revives a session: at the instant a session expires and after it, the touch
/// finds nothing and leaves the row as it was, which a read at an earlier instant shows; a
/// touch one second before the expiry still counts as a use. A session that does not exist is
/// not found.
///
/// Proves: decision 108 (touch: only a session that has not expired), SPEC 5.3 (30 days after
/// last use).
#[tokio::test]
async fn touch_session_never_revives_an_expired_session_and_finds_no_unknown_one() {
    let (_dir, storage, initialized) = seeded(false).await;
    let mut tx = begin(&storage, "late touches").await;
    let sam = member(&mut tx, initialized.admin.id, "sam").await;
    let created = session(&mut tx, sam, after(START), 1).await;
    let expiry = START + 30 * DAY;

    for offset in [expiry, expiry + 1, expiry + 5 * DAY] {
        assert_eq!(
            touch(&mut tx, created.id, after(offset)).await,
            None,
            "a touch {offset} seconds after now"
        );
    }
    assert_eq!(
        valid_session(&mut tx, 1, after(expiry - 1)).await,
        Some(created.clone()),
        "the row after three late touches, read one second before its expiry"
    );
    assert_eq!(
        touch(&mut tx, SessionId(999), after(START)).await,
        None,
        "a session that does not exist"
    );

    let last_moment = Session {
        last_seen_at: after(expiry - 1),
        expires_at: after(expiry - 1 + 30 * DAY),
        ..created.clone()
    };
    assert_eq!(
        touch(&mut tx, created.id, after(expiry - 1)).await,
        Some(last_moment.clone()),
        "a touch one second before the expiry"
    );
    assert_eq!(
        valid_session(&mut tx, 1, after(expiry)).await,
        Some(last_moment)
    );
    assert_eq!(session_rows(&mut tx).await, 1);
    roll_back(tx, "late touches").await;
    storage.close().await;
}

/// An expiry that would lie past the last instant a timestamp can hold is that instant, at a
/// session's creation and at a touch; a session created early enough keeps its own expiry.
///
/// Proves: decision 98 (no expiry lies past `Timestamp::MAX`, the last instant a timestamp holds),
/// decision 108 (create and touch never fail on the expiry).
#[tokio::test]
async fn an_expiry_past_the_last_instant_is_the_last_instant() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the last days").await;
    let last = Timestamp::MAX.as_second();

    let early = session(&mut tx, admin, stamp(last - 40 * DAY), 1).await;
    assert_eq!(early.expires_at, stamp(last - 10 * DAY));
    let late = session(&mut tx, admin, stamp(last - 10 * DAY), 2).await;
    assert_eq!(late.expires_at, Timestamp::MAX);
    let at_the_end = session(&mut tx, admin, Timestamp::MAX, 3).await;
    assert_eq!(at_the_end.expires_at, Timestamp::MAX);

    assert_eq!(
        touch(&mut tx, early.id, stamp(last - 20 * DAY)).await,
        Some(Session {
            last_seen_at: stamp(last - 20 * DAY),
            expires_at: Timestamp::MAX,
            ..early
        })
    );
    assert_eq!(
        valid_session(&mut tx, 2, stamp(last - 1)).await,
        Some(late),
        "one second before the last instant"
    );
    assert_eq!(valid(&mut tx, 2, Timestamp::MAX).await, None);
    roll_back(tx, "the last days").await;
    storage.close().await;
}

/// `delete_session` deletes the one session named and says so; the user's other session and
/// another user's session stay valid; deleting it again, or a session that does not exist,
/// deletes nothing and says so.
///
/// Proves: SPEC 5.3 (logout), decisions 106 and 108 (sign-out deletes the session).
#[tokio::test]
async fn delete_session_ends_one_session_and_no_other() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a sign-out").await;
    let sam = member(&mut tx, admin, "sam").await;
    let at = after(START);
    let laptop = session(&mut tx, sam, at, 1).await;
    let phone = session(&mut tx, sam, at, 2).await;
    let admins = session(&mut tx, admin, at, 3).await;

    for (id, deleted, label) in [
        (laptop.id, true, "the session"),
        (laptop.id, false, "the session again"),
        (SessionId(999), false, "a session that does not exist"),
    ] {
        assert_eq!(
            sessions::delete_session(&mut tx, id)
                .await
                .unwrap_or_else(|error| panic!("cannot delete {label}: {error}")),
            deleted,
            "{label}"
        );
    }
    assert_eq!(valid(&mut tx, 1, at).await, None);
    assert_eq!(valid_session(&mut tx, 2, at).await, Some(phone));
    assert_eq!(valid_session(&mut tx, 3, at).await, Some(admins));
    assert_eq!(session_rows(&mut tx).await, 2);
    roll_back(tx, "a sign-out").await;
    storage.close().await;
}

/// `delete_expired_sessions` deletes every session whose expiry is at or before the instant
/// given, whoever its user is, and returns how many: none one second before the first expiry,
/// one at that expiry, then two more, one of them another user's and one expiring at that very
/// instant; a touched session is judged by its rewritten expiry. The sessions that have not
/// expired stay valid, and the deleted ones are gone, not merely expired: they are not found
/// at an instant before their expiry.
///
/// Proves: SPEC 5.3 (expired sessions are deleted), decision 106 (every user's expired
/// sessions), decision 108 (delete the expired ones).
#[tokio::test]
async fn delete_expired_sessions_deletes_every_users_expired_sessions_and_no_other() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "expired sessions").await;
    let sam = member(&mut tx, admin, "sam").await;
    let _first = session(&mut tx, sam, after(START), 1).await;
    let _second = session(&mut tx, sam, after(START + 10 * DAY), 2).await;
    let third = session(&mut tx, sam, after(START + 5 * DAY), 3).await;
    let _fourth = session(&mut tx, admin, after(START + 2 * DAY), 4).await;
    let fifth = session(&mut tx, admin, after(START + 39 * DAY), 5).await;
    let third = touch(&mut tx, third.id, after(START + 25 * DAY))
        .await
        .unwrap_or_else(|| panic!("the third session was not touched"));
    assert_eq!(third.expires_at, after(START + 55 * DAY));

    for (offset, deleted, left, label) in [
        (
            START + 30 * DAY - 1,
            0,
            5,
            "one second before the first expiry",
        ),
        (START + 30 * DAY, 1, 4, "at the first expiry"),
        (START + 40 * DAY, 2, 2, "at the second session's expiry"),
        (START + 40 * DAY, 0, 2, "at the same instant again"),
    ] {
        assert_eq!(
            sessions::delete_expired_sessions(&mut tx, after(offset))
                .await
                .unwrap_or_else(|error| panic!("cannot delete the expired sessions: {error}")),
            deleted,
            "sessions deleted {label}"
        );
        assert_eq!(session_rows(&mut tx).await, left, "sessions left {label}");
    }
    let at = after(START + 40 * DAY);
    assert_eq!(valid_session(&mut tx, 3, at).await, Some(third));
    assert_eq!(valid_session(&mut tx, 5, at).await, Some(fifth));
    for (byte, offset) in [(1, START + 1), (2, START + 11 * DAY), (4, START + 3 * DAY)] {
        assert_eq!(
            valid(&mut tx, byte, after(offset)).await,
            None,
            "session {byte} at an instant before its expiry"
        );
    }
    roll_back(tx, "expired sessions").await;
    storage.close().await;
}

/// `end_user_sessions` deletes every session of the user named, the expired one included,
/// and returns how many; another user's session stays valid; a second call, a user without
/// sessions and a user who does not exist delete nothing.
///
/// Proves: SPEC 5.3 (a password reset revokes the user's sessions), decision 108 (end every
/// session of a user).
#[tokio::test]
async fn end_user_sessions_ends_every_session_of_that_user_only() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "ending a user's sessions").await;
    let sam = member(&mut tx, admin, "sam").await;
    let alex = member(&mut tx, admin, "alex").await;
    let _expired = session(&mut tx, sam, after(START), 1).await;
    let _laptop = session(&mut tx, sam, after(START + 35 * DAY), 2).await;
    let _phone = session(&mut tx, sam, after(START + 36 * DAY), 3).await;
    let admins = session(&mut tx, admin, after(START + 36 * DAY), 4).await;
    let at = after(START + 37 * DAY);

    for (user, ended, label) in [
        (sam, 3, "the user's three sessions"),
        (sam, 0, "the same user again"),
        (alex, 0, "a user without sessions"),
        (UserId(99), 0, "a user who does not exist"),
    ] {
        assert_eq!(
            sessions::end_user_sessions(&mut tx, user)
                .await
                .unwrap_or_else(|error| panic!("cannot end {label}: {error}")),
            ended,
            "{label}"
        );
    }
    for byte in [2, 3] {
        assert_eq!(valid(&mut tx, byte, at).await, None, "session {byte}");
    }
    assert_eq!(
        valid(&mut tx, 1, after(START + 1)).await,
        None,
        "the expired session at an instant before its expiry"
    );
    assert_eq!(valid_session(&mut tx, 4, at).await, Some(admins));
    assert_eq!(session_rows(&mut tx).await, 1);
    roll_back(tx, "ending a user's sessions").await;
    storage.close().await;
}

/// `end_other_sessions` deletes every session of the user except the one named, the expired
/// one included, and returns how many; the session named stays as it was and another user's
/// session stays valid; a second call deletes nothing. Naming another user's session keeps
/// nothing of the user's and deletes nothing of the other user's.
///
/// Proves: decision 108 (end all but one), decision 112 (one's own change keeps the session
/// in use).
#[tokio::test]
async fn end_other_sessions_keeps_the_session_in_use() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "ending the other sessions").await;
    let sam = member(&mut tx, admin, "sam").await;
    let _expired = session(&mut tx, sam, after(START), 1).await;
    let laptop = session(&mut tx, sam, after(START + 35 * DAY), 2).await;
    let _phone = session(&mut tx, sam, after(START + 36 * DAY), 3).await;
    let admins = session(&mut tx, admin, after(START + 36 * DAY), 4).await;
    let at = after(START + 37 * DAY);

    for (keep, ended, left, label) in [
        (laptop.id, 2, 2, "all but the session in use"),
        (laptop.id, 0, 2, "the same again"),
        (admins.id, 1, 1, "all but another user's session"),
        (
            SessionId(999),
            0,
            1,
            "all but a session that does not exist",
        ),
    ] {
        assert_eq!(
            sessions::end_other_sessions(&mut tx, sam, keep)
                .await
                .unwrap_or_else(|error| panic!("cannot end {label}: {error}")),
            ended,
            "sessions ended: {label}"
        );
        assert_eq!(session_rows(&mut tx).await, left, "sessions left: {label}");
        if keep == laptop.id {
            assert_eq!(
                valid_session(&mut tx, 2, at).await,
                Some(laptop.clone()),
                "the session in use: {label}"
            );
            assert_eq!(
                valid(&mut tx, 3, at).await,
                None,
                "the other device: {label}"
            );
            assert_eq!(
                valid(&mut tx, 1, after(START + 1)).await,
                None,
                "the expired session at an instant before its expiry: {label}"
            );
        }
        assert_eq!(
            valid_session(&mut tx, 4, at).await,
            Some(admins.clone()),
            "another user's session: {label}"
        );
    }
    assert_eq!(valid(&mut tx, 2, at).await, None, "the user's last session");
    roll_back(tx, "ending the other sessions").await;
    storage.close().await;
}

/// The write half of a sign-in creates the session of the user whose password was verified,
/// starting at the instant given for 30 days, and deletes every expired session, the user's
/// and another user's; the sessions that have not expired stay valid. `nage check` finds the
/// scenario sound.
///
/// Proves: SPEC 5.3 (expired sessions are deleted at each login), decision 106 (one write
/// transaction deletes the expired sessions, every user's, and creates the session), decision
/// 108 (create).
#[tokio::test]
async fn sign_in_creates_the_session_and_deletes_every_expired_one() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a sign-in").await;
    let sam = member(&mut tx, admin, "sam").await;
    let _sams_expired = session(&mut tx, sam, after(START), 1).await;
    let _admins_expired = session(&mut tx, admin, after(START + DAY), 2).await;
    let admins = session(&mut tx, admin, after(START + 20 * DAY), 3).await;
    let sams = session(&mut tx, sam, after(START + 25 * DAY), 4).await;
    let at = after(START + 35 * DAY);
    assert_eq!(
        session_rows(&mut tx).await,
        4,
        "sessions before the sign-in"
    );

    let verified = credentials(&mut tx, "sam").await;
    let signed_in = sessions::sign_in(&mut tx, &verified, at, &token(5))
        .await
        .unwrap_or_else(|error| panic!("cannot sign in: {error}"))
        .unwrap_or_else(|| panic!("the sign-in was refused"));
    assert_eq!(
        signed_in,
        Session {
            id: signed_in.id,
            user_id: sam,
            created_at: at,
            last_seen_at: at,
            expires_at: after(START + 65 * DAY),
        }
    );
    assert_eq!(
        valid(&mut tx, 5, at).await,
        Some(SignedIn {
            session: signed_in,
            user: verified.user.clone(),
        }),
        "the new session, found under its token hash"
    );
    assert_eq!(valid_session(&mut tx, 3, at).await, Some(admins));
    assert_eq!(valid_session(&mut tx, 4, at).await, Some(sams));
    assert_eq!(
        session_rows(&mut tx).await,
        3,
        "sessions after the sign-in: two expired ones deleted, one created"
    );
    for (byte, offset) in [(1, START + 1), (2, START + 2 * DAY)] {
        assert_eq!(
            valid(&mut tx, byte, after(offset)).await,
            None,
            "expired session {byte} at an instant before its expiry"
        );
    }
    assert_sound(&mut tx, "a sign-in").await;
    roll_back(tx, "a sign-in").await;
    storage.close().await;
}

/// The write half of a sign-in reads the user again and refuses, writing nothing (no session; at
/// the first refusal, the expired session of another user still stored), when the user is no longer
/// as verified: after an admin's reset, after the user's own change, after a deactivation (planted
/// with an unchecked update), and for credentials that name a user who does not exist. With the
/// credentials as they are now, the sign-in is accepted.
///
/// Proves: decision 106 (the write transaction reads the user again: still active, the same
/// hash), decision 112 (a reset and one's own change count at once), decision 115 (the
/// deactivation by an unchecked update).
#[tokio::test]
async fn sign_in_refuses_a_user_who_changed_after_the_password_was_verified() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "refused sign-ins").await;
    let sam = member(&mut tx, admin, "sam").await;
    let _expired = session(&mut tx, admin, after(START), 1).await;
    let at = after(START + 40 * DAY);

    let before_reset = credentials(&mut tx, "sam").await;
    users::reset_password(&mut tx, admin, at, sam, "the reset hash")
        .await
        .unwrap_or_else(|error| panic!("the reset was refused: {error}"));
    assert_not_signed_in(
        sessions::sign_in(&mut tx, &before_reset, at, &token(5)).await,
        "a sign-in verified before an admin's reset",
    );
    assert_eq!(valid(&mut tx, 5, at).await, None);
    assert_eq!(
        session_rows(&mut tx).await,
        1,
        "a refused sign-in deletes no expired session and creates none"
    );

    let after_reset = credentials(&mut tx, "sam").await;
    let kept = sessions::sign_in(&mut tx, &after_reset, at, &token(6))
        .await
        .unwrap_or_else(|error| panic!("cannot sign in: {error}"))
        .unwrap_or_else(|| panic!("the sign-in with the current credentials was refused"));
    assert_eq!(
        session_rows(&mut tx).await,
        1,
        "the expired one deleted, one created"
    );

    users::change_own_password(&mut tx, &after_reset, at, "sam's own hash", kept.id)
        .await
        .unwrap_or_else(|error| panic!("the own change was refused: {error}"));
    assert_not_signed_in(
        sessions::sign_in(&mut tx, &after_reset, at, &token(7)).await,
        "a sign-in verified before the user's own change",
    );

    let current = credentials(&mut tx, "sam").await;
    let ghost = Credentials {
        user: User {
            id: UserId(99),
            ..current.user.clone()
        },
        password_hash: current.password_hash.clone(),
    };
    assert_not_signed_in(
        sessions::sign_in(&mut tx, &ghost, at, &token(8)).await,
        "a sign-in for a user who does not exist",
    );
    test_support::deactivate_unchecked(&mut tx, sam, admin, at).await;
    assert_not_signed_in(
        sessions::sign_in(&mut tx, &current, at, &token(9)).await,
        "a sign-in verified before the user was deactivated",
    );
    assert_eq!(
        session_rows(&mut tx).await,
        1,
        "sessions after the refused sign-ins"
    );
    roll_back(tx, "refused sign-ins").await;
    storage.close().await;
}

/// The seconds between two uses of a session: any gap up to 40 days, a gap of exactly 30 days
/// or one second less, no gap, or one within an hour.
fn gap() -> impl Strategy<Value = i64> {
    prop_oneof![
        0..=40 * DAY,
        Just(30 * DAY),
        Just(30 * DAY - 1),
        Just(0),
        0..=3600_i64,
    ]
}

proptest! {
    #![proptest_config(test_support::proptest_config(file!()))]

    /// Over any run of uses, a session is found exactly while the instant is before its
    /// expiry, and its expiry is always the earlier of 30 days after its last use and 90 days
    /// after its creation: each use that finds the session valid is recorded by a touch, which
    /// returns the row the model expects; once the session has expired, no later read finds it
    /// and no touch revives it. Each case runs against a fresh database on its own runtime.
    ///
    /// Proves: SPEC 5.3 (expiry is the earlier of 30 days after last use and 90 days after
    /// creation), decisions 92 and 108 (the session operations, with the expiry at 30 days of
    /// use and at 90 in all).
    #[test]
    fn a_session_lives_thirty_days_from_its_last_use_and_ninety_in_all(
        gaps in prop::collection::vec(gap(), 1..=12)
    ) {
        let outcome: Result<(), TestCaseError> = block_on(async {
            let (_dir, storage, initialized) = seeded(false).await;
            let mut tx = begin(&storage, "a run of uses").await;
            let created_second = NOW_SECOND + START;
            let created = session(&mut tx, initialized.admin.id, stamp(created_second), 1).await;
            let mut last_seen = created_second;
            let mut expires = created_second + 30 * DAY;
            let mut clock = created_second;
            for gap in gaps {
                clock += gap;
                let found = valid_session(&mut tx, 1, stamp(clock)).await;
                let touched = touch(&mut tx, created.id, stamp(clock)).await;
                if clock < expires {
                    prop_assert_eq!(
                        found,
                        Some(Session {
                            last_seen_at: stamp(last_seen),
                            expires_at: stamp(expires),
                            ..created.clone()
                        }),
                        "the session read at second {}",
                        clock
                    );
                    last_seen = clock;
                    expires = (clock + 30 * DAY).min(created_second + 90 * DAY);
                    prop_assert_eq!(
                        touched,
                        Some(Session {
                            last_seen_at: stamp(last_seen),
                            expires_at: stamp(expires),
                            ..created.clone()
                        }),
                        "the touch at second {}",
                        clock
                    );
                } else {
                    prop_assert_eq!(found, None, "the expired session read at second {}", clock);
                    prop_assert_eq!(touched, None, "the touch of the expired session at second {}", clock);
                }
            }
            roll_back(tx, "a run of uses").await;
            storage.close().await;
            Ok(())
        });
        outcome?;
    }
}
