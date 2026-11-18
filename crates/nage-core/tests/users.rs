//! The user operations of slice 5 (SPEC 5.3, INV-26's Core half, decisions 106, 112 and 115):
//! the user found by the fold of a typed username with the stored hash, one's own password
//! change and an admin's reset with their stamps and the sessions they end, their messages,
//! and the shared helpers that set a test book up with a password that verifies.

mod common;

use nage_core::accounts;
use nage_core::book;
use nage_core::password::{self, Verification};
use nage_core::test_support::{
    self, CHEAP_HASH_PARAMS, TEST_BOOK_NAME, open_migrated, session_rows,
};
use nage_core::users::{self, Credentials, NewUser, PasswordChangeError, User, UserId};

use common::{
    DAY, after, assert_sound, begin, later, member, now, reader, refused, roll_back, seeded,
    session, valid,
};

/// The password the tests give the admin.
const PASSWORD: &str = "correct horse 42";
/// The password the tests give a second user.
const OTHER_PASSWORD: &str = "battery staple 7";

/// The user `id` as `conn` sees it, who must exist.
async fn stored(conn: &mut sqlx::SqliteConnection, id: UserId) -> User {
    users::user(conn, id)
        .await
        .unwrap_or_else(|error| panic!("cannot read user {id}: {error}"))
        .unwrap_or_else(|| panic!("user {id} does not exist"))
}

/// What `user_by_username` finds for `typed` as `conn` sees it.
async fn found(conn: &mut sqlx::SqliteConnection, typed: &str) -> Option<Credentials> {
    users::user_by_username(conn, typed)
        .await
        .unwrap_or_else(|error| panic!("cannot look {typed:?} up: {error}"))
}

/// The credentials of the user typed as `typed`, who must exist: what a caller holds after it
/// verified that user's password.
async fn credentials(conn: &mut sqlx::SqliteConnection, typed: &str) -> Credentials {
    found(conn, typed)
        .await
        .unwrap_or_else(|| panic!("{typed:?} finds nobody"))
}

/// The stored hash of the user typed as `typed`, who must exist.
async fn stored_hash(conn: &mut sqlx::SqliteConnection, typed: &str) -> String {
    credentials(conn, typed).await.password_hash
}

/// A user is found by the fold of what was typed, with the stored hash. Two usernames are
/// stored in a spelling other than the one typed: the admin's with a full-width first letter,
/// found by the plain letters in any case, with white space around and inside and with a
/// modifier capital, which shows that the stored fold is decision 95's and not lowercase alone
/// (lowercase alone keeps the full-width letter); and a second user's with a modifier capital,
/// which has no lowercase of its own, found by the plain small letter, which shows a fold that
/// normalizes before it lowercases. Each is returned as its row with its own hash: the admin's
/// verifies the admin's password and not the second user's, and the second user's verifies its own.
/// A text that is no user's username (an empty one, a blank one, the first word of a username, a
/// username with a word added, 70 letters) finds nobody. A deactivated user, planted with an
/// unchecked update, is found like any other, with the deactivation on its row.
///
/// Proves: decision 95 (a username's stored fold; a fold stronger than lowercase), decision
/// 106 (the user found by the fold of the typed username, with the stored hash), decision 115
/// (the lookup's two usernames).
#[tokio::test]
async fn a_user_is_found_by_the_fold_of_the_typed_username_with_the_stored_hash() {
    let (_dir, storage) = open_migrated().await;
    let ann = test_support::set_up(&storage, now(), "\u{FF21}nn Lee", PASSWORD)
        .await
        .admin;
    let bob = test_support::add_user(
        &storage,
        ann.id,
        later(),
        "\u{1D2E}ob",
        OTHER_PASSWORD,
        false,
    )
    .await;
    assert_eq!(
        ann.username, "\u{FF21}nn Lee",
        "a username is stored as typed"
    );
    assert_eq!(bob.username, "\u{1D2E}ob", "a username is stored as typed");

    let mut conn = reader(&storage).await;
    for typed in [
        "ann lee",
        "Ann Lee",
        "ANN LEE",
        "  ann   lee ",
        "ann\u{00A0}lee",
        "\u{FF21}nn Lee",
        "\u{1D2C}NN LEE",
    ] {
        let credentials = found(&mut conn, typed)
            .await
            .unwrap_or_else(|| panic!("{typed:?} finds nobody"));
        assert_eq!(credentials.user, ann, "the user {typed:?} finds");
        assert_eq!(
            password::verify(PASSWORD, &credentials.password_hash),
            Verification::Verified,
            "the admin's password against the hash {typed:?} finds"
        );
        assert_eq!(
            password::verify(OTHER_PASSWORD, &credentials.password_hash),
            Verification::WrongPassword,
            "the other password against the hash {typed:?} finds"
        );
    }
    for typed in ["bob", "Bob", "BOB", " bob ", "\u{1D2E}ob", "\u{FF22}ob"] {
        let credentials = found(&mut conn, typed)
            .await
            .unwrap_or_else(|| panic!("{typed:?} finds nobody"));
        assert_eq!(credentials.user, bob, "the user {typed:?} finds");
        assert_eq!(
            password::verify(OTHER_PASSWORD, &credentials.password_hash),
            Verification::Verified,
            "the second user's password against the hash {typed:?} finds"
        );
    }
    let long = "x".repeat(70);
    for typed in [
        "",
        "   ",
        "ann",
        "ann lee x",
        "bobby",
        "carol",
        long.as_str(),
    ] {
        assert_eq!(found(&mut conn, typed).await, None, "{typed:?}");
    }
    drop(conn);

    let mut tx = begin(&storage, "a deactivation").await;
    test_support::deactivate_unchecked(&mut tx, bob.id, ann.id, after(DAY)).await;
    let deactivated = found(&mut tx, "bob")
        .await
        .unwrap_or_else(|| panic!("the deactivated user is not found"));
    assert!(!deactivated.user.is_enabled());
    assert_eq!(
        deactivated.user,
        User {
            disabled_by: Some(ann.id),
            disabled_at: Some(after(DAY)),
            ..bob.clone()
        }
    );
    assert_eq!(
        password::verify(OTHER_PASSWORD, &deactivated.password_hash),
        Verification::Verified
    );
    roll_back(tx, "a deactivation").await;
    storage.close().await;
}

/// The `Debug` form of a user's credentials names the user and never shows the stored hash,
/// so a panic message or a log line that prints them gives no hash away.
///
/// Proves: SPEC 5.3 (a password hash is a secret), decision 106 (the credentials never show
/// the stored hash when they are printed).
#[test]
fn credentials_never_show_the_hash_when_printed() {
    let credentials = Credentials {
        user: User {
            id: UserId(7),
            username: String::from("sam"),
            display_name: String::from("Sam"),
            is_admin: false,
            created_by: Some(UserId(1)),
            created_at: now(),
            updated_by: None,
            updated_at: None,
            disabled_by: None,
            disabled_at: None,
        },
        password_hash: String::from("$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$aGFzaGhhc2g"),
    };
    for printed in [format!("{credentials:?}"), format!("{credentials:#?}")] {
        assert!(printed.contains("\"sam\""), "{printed}");
        assert!(printed.contains("<hidden>"), "{printed}");
        for secret in ["argon2id", "c2FsdHNhbHQ", "aGFzaGhhc2g"] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
    }
}

/// One's own password change, with the credentials the current password was verified against,
/// stores the new hash, writes the user as who changed the row and the instant as when, and
/// leaves every other column as it was; it ends every other session
/// of the user, the valid one on another device and the expired one alike, keeps the session
/// in use as it was, and touches neither another user's session nor another user's row. The
/// row is returned as stored, and `nage check` finds the scenario sound.
///
/// Proves: INV-26 (Core half: a changed user names who changed it and when), decision 93 (a
/// password change writes the updated pair), decision 112 (one's own change ends every other
/// session of the user and keeps the one in use).
#[tokio::test]
async fn change_own_password_stores_the_hash_stamps_the_user_and_ends_the_other_sessions() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an own change").await;
    let sam = member(&mut tx, admin, "sam").await;
    let before = stored(&mut tx, sam).await;
    assert_eq!((before.updated_by, before.updated_at), (None, None));
    let _unused = session(&mut tx, sam, after(2 * DAY), 1).await;
    let _phone = session(&mut tx, sam, after(40 * DAY), 2).await;
    let laptop = session(&mut tx, sam, after(41 * DAY), 3).await;
    let admins = session(&mut tx, admin, after(41 * DAY), 9).await;
    let at = after(42 * DAY);
    assert_eq!(session_rows(&mut tx).await, 4, "sessions before the change");
    assert_eq!(valid(&mut tx, 1, at).await, None, "session 1 has expired");
    assert!(valid(&mut tx, 2, at).await.is_some(), "session 2 is valid");

    let verified = credentials(&mut tx, "sam").await;
    let changed = users::change_own_password(&mut tx, &verified, at, "the new hash", laptop.id)
        .await
        .unwrap_or_else(|error| panic!("the own change was refused: {error}"));
    assert_eq!(
        changed,
        User {
            updated_by: Some(sam),
            updated_at: Some(at),
            ..before
        },
        "the row the change returned"
    );
    assert_eq!(stored(&mut tx, sam).await, changed, "the row as stored");
    assert_eq!(stored_hash(&mut tx, "sam").await, "the new hash");

    assert_eq!(
        valid(&mut tx, 3, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(laptop),
        "the session in use is kept as it was"
    );
    assert_eq!(
        valid(&mut tx, 2, at).await,
        None,
        "the other device is signed out"
    );
    assert_eq!(
        valid(&mut tx, 1, after(3 * DAY)).await,
        None,
        "the expired session is deleted: it is not found at an instant before its expiry"
    );
    assert_eq!(
        valid(&mut tx, 9, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(admins),
        "another user's session is not touched"
    );
    assert_eq!(session_rows(&mut tx).await, 2, "sessions after the change");
    let admin_row = stored(&mut tx, admin).await;
    assert_eq!(
        (admin_row.updated_by, admin_row.updated_at),
        (None, None),
        "another user's row is not touched"
    );
    assert_eq!(stored_hash(&mut tx, "admin").await, "hash");
    assert_sound(&mut tx, "an own password change").await;
    roll_back(tx, "an own change").await;
    storage.close().await;
}

/// One's own change is refused for credentials that name a user who does not exist, for an
/// empty new hash, for a user whose password was reset after the current one was verified, and
/// for a user who has been deactivated (planted with an unchecked update), each with its own
/// refusal. A refused change writes nothing: the stored hash, the row and the sessions are as
/// they were, and after the reset the change neither overwrites the reset's hash and stamps nor
/// ends a session.
///
/// Proves: decision 112 (one's own change: the acting user exists, is active and still holds
/// the password that was verified, and a new hash is given; a refusal writes nothing),
/// decision 115 (the deactivation by an unchecked update).
#[tokio::test]
async fn change_own_password_refuses_a_user_who_is_not_as_verified_and_an_empty_hash() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "refused own changes").await;
    let sam = member(&mut tx, admin, "sam").await;
    let before = stored(&mut tx, sam).await;
    let laptop = session(&mut tx, sam, after(DAY), 1).await;
    let phone = session(&mut tx, sam, after(DAY), 2).await;
    let at = after(2 * DAY);

    let verified = credentials(&mut tx, "sam").await;
    let ghost = Credentials {
        user: User {
            id: UserId(99),
            ..verified.user.clone()
        },
        password_hash: verified.password_hash.clone(),
    };
    assert_eq!(
        refused(
            users::change_own_password(&mut tx, &ghost, at, "a new hash", laptop.id).await,
            "the change of a user who does not exist",
        ),
        "NoSuchUser { user: UserId(99) }"
    );
    assert_eq!(
        refused(
            users::change_own_password(&mut tx, &verified, at, "", laptop.id).await,
            "a change to an empty hash",
        ),
        "PasswordHashEmpty"
    );
    assert_eq!(
        stored(&mut tx, sam).await,
        before,
        "the row after two refusals"
    );
    assert_eq!(stored_hash(&mut tx, "sam").await, "hash");
    assert_eq!(
        valid(&mut tx, 2, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(phone),
        "the other session after two refused changes"
    );
    assert_eq!(session_rows(&mut tx).await, 2);

    let reset = users::reset_password(&mut tx, admin, at, sam, "the reset hash")
        .await
        .unwrap_or_else(|error| panic!("the reset was refused: {error}"));
    let signed_in_again = session(&mut tx, sam, at, 3).await;
    let other_device = session(&mut tx, sam, at, 4).await;
    assert_eq!(
        refused(
            users::change_own_password(&mut tx, &verified, at, "a new hash", signed_in_again.id)
                .await,
            "a change verified before an admin's reset",
        ),
        "ChangedMeanwhile { username: \"sam\" }"
    );
    assert_eq!(
        stored(&mut tx, sam).await,
        reset,
        "the row after the refused change: as the reset left it"
    );
    assert_eq!(stored_hash(&mut tx, "sam").await, "the reset hash");
    assert_eq!(
        valid(&mut tx, 4, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(other_device),
        "the other session after the refused change"
    );
    assert_eq!(session_rows(&mut tx).await, 2);

    let current = credentials(&mut tx, "sam").await;
    test_support::deactivate_unchecked(&mut tx, sam, admin, at).await;
    assert_eq!(
        refused(
            users::change_own_password(&mut tx, &current, at, "a new hash", signed_in_again.id)
                .await,
            "the change of a deactivated user",
        ),
        "Deactivated { username: \"sam\" }"
    );
    assert_eq!(
        stored(&mut tx, sam).await,
        User {
            disabled_by: Some(admin),
            disabled_at: Some(at),
            ..reset
        },
        "the row after the last refusal: only the planted deactivation"
    );
    assert_eq!(stored_hash(&mut tx, "sam").await, "the reset hash");
    assert_eq!(
        session_rows(&mut tx).await,
        2,
        "sessions after the refused changes"
    );
    roll_back(tx, "refused own changes").await;
    storage.close().await;
}

/// An admin's reset stores the new hash and writes the admin as who changed the row and the
/// instant as when, over the stamps the user's own earlier change left; it ends every session
/// of the user, valid and expired, and touches neither the admin's session nor the admin's
/// row. The row is returned as stored, and `nage check` finds the scenario sound.
///
/// Proves: SPEC 5.3 (a password reset revokes the user's sessions), INV-26 (Core half: a
/// changed user names who changed it last and when), decisions 93 and 112 (a reset writes the
/// updated pair, naming the admin, and ends every session of the user).
#[tokio::test]
async fn reset_password_stores_the_hash_names_the_admin_and_ends_every_session() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a reset").await;
    let sam = member(&mut tx, admin, "sam").await;
    let first = session(&mut tx, sam, after(2 * DAY), 1).await;
    let verified = credentials(&mut tx, "sam").await;
    let own = users::change_own_password(
        &mut tx,
        &verified,
        after(3 * DAY),
        "sam's own hash",
        first.id,
    )
    .await
    .unwrap_or_else(|error| panic!("the own change was refused: {error}"));
    assert_eq!(
        (own.updated_by, own.updated_at),
        (Some(sam), Some(after(3 * DAY)))
    );
    let _second = session(&mut tx, sam, after(35 * DAY), 2).await;
    let admins = session(&mut tx, admin, after(39 * DAY), 9).await;
    let at = after(40 * DAY);
    assert_eq!(valid(&mut tx, 1, at).await, None, "session 1 has expired");
    assert!(valid(&mut tx, 2, at).await.is_some(), "session 2 is valid");
    assert_eq!(session_rows(&mut tx).await, 3, "sessions before the reset");

    let reset = users::reset_password(&mut tx, admin, at, sam, "the reset hash")
        .await
        .unwrap_or_else(|error| panic!("the reset was refused: {error}"));
    assert_eq!(
        reset,
        User {
            updated_by: Some(admin),
            updated_at: Some(at),
            ..own
        },
        "the row the reset returned"
    );
    assert_eq!(stored(&mut tx, sam).await, reset, "the row as stored");
    assert_eq!(stored_hash(&mut tx, "sam").await, "the reset hash");

    assert_eq!(
        valid(&mut tx, 2, at).await,
        None,
        "the valid session is ended"
    );
    assert_eq!(
        valid(&mut tx, 1, after(4 * DAY)).await,
        None,
        "the expired session is deleted: it is not found at an instant before its expiry"
    );
    assert_eq!(
        valid(&mut tx, 9, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(admins),
        "the admin's session is not touched"
    );
    assert_eq!(session_rows(&mut tx).await, 1, "sessions after the reset");
    let admin_row = stored(&mut tx, admin).await;
    assert_eq!(
        (admin_row.updated_by, admin_row.updated_at),
        (None, None),
        "the admin's row is not touched"
    );
    assert_eq!(stored_hash(&mut tx, "admin").await, "hash");
    assert_sound(&mut tx, "a password reset").await;
    roll_back(tx, "a reset").await;
    storage.close().await;
}

/// The acting admin may be the user being reset: an admin's reset of their own password
/// stores the hash, names the admin as who changed the row and ends every session of the
/// admin, another user's session staying.
///
/// Proves: decision 112 (the acting admin may be the user being reset, so that an admin who
/// forgot their password recovers at the server's terminal).
#[tokio::test]
async fn reset_password_may_name_the_admin_being_reset() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an admin's own reset").await;
    let sam = member(&mut tx, admin, "sam").await;
    let _laptop = session(&mut tx, admin, after(DAY), 1).await;
    let _phone = session(&mut tx, admin, after(DAY), 2).await;
    let sams = session(&mut tx, sam, after(DAY), 3).await;
    let at = after(2 * DAY);

    let reset = users::reset_password(&mut tx, admin, at, admin, "the admin's new hash")
        .await
        .unwrap_or_else(|error| panic!("the admin's own reset was refused: {error}"));
    assert_eq!(
        reset,
        User {
            updated_by: Some(admin),
            updated_at: Some(at),
            ..initialized.admin
        }
    );
    assert_eq!(stored(&mut tx, admin).await, reset, "the row as stored");
    assert_eq!(stored_hash(&mut tx, "admin").await, "the admin's new hash");
    assert_eq!(valid(&mut tx, 1, at).await, None);
    assert_eq!(valid(&mut tx, 2, at).await, None);
    assert_eq!(
        valid(&mut tx, 3, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(sams)
    );
    assert_eq!(session_rows(&mut tx).await, 1);
    assert_sound(&mut tx, "an admin's own reset").await;
    roll_back(tx, "an admin's own reset").await;
    storage.close().await;
}

/// A reset is refused when the acting user does not exist, is not an admin (resetting an
/// admin's password or their own) or is an admin who has been deactivated (planted with an
/// unchecked update), when the user to reset does not exist and when the hash is empty, each
/// with its own refusal; a refused reset writes nothing: both stored hashes, both rows and both
/// sessions are as they were. The same reset by the active admin is then accepted.
///
/// Proves: decisions 96 and 112 (a reset names an existing, enabled admin; a refusal writes
/// nothing), decision 115 (the deactivation by an unchecked update).
#[tokio::test]
async fn reset_password_refuses_an_actor_who_is_not_an_active_admin() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "refused resets").await;
    let sam = member(&mut tx, admin, "sam").await;
    let deputy = users::create_user(
        &mut tx,
        Some(admin),
        later(),
        &NewUser {
            username: String::from("deputy"),
            display_name: String::from("Deputy"),
            password_hash: String::from("hash"),
            is_admin: true,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the second admin: {error}"))
    .id;
    let sam_before = stored(&mut tx, sam).await;
    let sams = session(&mut tx, sam, after(DAY), 1).await;
    let admins = session(&mut tx, admin, after(DAY), 2).await;
    let at = after(2 * DAY);
    test_support::deactivate_unchecked(&mut tx, deputy, admin, at).await;

    for (by, user, hash, expected, label) in [
        (
            UserId(99),
            sam,
            "a new hash",
            "NoSuchAdmin { admin: UserId(99) }",
            "a reset by a user who does not exist",
        ),
        (
            sam,
            admin,
            "a new hash",
            "NotAnAdmin { username: \"sam\" }",
            "a reset of the admin by a user who is not an admin",
        ),
        (
            sam,
            sam,
            "a new hash",
            "NotAnAdmin { username: \"sam\" }",
            "a reset of their own password by a user who is not an admin",
        ),
        (
            deputy,
            sam,
            "a new hash",
            "NotAnAdmin { username: \"deputy\" }",
            "a reset by a deactivated admin",
        ),
        (
            admin,
            UserId(99),
            "a new hash",
            "NoSuchUser { user: UserId(99) }",
            "a reset of a user who does not exist",
        ),
        (
            admin,
            sam,
            "",
            "PasswordHashEmpty",
            "a reset to an empty hash",
        ),
    ] {
        assert_eq!(
            refused(
                users::reset_password(&mut tx, by, at, user, hash).await,
                label
            ),
            expected,
            "{label}"
        );
    }
    assert_eq!(stored(&mut tx, sam).await, sam_before, "sam's row");
    assert_eq!(
        stored(&mut tx, admin).await,
        initialized.admin,
        "the admin's row"
    );
    assert_eq!(stored_hash(&mut tx, "sam").await, "hash");
    assert_eq!(stored_hash(&mut tx, "admin").await, "hash");
    assert_eq!(
        valid(&mut tx, 1, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(sams)
    );
    assert_eq!(
        valid(&mut tx, 2, at)
            .await
            .map(|signed_in| signed_in.session),
        Some(admins)
    );
    assert_eq!(
        session_rows(&mut tx).await,
        2,
        "sessions after the refusals"
    );

    let reset = users::reset_password(&mut tx, admin, at, sam, "a new hash")
        .await
        .unwrap_or_else(|error| panic!("the active admin's reset was refused: {error}"));
    assert_eq!(reset.updated_by, Some(admin));
    roll_back(tx, "refused resets").await;
    storage.close().await;
}

/// Every refusal of a password change or reset says what was refused, why and what to do
/// instead, naming the user, where it names one, by username where the user is known and by
/// number where it is not; no two share a message; a database failure is not a refusal and
/// passes its message on. The test prints each moment with its message
/// (`cargo test -p nage-core --test users refusals_explain -- --nocapture` shows them).
///
/// Proves: decisions 102 and 112 (every message of one's own change and of an admin's reset,
/// in its three parts).
#[test]
fn password_change_refusals_explain_themselves() {
    let cases: [(&str, &str, PasswordChangeError, &str); 6] = [
        (
            "NotAnAdmin",
            "`nage user reset-password --by alex --username sam` is run, and alex is not an admin, or is an admin who has been deactivated.",
            PasswordChangeError::NotAnAdmin {
                username: String::from("alex"),
            },
            "The password was not changed because alex, who was named as the one resetting it, is not an active admin, and only an active admin can reset a password. Name an active admin, then try again.",
        ),
        (
            "ChangedMeanwhile",
            "sam saves a new password on the change-password page in the very second in which an admin resets sam's password at the server's terminal. (No page or command shows this message: the page sends sam to the sign-in page, which says it in its own words.)",
            PasswordChangeError::ChangedMeanwhile {
                username: String::from("sam"),
            },
            "The password was not changed because sam's password was changed or reset somewhere else a moment ago, after the current password had been checked. Sign in again with the password that is valid now, then change it if you still want to.",
        ),
        (
            "Deactivated",
            "sam saves a new password on the change-password page a moment after an admin deactivated sam. (Cannot happen before deactivation arrives with the user screens of a later milestone.)",
            PasswordChangeError::Deactivated {
                username: String::from("sam"),
            },
            "The password was not changed because sam has been deactivated, and a deactivated user cannot sign in or change a password. Ask an admin to reactivate sam first.",
        ),
        (
            "NoSuchUser",
            "A password is changed for user number 12, and no user has that number. (No page or command of this step can cause it: the page knows who is signed in, and the commands look the user up by username first.)",
            PasswordChangeError::NoSuchUser { user: UserId(12) },
            "The password was not changed because the user it belongs to does not exist (user number 12). Check which user was meant, then try again.",
        ),
        (
            "NoSuchAdmin",
            "A password is reset in the name of user number 99, and no user has that number. (No command of this step can cause it: `nage user reset-password` looks the admin up by username first.)",
            PasswordChangeError::NoSuchAdmin { admin: UserId(99) },
            "The password was not changed because the admin named as the one resetting it does not exist (user number 99). Name an existing admin, then try again.",
        ),
        (
            "PasswordHashEmpty",
            "A password is changed without a new one. (No page or command of this step can cause it: each checks the new password first.)",
            PasswordChangeError::PasswordHashEmpty,
            "The password was not changed because no new password was given, and every user needs one to sign in. Enter a new password, then try again.",
        ),
    ];
    let mut messages = Vec::new();
    for (variant, moment, error, expected) in &cases {
        let message = error.to_string();
        println!("[PasswordChangeError::{variant}] {moment} => {message}");
        assert_eq!(&message, expected, "the message of {error:?}");
        messages.push(message);
    }
    let mut distinct = messages.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        messages.len(),
        "two refusals share a message"
    );
    assert_eq!(
        PasswordChangeError::Database(sqlx::Error::RowNotFound).to_string(),
        format!("database error: {}", sqlx::Error::RowNotFound),
        "a database failure is not a refusal"
    );
}

/// The shared helpers set a test book up with passwords that verify: `set_up` creates the
/// book "Household" in CAD with the starter chart and an admin whose stored hash was made with
/// the cheap set (m=8 KiB, t=1, p=1) from the password given; `add_user` adds a user created
/// by that admin, an admin only when asked, with a hash of its own; `session_rows` counts the
/// sessions; `deactivate_unchecked` writes the deactivation pair on one user's row.
///
/// Proves: decision 59 (the shared helpers), decision 107 (tests hand a cheap set from
/// `nage_core::test_support`), decision 115 (the deactivation by an unchecked update).
#[tokio::test]
async fn the_shared_helpers_set_up_a_book_whose_passwords_verify() {
    assert_eq!(
        (
            CHEAP_HASH_PARAMS.memory_kib,
            CHEAP_HASH_PARAMS.passes,
            CHEAP_HASH_PARAMS.lanes
        ),
        (8, 1, 1)
    );
    assert_eq!(TEST_BOOK_NAME, "Household");
    let (_dir, storage) = open_migrated().await;
    let initialized = test_support::set_up(&storage, now(), "admin", PASSWORD).await;
    let admin = initialized.admin;
    assert_eq!(
        (
            initialized.book.name.as_str(),
            initialized.book.currency_code.as_str()
        ),
        ("Household", "CAD")
    );
    assert_eq!(
        (
            admin.username.as_str(),
            admin.display_name.as_str(),
            admin.is_admin
        ),
        ("admin", "admin", true)
    );
    let sam =
        test_support::add_user(&storage, admin.id, later(), "sam", OTHER_PASSWORD, false).await;
    let deputy =
        test_support::add_user(&storage, admin.id, later(), "deputy", OTHER_PASSWORD, true).await;
    assert_eq!((sam.is_admin, sam.created_by), (false, Some(admin.id)));
    assert_eq!((deputy.is_admin, deputy.created_by), (true, Some(admin.id)));

    let mut tx = begin(&storage, "the helpers").await;
    let book = book::book(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"));
    assert_eq!(book, Some(initialized.book), "the book as committed");
    let chart = accounts::accounts(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"));
    assert_eq!(chart.len(), 27, "accounts with the starter chart");
    for (username, own, other) in [
        ("admin", PASSWORD, OTHER_PASSWORD),
        ("sam", OTHER_PASSWORD, PASSWORD),
    ] {
        let hash = stored_hash(&mut tx, username).await;
        assert!(hash.starts_with("$argon2id$v=19$m=8,t=1,p=1$"), "{hash}");
        assert_eq!(password::verify(own, &hash), Verification::Verified);
        assert_eq!(password::verify(other, &hash), Verification::WrongPassword);
    }
    assert_ne!(
        stored_hash(&mut tx, "sam").await,
        stored_hash(&mut tx, "deputy").await,
        "two users with one password share a hash"
    );

    assert_eq!(session_rows(&mut tx).await, 0);
    let _session = session(&mut tx, sam.id, after(DAY), 1).await;
    assert_eq!(session_rows(&mut tx).await, 1);

    test_support::deactivate_unchecked(&mut tx, sam.id, admin.id, after(DAY)).await;
    assert_eq!(
        stored(&mut tx, sam.id).await,
        User {
            disabled_by: Some(admin.id),
            disabled_at: Some(after(DAY)),
            ..sam
        }
    );
    assert_eq!(
        stored(&mut tx, deputy.id).await,
        deputy,
        "another user's row"
    );
    roll_back(tx, "the helpers").await;
    storage.close().await;
}
