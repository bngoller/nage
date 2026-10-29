//! How requests are logged (decision 47). This must stay the only test in its binary:
//! tracing caches each callsite's interest process-wide, so another test thread that first
//! reaches a callsite while the capture's subscriber is the only registered dispatcher
//! caches `never` from its own no-op default and the capture sees nothing.

mod common;

use axum::http::StatusCode;
use tracing::Level;

use common::{capture_logs, get, lines_at, state};

/// The path of the sentinel request each capture ends with: a 404 from the fallback, logged at
/// `info`. The requests run one after another, so any later line of an earlier request would
/// have to appear before the sentinel's line; finding that line proves the count is complete.
const SENTINEL: &str = "/request-log-sentinel";

/// The request line of the sentinel request.
const SENTINEL_LINE: &str = "method=GET path=/request-log-sentinel status=404 latency_ms=";

/// Lines of `logs` that are request events (they carry `method=`).
fn request_lines(logs: &str) -> Vec<&str> {
    logs.lines()
        .filter(|line| line.contains("method="))
        .collect()
}

/// Asserts that the sentinel's INFO line is in `logs`, then that exactly `expected` request
/// lines were captured. `what` names the capture in the failure message.
fn assert_request_count(logs: &str, expected: usize, what: &str) {
    assert_eq!(
        lines_at(logs, "INFO", SENTINEL_LINE).len(),
        1,
        "number of INFO lines with `{SENTINEL_LINE}` in the {what} capture; captured:\n{logs}"
    );
    assert_eq!(
        request_lines(logs).len(),
        expected,
        "number of request lines in the {what} capture, sentinel included; captured:\n{logs}"
    );
}

/// The request log keeps a passing health check out of the default `info` output and shows
/// a failing one: a 2xx `/health` response is logged at `debug`, any other at `info`, with
/// `method`, `path`, `status` and `latency_ms`. Each request produces exactly one request
/// event, counted only after a later sentinel request's line has arrived. Logs are captured
/// in process with a thread-scoped subscriber.
///
/// Proves: decision 47.
#[test]
fn health_requests_log_at_debug_when_ok_and_info_when_failing() {
    let ((health_ok, index_ok, sentinel_info), info_logs) = capture_logs(Level::INFO, || async {
        let health = get(state(true), "/health").await.status();
        let index = get(state(true), "/").await.status();
        let sentinel = get(state(true), SENTINEL).await.status();
        (health, index, sentinel)
    });
    let ((health_failing, sentinel_failing), failing_logs) = capture_logs(Level::INFO, || async {
        let health = get(state(false), "/health").await.status();
        let sentinel = get(state(true), SENTINEL).await.status();
        (health, sentinel)
    });
    let ((health_ok_debug, sentinel_debug), debug_logs) = capture_logs(Level::DEBUG, || async {
        let health = get(state(true), "/health").await.status();
        let sentinel = get(state(true), SENTINEL).await.status();
        (health, sentinel)
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
    for sentinel in [sentinel_info, sentinel_failing, sentinel_debug] {
        assert_eq!(sentinel, StatusCode::NOT_FOUND, "GET {SENTINEL} status");
    }

    // At INFO: the index request and the sentinel; the passing health check is not shown.
    assert_request_count(&info_logs, 2, "INFO");
    let index_line = "method=GET path=/ status=200 latency_ms=";
    assert_eq!(
        lines_at(&info_logs, "INFO", index_line).len(),
        1,
        "number of captured INFO lines with `{index_line}` at max level INFO; captured:\n{info_logs}"
    );
    // At INFO with a failing probe: the failing health check and the sentinel.
    assert_request_count(&failing_logs, 2, "failing INFO");
    let failing_line = "method=GET path=/health status=503 latency_ms=";
    assert_eq!(
        lines_at(&failing_logs, "INFO", failing_line).len(),
        1,
        "number of captured INFO lines with `{failing_line}` at max level INFO; captured:\n{failing_logs}"
    );
    // At DEBUG: the passing health check at DEBUG and the sentinel.
    assert_request_count(&debug_logs, 2, "DEBUG");
    let ok_line = "method=GET path=/health status=200 latency_ms=";
    assert_eq!(
        lines_at(&debug_logs, "DEBUG", ok_line).len(),
        1,
        "number of captured DEBUG lines with `{ok_line}` at max level DEBUG; captured:\n{debug_logs}"
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
