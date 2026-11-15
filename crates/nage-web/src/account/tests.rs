//! The change-password form's check of the new password, without a server (decision 112).

use nage_core::password::PasswordError;

use super::{PasswordForm, check_new};
use crate::messages::Refusal;
use crate::pages::PasswordProblems;

/// The form with `new` and `again` in the boxes of the new password.
fn form(new: &str, again: &str) -> PasswordForm {
    PasswordForm {
        current_password: String::from("correct horse 42"),
        new_password: new.to_owned(),
        new_password_again: again.to_owned(),
    }
}

/// The new password is checked by the core's rules, as typed, and the second box against
/// the first; both problems are found at once, each for its own box, and neither names the
/// current password or sets the notice, which are the handler's. A new password that is the
/// same as the current one is not a problem.
///
/// Proves: decision 112 (the new password meets decision 107's rules; the form follows
/// decision 105's rules).
#[test]
fn the_new_password_is_checked_by_the_rules_and_against_its_second_box() {
    assert_eq!(
        check_new(&form("tulip window 99", "tulip window 99")),
        PasswordProblems::default(),
        "the problems of a good new password"
    );
    assert_eq!(
        check_new(&form("correct horse 42", "correct horse 42")),
        PasswordProblems::default(),
        "the problems of a new password equal to the current one"
    );
    assert_eq!(
        check_new(&form(" eight ch ", " eight ch ")),
        PasswordProblems::default(),
        "the problems of a new password of ten characters with a space at each end"
    );
    assert_eq!(
        check_new(&form("123456789", "1234567890")),
        PasswordProblems {
            notice: None,
            current: None,
            new: Some(PasswordError::TooShort.to_string()),
            again: Some(Refusal::NewPasswordsDiffer.to_string()),
        },
        "the problems of a short new password typed differently the second time"
    );
    assert_eq!(
        check_new(&form("tulip window 99", "tulip window 99 ")),
        PasswordProblems {
            again: Some(Refusal::NewPasswordsDiffer.to_string()),
            ..PasswordProblems::default()
        },
        "the problems of a second box with a space more"
    );
    assert_eq!(
        check_new(&form("", "")),
        PasswordProblems {
            new: Some(PasswordError::TooShort.to_string()),
            ..PasswordProblems::default()
        },
        "the problems of an empty new password"
    );
}
