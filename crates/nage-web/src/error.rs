//! The failures a handler maps to a response: busy is the 503 page with `Retry-After`,
//! anything else a 500 after an `error` log (SPEC 5.4, decisions 50 and 53). A refusal is not
//! one of them: a handler answers a refusal itself, with the page that explains it
//! (decision 102).

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use nage_core::storage::{self, BusyCause};

use crate::pages::{BusyPage, Layout, render};

/// The `Retry-After` value of the busy response: the seconds a client waits before trying
/// again (SPEC 5.4, decision 53).
const RETRY_AFTER_SECONDS: &str = "5";

/// A handler's failure (decision 53). The set-up and sign-in forms of milestone 2 slice 5 are
/// the first routes that raise it: with the writer held they answer the busy page.
#[derive(Debug)]
pub enum AppError {
    /// The database is in use: 503 with `Retry-After: 5` and the busy page (SPEC 5.4).
    Busy(BusyCause),
    /// Any other storage error: 500 after an `error` log (decision 50).
    Storage(storage::Error),
    /// Any other failure that no user caused, with its message: the hasher or its thread
    /// failed, the clock is outside the years a stamp can hold, the operating system gave no
    /// random bytes, a core operation failed in a way that is not a refusal. 500 after an
    /// `error` log; the message is logged and never shown.
    Internal(String),
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
    /// cause logged at `debug` (the request log already shows the 503); `Storage` and
    /// `Internal` log the error at `error` and answer a bare 500 (decision 50). Should the busy page fail to
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
            Self::Internal(error) => {
                tracing::error!(%error, "request failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

use nage_core::timestamp::TimestampError;

impl From<TimestampError> for AppError {
    /// A clock outside the years a stamp holds (decision 98): `Internal`, so that the request
    /// answers 500 instead of stamping a wrong time.
    fn from(error: TimestampError) -> Self {
        Self::Internal(format!(
            "the clock is outside the years a stamp holds: {error}"
        ))
    }
}

use nage_core::accounts::AccountError;
use nage_core::chart::ChartError;

impl From<ChartError> for AppError {
    /// A model of the account pages could not be read (decision 120): a database failure is
    /// `Storage` and everything else `Internal`, so that the page answers 500 rather than a
    /// wrong figure or a missing section.
    fn from(error: ChartError) -> Self {
        match error {
            ChartError::Database(error) => storage::Error::Sqlx(error).into(),
            other => Self::Internal(format!("an account page could not be read: {other}")),
        }
    }
}

impl From<AccountError> for AppError {
    /// An account operation failed in a way that is not a refusal (decision 102): a database
    /// failure is `Storage`, a missing built-in account and anything a handler did not answer
    /// itself `Internal`.
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::Database(error) => storage::Error::Sqlx(error).into(),
            other => Self::Internal(format!("an account operation failed: {other}")),
        }
    }
}

use nage_core::transactions::OpeningBalanceError;

impl From<OpeningBalanceError> for AppError {
    /// An opening-balance operation failed in a way that is not a refusal a page answers
    /// (decision 102): a database failure is `Storage`, anything else `Internal`.
    fn from(error: OpeningBalanceError) -> Self {
        match error {
            OpeningBalanceError::Database(error) => storage::Error::Sqlx(error).into(),
            other => Self::Internal(format!("an opening-balance operation failed: {other}")),
        }
    }
}

use nage_core::home::HomeError;

impl From<HomeError> for AppError {
    /// The home page's model could not be read (decision 120): a database failure is
    /// `Storage` and everything else `Internal`, an amount that does not fit in 64 bits
    /// included, so that the page answers 500 rather than a wrong figure.
    fn from(error: HomeError) -> Self {
        match error {
            HomeError::Database(error) => storage::Error::Sqlx(error).into(),
            other => Self::Internal(format!("the home page could not be read: {other}")),
        }
    }
}
