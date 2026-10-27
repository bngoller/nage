//! The response headers of SPEC 5.3 on every kind of response.

mod common;

use axum::http::StatusCode;

use common::{assert_security_headers, asset, get, hashed_url, state};

/// `GET /` carries the three SPEC 5.3 headers with their exact values.
///
/// Cites: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_index() {
    let response = get(state(true), "/").await;
    assert_security_headers(&response, "/");
    assert_eq!(response.status(), StatusCode::OK, "GET / status");
}

/// `GET /health` carries the three SPEC 5.3 headers, on the 200 and on the 503 answer.
///
/// Cites: SPEC 5.3, decision 40.
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

/// A content-hashed static asset carries the three SPEC 5.3 headers.
///
/// Cites: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_static_asset() {
    let url = hashed_url("app.css", asset("app.css").body);
    let response = get(state(true), &url).await;
    assert_security_headers(&response, &url);
    assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
}

/// A 404, from the fallback and from the static route, carries the three SPEC 5.3 headers.
///
/// Cites: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_not_found() {
    for uri in ["/no-such-page", "/static/app.css"] {
        let response = get(state(true), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
        assert_security_headers(&response, uri);
    }
}
