//! The Origin check (SPEC 5.3, decision 110): which mutating requests are let through, the
//! 403 page of the others, and that the check runs before the session rule.

mod common;

use axum::http::StatusCode;
use nage_web::messages::Refusal;

use common::app::{ADMIN, App, HOST, Options, PASSWORD, Reply, Req, WRONG_PASSWORD};

/// A sign-in form with a wrong password for a username of its own (so that no pause builds
/// up over a test): let through, it is answered 422 by the sign-in page; refused by the
/// Origin check, it is answered 403.
fn attempt(number: usize) -> Req {
    let username = format!("visitor{number}");
    Req::form(
        "/login",
        &[
            ("username", username.as_str()),
            ("password", WRONG_PASSWORD),
        ],
    )
}

/// Asserts that `reply` is the Origin check's 403 page.
fn assert_forbidden(reply: &Reply, what: &str) {
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "status of {what}");
    assert!(
        reply.shows(&Refusal::CrossSite.to_string()),
        "{what} does not show the 403 page:\n{}",
        reply.body
    );
    assert!(
        reply.body.contains("<title>Nage</title>"),
        "the 403 page of {what} is not on the base layout:\n{}",
        reply.body
    );
    assert!(
        reply.set_cookies().is_empty(),
        "{what} sets a cookie: {:?}",
        reply.set_cookies()
    );
}

/// Asserts that `reply` got past the Origin check and reached the sign-in page.
fn assert_let_through(reply: &Reply, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of {what}:\n{}",
        reply.body
    );
}

/// A form whose `Origin` names the request's `Host` is let through: the same host and port,
/// the host in other capitals, and any scheme, since the scheme is ignored. Without an
/// `Origin`, a `Referer` that names the host is enough, whatever path it carries.
///
/// Proves: SPEC 5.3 (`Origin`, or `Referer` when `Origin` is absent, whose host and port
/// equal the request's `Host`; the scheme is ignored), decision 110 (the host ignoring ASCII
/// case).
#[tokio::test]
async fn a_form_from_nages_own_page_is_let_through() {
    let app = App::with_book().await;
    let cases = [
        (HOST, Some("http://nage.test"), None, "the same host"),
        (HOST, Some("https://nage.test"), None, "another scheme"),
        (
            HOST,
            Some("http://NAGE.Test"),
            None,
            "the host in other capitals",
        ),
        (
            "nage.test:8787",
            Some("http://nage.test:8787"),
            None,
            "the same host and port",
        ),
        (
            "[::1]:8787",
            Some("http://[::1]:8787"),
            None,
            "an IPv6 host and port",
        ),
        (
            HOST,
            None,
            Some("http://nage.test/login?next=%2F"),
            "a Referer in place of the Origin",
        ),
        (
            HOST,
            Some("http://nage.test"),
            Some("http://elsewhere.example/page"),
            "an Origin that matches beside a Referer that does not",
        ),
    ];
    for (number, (host, origin, referer, what)) in cases.into_iter().enumerate() {
        let mut request = attempt(number).header("host", host).without("origin");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(referer) = referer {
            request = request.header("referer", referer);
        }
        let reply = app.client().send(request).await;
        assert_let_through(&reply, &format!("a form from {what}"));
    }
}

/// A form that does not say it comes from Nage's own page gets 403 with the page that says
/// what was refused, why and what to do: another host, the same host on another port, a
/// port where the `Host` has none and none where it has one, a host that only begins or ends
/// like Nage's, `Origin: null`, an `Origin` without a host, no `Origin` and no `Referer`, a
/// `Referer` from elsewhere, and an `Origin` from elsewhere beside a `Referer` that matches,
/// since the `Referer` counts only when there is no `Origin`.
///
/// Proves: SPEC 5.3 (a missing or mismatched header gets 403), decision 110 (compared as
/// written; an absent port matches only an absent port; `Origin: null` never matches; the
/// 403 page), decision 102.
#[tokio::test]
async fn a_form_from_elsewhere_gets_the_403_page() {
    let app = App::with_book().await;
    let cases = [
        (HOST, Some("http://evil.example"), None, "another host"),
        (
            "nage.test:8787",
            Some("http://nage.test:9999"),
            None,
            "another port",
        ),
        (
            HOST,
            Some("http://nage.test:80"),
            None,
            "a port where the Host has none",
        ),
        (
            "nage.test:80",
            Some("http://nage.test"),
            None,
            "no port where the Host has one",
        ),
        (
            HOST,
            Some("http://nage.test.evil.example"),
            None,
            "a host that begins like Nage's",
        ),
        (
            HOST,
            Some("http://evil-nage.test"),
            None,
            "a host that ends like Nage's",
        ),
        (HOST, Some("null"), None, "Origin: null"),
        (HOST, Some("http://"), None, "an Origin without a host"),
        (HOST, Some("nage.test"), None, "an Origin without a scheme"),
        (HOST, None, None, "neither an Origin nor a Referer"),
        (
            HOST,
            None,
            Some("http://evil.example/nage.test"),
            "a Referer from elsewhere",
        ),
        (
            HOST,
            Some("http://evil.example"),
            Some("http://nage.test/login"),
            "an Origin from elsewhere beside a Referer that matches",
        ),
    ];
    for (number, (host, origin, referer, what)) in cases.into_iter().enumerate() {
        let mut request = attempt(number).header("host", host).without("origin");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(referer) = referer {
            request = request.header("referer", referer);
        }
        let reply = app.client().send(request).await;
        assert_forbidden(&reply, &format!("a form with {what}"));
    }
    assert_eq!(
        app.work.verifications(),
        0,
        "passwords verified for forms the Origin check refused"
    );
}

/// `X-Forwarded-Host` counts only with trust-proxy on. With it on, a form whose `Origin`
/// names the first value of `X-Forwarded-Host` is let through even though the `Host` the
/// proxy sent is another, and so is one that names the `Host`, as a direct request on the
/// LAN does; a later value of `X-Forwarded-Host` does not count. With trust-proxy off the
/// header is ignored.
///
/// Proves: SPEC 5.3 (with trust-proxy on, its `X-Forwarded-Host`), decision 110 (either of
/// the two matching; only the first value).
#[tokio::test]
async fn x_forwarded_host_counts_only_with_trust_proxy() {
    let forwarded = |origin: &str, forwarded_host: &str| {
        attempt(origin.len() + forwarded_host.len())
            .header("host", "127.0.0.1:8787")
            .header("origin", origin)
            .header("x-forwarded-host", forwarded_host)
    };

    let trusting = App::start_with(Options {
        trust_proxy: true,
        ..Options::default()
    })
    .await;
    trusting.set_up().await;
    for (origin, forwarded_host, what) in [
        ("https://nage.example", "nage.example", "the forwarded host"),
        (
            "https://nage.example",
            "nage.example, proxy.internal",
            "the first of two forwarded hosts",
        ),
        (
            "http://127.0.0.1:8787",
            "nage.example",
            "the Host beside a forwarded host",
        ),
    ] {
        let reply = trusting
            .client()
            .send(forwarded(origin, forwarded_host))
            .await;
        assert_let_through(&reply, &format!("a form that names {what}, trust-proxy on"));
    }
    let direct = trusting
        .client()
        .send(
            attempt(0)
                .header("host", "203.0.113.20:8787")
                .header("origin", "http://203.0.113.20:8787"),
        )
        .await;
    assert_let_through(
        &direct,
        "a direct form without X-Forwarded-Host, trust-proxy on",
    );
    let second = trusting
        .client()
        .send(forwarded(
            "https://nage.example",
            "proxy.internal, nage.example",
        ))
        .await;
    assert_forbidden(
        &second,
        "a form that names the second forwarded host, trust-proxy on",
    );

    let plain = App::with_book().await;
    let reply = plain
        .client()
        .send(forwarded("https://nage.example", "nage.example"))
        .await;
    assert_forbidden(
        &reply,
        "a form that names the forwarded host, trust-proxy off",
    );
}

/// The Origin check runs before the session rule and covers every mutating method and every
/// address, `/setup` included: a cross-site POST, PUT, PATCH or DELETE to a page
/// behind the sign-in gets the 403 page, not the 303 to the sign-in page; a cross-site
/// set-up creates nothing; a cross-site sign-out keeps the session. A GET or HEAD needs no
/// `Origin` and is not refused for a foreign one.
///
/// Proves: decision 110 (the check runs before the session rule, covers `/setup`, and a
/// cross-site post to a page behind the sign-in gets the 403), SPEC 5.3
/// (mutating requests: POST, PUT, PATCH, DELETE).
#[tokio::test]
async fn the_origin_check_covers_every_mutating_request_before_the_session_rule() {
    let empty = App::start().await;
    let reply = empty
        .client()
        .send(
            Req::form(
                "/setup",
                &[
                    ("book_name", "Household"),
                    ("currency", "CAD"),
                    ("username", ADMIN),
                    ("display_name", "Sam"),
                    ("password", PASSWORD),
                    ("password_again", PASSWORD),
                ],
            )
            .header("origin", "http://evil.example"),
        )
        .await;
    assert_forbidden(&reply, "a cross-site set-up");
    assert_eq!(empty.users().await, 0, "users after a cross-site set-up");

    let app = App::with_book().await;
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        for uri in ["/account/password", "/no-such-page", "/health"] {
            let reply = app
                .client()
                .send(Req::new(method, uri).header("origin", "http://evil.example"))
                .await;
            assert_forbidden(&reply, &format!("a cross-site {method} {uri}"));
        }
    }

    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reply = client
        .send(Req::form("/logout", &[]).header("origin", "http://evil.example"))
        .await;
    assert_forbidden(&reply, "a cross-site sign-out");
    assert_eq!(
        app.session_rows().await,
        1,
        "sessions after a cross-site sign-out"
    );
    assert_eq!(
        client.get("/").await.status,
        StatusCode::OK,
        "GET / after a cross-site sign-out"
    );

    for method in ["GET", "HEAD"] {
        let reply = client
            .send(Req::new(method, "/").header("origin", "http://evil.example"))
            .await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "{method} / with a foreign Origin"
        );
    }
}
