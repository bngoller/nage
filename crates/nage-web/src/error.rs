//! The storage failures a handler maps to a response: busy is the 503 page with
//! `Retry-After`, anything else a 500 after an `error` log (SPEC 5.4, decisions 50 and 53).

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use nage_core::storage::{self, BusyCause};

use crate::pages::{BusyPage, Layout, render};

/// The `Retry-After` value of the busy response: the seconds a client waits before trying
/// again (SPEC 5.4, decision 53).
const RETRY_AFTER_SECONDS: &str = "5";

/// A handler's storage failure (decision 53). No route raises it before the first POST
/// (milestone 2 slice 5); slice 3 proves the mapping and the page.
#[derive(Debug)]
pub enum AppError {
    /// The database is in use: 503 with `Retry-After: 5` and the busy page (SPEC 5.4).
    Busy(BusyCause),
    /// Any other storage error: 500 after an `error` log (decision 50).
    Storage(storage::Error),
}

impl From<storage::Error> for AppError {
    /// `storage::Error::Busy(cause)` becomes `Busy(cause)`, everything else `Storage`.
    fn from(error: storage::Error) -> Self {
        match error {
            storage::Error::Busy(cause) => Self::Busy(cause),
            other => Self::Storage(other),
        }
    }
}

impl IntoResponse for AppError {
    /// `Busy` answers 503 with `Retry-After: 5` and the busy page on the base layout, the
    /// cause logged at `debug` (the request log already shows the 503); `Storage` logs the
    /// error at `error` and answers a bare 500 (decision 50). Should the busy page fail to
    /// render, `render` has logged the error and its 500 is returned unchanged, as decision
    /// 50 says for every failed render: a template fault is not cured by trying again.
    fn into_response(self) -> Response {
        match self {
            Self::Busy(cause) => {
                tracing::debug!(%cause, "database busy");
                let page = render(&BusyPage {
                    layout: Layout::embedded(),
                });
                if page.status() != StatusCode::OK {
                    return page;
                }
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(
                        header::RETRY_AFTER,
                        HeaderValue::from_static(RETRY_AFTER_SECONDS),
                    )],
                    page,
                )
                    .into_response()
            }
            Self::Storage(error) => {
                tracing::error!(%error, "storage failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
