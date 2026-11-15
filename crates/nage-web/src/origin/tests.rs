//! The Origin rule, without a server (decision 110).

use axum::http::{HeaderMap, HeaderName, HeaderValue};

use super::from_own_page;

/// The headers `pairs` name.
fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            HeaderName::from_bytes(name.as_bytes())
                .unwrap_or_else(|error| panic!("{name:?} is no header name: {error}")),
            HeaderValue::from_str(value)
                .unwrap_or_else(|error| panic!("{value:?} is no header value: {error}")),
        );
    }
    map
}

/// The host and port of `Origin`, or of `Referer` when there is no `Origin`, must be those of
/// `Host`, compared as written with the host ignoring ASCII case and the scheme ignored; with
/// trust-proxy on, the first value of `X-Forwarded-Host` counts as well, and without it that
/// header is ignored.
///
/// Proves: SPEC 5.3 (the Origin rule), decision 110 (compared as written; an absent port
/// matches only an absent port; `null` never matches; either `Host` or the first
/// `X-Forwarded-Host` value).
#[test]
fn a_request_comes_from_nages_own_page_when_its_origin_names_the_host() {
    let own: [(&[(&str, &str)], bool); 9] = [
        (
            &[("host", "nage.test"), ("origin", "http://nage.test")],
            false,
        ),
        (
            &[("host", "nage.test"), ("origin", "https://NAGE.test")],
            false,
        ),
        (
            &[
                ("host", "nage.test:8787"),
                ("origin", "http://nage.test:8787"),
            ],
            false,
        ),
        (
            &[("host", "[::1]:8787"), ("origin", "http://[::1]:8787")],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("referer", "https://nage.test/a/b?c=d#e"),
            ],
            false,
        ),
        (
            &[("host", "nage.test"), ("referer", "http://nage.test")],
            false,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "nage.example"),
                ("origin", "https://nage.example"),
            ],
            true,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "nage.example , proxy.internal"),
                ("origin", "https://nage.example"),
            ],
            true,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "nage.example"),
                ("origin", "http://127.0.0.1:8787"),
            ],
            true,
        ),
    ];
    for (pairs, trust_proxy) in own {
        assert!(
            from_own_page(&headers(pairs), trust_proxy),
            "{pairs:?} with trust-proxy {trust_proxy} is not taken for Nage's own page"
        );
    }

    let foreign: [(&[(&str, &str)], bool); 15] = [
        (&[("host", "nage.test"), ("origin", "://nage.test")], false),
        (&[("host", "nage.test")], false),
        (&[("host", "nage.test"), ("origin", "null")], false),
        (
            &[("host", "nage.test"), ("origin", "http://evil.example")],
            false,
        ),
        (
            &[("host", "nage.test"), ("origin", "http://nage.test:80")],
            false,
        ),
        (
            &[("host", "nage.test:80"), ("origin", "http://nage.test")],
            false,
        ),
        (
            &[
                ("host", "nage.test:8787"),
                ("origin", "http://nage.test:8788"),
            ],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("origin", "http://nage.test.evil.example"),
            ],
            false,
        ),
        (&[("host", "nage.test"), ("origin", "nage.test")], false),
        (&[("host", "nage.test"), ("origin", "http://")], false),
        (
            &[("host", "nage.test"), ("origin", "http://user@nage.test")],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("referer", "http://evil.example/nage.test"),
            ],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("origin", "http://evil.example"),
                ("referer", "http://nage.test/"),
            ],
            false,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "nage.example"),
                ("origin", "https://nage.example"),
            ],
            false,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "proxy.internal, nage.example"),
                ("origin", "https://nage.example"),
            ],
            true,
        ),
    ];
    for (pairs, trust_proxy) in foreign {
        assert!(
            !from_own_page(&headers(pairs), trust_proxy),
            "{pairs:?} with trust-proxy {trust_proxy} is taken for Nage's own page"
        );
    }
}
