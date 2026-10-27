//! `GET /health` through the injectable probe. How its requests are logged is proved in
//! `tests/request_log.rs`, a binary of its own.

mod common;

use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::Value;

use common::{VERSION, body_bytes, counting_state, get, get_via, header, state};

/// Parses a `/health` body, naming the raw text on failure.
fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!(
            "GET /health body is not JSON ({error}): {}",
            String::from_utf8_lossy(bytes)
        )
    })
}

/// With a passing probe, `GET /health` answers 200 with a JSON body whose `ok` is `true` and
/// whose `version` is the app's version.
///
/// Cites: SPEC 5.2, decision 41.
#[tokio::test]
async fn health_returns_200_json_when_probe_passes() {
    let response = get(state(true), "/health").await;
    assert_eq!(response.status(), StatusCode::OK, "GET /health status");
    assert_eq!(
        header(&response, "content-type").as_deref(),
        Some("application/json"),
        "GET /health content-type"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(true)),
        "GET /health field ok in {body}"
    );
    assert_eq!(
        body.get("version"),
        Some(&Value::from(VERSION)),
        "GET /health field version in {body}"
    );
}

/// With a failing probe, `GET /health` answers 503 with a JSON body whose `ok` is `false` and
/// which still carries the version.
///
/// Cites: decision 41.
#[tokio::test]
async fn health_returns_503_json_when_probe_fails() {
    let response = get(state(false), "/health").await;
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status with a failing probe"
    );
    assert_eq!(
        header(&response, "content-type").as_deref(),
        Some("application/json"),
        "GET /health content-type with a failing probe"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "GET /health field ok with a failing probe in {body}"
    );
    assert_eq!(
        body.get("version"),
        Some(&Value::from(VERSION)),
        "GET /health field version with a failing probe in {body}"
    );
}

/// The probe runs once per request, not once per router: two requests through the same
/// router instance run it twice.
///
/// Cites: decision 41.
#[tokio::test]
async fn health_runs_the_probe_on_every_request() {
    let (state, calls) = counting_state(true);
    let router = nage_web::router(state);
    let first = get_via(&router, "/health").await;
    let second = get_via(&router, "/health").await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "probe calls after two GET /health through one router"
    );
    assert_eq!(first.status(), StatusCode::OK, "first GET /health status");
    assert_eq!(second.status(), StatusCode::OK, "second GET /health status");
}
