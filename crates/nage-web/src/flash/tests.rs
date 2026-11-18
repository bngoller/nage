//! The one-time message's codes, its cookie's text and what a page does about it, without a
//! server (decision 117).

use axum::http::{HeaderMap, HeaderValue, Method, header};

use super::{Flash, MAX_AGE_SECONDS, Pending, clear, pending, set};
use crate::MESSAGE_COOKIE;
use crate::messages::{Notice, Refusal};
use crate::pages::Message;

/// The headers of a request that sends `cookies` in one `Cookie` header.
fn cookies(cookies: &str) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert(
        header::COOKIE,
        HeaderValue::from_str(cookies)
            .unwrap_or_else(|error| panic!("{cookies:?} is no header value: {error}")),
    );
    map
}

/// The cookie holds one of three fixed codes, one per message, and only the exact code names
/// its message: another case of letters, white space around it, a code with something after
/// it, the empty text and a sentence name none.
///
/// Proves: decision 117 (the cookie holds a short code from a fixed set, never text; a code
/// Nage does not know is ignored).
#[test]
fn each_message_has_one_fixed_code_and_only_that_code_names_it() {
    let codes: Vec<&str> = Flash::ALL.into_iter().map(Flash::code).collect();
    assert_eq!(
        codes,
        [
            "password-changed",
            "signed-out",
            "password-changed-elsewhere"
        ],
        "the codes of the one-time messages"
    );
    for flash in Flash::ALL {
        assert_eq!(
            Flash::from_code(flash.code()),
            Some(flash),
            "the message of the code {:?}",
            flash.code()
        );
    }
    for other in [
        "",
        "Signed-Out",
        "SIGNED-OUT",
        " signed-out",
        "signed-out ",
        "signed-out;",
        "signed-out-elsewhere",
        "password-changed-",
        "signed_out",
        "0",
        "You are signed out.",
        "Your password was changed.",
    ] {
        assert_eq!(
            Flash::from_code(other),
            None,
            "the message of the text {other:?}"
        );
    }
}

/// The cookie that stores a message is `nage_message=<code>` with `Path=/`, `HttpOnly`,
/// `SameSite=Lax` and a `Max-Age` of 60 seconds, and the one that clears it is the same with
/// an empty value and `Max-Age=0`; each carries `Secure` exactly when asked to.
///
/// Proves: decision 117 (the cookie's name and attributes: `Path=/`, `HttpOnly`,
/// `SameSite=Lax`, `Secure` when it is asked for, and a short lifetime).
#[test]
fn the_message_cookie_is_written_with_its_attributes() {
    assert_eq!(MESSAGE_COOKIE, "nage_message", "the cookie's name");
    assert_eq!(MAX_AGE_SECONDS, 60, "the cookie's lifetime in seconds");
    assert_eq!(
        set(Flash::SignedOut, false),
        "nage_message=signed-out; Path=/; HttpOnly; SameSite=Lax; Max-Age=60",
        "the cookie of a sign-out"
    );
    assert_eq!(
        set(Flash::PasswordChanged, true),
        "nage_message=password-changed; Path=/; HttpOnly; SameSite=Lax; Max-Age=60; Secure",
        "the cookie of a change behind a trusted HTTPS proxy"
    );
    assert_eq!(
        set(Flash::PasswordChangedElsewhere, false),
        "nage_message=password-changed-elsewhere; Path=/; HttpOnly; SameSite=Lax; Max-Age=60",
        "the cookie of an overtaken change"
    );
    assert_eq!(
        clear(false),
        "nage_message=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        "the cookie that clears the message"
    );
    assert_eq!(
        clear(true),
        "nage_message=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure",
        "the cookie that clears the message behind a trusted HTTPS proxy"
    );
}

/// On a GET a page shows the message whose code the request's `nage_message` cookie holds and
/// clears the cookie, whatever other cookies stand beside it and whichever comes first, in one
/// `Cookie` header or in the second of two; a value that is no code is shown as nothing and
/// cleared all the same; without the cookie nothing is shown and nothing cleared, and a cookie
/// whose name only resembles the message's is not it. Of two cookies of that name the first
/// counts. On a HEAD and on a POST nothing is shown and nothing cleared, whatever the cookie
/// holds.
///
/// Proves: decision 117 (a page that reads the message shows it on a GET and clears the
/// cookie; a HEAD reads nothing; a code Nage does not know is ignored and cleared).
#[test]
fn a_get_shows_and_clears_the_message_and_no_other_method_reads_it() {
    let shown = |flash| Pending {
        flash: Some(flash),
        clear: true,
    };
    let only_cleared = Pending {
        flash: None,
        clear: true,
    };
    let untouched = Pending {
        flash: None,
        clear: false,
    };
    for (sent, expected) in [
        ("nage_message=signed-out", shown(Flash::SignedOut)),
        (
            "nage_session=abc; nage_message=password-changed",
            shown(Flash::PasswordChanged),
        ),
        (
            "nage_message=password-changed-elsewhere; nage_session=abc",
            shown(Flash::PasswordChangedElsewhere),
        ),
        (
            "nage_message=signed-out; nage_message=password-changed",
            shown(Flash::SignedOut),
        ),
        ("nage_message=", only_cleared),
        ("nage_message=nothing-nage-knows", only_cleared),
        ("nage_session=abc; nage_message=Signed-Out", only_cleared),
        ("nage_session=abc", untouched),
        ("xnage_message=signed-out", untouched),
        ("nage_message2=signed-out", untouched),
    ] {
        assert_eq!(
            pending(&Method::GET, &cookies(sent)),
            expected,
            "what a GET does with the cookies {sent:?}"
        );
    }
    let mut two = HeaderMap::new();
    for value in ["nage_session=abc", "nage_message=signed-out"] {
        two.append(header::COOKIE, HeaderValue::from_static(value));
    }
    assert_eq!(
        pending(&Method::GET, &two),
        shown(Flash::SignedOut),
        "what a GET does with the message in the second of two Cookie headers"
    );
    assert_eq!(
        pending(&Method::GET, &HeaderMap::new()),
        untouched,
        "what a GET without a Cookie header does"
    );
    for method in [Method::HEAD, Method::POST] {
        assert_eq!(
            pending(&method, &cookies("nage_message=signed-out")),
            untouched,
            "what a {method} does with a message"
        );
    }
}

/// Each message is shown in its own words, and only where it is true: the confirmation of a
/// change as a status on a page with a signed-in user and on no other; that of a sign-out as a
/// status on a page without one and on no other; the overtaken change as an alert on either,
/// in the words for a page without a signed-in user and in those for a page with one.
///
/// Proves: decision 117 (what each one-time message says; a message shows only where it is
/// true, the sign-out's on a page without a signed-in user and the change's on a page with
/// one; the overtaken change's in the words for a page without a signed-in user and in those
/// for a person still signed in), decision 112.
#[test]
fn each_message_is_shown_in_its_own_words() {
    let status = |text: String| Some(Message { text, alert: false });
    let alert = |text: String| Some(Message { text, alert: true });
    assert_eq!(
        Flash::PasswordChanged.shown(true),
        status(Notice::PasswordChanged.to_string()),
        "the confirmation of a change on a page with a signed-in user"
    );
    assert_eq!(
        Flash::PasswordChanged.shown(false),
        None,
        "the confirmation of a change on a page without a signed-in user"
    );
    assert_eq!(
        Flash::SignedOut.shown(false),
        status(Notice::SignedOut.to_string()),
        "the confirmation of a sign-out on a page without a signed-in user"
    );
    assert_eq!(
        Flash::SignedOut.shown(true),
        None,
        "the confirmation of a sign-out on a page with a signed-in user"
    );
    assert_eq!(
        Flash::PasswordChangedElsewhere.shown(false),
        alert(Refusal::PasswordChangedElsewhere.to_string()),
        "the overtaken change on the sign-in page"
    );
    assert_eq!(
        Flash::PasswordChangedElsewhere.shown(true),
        alert(Refusal::PasswordChangedElsewhereSignedIn.to_string()),
        "the overtaken change on a page of a person still signed in"
    );
}
