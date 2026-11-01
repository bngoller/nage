//! `GET /health` through the injectable probe and through the production probe over a
//! database. How its requests are logged is proved in `tests/request_log.rs`, and what the
//! storage probe logs in `tests/health_log.rs`, each a binary of its own.

mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use nage_core::storage::StorageOptions;
use nage_core::test_support::{migrate, open_fresh, open_migrated, open_with};
use nage_web::{AppState, HealthProbe};
use serde_json::Value;

use common::{
    VERSION, body_bytes, body_text, counting_state, get, get_via, header, state, storage_state,
};

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
/// Proves: SPEC 5.2, decision 41.
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
/// Proves: decision 41.
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
/// Proves: decision 41.
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

/// With the production probe over a freshly migrated database that has no `book` row yet,
/// `GET /health` answers 200 with `ok` true and the version: the count query succeeding is
/// health, whatever the count.
///
/// Proves: SPEC 5.2, decision 55.
#[tokio::test]
async fn health_is_200_on_a_migrated_database_with_no_book_row() {
    let (_dir, storage) = open_migrated().await;
    let response = get(storage_state(storage), "/health").await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /health status on a migrated database with no book row"
    );
    assert_eq!(
        header(&response, "content-type").as_deref(),
        Some("application/json"),
        "GET /health content-type on a migrated database"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(true)),
        "GET /health field ok on a migrated database in {body}"
    );
    assert_eq!(
        body.get("version"),
        Some(&Value::from(VERSION)),
        "GET /health field version on a migrated database in {body}"
    );
}

/// With the production probe over an opened but unmigrated database, `GET /health` answers
/// 503 with `ok` false: the `book` table does not exist yet, so the count query fails.
///
/// Proves: decision 55.
#[tokio::test]
async fn health_is_503_on_an_unmigrated_database() {
    let (_dir, storage) = open_fresh().await;
    let response = get(storage_state(storage), "/health").await;
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status on an unmigrated database"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "GET /health field ok on an unmigrated database in {body}"
    );
    assert_eq!(
        body.get("version"),
        Some(&Value::from(VERSION)),
        "GET /health field version on an unmigrated database in {body}"
    );
}

/// A probe under a 10 ms deadline whose check sleeps for a second answers 503, and the
/// request returns long before the sleep would have ended: the deadline cuts the check short
/// instead of waiting for it.
///
/// Proves: decision 55.
#[tokio::test]
async fn health_is_503_when_the_probe_misses_its_deadline() {
    let health = HealthProbe::with_deadline(Duration::from_millis(10), || async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        true
    });
    let state = AppState {
        version: VERSION,
        health,
    };
    let started = Instant::now();
    let response = get(state, "/health").await;
    let elapsed = started.elapsed();
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status when the probe misses its 10 ms deadline"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "GET /health took {elapsed:?} under a 10 ms probe deadline: the deadline did not cut the 1 s check short"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "GET /health field ok when the probe misses its deadline in {body}"
    );
    assert_eq!(
        body.get("version"),
        Some(&Value::from(VERSION)),
        "GET /health field version when the probe misses its deadline in {body}"
    );
}

/// The production probe enforces the deadline it is given: over a migrated database whose
/// only reader connection is held elsewhere, the count query queues on the read pool, a
/// 10 ms deadline cuts it short, and `GET /health` answers 503 long before the 10 s read
/// queue would have expired. A `HealthProbe::storage` that ignored its deadline would wait
/// out the queue instead.
///
/// Proves: decision 55.
#[tokio::test]
async fn health_storage_probe_enforces_its_deadline() {
    let options = StorageOptions {
        readers: 1,
        read_queue: Duration::from_secs(10),
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    migrate(&storage).await;
    let held = storage
        .reader()
        .acquire()
        .await
        .unwrap_or_else(|error| panic!("cannot hold the only reader connection: {error}"));
    let state = AppState {
        version: VERSION,
        health: HealthProbe::storage(storage.clone(), Duration::from_millis(10)),
    };
    let started = Instant::now();
    let response = get(state, "/health").await;
    let elapsed = started.elapsed();
    drop(held);
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status while the only reader connection is held and the probe deadline is 10 ms"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "GET /health took {elapsed:?} with a 10 ms probe deadline and a 10 s read queue: the storage probe did not enforce its deadline"
    );
    let body = json(&body_bytes(response).await);
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "GET /health field ok while the only reader connection is held in {body}"
    );
}

/// `GET /health` with a passing probe answers 200, and its body text, with the app's version
/// replaced by `[version]`, matches the committed snapshot beside this file. The body is not
/// a contract (SPEC 5.2: callers read the status, and the tests above check the fields); the
/// snapshot exists to exercise the golden harness that later report and import goldens use.
///
/// Proves: decision 60.
#[tokio::test]
async fn health_body_is_the_snapshot() {
    let response = get(state(true), "/health").await;
    assert_eq!(response.status(), StatusCode::OK, "GET /health status");
    let body = body_text(response).await;
    assert!(
        body.contains(VERSION),
        "GET /health body does not carry the version {VERSION}: {body}"
    );
    let redacted = body.replace(VERSION, "[version]");
    insta::assert_snapshot!(redacted);
}
