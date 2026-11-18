//! One-time messages (SPEC 5.2, 5.3, INV-28, decision 117): a message that appears at the
//! top of the next page, once. It travels in a second cookie beside `nage_session`, named
//! [`crate::MESSAGE_COOKIE`], that holds a short code from a fixed set, never text, so whoever
//! can set cookies in that browser can only make Nage's own fixed messages appear, never other
//! words; a cookie set for another path or for a parent domain is not removed by Nage's
//! clearing and shows its message until it expires.
//!
//! - **Set** on three answers only: the 303 of an accepted change of one's own password, the
//!   303 of a sign-out, and the 303 that sends an overtaken change to the sign-in page. A form
//!   refused for what was typed, a busy answer and a failure leave no message. The messages
//!   share the one cookie, so a message set while another is still waiting replaces it.
//! - **Shown and cleared** by the next page a GET renders: the home page, the change-password
//!   page, the page for an unknown address, the sign-in page, the set-up page and its closed
//!   page. A message shows only on a page where it is true (`Flash::shown`): the sign-out's on
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

/// The messages that travel as one-time messages (decision 117).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flash {
    /// One's own password was changed (`Notice::PasswordChanged`, decision 112).
    PasswordChanged,
    /// The person signed out (`Notice::SignedOut`, decision 106).
    SignedOut,
    /// A change of one's own password was overtaken by a reset or another change
    /// (`Refusal::PasswordChangedElsewhere`, decision 112).
    PasswordChangedElsewhere,
}

impl Flash {
    /// Every one-time message.
    pub(crate) const ALL: [Flash; 3] = [
        Flash::PasswordChanged,
        Flash::SignedOut,
        Flash::PasswordChangedElsewhere,
    ];

    /// The fixed code the cookie holds for this message: `password-changed`, `signed-out`
    /// or `password-changed-elsewhere`.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Flash::PasswordChanged => "password-changed",
            Flash::SignedOut => "signed-out",
            Flash::PasswordChangedElsewhere => "password-changed-elsewhere",
        }
    }

    /// The message whose code is exactly `code`; `None` for any other text, which a page
    /// shows nothing for.
    pub(crate) fn from_code(code: &str) -> Option<Flash> {
        Flash::ALL.into_iter().find(|flash| flash.code() == code)
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
/// `nage_message=<code>; Path=/; HttpOnly; SameSite=Lax; Max-Age=60`, followed by `; Secure`
/// when `secure` is true.
pub(crate) fn set(flash: Flash, secure: bool) -> HeaderValue {
    let secure_attribute = if secure { "; Secure" } else { "" };
    // A code is a fixed lowercase word, so the value is always a valid header; should it ever
    // not be, the browser is told to drop the message instead.
    HeaderValue::from_str(&format!(
        "{MESSAGE_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={MAX_AGE_SECONDS}{secure_attribute}",
        flash.code()
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

use crate::MESSAGE_COOKIE;
use crate::messages::{Notice, Refusal};

impl Pending {
    /// The message the page shows: the pending one, where it is true (`Flash::shown`).
    /// `signed_in` says whether the page knows a signed-in user.
    pub(crate) fn message(self, signed_in: bool) -> Option<Message> {
        self.flash.and_then(|flash| flash.shown(signed_in))
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
