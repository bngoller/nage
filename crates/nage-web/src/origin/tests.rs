//! The Origin rule, without a server (decision 110), and the layer that applies it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::response::Response;
use axum::routing::any;
use nage_core::test_support::{CHEAP_HASH_PARAMS, open_fresh};
use nage_core::timestamp::Timestamp;
use tower::ServiceExt as _;

use super::{cross_site, from_own_page, origin_check, passes_without_check};
use crate::{AppState, Clock, Ctx, Hashing, HealthProbe};

/// The method whose name is `name`, as a request line writes it.
fn method(name: &str) -> Method {
    Method::from_bytes(name.as_bytes())
        .unwrap_or_else(|error| panic!("{name:?} is no method: {error}"))
}

/// Only GET, HEAD, OPTIONS and TRACE pass without the Origin check; every mutating method,
/// CONNECT, QUERY (which the `http` crate counts as safe) and every extension method need it,
/// a method's name being compared exactly, so `get` in lower case is an extension method.
///
/// Proves: decision 110 as amended (QUERY, an extension method and every mutating method need the check), inherited gap s5-authors-row110
#[test]
fn only_get_head_options_and_trace_pass_without_the_origin_check() {
    for name in ["GET", "HEAD", "OPTIONS", "TRACE"] {
        assert!(
            passes_without_check(&method(name)),
            "{name} does not pass without the Origin check"
        );
    }
    for name in [
        "POST", "PUT", "PATCH", "DELETE", "CONNECT", "QUERY", "PROPFIND", "MKCOL", "LINK", "get",
        "Query", "FOO",
    ] {
        assert!(
            !passes_without_check(&method(name)),
            "{name} passes without the Origin check"
        );
    }
}

/// A router whose two routes, `/setup` and every address under `/static/`, sit behind the Origin
/// check alone and record in the returned flag whether one ran.
async fn guarded() -> (Router, Arc<AtomicBool>, nage_core::test_support::TestDir) {
    let (dir, storage) = open_fresh().await;
    let ctx = Ctx::new(AppState {
        version: env!("CARGO_PKG_VERSION"),
        health: HealthProbe::always(true),
        storage,
        clock: Clock::new(|| Timestamp::from_second(1_800_000_000)),
        hashing: Hashing::argon2id(CHEAP_HASH_PARAMS),
        trust_proxy: false,
        zone: jiff::tz::TimeZone::UTC,
    });
    let reached = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&reached);
    let static_flag = Arc::clone(&reached);
    let router = Router::new()
        .route(
            "/setup",
            any(move || async move {
                flag.store(true, Ordering::SeqCst);
                StatusCode::OK
            }),
        )
        .route(
            "/static/{*rest}",
            any(move || async move {
                static_flag.store(true, Ordering::SeqCst);
                StatusCode::OK
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            origin_check,
        ))
        .with_state(ctx);
    (router, reached, dir)
}

/// Sends a `name` request for `/setup` on host `nage.test` with these extra headers.
async fn send(router: &Router, name: &str, pairs: &[(&str, &str)]) -> Response {
    send_to(router, name, "/setup", pairs).await
}

/// Sends a `name` request for `uri` on host `nage.test` with these extra headers.
async fn send_to(router: &Router, name: &str, uri: &str, pairs: &[(&str, &str)]) -> Response {
    let mut builder = Request::builder()
        .method(method(name))
        .uri(uri)
        .header("host", "nage.test");
    for (header, value) in pairs {
        builder = builder.header(*header, *value);
    }
    let request = builder
        .body(Body::from("book_name=Theirs"))
        .unwrap_or_else(|error| panic!("cannot build {name} {uri}: {error}"));
    match router.clone().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// The whole body of `response`.
async fn body(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|error| panic!("cannot read the response body: {error}"))
        .to_vec()
}

/// A QUERY request from another site (an `X-Forwarded-Host` naming that site among them, since
/// trust-proxy is off), or naming no page at all, is refused with 403 and the
/// page that says what was refused, headed "This form was not accepted", and the route behind the
/// layer never runs, an address under `/static/` included (for a POST too); the same request from
/// Nage's own page reaches it, and GET, HEAD, OPTIONS and TRACE reach it without an Origin and
/// with a foreign one.
///
/// Proves: decision 110 as amended, SPEC 5.3 (a cross-site QUERY gets 403 and nothing behind the layer runs)
#[tokio::test]
async fn a_cross_site_query_request_is_refused_with_403() {
    let (router, reached, _dir) = guarded().await;
    let refused_page = body(cross_site()).await;
    for pairs in [
        &[("origin", "http://evil.example")][..],
        &[("referer", "http://evil.example/form")][..],
        &[("origin", "null")][..],
        &[][..],
        &[
            ("x-forwarded-host", "evil.example"),
            ("origin", "http://evil.example"),
        ][..],
    ] {
        let response = send(&router, "QUERY", pairs).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "QUERY with {pairs:?}"
        );
        assert_eq!(
            body(response).await,
            refused_page,
            "the page of QUERY with {pairs:?}"
        );
        assert!(
            !reached.load(Ordering::SeqCst),
            "the route behind the Origin check ran for QUERY with {pairs:?}"
        );
    }

    // The page says what was refused in its own words: its heading is the cross-site page's.
    let text = String::from_utf8_lossy(&refused_page);
    assert!(
        text.contains("This form was not accepted"),
        "the 403 page does not carry its heading:\n{text}"
    );

    // An address under /static/ is covered too: the session rule's exemption is not the check's.
    for name in ["QUERY", "POST"] {
        let response = send_to(
            &router,
            name,
            "/static/app.css",
            &[("origin", "http://evil.example")],
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "a cross-site {name} to /static/app.css"
        );
        assert!(
            !reached.load(Ordering::SeqCst),
            "the route under /static/ ran for a cross-site {name}"
        );
    }

    let response = send(&router, "QUERY", &[("origin", "http://nage.test")]).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "QUERY from Nage's own page"
    );
    assert!(
        reached.load(Ordering::SeqCst),
        "QUERY from Nage's own page did not reach the route"
    );

    // GET, HEAD, OPTIONS and TRACE pass the layer without an Origin and with a foreign one.
    for name in ["GET", "HEAD", "OPTIONS", "TRACE"] {
        for pairs in [&[][..], &[("origin", "http://evil.example")][..]] {
            reached.store(false, Ordering::SeqCst);
            let response = send(&router, name, pairs).await;
            assert_eq!(response.status(), StatusCode::OK, "{name} with {pairs:?}");
            assert!(
                reached.load(Ordering::SeqCst),
                "{name} with {pairs:?} did not reach the route"
            );
        }
    }
}

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
/// header is ignored. A request without `Host` never matches, a `Referer` beside any `Origin`
/// (`null` too, or one that is not plain text) never counts, and the host of a URL is read after
/// its first `://`, behind a scheme as RFC 3986 writes one, and ends at its path, query or
/// fragment: a value without a scheme whose query holds `://nage.test`, and a URL of another site
/// whose path or query holds it, are refused, while Nage's own page whose query holds it is not.
/// With trust-proxy on, a request without `X-Forwarded-Host` still needs its `Host` named, only
/// the first `X-Forwarded-Host` line counts, and an empty host part never matches an empty `Host`
/// or an empty forwarded host.
///
/// Proves: SPEC 5.3 (the Origin rule), decision 110 (compared as written; an absent port
/// matches only an absent port; `null` never matches; either `Host` or the first
/// `X-Forwarded-Host` value).
#[test]
fn a_request_comes_from_nages_own_page_when_its_origin_names_the_host() {
    let own: [(&[(&str, &str)], bool); 12] = [
        (
            &[
                ("host", "nage.test"),
                ("referer", "https://nage.test/?u=https://nage.test"),
            ],
            false,
        ),
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
            &[("host", "nage.test"), ("referer", "https://nage.test?c=d")],
            false,
        ),
        (
            &[("host", "nage.test"), ("referer", "https://nage.test#e")],
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

    let foreign: [(&[(&str, &str)], bool); 25] = [
        (
            &[
                ("host", "nage.test"),
                ("origin", "evil.example/?next=http://nage.test"),
            ],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("referer", "http://evil.example/?u=http://nage.test"),
            ],
            false,
        ),
        (
            &[
                ("host", "nage.test"),
                ("origin", "http://evil.example/?u=http://nage.test"),
            ],
            false,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("origin", "https://evil.example"),
            ],
            true,
        ),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", "proxy.internal"),
                ("x-forwarded-host", "nage.example"),
                ("origin", "https://nage.example"),
            ],
            true,
        ),
        (&[("host", ""), ("origin", "http://")], false),
        (
            &[
                ("host", "127.0.0.1:8787"),
                ("x-forwarded-host", ""),
                ("origin", "http://"),
            ],
            true,
        ),
        (
            &[("host", "nage.test"), ("origin", "1http://nage.test")],
            false,
        ),
        (&[("host", "nage.test"), ("origin", "://nage.test")], false),
        (&[("host", "nage.test")], false),
        (&[("origin", "http://nage.test")], false),
        (
            &[
                ("host", "nage.test"),
                ("origin", "null"),
                ("referer", "http://nage.test/"),
            ],
            false,
        ),
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

    // An Origin that is not plain text (a byte outside ASCII) is still an Origin: the Referer
    // beside it, which names the host, does not count.
    let mut unreadable = headers(&[("host", "nage.test"), ("referer", "http://nage.test/")]);
    unreadable.insert(
        HeaderName::from_static("origin"),
        HeaderValue::from_bytes(b"http://nag\xC3\xA9.test")
            .unwrap_or_else(|error| panic!("the unreadable Origin: {error}")),
    );
    assert!(
        !from_own_page(&unreadable, false),
        "an Origin that is not plain text let the Referer beside it decide"
    );
}
