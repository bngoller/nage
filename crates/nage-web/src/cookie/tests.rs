//! The cookie's text and the token, without a server (decision 108), and which `Set-Cookie`
//! sets the session cookie: the one for `nage_session`, found by its name (decision 117).

use axum::http::{HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};

use super::{clear, is_secure, new_token, read, session_cookie, set, sets_session, token_hash};

/// The headers with one `name: value`.
fn header(name: &'static str, value: &str) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert(
        name,
        HeaderValue::from_str(value)
            .unwrap_or_else(|error| panic!("{value:?} is no header value: {error}")),
    );
    map
}

/// A new token is 64 lowercase hexadecimal characters, different each time, and its hash is
/// the SHA-256 of the 32 bytes it writes, as `token_hash` computes it from the cookie's
/// value; a value of any other shape has no hash and is never looked up.
///
/// Proves: SPEC 5.3 (32 random bytes; only the SHA-256 of the token is stored), decision 108.
#[test]
fn a_token_is_32_random_bytes_in_hexadecimal_and_its_hash_their_sha_256() {
    let first = new_token().unwrap_or_else(|error| panic!("no token: {error:?}"));
    let second = new_token().unwrap_or_else(|error| panic!("no second token: {error:?}"));
    assert_ne!(first.token, second.token, "two tokens are the same");
    for new in [&first, &second] {
        assert!(
            new.token.len() == 64
                && new
                    .token
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the token is not 64 lowercase hexadecimal characters: {:?}",
            new.token
        );
        assert_eq!(
            token_hash(&new.token).as_ref(),
            Some(&new.hash),
            "the hash of the token as the cookie carries it"
        );
    }

    let token = "00".repeat(31) + "ff";
    let mut bytes = [0_u8; 32];
    bytes[31] = 0xff;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let mut hex = String::new();
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    assert_eq!(
        token_hash(&token).map(|hash| hash.as_str().to_owned()),
        Some(hex),
        "the hash of the token 00..00ff"
    );
    for malformed in [
        String::new(),
        String::from("abc"),
        "0".repeat(63),
        "0".repeat(65),
        "0".repeat(63) + "g",
        "A".repeat(64),
        " ".repeat(64),
    ] {
        assert_eq!(
            token_hash(&malformed),
            None,
            "the hash of the malformed token {malformed:?}"
        );
    }
}

/// The cookie is written as `nage_session=<token>; Path=/; HttpOnly; SameSite=Lax;
/// Max-Age=<seconds>`, with `; Secure` after it when asked; clearing it writes an empty
/// value and `Max-Age=0` with the same attributes. The request's cookie is read as the first
/// `nage_session` among the cookies sent.
///
/// Proves: SPEC 5.3 (cookie `nage_session`, `Path=/`, HttpOnly, SameSite=Lax, `Max-Age`),
/// decision 108.
#[test]
fn the_cookie_is_written_and_read_by_hand() {
    let token = "ab".repeat(32);
    assert_eq!(
        set(&token, 2_592_000, false),
        format!("nage_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000").as_str(),
        "the cookie without Secure"
    );
    assert_eq!(
        set(&token, 1, true),
        format!("nage_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1; Secure").as_str(),
        "the cookie with Secure"
    );
    assert_eq!(
        clear(false),
        "nage_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        "the clearing cookie without Secure"
    );
    assert_eq!(
        clear(true),
        "nage_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure",
        "the clearing cookie with Secure"
    );

    for (sent, value) in [
        (format!("nage_session={token}"), Some(token.clone())),
        (
            format!("theme=dark; nage_session={token}; other=1"),
            Some(token.clone()),
        ),
        (
            format!("nage_session={token};theme=dark"),
            Some(token.clone()),
        ),
        (
            format!("nage_session=first; nage_session={token}"),
            Some(String::from("first")),
        ),
        (String::from("nage_session="), Some(String::new())),
        (String::from("theme=dark; other=1"), None),
        (format!("xnage_session={token}"), None),
        (format!("nage_session_old={token}"), None),
    ] {
        assert_eq!(
            session_cookie(&header("cookie", &sent)),
            value,
            "the session cookie in {sent:?}"
        );
    }
    assert_eq!(
        session_cookie(&HeaderMap::new()),
        None,
        "the session cookie of a request without cookies"
    );
}

/// The one reader of the session, the one-time message and the appearance returns the first
/// cookie of exactly the name asked for across every `Cookie` header, as it stands; a header
/// holding a byte outside ASCII is read with that byte replaced and still yields the cookies
/// beside it; only spaces and tabs are dropped before a name, so a name after a no-break space
/// is another cookie's (a line feed cannot reach the reader: no header value carries one); and
/// the appearance reader's cases hold as before.
///
/// Proves: Decision 147: the first cookie of exactly that name across every Cookie header, a byte outside ASCII replaced and never hiding the others, only spaces and tabs dropped before a name, a value taken as it stands, the appearance reader's cases carried over.
#[test]
fn one_reader_reads_every_cookie_header_lossily() {
    /// The `Cookie` headers sent, the name asked for and the value expected.
    type Case = (&'static [&'static [u8]], &'static str, Option<&'static str>);
    let headers = |values: &[&[u8]]| {
        let mut map = HeaderMap::new();
        for value in values {
            map.append(
                axum::http::header::COOKIE,
                HeaderValue::from_bytes(value)
                    .unwrap_or_else(|error| panic!("{value:?} is no header value: {error}")),
            );
        }
        map
    };
    let cases: [Case; 22] = [
        (&[b"nage_session=abc"], "nage_session", Some("abc")),
        (
            &[b"other=1; nage_message=account-created.12; x=2"],
            "nage_message",
            Some("account-created.12"),
        ),
        (
            &[b"nage_theme=dark;nage_session=s"],
            "nage_session",
            Some("s"),
        ),
        (
            &[b"nage_theme=first; nage_theme=second"],
            "nage_theme",
            Some("first"),
        ),
        (&[b"a=1", b"nage_theme=light"], "nage_theme", Some("light")),
        (
            &[b"nage_theme=dark", b"nage_theme=light"],
            "nage_theme",
            Some("dark"),
        ),
        (
            &[b"other=caf\xc3\xa9; nage_session=tok"],
            "nage_session",
            Some("tok"),
        ),
        (
            &[b"other=caf\xe9; nage_message=signed-out"],
            "nage_message",
            Some("signed-out"),
        ),
        (
            &[b"other=caf\xe9", b"nage_theme=dark"],
            "nage_theme",
            Some("dark"),
        ),
        (&[b"\tnage_theme=dark"], "nage_theme", Some("dark")),
        (&[b"x=1; \t nage_theme=dark"], "nage_theme", Some("dark")),
        (&[b"x=1;\xc2\xa0nage_theme=dark"], "nage_theme", None),
        (&[b"nage_theme=light "], "nage_theme", Some("light ")),
        (&[b"nage_theme= dark"], "nage_theme", Some(" dark")),
        (&[b"nage_theme="], "nage_theme", Some("")),
        (&[b"nage_theme"], "nage_theme", None),
        (&[b"xnage_theme=dark"], "nage_theme", None),
        (&[b"nage_theme_old=dark"], "nage_theme", None),
        (&[b"nage_theme =dark"], "nage_theme", None),
        (&[b"NAGE_THEME=dark"], "nage_theme", None),
        (&[b"theme=dark; other=1"], "nage_theme", None),
        (&[], "nage_session", None),
    ];
    for (sent, name, expected) in cases {
        assert_eq!(
            read(&headers(sent), name).as_deref(),
            expected,
            "web: one cookie reader: the cookie {name} in {sent:?}"
        );
    }
}

/// A cookie is Secure exactly when trust-proxy is on and the first value of
/// `X-Forwarded-Proto` is `https`.
///
/// Proves: SPEC 5.3 (Secure if and only if the trust-proxy flag is on and the request
/// carries `X-Forwarded-Proto: https`), decision 108.
#[test]
fn a_cookie_is_secure_only_behind_a_trusted_https_proxy() {
    for (forwarded_proto, trust_proxy, secure) in [
        (None, false, false),
        (None, true, false),
        (Some("https"), false, false),
        (Some("https"), true, true),
        (Some("HTTPS"), true, true),
        (Some(" https "), true, true),
        (Some("https, http"), true, true),
        (Some("http"), true, false),
        (Some("http, https"), true, false),
        (Some("httpsx"), true, false),
        (Some(""), true, false),
    ] {
        let headers = match forwarded_proto {
            Some(value) => header("x-forwarded-proto", value),
            None => HeaderMap::new(),
        };
        assert_eq!(
            is_secure(&headers, trust_proxy),
            secure,
            "Secure with X-Forwarded-Proto {forwarded_proto:?} and trust-proxy {trust_proxy}"
        );
    }
}

/// An answer sets the session cookie itself only when one of its `Set-Cookie` headers is for
/// the cookie named `nage_session`, whether it stores a token or clears it and whatever
/// stands beside it. An answer without a `Set-Cookie`, one that only sets or clears the
/// one-time message's cookie, and one that sets a cookie whose name only resembles the
/// session's do not.
///
/// Proves: decision 117 (`cookie::sets_session`, which the session rule's two checks for an
/// answer that sets a cookie call, looks for `nage_session` by name), decision 108.
#[test]
fn only_a_set_cookie_for_the_session_cookie_sets_it() {
    let answer = |cookies: &[&str]| {
        let mut map = HeaderMap::new();
        for cookie in cookies {
            map.append(
                axum::http::header::SET_COOKIE,
                HeaderValue::from_str(cookie)
                    .unwrap_or_else(|error| panic!("{cookie:?} is no header value: {error}")),
            );
        }
        map
    };
    let token = "ab".repeat(32);
    let session = format!("nage_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000");
    let cleared = "nage_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    let message = "nage_message=signed-out; Path=/; HttpOnly; SameSite=Lax; Max-Age=60";
    let message_cleared = "nage_message=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    for (cookies, expected) in [
        (vec![], false),
        (vec![message], false),
        (vec![message_cleared], false),
        (vec!["nage_session2=x; Path=/"], false),
        (vec!["xnage_session=x; Path=/"], false),
        (vec![session.as_str()], true),
        (vec![cleared], true),
        (vec![message, session.as_str()], true),
        (vec![cleared, message], true),
        (vec!["nage_session =x; Path=/"], true),
    ] {
        assert_eq!(
            sets_session(&answer(&cookies)),
            expected,
            "whether an answer with the cookies {cookies:?} sets the session cookie"
        );
    }
}
