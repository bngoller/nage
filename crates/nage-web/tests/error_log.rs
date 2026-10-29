//! What `AppError` logs when it becomes a response (SPEC 5.4, decision 53). This must stay
//! the only test in its binary: tracing caches each callsite's interest process-wide, so
//! another test thread that first reaches a callsite while the capture's subscriber is the
//! only registered dispatcher caches `never` from its own no-op default and the capture sees
//! nothing.

mod common;

use std::io;

use axum::http::StatusCode;
use axum::response::IntoResponse as _;
use nage_core::storage::{self, BusyCause};
use nage_web::AppError;
use tracing::Level;

use common::{capture_logs, lines_at};

/// A storage error other than busy logs exactly one ERROR line that says the storage failed
/// and carries the error's text; a busy error logs exactly one DEBUG line that says the
/// database is busy and names the cause, and no ERROR line (the request log already shows
/// its 503). Both are captured at max level DEBUG in process with a thread-scoped subscriber.
///
/// Proves: SPEC 5.4, decision 53.
#[test]
fn app_error_logs_the_storage_error_and_the_busy_cause() {
    let (storage_status, storage_logs) = capture_logs(Level::DEBUG, || async {
        AppError::from(storage::Error::Sqlx(
            io::Error::other("the disk went away").into(),
        ))
        .into_response()
        .status()
    });
    let (busy_status, busy_logs) = capture_logs(Level::DEBUG, || async {
        AppError::from(storage::Error::Busy(BusyCause::WriteQueue))
            .into_response()
            .status()
    });

    assert_eq!(
        storage_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "status of the response to a storage error"
    );
    assert_eq!(
        busy_status,
        StatusCode::SERVICE_UNAVAILABLE,
        "status of the response to a busy error"
    );

    let errors = lines_at(&storage_logs, "ERROR", "");
    assert_eq!(
        errors.len(),
        1,
        "number of ERROR lines for a storage error; captured:\n{storage_logs}"
    );
    let line = errors[0];
    assert!(
        line.contains("storage failed"),
        "the ERROR line for a storage error does not say \"storage failed\": {line}"
    );
    assert!(
        line.contains("the disk went away"),
        "the ERROR line for a storage error does not carry the error text: {line}"
    );

    let cause = BusyCause::WriteQueue.to_string();
    let busy = lines_at(&busy_logs, "DEBUG", "database busy");
    assert_eq!(
        busy.len(),
        1,
        "number of DEBUG lines saying \"database busy\" for a busy error; captured:\n{busy_logs}"
    );
    let line = busy[0];
    assert!(
        line.contains(&cause),
        "the DEBUG line for a busy error does not name the cause `{cause}`: {line}"
    );
    assert!(
        line.contains("write queue"),
        "the DEBUG line for a busy error does not name the write queue: {line}"
    );
    let errors = lines_at(&busy_logs, "ERROR", "");
    assert!(
        errors.is_empty(),
        "a busy error logged at ERROR: {errors:?}; captured:\n{busy_logs}"
    );
}
