//! `next`, without a server (decision 109).

use axum::http::{Method, Uri};

use super::{MAX_NEXT_BYTES, login_location, safe_next};

/// A `next` is followed only when it starts with exactly one `/`, holds no backslash and no
/// control character and is at most 2 KiB long; anything else is `None`.
///
/// Proves: SPEC 5.3 (never an absolute URL), decision 109 (the test a `next` passes).
#[test]
fn a_next_is_safe_only_as_a_path_on_this_site() {
    assert_eq!(MAX_NEXT_BYTES, 2048, "the longest next in bytes");
    let longest = format!("/{}", "a".repeat(MAX_NEXT_BYTES - 1));
    for next in [
        "/",
        "/accounts",
        "/accounts?kind=asset&page=2",
        "/a/b/c?next=/d",
        "/caf\u{e9}",
        "/a%0Ab",
        longest.as_str(),
    ] {
        assert_eq!(
            safe_next(next),
            Some(next),
            "the next {:?}",
            &next[..next.len().min(40)]
        );
    }
    let too_long = format!("/{}", "a".repeat(MAX_NEXT_BYTES));
    for next in [
        "",
        "accounts",
        "//evil.example",
        "///evil.example",
        "https://evil.example/",
        "http:/evil.example",
        "/\\evil.example",
        "/a\\b",
        "/a\nb",
        "/a\tb",
        "/a\u{0}b",
        "/a\u{7f}b",
        " /accounts",
        too_long.as_str(),
    ] {
        assert_eq!(
            safe_next(next),
            None,
            "the next {:?}",
            &next[..next.len().min(40)]
        );
    }
}

/// A request without a session is sent to `/login`; a GET carries its path and query as
/// `next`, percent-encoded into one query value, except the start page, which carries none;
/// a request that is not a GET, and a GET whose address is not a safe `next`, carry none.
///
/// Proves: SPEC 5.3 (`/login?next=<relative path>`), decision 109.
#[test]
fn a_request_without_a_session_is_sent_to_the_sign_in_page_with_its_address() {
    let uri = |text: &str| -> Uri {
        text.parse()
            .unwrap_or_else(|error| panic!("{text:?} is no address: {error}"))
    };
    assert_eq!(login_location(&Method::GET, &uri("/")), "/login");
    assert_eq!(
        login_location(&Method::GET, &uri("/accounts")),
        "/login?next=%2Faccounts"
    );
    assert_eq!(
        login_location(&Method::GET, &uri("/accounts?kind=asset&page=2")),
        "/login?next=%2Faccounts%3Fkind%3Dasset%26page%3D2"
    );
    assert_eq!(
        login_location(&Method::GET, &uri("/a-b_c.d~e/caf%C3%A9")),
        "/login?next=%2Fa-b_c.d~e%2Fcaf%25C3%25A9"
    );
    assert_eq!(
        login_location(&Method::GET, &uri("//evil.example/accounts")),
        "/login"
    );
    for method in [Method::POST, Method::HEAD, Method::PUT, Method::DELETE] {
        assert_eq!(
            login_location(&method, &uri("/accounts")),
            "/login",
            "where a {method} is sent"
        );
    }
}
