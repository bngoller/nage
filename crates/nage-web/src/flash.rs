//! One-time messages (SPEC 5.2, 5.3, INV-28, decision 117): a message that appears at the
//! top of the next page, once. It travels in a second cookie beside `nage_session`, named
//! [`crate::MESSAGE_COOKIE`], that holds a short code from a fixed set, never text, so whoever
//! can set cookies in that browser can only make Nage's own fixed messages appear, never other
//! words; a cookie set for another path or for a parent domain is not removed by Nage's
//! clearing and shows its message until it expires.
//!
//! - **Set** on these answers only: the 303 of an accepted change of one's own password, the
//!   303 of a sign-out, the 303 that sends an overtaken change to the sign-in page, and the 303
//!   of an account save that wrote something (created, changed, archived, unarchived), whose
//!   code carries the account's number in digits and nothing else (decision 137, the pause's
//!   P7); an account's page shows it only when the number is its own, and a message of the
//!   three older codes as every other page does. A form
//!   refused for what was typed, a busy answer and a failure leave no message. The messages
//!   share the one cookie, so a message set while another is still waiting replaces it.
//! - **Shown and cleared** by the next page a GET renders: the home page, the change-password
//!   page, the page for an unknown address, the sign-in page, the set-up page and its closed
//!   page, and every account page (the list, the choice page, the create and change forms, the
//!   confirmations and an account's page), of which only the page of the account whose number
//!   an account message carries shows it; every other one, another account's page included,
//!   clears it unseen (decision 137). A message of the three older codes shows on an account's
//!   page as on every other page. A message shows only on a page where it is true
//!   (`Flash::shown`): the sign-out's on
//!   a page without a signed-in user, the change's on a page with one. A code Nage does not
//!   know, and a message that is not true on the page, show nothing and are cleared all the
//!   same.
//! - **Left as it is** by every other answer: a HEAD (it has no body to show a message in), a
//!   POST, a redirect, the busy page and a failure; `/health` and `/static/*` never read it.
//! - The cookie is `nage_message=<code>` with `Path=/`, `HttpOnly`, `SameSite=Lax`, a
//!   `Max-Age` of [`MAX_AGE_SECONDS`] and `Secure` by the session cookie's rule
//!   (`cookie::is_secure`); like every cookie it is never logged.

use axum::http::{HeaderMap, HeaderValue, Method};

use crate::pages::Message;

#[cfg(test)]
mod tests;

/// How long the browser keeps a one-time message that no page has shown, in seconds: long
/// enough for the redirect that carries it, short enough that a message nobody saw does not
/// turn up later.
pub(crate) const MAX_AGE_SECONDS: i64 = 60;

/// The messages that travel as one-time messages (decision 117). The four account messages
/// carry the number of the account they are about (decision 137, the pause's P7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flash {
    /// One's own password was changed (`Notice::PasswordChanged`, decision 112).
    PasswordChanged,
    /// The person signed out (`Notice::SignedOut`, decision 106).
    SignedOut,
    /// A change of one's own password was overtaken by a reset or another change
    /// (`Refusal::PasswordChangedElsewhere`, decision 112).
    PasswordChangedElsewhere,
    /// An account or category was created (`Notice::AccountCreated`, decision 137).
    AccountCreated(AccountId),
    /// An account or category was changed (`Notice::AccountChanged`, decision 137).
    AccountChanged(AccountId),
    /// An account or category was archived (`Notice::AccountArchived`, decision 137).
    AccountArchived(AccountId),
    /// An account or category was unarchived (`Notice::AccountUnarchived`, decision 137).
    AccountUnarchived(AccountId),
}

/// What stands between an account message's fixed code and the account's number in the cookie.
pub(crate) const NUMBER_SEPARATOR: char = '.';

impl Flash {
    /// The three messages about no account.
    pub(crate) const FIXED: [Flash; 3] = [
        Flash::PasswordChanged,
        Flash::SignedOut,
        Flash::PasswordChangedElsewhere,
    ];

    /// The fixed code of this message: `password-changed`, `signed-out`,
    /// `password-changed-elsewhere`, `account-created`, `account-changed`, `account-archived`
    /// or `account-unarchived`.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Flash::PasswordChanged => "password-changed",
            Flash::SignedOut => "signed-out",
            Flash::PasswordChangedElsewhere => "password-changed-elsewhere",
            Flash::AccountCreated(_) => "account-created",
            Flash::AccountChanged(_) => "account-changed",
            Flash::AccountArchived(_) => "account-archived",
            Flash::AccountUnarchived(_) => "account-unarchived",
        }
    }

    /// The account an account message is about; `None` for the three others.
    pub(crate) const fn account(self) -> Option<AccountId> {
        match self {
            Flash::AccountCreated(id)
            | Flash::AccountChanged(id)
            | Flash::AccountArchived(id)
            | Flash::AccountUnarchived(id) => Some(id),
            Flash::PasswordChanged | Flash::SignedOut | Flash::PasswordChangedElsewhere => None,
        }
    }

    /// What the cookie holds for this message: its fixed code, and for an account message the
    /// separator and the account's number in digits as Nage prints it, nothing else
    /// (`account-created.12`).
    pub(crate) fn value(self) -> String {
        match self.account() {
            Some(id) => format!("{}{NUMBER_SEPARATOR}{id}", self.code()),
            None => self.code().to_owned(),
        }
    }

    /// The message as the page of the account `id` shows it (decision 137, the pause's P7),
    /// worded for an account or a category by `word`: only an account message whose number is
    /// `id`; `None` for any other, which the page clears unseen.
    pub(crate) fn shown_on_account(self, id: AccountId, word: Word) -> Option<Message> {
        if self.account() != Some(id) {
            return None;
        }
        let notice = match self {
            Flash::AccountCreated(_) => Notice::AccountCreated { word },
            Flash::AccountChanged(_) => Notice::AccountChanged { word },
            Flash::AccountArchived(_) => Notice::AccountArchived { word },
            Flash::AccountUnarchived(_) => Notice::AccountUnarchived { word },
            Flash::PasswordChanged | Flash::SignedOut | Flash::PasswordChangedElsewhere => {
                return None;
            }
        };
        Some(Message {
            text: notice.to_string(),
            alert: false,
        })
    }

    /// The message whose cookie value is exactly `text` ([`Flash::value`]): a fixed code alone,
    /// or an account message's code, the separator and a number well formed as Nage prints it
    /// (`crate::accounts::well_formed_number`); `None` for any other text, which a page shows
    /// nothing for.
    pub(crate) fn from_code(text: &str) -> Option<Flash> {
        if let Some(flash) = Flash::FIXED.into_iter().find(|flash| flash.code() == text) {
            return Some(flash);
        }
        let (code, number) = text.split_once(NUMBER_SEPARATOR)?;
        let id = well_formed_number(number)?;
        [
            Flash::AccountCreated(id),
            Flash::AccountChanged(id),
            Flash::AccountArchived(id),
            Flash::AccountUnarchived(id),
        ]
        .into_iter()
        .find(|flash| flash.code() == code)
    }

    /// The message as a page shows it, or `None` where it is not true. `signed_in` says
    /// whether the page knows a signed-in user. `PasswordChanged` and `SignedOut` are
    /// confirmations (`Notice`), shown as a status: `PasswordChanged` only on a page with a
    /// signed-in user, `SignedOut` only on a page without one. `PasswordChangedElsewhere` is
    /// shown as an alert on either: in the words of `Refusal::PasswordChangedElsewhere` on a
    /// page without a signed-in user (the sign-in page, when the other change or the reset
    /// ended the session in use), and in those of `Refusal::PasswordChangedElsewhereSignedIn`
    /// on a page with one (the home page, when the session stayed valid). A page that shows
    /// nothing clears the cookie all the same.
    pub(crate) fn shown(self, signed_in: bool) -> Option<Message> {
        let (text, alert) = match (self, signed_in) {
            (Flash::PasswordChanged, true) => (Notice::PasswordChanged.to_string(), false),
            (Flash::SignedOut, false) => (Notice::SignedOut.to_string(), false),
            (Flash::PasswordChangedElsewhere, false) => {
                (Refusal::PasswordChangedElsewhere.to_string(), true)
            }
            (Flash::PasswordChangedElsewhere, true) => {
                (Refusal::PasswordChangedElsewhereSignedIn.to_string(), true)
            }
            (Flash::PasswordChanged, false) | (Flash::SignedOut, true) => return None,
            (
                Flash::AccountCreated(_)
                | Flash::AccountChanged(_)
                | Flash::AccountArchived(_)
                | Flash::AccountUnarchived(_),
                _,
            ) => return None,
        };
        Some(Message { text, alert })
    }
}

/// What a page does about the one-time message of the request it answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    /// The message to show, when the request carries the code of one.
    pub(crate) flash: Option<Flash>,
    /// Whether the answer clears the cookie: the request carries one, whatever it holds.
    pub(crate) clear: bool,
}

/// What a page that answers a request with `method` and `headers` does about the one-time
/// message. On a GET, the value of the first cookie named `nage_message` in the `Cookie`
/// headers that are read (`message_cookie`: one that holds a byte outside ASCII is not),
/// whatever other cookies stand beside it: a known code is shown and cleared, any other value
/// is only cleared, and without the cookie nothing is shown or cleared. On any other method
/// nothing is shown or cleared, so a HEAD leaves the message for the page that can show it.
pub(crate) fn pending(method: &Method, headers: &HeaderMap) -> Pending {
    let sent = if *method == Method::GET {
        message_cookie(headers)
    } else {
        None
    };
    Pending {
        flash: sent.and_then(Flash::from_code),
        clear: sent.is_some(),
    }
}

/// The `Set-Cookie` value that stores `flash`:
/// `nage_message=<value>; Path=/; HttpOnly; SameSite=Lax; Max-Age=60`, followed by `; Secure`
/// when `secure` is true, the value being the code with an account message's number
/// ([`Flash::value`]).
pub(crate) fn set(flash: Flash, secure: bool) -> HeaderValue {
    let secure_attribute = if secure { "; Secure" } else { "" };
    // A value is a fixed lowercase word with at most a dot and digits, so it is always a valid
    // header; should it ever not be, the browser is told to drop the message instead.
    HeaderValue::from_str(&format!(
        "{MESSAGE_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={MAX_AGE_SECONDS}{secure_attribute}",
        flash.value()
    ))
    .unwrap_or_else(|_| clear(secure))
}

/// The `Set-Cookie` value that clears the message's cookie: an empty value with `Max-Age=0`
/// and the same attributes, `Secure` included when `secure` is true.
pub(crate) fn clear(secure: bool) -> HeaderValue {
    if secure {
        HeaderValue::from_static("nage_message=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure")
    } else {
        HeaderValue::from_static("nage_message=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
    }
}

use axum::http::header;
use axum::response::Response;
use nage_core::accounts::{AccountId, Word};

use crate::MESSAGE_COOKIE;
use crate::accounts::well_formed_number;
use crate::messages::{Notice, Refusal};

impl Pending {
    /// The message the page shows: the pending one, where it is true (`Flash::shown`).
    /// `signed_in` says whether the page knows a signed-in user.
    pub(crate) fn message(self, signed_in: bool) -> Option<Message> {
        self.flash.and_then(|flash| flash.shown(signed_in))
    }

    /// The message the page of the account `id` shows: an account message, worded by `word`,
    /// only when it is about that account (`Flash::shown_on_account`); a message of the three
    /// older codes as every other page with a signed-in user shows it (`Flash::shown`), since
    /// only the four account codes are tied to an account's number (decision 137).
    pub(crate) fn account_message(self, id: AccountId, word: Word) -> Option<Message> {
        self.flash.and_then(|flash| {
            flash
                .shown_on_account(id, word)
                .or_else(|| flash.shown(true))
        })
    }

    /// `page`, the answer of a page that shows [`Pending::message`], with the message's
    /// cookie cleared when the request carried one ([`clear`]); a page that could not be
    /// rendered, a 500, leaves the cookie as it is.
    pub(crate) fn settle(self, mut page: Response, secure: bool) -> Response {
        if self.clear && !page.status().is_server_error() {
            page.headers_mut().append(header::SET_COOKIE, clear(secure));
        }
        page
    }
}

/// The value of the request's `nage_message` cookie, as sent: the first cookie of that name
/// in the `Cookie` headers, whatever other cookies stand beside it; `None` when there is
/// none. A `Cookie` header that holds a byte outside ASCII, in any cookie's name or value, is
/// not read at all: a message sent in it is neither shown nor cleared, and stays until it
/// expires.
fn message_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(MESSAGE_COOKIE)?.strip_prefix('='))
}
