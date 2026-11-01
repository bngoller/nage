//! The response headers of SPEC 5.3 on every kind of response.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _;

use common::{assert_security_headers, asset, get, hashed_url, state};

/// `GET /` carries the three SPEC 5.3 headers with their exact values.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_index() {
    let response = get(state(true), "/").await;
    assert_security_headers(&response, "/");
    assert_eq!(response.status(), StatusCode::OK, "GET / status");
}

/// `GET /health` carries the three SPEC 5.3 headers, on the 200 and on the 503 answer.
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

/// A content-hashed static asset carries the three SPEC 5.3 headers.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_static_asset() {
    let url = hashed_url("app.css", asset("app.css").body);
    let response = get(state(true), &url).await;
    assert_security_headers(&response, &url);
    assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
}

/// A 404, from the fallback and from the static route, carries the three SPEC 5.3 headers.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_not_found() {
    for uri in ["/no-such-page", "/static/app.css"] {
        let response = get(state(true), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
        assert_security_headers(&response, uri);
    }
}

/// A 405 from the method router, `POST /health` on a GET-only route, carries the three
/// SPEC 5.3 headers, each once: the headers layer wraps the whole router, so a response axum
/// produces below the handlers gets them too. The same wrong method on a path no route
/// matches, `POST /no-such-page`, gets the router's 404 fallback with the headers as well:
/// a 405 alone cannot tell `.layer` from `.route_layer`, since axum puts each method router's
/// 405 fallback inside the route layer, but only `.layer` reaches the router's own fallback.
/// The requests are built here rather than through a helper, since no route takes a POST yet.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn security_headers_on_method_not_allowed() {
    for (uri, status) in [
        ("/health", StatusCode::METHOD_NOT_ALLOWED),
        ("/no-such-page", StatusCode::NOT_FOUND),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("cannot build POST {uri}: {error}"));
        let response = match nage_web::router(state(true)).oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        assert_eq!(response.status(), status, "POST {uri} status");
        assert_security_headers(&response, &format!("POST {uri}"));
    }
}
