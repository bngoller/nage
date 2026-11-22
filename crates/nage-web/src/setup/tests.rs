//! The set-up form's check, without a server (decision 105).

use nage_core::book::BookError;
use nage_core::fold::NameError;
use nage_core::password::PasswordError;
use nage_core::users::UserError;

use super::{DEFAULT_CURRENCY, SetupForm, check, view};
use crate::messages::Refusal;
use crate::pages::SetupProblems;

/// A form that is accepted.
fn good() -> SetupForm {
    SetupForm {
        book_name: String::from("Household"),
        currency: String::from("EUR"),
        username: String::from("sam"),
        display_name: String::from("Sam"),
        password: String::from("correct horse 42"),
        password_again: String::from("correct horse 42"),
        starter_chart: Some(String::from("on")),
    }
}

/// A form without a problem is accepted as typed: the names and the password untouched (the
/// core normalizes the names, and a password is never trimmed), the currency from the table,
/// the starter chart exactly when the tick box was sent, whatever its value.
///
/// Proves: decision 105, decision 107 (the password as typed).
#[test]
fn a_good_form_is_accepted_as_typed() {
    let mut form = good();
    form.book_name = String::from("  Our  book ");
    form.password = String::from(" eight ch ");
    form.password_again = String::from(" eight ch ");
    let accepted = check(&form).unwrap_or_else(|problems| panic!("refused: {problems:?}"));
    assert_eq!(
        accepted.book_name, "  Our  book ",
        "the book name handed on"
    );
    assert_eq!(accepted.currency.code(), "EUR", "the currency handed on");
    assert_eq!(accepted.username, "sam", "the username handed on");
    assert_eq!(accepted.display_name, "Sam", "the display name handed on");
    assert_eq!(accepted.password, " eight ch ", "the password handed on");
    assert!(accepted.starter_chart, "the ticked box is not handed on");

    for (sent, wanted) in [(None, false), (Some(""), true), (Some("off"), true)] {
        let mut form = good();
        form.starter_chart = sent.map(str::to_owned);
        let accepted = check(&form).unwrap_or_else(|problems| panic!("refused: {problems:?}"));
        assert_eq!(
            accepted.starter_chart, wanted,
            "the starter chart for a tick box sent as {sent:?}"
        );
    }
}

/// Every problem of a form is found at once, each for its own box and in the words of the
/// rule it breaks, with the notice above; a form with one problem names that one alone. What
/// a refused form shows again is everything typed except the passwords, with the list back
/// on CAD when the currency sent is not in the table.
///
/// Proves: decision 105 (checked whole, every problem at once, a problem with one box under
/// that box; what was typed is kept), decision 114 (the core's messages as they stand).
#[test]
fn every_problem_of_a_form_is_found_at_once() {
    let form = SetupForm {
        book_name: String::from(" \t "),
        currency: String::from("eur"),
        username: "u".repeat(65),
        display_name: String::from("Sam\nthe admin"),
        password: String::from("123456789"),
        password_again: String::from("1234567890"),
        starter_chart: None,
    };
    let problems = check(&form).map(|_| ()).unwrap_err();
    assert_eq!(
        *problems,
        SetupProblems {
            notice: Some(Refusal::SetupNotDone.to_string()),
            book_name: Some(BookError::Name(NameError::Empty).to_string()),
            currency: Some(
                Refusal::UnknownCurrency {
                    typed: String::from("eur")
                }
                .to_string()
            ),
            username: Some(UserError::Username(NameError::TooLong { chars: 65 }).to_string()),
            display_name: Some(UserError::DisplayName(NameError::Control).to_string()),
            password: Some(PasswordError::TooShort.to_string()),
            password_again: Some(Refusal::SetupPasswordsDiffer.to_string()),
        },
        "the problems of a form with one in every box"
    );
    let shown = view(&form);
    assert_eq!(shown.book_name, " \t ", "the book name shown again");
    assert_eq!(shown.currency, DEFAULT_CURRENCY, "the currency shown again");
    assert_eq!(shown.username, "u".repeat(65), "the username shown again");
    assert_eq!(
        shown.display_name, "Sam\nthe admin",
        "the display name shown again"
    );
    assert!(!shown.starter_chart, "the tick box shown again");

    let mut one = good();
    one.password_again = String::from("correct horse 43");
    let problems = check(&one).map(|_| ()).unwrap_err();
    assert_eq!(
        *problems,
        SetupProblems {
            notice: Some(Refusal::SetupNotDone.to_string()),
            password_again: Some(Refusal::SetupPasswordsDiffer.to_string()),
            ..SetupProblems::default()
        },
        "the problems of a form whose second password differs"
    );
    let shown = view(&one);
    assert_eq!(shown.currency, "EUR", "the currency a refused form keeps");
    assert!(shown.starter_chart, "the tick a refused form keeps");

    let mut long = good();
    long.password = "p".repeat(1025);
    long.password_again = long.password.clone();
    let problems = check(&long).map(|_| ()).unwrap_err();
    assert_eq!(
        problems.password,
        Some(PasswordError::TooLong.to_string()),
        "the problem of a password of 1,025 bytes"
    );
    assert_eq!(
        problems.password_again, None,
        "the second box of a password typed the same twice"
    );
}
