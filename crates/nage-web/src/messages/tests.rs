//! The texts the pages word themselves, pinned (decisions 102, 105, 106, 110 to 112, 117, 118
//! and 124).

use super::{Notice, Refusal, wait};

/// Each refusal a page words itself says what was refused, why and what to do instead, and
/// no two share a text; the two confirmations say what happened. The test prints each moment
/// with its text (`cargo test -p nage-web --lib explain -- --nocapture` shows them).
///
/// Proves: decision 102 (the three parts), decisions 105, 106, 110, 111 and 112 (what each
/// page says), decision 114 (every refusal slice 5 adds is three-part; "another user"),
/// decision 117 (the one-time messages' texts), decision 118 (what the page for an unknown
/// address says), decision 124 (what the refusal of an appearance the menu does not offer
/// says).
#[test]
fn page_refusals_explain_themselves() {
    let cases: [(&str, &str, Refusal, &str); 16] = [
        (
            "Refusal::SetupClosed",
            "Sam has set Nage up. Alex, who does not know that yet, opens the set-up address.",
            Refusal::SetupClosed,
            "Nage cannot be set up a second time, because it already has its book and its first user, and a book is set up only once. Sign in to use it; if another user set it up, ask them to add you first.",
        ),
        (
            "Refusal::SetupNotDone",
            "The set-up form is sent with an empty book name and a password of 8 characters. Above the form:",
            Refusal::SetupNotDone,
            "Nage was not set up, because some of the boxes below need a change. Each one says what to change; then send the form again.",
        ),
        (
            "Refusal::UnknownCurrency",
            "A set-up form that was changed by hand names the currency XXX, which the page's own list never offers.",
            Refusal::UnknownCurrency {
                typed: String::from("XXX"),
            },
            "The book was not set up because \"XXX\" is not one of the currency codes in the list. Choose the book's currency from the list, then try again.",
        ),
        (
            "Refusal::SetupPasswordsDiffer",
            "On the set-up page the password is typed as correct horse 42 the first time and correct horse 43 the second time.",
            Refusal::SetupPasswordsDiffer,
            "The user was not created because the two password boxes do not hold the same password, so one of them was mistyped. Type the password again in both boxes.",
        ),
        (
            "Refusal::SignIn",
            "Sam signs in with a wrong password; or someone types a username nobody has; or a deactivated user types the right password.",
            Refusal::SignIn,
            "You were not signed in because the username or the password is not right. Check both and try again; if you have forgotten your password, ask an admin to reset it.",
        ),
        (
            "Refusal::SignInPaused",
            "Five wrong passwords for sam in a row, and a sixth try follows at once.",
            Refusal::SignInPaused { seconds: 60 },
            "You were not signed in because too many wrong passwords were tried for this username, so Nage has paused sign-ins for it. Wait 1 minute, then try again.",
        ),
        (
            "Refusal::PasswordNotChanged",
            "The change-password form is sent with a wrong current password. Above the form:",
            Refusal::PasswordNotChanged,
            "Your password was not changed, because some of the boxes below need a change. Each one says what to change; then send the form again.",
        ),
        (
            "Refusal::CurrentPasswordWrong",
            "On the change-password page Sam mistypes the current password.",
            Refusal::CurrentPasswordWrong,
            "Your password was not changed because the current password is not right. Type the password you sign in with now, then try again.",
        ),
        (
            "Refusal::NewPasswordsDiffer",
            "On the change-password page the new password is typed differently the second time.",
            Refusal::NewPasswordsDiffer,
            "Your password was not changed because the two boxes for the new password do not hold the same password, so one of them was mistyped. Type the new password again in both boxes.",
        ),
        (
            "Refusal::PasswordCheckPaused",
            "Seven wrong passwords for sam in a row, and the change-password form is sent 44 seconds into the pause of 5 minutes that the seventh started.",
            Refusal::PasswordCheckPaused { seconds: 256 },
            "Your password was not changed because too many wrong passwords were tried for your username, so Nage has paused password checks for it. Wait 4 minutes and 16 seconds, then try again.",
        ),
        (
            "Refusal::PasswordChangedElsewhere",
            "Sam saves a new password on the phone in the very second in which an admin resets Sam's password at the server's terminal. The phone is sent to the sign-in page, which says once, at its top:",
            Refusal::PasswordChangedElsewhere,
            "Your password was not changed because it was changed or reset somewhere else a moment ago. Sign in with the password that is valid now, then change it if you still want to.",
        ),
        (
            "Refusal::PasswordChangedElsewhereSignedIn",
            "Sam taps Change password twice and the two sends overlap, or has the change-password page open in two tabs and sends both forms in the same moment. The first change is saved; the second send lands on the home page, still signed in, which says once, at its top:",
            Refusal::PasswordChangedElsewhereSignedIn,
            "This password change was not made because Nage received the form twice in the same moment (a double tap on the button, or the page open in two tabs) and had just saved the first one. Your password is now the one that first form set, and you are still signed in. To change it again, open Change password from the menu.",
        ),
        (
            "Refusal::NotFound",
            "Sam, signed in, follows an old bookmark to an address Nage has no page for, or mistypes an address.",
            Refusal::NotFound,
            "Nage could not show this page because it has no page at this address: the address may be mistyped, or the link that led here may be out of date. Go to the home page and find what you were looking for from there.",
        ),
        (
            "Refusal::CrossSite",
            "A form is sent to Nage from a page of another site, or by a browser that does not say where the form came from.",
            Refusal::CrossSite,
            "Nage did not carry out this request because it could not tell that the form was sent from one of its own pages: the browser named another site as the sender, or none at all. Open Nage in your browser, go to the page again and send the form from there.",
        ),
        (
            "Refusal::FormTooLarge",
            "A whole book chapter is pasted into a box and the form is sent.",
            Refusal::FormTooLarge,
            "Nage did not accept this form because it holds more than 64 KiB of text, about 65,000 letters, which is more than any form in Nage needs. Shorten what you entered, then send the form again.",
        ),
        (
            "Refusal::UnknownTheme",
            "A form changed by hand posts the appearance sepia to Nage, which the menu's three buttons, Automatic, Light and Dark, never send.",
            Refusal::UnknownTheme,
            "Nage did not change the appearance because the form asked for one that the menu does not offer. Go back to Nage and choose Automatic, Light or Dark in the menu.",
        ),
    ];
    for (variant, moment, refusal, expected) in &cases {
        let message = refusal.to_string();
        println!("[{variant}] {moment} => {message}");
        assert_eq!(&message, expected, "the message of {refusal:?}");
        assert!(
            message.contains(" because ") || message.contains(", because "),
            "the message of {refusal:?} does not say why: {message}"
        );
    }
    for (index, (_, _, _, text)) in cases.iter().enumerate() {
        for (_, _, other, other_text) in &cases[index + 1..] {
            assert_ne!(text, other_text, "{other:?} shares a message with another");
        }
    }

    let confirmation = Notice::PasswordChanged.to_string();
    println!(
        "[Notice::PasswordChanged] Sam changes the password on the phone while the laptop is signed in too. Back on the change-password page, once, at its top: => {confirmation}"
    );
    assert_eq!(
        confirmation,
        "Your password was changed. Every other device that was signed in as you has been signed out; this one stays signed in.",
        "the confirmation of a change"
    );
    let signed_out = Notice::SignedOut.to_string();
    println!(
        "[Notice::SignedOut] Sam taps Menu and then Sign out on the family tablet. The sign-in page says once, at its top: => {signed_out}"
    );
    assert_eq!(
        signed_out, "You are signed out.",
        "the confirmation of a sign-out"
    );
}

/// A wait is said in words, in seconds below a minute, in whole minutes where it is whole
/// minutes, and in both otherwise, each in the singular for one.
///
/// Proves: decision 111 (the refused form says how long to wait).
#[test]
fn a_wait_is_said_in_words() {
    for (seconds, words) in [
        (1, "1 second"),
        (2, "2 seconds"),
        (59, "59 seconds"),
        (60, "1 minute"),
        (61, "1 minute and 1 second"),
        (64, "1 minute and 4 seconds"),
        (120, "2 minutes"),
        (256, "4 minutes and 16 seconds"),
        (300, "5 minutes"),
        (600, "10 minutes"),
    ] {
        assert_eq!(wait(seconds), words, "the wait of {seconds} seconds");
    }
}
