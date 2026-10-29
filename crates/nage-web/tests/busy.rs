//! The storage failures a handler maps to a response: `AppError`'s 503 busy page and its 500
//! (SPEC 5.4, decision 53). No route raises them before the first POST (milestone 2 slice 5),
//! so the mapping and the page are proved directly on `AppError::into_response`.

mod common;

use std::io;

use axum::http::StatusCode;
use axum::response::IntoResponse as _;
use nage_core::storage::{self, BusyCause};
use nage_web::AppError;

use common::{body_text, header, start_tags};

/// `storage::Error::Busy` with each queue cause and with a SQLite cause becomes
/// `AppError::Busy` with that cause and answers 503 with `Retry-After: 5` and the HTML busy
/// page on the base layout: the body names "busy", links back to `/`, loads its scripts by
/// `src` only (the base layout legitimately loads htmx) and carries no inline style.
///
/// Proves: SPEC 5.3, SPEC 5.4, decision 53.
#[tokio::test]
async fn busy_error_is_503_with_retry_after_and_the_page() {
    for cause in [
        BusyCause::WriteQueue,
        BusyCause::ReadQueue,
        BusyCause::Sqlite { code: 5 },
    ] {
        let error = AppError::from(storage::Error::Busy(cause));
        assert!(
            matches!(&error, AppError::Busy(mapped) if *mapped == cause),
            "AppError::from(Busy({cause:?})) is {error:?}, not AppError::Busy({cause:?})"
        );
        let response = error.into_response();
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "status of the busy response for {cause:?}"
        );
        assert_eq!(
            header(&response, "retry-after").as_deref(),
            Some("5"),
            "Retry-After of the busy response for {cause:?}"
        );
        let content_type = header(&response, "content-type")
            .unwrap_or_else(|| panic!("the busy response for {cause:?} has no content-type"));
        assert!(
            content_type.starts_with("text/html"),
            "content-type of the busy response for {cause:?}: {content_type}"
        );

        let html = body_text(response).await;
        let lower = html.to_ascii_lowercase();
        assert!(
            lower.contains("busy"),
            "the busy page for {cause:?} does not name \"busy\":\n{html}"
        );
        assert!(
            html.contains(r#"href="/""#),
            "the busy page for {cause:?} has no link back to /:\n{html}"
        );
        let scripts = start_tags(&lower, "<script");
        assert!(
            !scripts.is_empty(),
            "the busy page for {cause:?} has no <script> tag, so it is not on the base layout:\n{html}"
        );
        for tag in scripts {
            assert!(
                tag.contains(" src="),
                "the busy page for {cause:?} has an inline script `{tag}`:\n{html}"
            );
        }
        assert!(
            !lower.contains("<style"),
            "the busy page for {cause:?} has an inline <style> element:\n{html}"
        );
        assert!(
            !lower.contains(" style="),
            "the busy page for {cause:?} has an inline style attribute:\n{html}"
        );
    }
}

/// A storage error other than busy becomes `AppError::Storage` and answers 500 without a
/// `Retry-After`: an `Error::Sqlx` (an I/O failure; nage-web has no sqlx dependency of its
/// own, so the `sqlx::Error` is reached through its `From<io::Error>` by way of the variant's
/// field type) and an `Error::NewerDatabase`.
///
/// Proves: SPEC 5.4, decision 53.
#[tokio::test]
async fn storage_error_is_500() {
    let errors = [
        storage::Error::Sqlx(io::Error::other("the disk went away").into()),
        storage::Error::NewerDatabase { version: 9999 },
    ];
    for error in errors {
        let what = error.to_string();
        let error = AppError::from(error);
        assert!(
            matches!(&error, AppError::Storage(_)),
            "AppError::from({what}) is {error:?}, not AppError::Storage"
        );
        let response = error.into_response();
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "status of the response to the storage error `{what}`"
        );
        assert_eq!(
            header(&response, "retry-after"),
            None,
            "Retry-After on the response to the storage error `{what}`"
        );
    }
}
