//! What the request log says about who is signed in (decisions 47 and 109). This must stay
//! the only test in its binary: tracing caches each callsite's interest process-wide, so
//! another test thread that first reaches a callsite while the capture's subscriber is the
//! only registered dispatcher caches `never` from its own no-op default and the capture sees
//! nothing.

mod common;

use axum::http::StatusCode;
use tracing::Level;

use common::app::{ADMIN, App, PASSWORD};
use common::{capture_logs, lines_at};

/// A username and a password no log line may hold.
const TYPED_USERNAME: &str = "marker-user-zx81";
const TYPED_PASSWORD: &str = "marker-pass-ql77";

/// The request event of a request with a valid session carries `user_id`, the signed-in
/// user's id; a request without a session has no `user_id` field: a page that sends to the
/// sign-in page, a refused sign-in, and the accepted sign-in itself, which arrived without a
/// session. At max level DEBUG, where every event of the application and of the database
/// layer is captured, no line holds the typed username, a typed password, the session
/// token or the cookie's name.
///
/// Proves: decision 109 (the response event gains the signed-in user's id, none without a
/// session; the typed username is never logged, nor the cookie and form bodies), SPEC 5.2,
/// decision 47.
#[test]
fn the_request_log_names_the_signed_in_user_and_never_what_was_typed() {
    let ((statuses, admin, token), logs) = capture_logs(Level::DEBUG, || async {
        let app = App::with_book().await;
        let admin = app.credentials(ADMIN).await.user.id;
        let mut anonymous = app.client();
        let page = anonymous.get("/account/password").await.status;
        let refused = anonymous
            .post(
                "/login",
                &[("username", TYPED_USERNAME), ("password", TYPED_PASSWORD)],
            )
            .await
            .status;
        let mut client = app.signed_in(ADMIN, PASSWORD).await;
        let token = client
            .cookie
            .clone()
            .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
        let home = client.get("/").await.status;
        let missing = client.get("/request-log-sentinel").await.status;
        ([page, refused, home, missing], admin, token)
    });
    assert_eq!(
        statuses,
        [
            StatusCode::SEE_OTHER,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::OK,
            StatusCode::NOT_FOUND,
        ],
        "the statuses of the four requests"
    );

    let with_user = format!(" user_id={admin}");
    for (line, signed_in) in [
        (
            "method=GET path=/account/password status=303 latency_ms=",
            false,
        ),
        ("method=POST path=/login status=422 latency_ms=", false),
        ("method=POST path=/login status=303 latency_ms=", false),
        ("method=GET path=/ status=200 latency_ms=", true),
        (
            "method=GET path=/request-log-sentinel status=404 latency_ms=",
            true,
        ),
    ] {
        let found = lines_at(&logs, "INFO", line);
        assert_eq!(
            found.len(),
            1,
            "number of INFO lines with `{line}`; captured:\n{logs}"
        );
        assert_eq!(
            found[0].contains(&with_user),
            signed_in,
            "whether the request line carries `{with_user}`: {}",
            found[0]
        );
        assert_eq!(
            found[0].contains("user_id="),
            signed_in,
            "whether the request line carries a user_id field at all: {}",
            found[0]
        );
    }

    for secret in [
        TYPED_USERNAME,
        TYPED_PASSWORD,
        PASSWORD,
        token.as_str(),
        "nage_session",
    ] {
        let leaks: Vec<&str> = logs.lines().filter(|line| line.contains(secret)).collect();
        assert!(leaks.is_empty(), "the log holds {secret:?}: {leaks:#?}");
    }
}
