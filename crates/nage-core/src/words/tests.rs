//! The three parts of every refusal the core words, joined as the pages show them (decision 150,
//! assumption A6 of PLAN M2 slice 8). The sentences the pages showed before this slice are held
//! here as literal texts, never compared with the errors' own `Display` alone, so that a `Display`
//! written as the joined parts cannot compare a text with itself; the joined parts and the
//! `Display` the pages are handed are each compared with the literal.

use crate::accounts::AccountError;
use crate::book::BookError;
use crate::fold::NameError;
use crate::password::{HashError, PasswordError};
use crate::setup::SetupError;
use crate::users::{PasswordChangeError, UserError, UserId};

use super::Parts;

/// The parts of `parts`, which must be a refusal's (`label` names it).
#[track_caller]
fn refusal(parts: Option<Parts>, label: &str) -> Parts {
    parts.unwrap_or_else(|| panic!("{label} has no parts, although it is a refusal"))
}

/// One refusal: its label, its parts, the text its `Display` writes (what a page is handed) and
/// the sentence the pages showed before the slice.
type Case = (&'static str, Option<Parts>, String, &'static str);

/// The case of the refusal `$error`, labelled `$label`, whose pages showed `$sentence`.
macro_rules! case {
    ($label:expr, $error:expr, $sentence:expr) => {{
        let error = $error;
        let case: Case = (
            $label,
            Option::<Parts>::from(error.parts()),
            error.to_string(),
            $sentence,
        );
        case
    }};
}

/// Every refusal the core words, by user, book, password, password change and set-up, joined
/// to the sentence the pages showed before the slice, held below as a literal text, and its
/// `Display`, which the pages are handed, equal to the same literal; and the failures nobody
/// caused (a database error, a corrupt book row, a hasher that cannot hash, an account the
/// set-up could not create) have no parts. The literal texts are the `Display` texts of the base,
/// copied by hand.
///
/// Proves: decision 150 (every core refusal's parts join to the sentence the pages showed before the slice, held as literal texts; assumption A6)
#[test]
fn every_refusal_joins_to_todays_sentence() {
    let alex = || String::from("alex");
    let cases: Vec<Case> = vec![
        case!(
            "UserError::Username(Empty)",
            UserError::Username(NameError::Empty),
            "The user was not created because the username is empty. Enter a username, then try again."
        ),
        case!(
            "UserError::Username(TooLong)",
            UserError::Username(NameError::TooLong { chars: 70 }),
            "The user was not created because the username is longer than 64 characters (it has 70). Shorten the username, then try again."
        ),
        case!(
            "UserError::Username(Control)",
            UserError::Username(NameError::Control),
            "The user was not created because the username contains a tab, a line break or another character that cannot be shown. Type the username again on one line, then try again."
        ),
        case!(
            "UserError::DisplayName(Empty)",
            UserError::DisplayName(NameError::Empty),
            "The user was not created because the display name is empty. Enter a display name, then try again."
        ),
        case!(
            "UserError::DisplayName(Colon)",
            UserError::DisplayName(NameError::Colon),
            "The user was not created because the display name contains a colon (:). Remove the colon from the display name, then try again."
        ),
        case!(
            "UserError::PasswordHashEmpty",
            UserError::PasswordHashEmpty,
            "The user was not created because no password was given, and every user needs one to sign in. Enter a password, then try again."
        ),
        case!(
            "UserError::FirstUserMustBeAdmin",
            UserError::FirstUserMustBeAdmin,
            "The user was not created because the first user of a book must be an admin, since only an admin can add the other users. Create the first user as an admin."
        ),
        case!(
            "UserError::CreatorRequired",
            UserError::CreatorRequired,
            "The user was not created because no admin was named as the one adding them, and every user after the first is added by an admin. Name the admin who is adding this user, then try again."
        ),
        case!(
            "UserError::NoSuchCreator",
            UserError::NoSuchCreator,
            "The user was not created because the admin named as the one adding them does not exist. Name an existing admin, then try again."
        ),
        case!(
            "UserError::CreatorNotAdmin",
            UserError::CreatorNotAdmin { username: alex() },
            "The user was not created because alex, who was named as the one adding them, is not an active admin, and only an active admin can add users. Name an active admin, then try again."
        ),
        case!(
            "UserError::UsernameTaken",
            UserError::UsernameTaken,
            "The user was not created because another user already has this username; usernames that differ only in capitals or extra spaces count as the same. Choose a different username, then try again."
        ),
        case!(
            "BookError::AlreadyInitialized",
            BookError::AlreadyInitialized,
            "The book was not set up because Nage already holds a book here, and a book is set up only once, since its currency never changes. Sign in to use that book; to start another one, run Nage with a new, empty data directory."
        ),
        case!(
            "BookError::Name(Empty)",
            BookError::Name(NameError::Empty),
            "The book was not set up because the book name is empty. Enter a book name, then try again."
        ),
        case!(
            "BookError::Name(TooLong)",
            BookError::Name(NameError::TooLong { chars: 65 }),
            "The book was not set up because the book name is longer than 64 characters (it has 65). Shorten the book name, then try again."
        ),
        case!(
            "PasswordError::TooShort",
            PasswordError::TooShort,
            "This password cannot be used because it has fewer than 10 characters, and a short password is easy to guess. Choose one with at least 10 characters; a few words in a row work well."
        ),
        case!(
            "PasswordError::TooLong",
            PasswordError::TooLong,
            "This password cannot be used because it is longer than 1,024 bytes, about a thousand letters, which is the most Nage accepts. Choose a shorter one."
        ),
        case!(
            "HashError::Password(TooShort)",
            HashError::Password(PasswordError::TooShort),
            "This password cannot be used because it has fewer than 10 characters, and a short password is easy to guess. Choose one with at least 10 characters; a few words in a row work well."
        ),
        case!(
            "PasswordChangeError::NoSuchUser",
            PasswordChangeError::NoSuchUser { user: UserId(7) },
            "The password was not changed because the user it belongs to does not exist (user number 7). Check which user was meant, then try again."
        ),
        case!(
            "PasswordChangeError::Deactivated",
            PasswordChangeError::Deactivated { username: alex() },
            "The password was not changed because alex has been deactivated, and a deactivated user cannot sign in or change a password. Ask an admin to reactivate alex first."
        ),
        case!(
            "PasswordChangeError::ChangedMeanwhile",
            PasswordChangeError::ChangedMeanwhile { username: alex() },
            "The password was not changed because alex's password was changed or reset somewhere else a moment ago, after the current password had been checked. Sign in again with the password that is valid now, then change it if you still want to."
        ),
        case!(
            "PasswordChangeError::NoSuchAdmin",
            PasswordChangeError::NoSuchAdmin { admin: UserId(3) },
            "The password was not changed because the admin named as the one resetting it does not exist (user number 3). Name an existing admin, then try again."
        ),
        case!(
            "PasswordChangeError::NotAnAdmin",
            PasswordChangeError::NotAnAdmin { username: alex() },
            "The password was not changed because alex, who was named as the one resetting it, is not an active admin, and only an active admin can reset a password. Name an active admin, then try again."
        ),
        case!(
            "PasswordChangeError::PasswordHashEmpty",
            PasswordChangeError::PasswordHashEmpty,
            "The password was not changed because no new password was given, and every user needs one to sign in. Enter a new password, then try again."
        ),
        case!(
            "SetupError::AlreadyInitialized",
            SetupError::AlreadyInitialized,
            "The book was not set up because Nage already holds a book here, and a book is set up only once, since its currency never changes. Sign in to use that book; to start another one, run Nage with a new, empty data directory."
        ),
        case!(
            "SetupError::Book(Name(Colon))",
            SetupError::Book(BookError::Name(NameError::Colon)),
            "The book was not set up because the book name contains a colon (:). Remove the colon from the book name, then try again."
        ),
        case!(
            "SetupError::User(UsernameTaken)",
            SetupError::User(UserError::UsernameTaken),
            "The user was not created because another user already has this username; usernames that differ only in capitals or extra spaces count as the same. Choose a different username, then try again."
        ),
    ];
    for (label, parts, display, sentence) in cases {
        let parts = refusal(parts, label);
        assert_eq!(parts.joined(), sentence, "the joined parts of {label}");
        assert_eq!(
            display, sentence,
            "the Display of {label}, as a page shows it"
        );
    }

    let not_refusals = [
        (
            "UserError::Database",
            UserError::Database(sqlx::Error::RowNotFound)
                .parts()
                .is_some(),
        ),
        (
            "BookError::Corrupt",
            BookError::Corrupt(String::from("minor_digits is 9"))
                .parts()
                .is_some(),
        ),
        (
            "BookError::Database",
            BookError::Database(sqlx::Error::RowNotFound)
                .parts()
                .is_some(),
        ),
        (
            "HashError::Hasher",
            HashError::Hasher(String::from("no salt")).parts().is_some(),
        ),
        (
            "PasswordChangeError::Database",
            PasswordChangeError::Database(sqlx::Error::RowNotFound)
                .parts()
                .is_some(),
        ),
        (
            "SetupError::Account",
            SetupError::Account(AccountError::from(sqlx::Error::RowNotFound))
                .parts()
                .is_some(),
        ),
        (
            "SetupError::Database",
            SetupError::Database(sqlx::Error::RowNotFound)
                .parts()
                .is_some(),
        ),
    ];
    for (label, has_parts) in not_refusals {
        assert!(!has_parts, "{label} has parts, although nobody caused it");
    }
}
