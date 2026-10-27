//! How requests are logged (decision 47). This must stay the only test in its binary:
//! tracing caches each callsite's interest process-wide, so another test thread that first
//! reaches a callsite while the capture's subscriber is the only registered dispatcher
//! caches `never` from its own no-op default and the capture sees nothing.

mod common;

use axum::http::StatusCode;
use tracing::Level;

use common::{capture_logs, get, state};

/// Lines of `logs` written at `level` (as the `fmt` subscriber prints it) that contain `needle`.
fn lines_at<'a>(logs: &'a str, level: &str, needle: &str) -> Vec<&'a str> {
    let level = format!(" {level} ");
    logs.lines()
        .filter(|line| line.contains(&level) && line.contains(needle))
        .collect()
}

/// The request log keeps a passing health check out of the default `info` output and shows
/// a failing one: a 2xx `/health` response is logged at `debug`, any other at `info`, with
/// `method`, `path`, `status` and `latency_ms`. Logs are captured in process with a
/// thread-scoped subscriber.
///
/// Cites: decision 47.
#[test]
fn health_requests_log_at_debug_when_ok_and_info_when_failing() {
    let ((health_ok, index_ok), info_logs) = capture_logs(Level::INFO, || async {
        let health = get(state(true), "/health").await.status();
        let index = get(state(true), "/").await.status();
        (health, index)
    });
    let (health_failing, failing_logs) = capture_logs(Level::INFO, || async {
        get(state(false), "/health").await.status()
    });
    let (health_ok_debug, debug_logs) = capture_logs(Level::DEBUG, || async {
        get(state(true), "/health").await.status()
    });

    assert_eq!(health_ok, StatusCode::OK, "GET /health status");
    assert_eq!(index_ok, StatusCode::OK, "GET / status");
    assert_eq!(
        health_failing,
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status with a failing probe"
    );
    assert_eq!(
        health_ok_debug,
        StatusCode::OK,
        "GET /health status under a debug capture"
    );

    let index_line = "method=GET path=/ status=200 latency_ms=";
    assert!(
        !lines_at(&info_logs, "INFO", index_line).is_empty(),
        "no captured INFO line with `{index_line}` at max level INFO; captured:\n{info_logs}"
    );
    let failing_line = "method=GET path=/health status=503 latency_ms=";
    assert!(
        !lines_at(&failing_logs, "INFO", failing_line).is_empty(),
        "no captured INFO line with `{failing_line}` at max level INFO; captured:\n{failing_logs}"
    );
    let ok_line = "method=GET path=/health status=200 latency_ms=";
    assert!(
        !lines_at(&debug_logs, "DEBUG", ok_line).is_empty(),
        "no captured DEBUG line with `{ok_line}` at max level DEBUG; captured:\n{debug_logs}"
    );

    let leaked: Vec<&str> = info_logs
        .lines()
        .filter(|line| line.contains("path=/health"))
        .collect();
    assert!(
        leaked.is_empty(),
        "a passing GET /health was logged at max level INFO: {leaked:?}"
    );
}
