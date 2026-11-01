//! What the storage health probe and its deadline log (decision 55). This must stay the only
//! test in its binary: tracing caches each callsite's interest process-wide, so another test
//! thread that first reaches a callsite while the capture's subscriber is the only registered
//! dispatcher caches `never` from its own no-op default and the capture sees nothing.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::test_support::{open_fresh, open_migrated};
use nage_web::{AppState, HealthProbe};
use tracing::Level;

use common::{VERSION, capture_logs, get, storage_state};

/// The production probe over an unmigrated database answers 503 and the capture at max level
/// WARN holds exactly one line: a WARN line that mentions the health probe and carries the
/// error (the missing `book` table). Over a migrated database it answers 200 and the capture
/// at WARN is empty. A probe under a 10 ms deadline whose check sleeps for a second answers
/// 503 and the capture holds exactly one WARN line that mentions the health probe and the
/// deadline (the timeout half of row 55's "one `warn` line"). Logs are captured in process
/// with a thread-scoped subscriber.
///
/// Proves: decision 55.
#[test]
fn failing_storage_probe_logs_one_warn_line() {
    let (failing, failing_logs) = capture_logs(Level::WARN, || async {
        let (_dir, storage) = open_fresh().await;
        get(storage_state(storage), "/health").await.status()
    });
    let (passing, passing_logs) = capture_logs(Level::WARN, || async {
        let (_dir, storage) = open_migrated().await;
        get(storage_state(storage), "/health").await.status()
    });
    let (late, late_logs) = capture_logs(Level::WARN, || async {
        let health = HealthProbe::with_deadline(Duration::from_millis(10), || async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            true
        });
        let state = AppState {
            version: VERSION,
            health,
        };
        get(state, "/health").await.status()
    });

    assert_eq!(
        failing,
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status on an unmigrated database"
    );
    assert_eq!(
        passing,
        StatusCode::OK,
        "GET /health status on a migrated database"
    );
    assert_eq!(
        late,
        StatusCode::SERVICE_UNAVAILABLE,
        "GET /health status when the probe misses its 10 ms deadline"
    );

    let lines: Vec<&str> = failing_logs.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "number of lines captured at max level WARN for a failing storage probe; captured:\n{failing_logs}"
    );
    let line = lines[0];
    assert!(
        line.contains(" WARN "),
        "the one line of a failing storage probe is not at WARN: {line}"
    );
    let lower = line.to_ascii_lowercase();
    assert!(
        lower.contains("health") && lower.contains("probe"),
        "the WARN line of a failing storage probe does not mention the health probe: {line}"
    );
    assert!(
        lower.contains("no such table"),
        "the WARN line of a failing storage probe does not carry the error (no such table: book): {line}"
    );

    assert!(
        passing_logs.trim().is_empty(),
        "a passing storage probe logged at max level WARN:\n{passing_logs}"
    );

    let lines: Vec<&str> = late_logs.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "number of lines captured at max level WARN for a probe that misses its deadline; captured:\n{late_logs}"
    );
    let line = lines[0];
    assert!(
        line.contains(" WARN "),
        "the one line of a probe that misses its deadline is not at WARN: {line}"
    );
    let lower = line.to_ascii_lowercase();
    assert!(
        lower.contains("health") && lower.contains("probe"),
        "the WARN line of a probe that misses its deadline does not mention the health probe: {line}"
    );
    assert!(
        lower.contains("deadline") && lower.contains("deadline_ms=10"),
        "the WARN line of a probe that misses its deadline does not name the 10 ms deadline: {line}"
    );
}
