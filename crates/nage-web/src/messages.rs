//! What the pages say when they refuse or confirm something (SPEC decisions 102, 105, 106,
//! 110 to 112, 117 and 118). Each refusal says what was refused, why and what to do instead;
//! the other person who uses the book is "another user". The refusals the core words (a name,
//! a password) are shown as the core words them; these are the ones only a page can cause, and
//! a change of one's own password that a reset or another change overtook, which a page words
//! in the second person. The texts are public so that the tests of the pages compare with them
//! instead of repeating them.

use std::fmt;

use nage_core::accounts::Word;

#[cfg(test)]
mod tests;

/// A refusal a page words itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `/setup` once a user exists: the page of the 404 (decision 105).
    SetupClosed,
    /// Above a refused set-up form: nothing was written.
    SetupNotDone,
    /// The set-up form named a currency that is not in the list, which the page's own list
    /// never does.
    UnknownCurrency { typed: String },
    /// The two password boxes of the set-up form differ.
    SetupPasswordsDiffer,
    /// A sign-in refused for its username or its password: the one message, which says
    /// neither whether the username exists nor which of the two was wrong (decision 106).
    SignIn,
    /// A sign-in attempted during a pause, refused unchecked, or one whose verification ended
    /// after a pause for its username had begun (decision 111).
    SignInPaused { seconds: u64 },
    /// Above a refused change-password form: nothing was changed.
    PasswordNotChanged,
    /// The current password typed on the change-password page is wrong (decision 112).
    CurrentPasswordWrong,
    /// The two boxes for the new password differ.
    NewPasswordsDiffer,
    /// A change of password attempted during a pause, refused unchecked, or one whose check of
    /// the current password ended after a pause for the username had begun (decision 111).
    PasswordCheckPaused { seconds: u64 },
    /// A change of one's own password that a reset or another change overtook between the
    /// check of the current password and the save
    /// (`nage_core::users::PasswordChangeError::ChangedMeanwhile`, decision 112), as the
    /// sign-in page says it once, when that reset or change ended the session in use
    /// (decision 117).
    PasswordChangedElsewhere,
    /// The same refusal as a page under the menu says it once, when the session in use
    /// stayed valid, as when the same form is sent twice in the same moment, by a double tap
    /// on the button or from two tabs (decision 117): the person is still signed in, so
    /// nothing tells them to sign in.
    PasswordChangedElsewhereSignedIn,
    /// An address no route answers, asked for with a valid session: the page of the 404,
    /// which never names the address (decision 118).
    NotFound,
    /// A mutating request whose `Origin` or `Referer` does not name Nage: the page of the 403
    /// (decision 110).
    CrossSite,
    /// A form larger than 64 KiB: the page of the 413 (decision 110).
    FormTooLarge,
    /// The appearance form named a choice the menu does not offer, which the menu's own
    /// buttons never do: the page of the 422 of `POST /theme`.
    UnknownTheme,
    /// Above a refused create form (decision 132): nothing was written.
    AccountNotCreated { word: Word },
    /// Above a refused change form (decision 134): nothing was written.
    AccountNotSaved { word: Word },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetupClosed => f.write_str(
                "Nage cannot be set up a second time, because it already has its book and its first user, and a book is set up only once. Sign in to use it; if another user set it up, ask them to add you first.",
            ),
            Self::SetupNotDone => f.write_str(
                "Nage was not set up, because some of the boxes below need a change. Each one says what to change; then send the form again.",
            ),
            Self::UnknownCurrency { typed } => write!(
                f,
                "The book was not set up because \"{typed}\" is not one of the currency codes in the list. Choose the book's currency from the list, then try again."
            ),
            Self::SetupPasswordsDiffer => f.write_str(
                "The user was not created because the two password boxes do not hold the same password, so one of them was mistyped. Type the password again in both boxes.",
            ),
            Self::SignIn => f.write_str(
                "You were not signed in because the username or the password is not right. Check both and try again; if you have forgotten your password, ask an admin to reset it.",
            ),
            Self::SignInPaused { seconds } => write!(
                f,
                "You were not signed in because too many wrong passwords were tried for this username, so Nage has paused sign-ins for it. Wait {}, then try again.",
                wait(*seconds)
            ),
            Self::PasswordNotChanged => f.write_str(
                "Your password was not changed, because some of the boxes below need a change. Each one says what to change; then send the form again.",
            ),
            Self::CurrentPasswordWrong => f.write_str(
                "Your password was not changed because the current password is not right. Type the password you sign in with now, then try again.",
            ),
            Self::NewPasswordsDiffer => f.write_str(
                "Your password was not changed because the two boxes for the new password do not hold the same password, so one of them was mistyped. Type the new password again in both boxes.",
            ),
            Self::PasswordCheckPaused { seconds } => write!(
                f,
                "Your password was not changed because too many wrong passwords were tried for your username, so Nage has paused password checks for it. Wait {}, then try again.",
                wait(*seconds)
            ),
            Self::PasswordChangedElsewhere => f.write_str(
                "Your password was not changed because it was changed or reset somewhere else a moment ago. Sign in with the password that is valid now, then change it if you still want to.",
            ),
            Self::PasswordChangedElsewhereSignedIn => f.write_str(
                "This password change was not made because Nage received the form twice in the same moment (a double tap on the button, or the page open in two tabs) and had just saved the first one. Your password is now the one that first form set, and you are still signed in. To change it again, open Change password from the menu.",
            ),
            Self::NotFound => f.write_str(
                "Nage could not show this page because it has no page at this address: the address may be mistyped, or the link that led here may be out of date. Go to the home page and find what you were looking for from there.",
            ),
            Self::CrossSite => f.write_str(
                "Nage did not carry out this request because it could not tell that the form was sent from one of its own pages: the browser named another site as the sender, or none at all. Open Nage in your browser, go to the page again and send the form from there.",
            ),
            Self::FormTooLarge => f.write_str(
                "Nage did not accept this form because it holds more than 64 KiB of text, about 65,000 letters, which is more than any form in Nage needs. Shorten what you entered, then send the form again.",
            ),
            Self::UnknownTheme => f.write_str(
                "Nage did not change the appearance because the form asked for one that the menu does not offer. Go back to Nage and choose Automatic, Light or Dark in the menu.",
            ),
            Self::AccountNotCreated { word } => write!(
                f,
                "The {word} was not created, because some of the boxes below need a change. Each one says what to change; then send the form again."
            ),
            Self::AccountNotSaved { word } => write!(
                f,
                "The {word} was not saved, because some of the boxes below need a change. Each one says what to change; then send the form again."
            ),
        }
    }
}

/// A confirmation a page shows, once (decision 117).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// One's own password was changed and the other devices were signed out (decision 112):
    /// on the change-password page, after an accepted change.
    PasswordChanged,
    /// The person signed out (decision 106): on the sign-in page, so that a sign-out on a
    /// shared device is seen to have worked.
    SignedOut,
    /// An account or category was created (decision 132): on its page.
    AccountCreated { word: Word },
    /// An account or category was changed (decision 134): on its page.
    AccountChanged { word: Word },
    /// An account or category was archived (decision 135): on its page.
    AccountArchived { word: Word },
    /// An account or category was unarchived (decision 135): on its page.
    AccountUnarchived { word: Word },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PasswordChanged => f.write_str(
                "Your password was changed. Every other device that was signed in as you has been signed out; this one stays signed in.",
            ),
            Self::SignedOut => f.write_str("You are signed out."),
            Self::AccountCreated { word } => write!(f, "This {word} was created."),
            Self::AccountChanged { word } => write!(f, "This {word} was changed."),
            Self::AccountArchived { word } => write!(f, "This {word} was archived."),
            Self::AccountUnarchived { word } => write!(f, "This {word} was unarchived."),
        }
    }
}

/// How long to wait, in words (decision 111: a refused form says it, since browsers ignore
/// `Retry-After` on a page): `1 second`, `8 seconds`, `1 minute`, `4 minutes and 16 seconds`,
/// `10 minutes`.
pub fn wait(seconds: u64) -> String {
    let unit = |count: u64, word: &str| {
        if count == 1 {
            format!("1 {word}")
        } else {
            format!("{count} {word}s")
        }
    };
    match (seconds / 60, seconds % 60) {
        (0, rest) => unit(rest, "second"),
        (minutes, 0) => unit(minutes, "minute"),
        (minutes, rest) => format!("{} and {}", unit(minutes, "minute"), unit(rest, "second")),
    }
}
