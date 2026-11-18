//! The response headers of SPEC 5.3 on every kind of response, the answers of the rules of
//! milestone 2 slice 5 included (decision 110: the security headers are the second layer,
//! outside the body limit, the Origin check and the session rule), and from milestone 2 slice
//! 6 the `Cache-Control: no-store` of every answer but a served asset (decision 119).

mod common;

use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use nage_core::storage::StorageOptions;
use nage_web::MAX_FORM_BYTES;

use common::app::{self, ADMIN, App, Options, PASSWORD, Req, WRONG_PASSWORD};
use common::{IMMUTABLE, assert_security_headers, asset, get, hashed_url, state};

/// `GET /` with a session carries the four SPEC 5.3 headers, `Cache-Control: no-store` among
/// them, with their exact values.
///
/// Proves: SPEC 5.3, decision 40, decision 119.
#[tokio::test]
async fn security_headers_on_index() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reply = client.get("/").await;
    app::assert_security_headers(&reply, "/");
    assert_eq!(reply.status, StatusCode::OK, "GET / status");
}

/// `GET /health` carries the three headers of decision 40, on the 200 and on the 503 answer.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_health() {
    let response = get(state(true), "/health").await;
    assert_security_headers(&response, "/health");
    assert_eq!(response.status(), StatusCode::OK, "GET /health status");

    let failing = get(state(false), "/health").await;
    assert_security_headers(&failing, "/health with a failing probe");
    assert_eq!(
        failing.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status with a failing probe"
    );
}

/// A content-hashed static asset carries the three headers of decision 40.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_static_asset() {
    let url = hashed_url("app.css", asset("app.css").body);
    let response = get(state(true), &url).await;
    assert_security_headers(&response, &url);
    assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
}

/// A 404 carries the SPEC 5.3 headers: the three of decision 40 from the static route, which
/// needs no session, and all four, `Cache-Control: no-store` among them, from the fallback,
/// which a signed-in user reaches at an unknown address.
///
/// Proves: SPEC 5.3, decision 40, decision 119.
#[tokio::test]
async fn security_headers_on_not_found() {
    let uri = "/static/app.css";
    let response = get(state(true), uri).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
    assert_security_headers(&response, uri);

    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reply = client.get("/no-such-page").await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "GET /no-such-page status with a session"
    );
    app::assert_security_headers(&reply, "/no-such-page");
}

/// A 405 from the method router, `POST /health` on a GET-only route, carries the four SPEC 5.3
/// headers, `Cache-Control: no-store` among them, each once: the headers layer wraps the whole
/// router, so a response axum produces below the handlers gets them too. The same wrong method
/// on a path no route matches, `POST /no-such-page`, gets the router's 404 fallback with the
/// headers as well: a 405 alone cannot tell `.layer` from `.route_layer`, since axum puts each
/// method router's 405 fallback inside the route layer, but only `.layer` reaches the router's
/// own fallback. Both are forms sent from Nage's own page by a signed-in user, so that they
/// pass the Origin check and the session rule and reach the router's own answers.
///
/// Proves: SPEC 5.3, decision 40, decision 119.
#[tokio::test]
async fn security_headers_on_method_not_allowed() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for (uri, status) in [
        ("/health", StatusCode::METHOD_NOT_ALLOWED),
        ("/no-such-page", StatusCode::NOT_FOUND),
    ] {
        let reply = client.post(uri, &[]).await;
        assert_eq!(reply.status, status, "POST {uri} status");
        app::assert_security_headers(&reply, &format!("POST {uri}"));
    }
}

/// Every answer of the rules of milestone 2 slice 5 carries the four SPEC 5.3 headers,
/// `Cache-Control: no-store` among them, each once: the 303 to the sign-in page, the 401 of an
/// htmx request, the Origin check's 403, the body limit's 413, a refused sign-in's 422, a
/// paused one's 429, the closed set-up page's 404 and the 303 of an accepted sign-in.
///
/// Proves: SPEC 5.3 (response headers on every response), decision 40, decision 110 (the
/// security headers lie outside the three inner layers), decision 119.
#[tokio::test]
async fn security_headers_on_the_answers_of_the_rules() {
    let app = App::with_book().await;
    let wrong = [("username", ADMIN), ("password", WRONG_PASSWORD)];
    let cases = [
        (Req::get("/account/password"), StatusCode::SEE_OTHER),
        (
            Req::get("/account/password").header("hx-request", "true"),
            StatusCode::UNAUTHORIZED,
        ),
        (
            Req::form("/login", &wrong).header("origin", "http://evil.example"),
            StatusCode::FORBIDDEN,
        ),
        (
            Req::form("/login", &[]).body(vec![b'x'; MAX_FORM_BYTES + 1]),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            Req::form("/login", &wrong),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Req::form("/login", &wrong),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Req::form("/login", &wrong),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Req::form("/login", &wrong),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Req::form("/login", &wrong),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (Req::form("/login", &wrong), StatusCode::TOO_MANY_REQUESTS),
        (Req::get("/setup"), StatusCode::NOT_FOUND),
    ];
    for (request, status) in cases {
        let reply = app.client().send(request).await;
        assert_eq!(reply.status, status, "status of an answer of the rules");
        app::assert_security_headers(&reply, &format!("the {status} of the rules"));
    }
    app.clock.advance(60);
    let reply = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of an accepted sign-in"
    );
    app::assert_security_headers(&reply, "the 303 of an accepted sign-in");
}

/// Every `Cache-Control` value of an answer with the headers `headers`, in order.
fn cache_control(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("cache-control")
        .iter()
        .map(|value| value.to_str().unwrap_or_default().to_owned())
        .collect()
}

/// Every answer carries `Cache-Control: no-store` exactly once, except a served asset, which
/// keeps its own `immutable` value and no other: `GET /health`, the bare 404 of an unknown
/// address under `/static/`, the busy page of a sign-in sent while the writer is held, the
/// Origin check's 403 page, the session rule's 303 to the sign-in page and the bare 500 of a
/// home page that has no today each carry `no-store`, while the content-hashed `app.css`
/// carries only its `immutable` value.
///
/// Proves: decision 119 (every answer carries `Cache-Control: no-store`, a failure's bare 500
/// included, except one that carries a `Cache-Control` of its own, as a served asset keeps
/// decision 46's), SPEC 5.3.
#[tokio::test]
async fn every_answer_but_an_asset_is_not_to_be_stored() {
    let url = hashed_url("app.css", asset("app.css").body);
    let served = get(state(true), &url).await;
    assert_eq!(served.status(), StatusCode::OK, "GET {url} status");
    assert_eq!(
        cache_control(served.headers()),
        [IMMUTABLE],
        "Cache-Control of the asset {url}"
    );
    for uri in ["/health", "/static/app.css"] {
        let response = get(state(true), uri).await;
        assert_eq!(
            cache_control(response.headers()),
            ["no-store"],
            "Cache-Control of GET {uri}"
        );
    }

    let app = App::start_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await;
    app.set_up().await;
    let held = app.write().await;
    let busy = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    drop(held);
    let cross_site = app
        .client()
        .send(Req::form("/login", &[]).header("origin", "http://evil.example"))
        .await;
    let redirect = app.client().get("/account/password").await;
    for (reply, status, what) in [
        (&busy, StatusCode::SERVICE_UNAVAILABLE, "the busy page"),
        (&cross_site, StatusCode::FORBIDDEN, "the 403 page"),
        (
            &redirect,
            StatusCode::SEE_OTHER,
            "the 303 to the sign-in page",
        ),
    ] {
        assert_eq!(reply.status, status, "status of {what}");
        assert_eq!(
            cache_control(&reply.headers),
            ["no-store"],
            "Cache-Control of {what}"
        );
    }

    let late = App::start_with(Options {
        zone: Some("Pacific/Kiritimati"),
        ..Options::default()
    })
    .await;
    // 2199-12-31T12:00:00Z: already 2200 in that zone, so the home page has no today.
    late.clock.set(7_258_075_200 - app::START);
    late.set_up().await;
    let failure = late.signed_in(ADMIN, PASSWORD).await.get("/").await;
    assert_eq!(
        failure.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "status of a home page without a today"
    );
    assert_eq!(
        cache_control(&failure.headers),
        ["no-store"],
        "Cache-Control of the bare 500"
    );
}
