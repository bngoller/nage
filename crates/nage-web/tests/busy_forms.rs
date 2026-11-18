//! The busy path through the first POST routes (SPEC 5.4, decision 53's HTTP proof, decision
//! 115): with the writer held and a short queue, set-up and sign-in answer the busy page and
//! leave no row.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::storage::StorageOptions;

use common::app::{ADMIN, App, Options, PASSWORD, Reply};

/// An application whose requests wait a tenth of a second for the writer.
async fn impatient() -> App {
    App::start_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await
}

/// Asserts that `reply` is the busy answer: 503, `Retry-After: 5`, the busy page, no cookie.
fn assert_busy(reply: &Reply, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "status of {what}:\n{}",
        reply.body
    );
    assert_eq!(
        reply.header("retry-after"),
        Some("5"),
        "Retry-After of {what}"
    );
    assert!(
        reply.shows("Nage is busy"),
        "{what} does not show the busy page:\n{}",
        reply.body
    );
    assert!(
        reply.set_cookies().is_empty(),
        "{what} sets a cookie: {:?}",
        reply.set_cookies()
    );
}

/// A set-up sent while another write holds the writer waits out the short queue and answers
/// 503 with `Retry-After: 5` and the busy page; nothing is written: no book, no user, no
/// session. Sent again once the writer is free, the same form is accepted.
///
/// Proves: SPEC 5.4 (503 with `Retry-After: 5` and the busy page when the queue wait
/// expires), decision 53 (its HTTP proof on the first POST route), INV-28 (a busy set-up
/// leaves no row).
#[tokio::test]
async fn a_busy_set_up_answers_503_and_leaves_no_row() {
    let app = impatient().await;
    let form = [
        ("book_name", "Household"),
        ("currency", "CAD"),
        ("username", ADMIN),
        ("display_name", "Sam"),
        ("password", PASSWORD),
        ("password_again", PASSWORD),
    ];
    let held = app.write().await;
    let reply = app.client().post("/setup", &form).await;
    drop(held);
    assert_busy(&reply, "a set-up while the writer is held");
    assert_eq!(app.users().await, 0, "users after a busy set-up");
    assert_eq!(app.session_rows().await, 0, "sessions after a busy set-up");

    let again = app.client().post("/setup", &form).await;
    assert_eq!(
        again.status,
        StatusCode::SEE_OTHER,
        "status of the same set-up once the writer is free"
    );
    assert_eq!(
        app.users().await,
        1,
        "users after the set-up that went through"
    );
}

/// A sign-in with the right password sent while another write holds the writer answers 503
/// with `Retry-After: 5` and the busy page, and no session is written. Sent again once the
/// writer is free, it signs the user in.
///
/// Proves: SPEC 5.4, decision 53 (its HTTP proof), INV-28 (a busy sign-in leaves no row).
#[tokio::test]
async fn a_busy_sign_in_answers_503_and_leaves_no_row() {
    let app = impatient().await;
    app.set_up().await;
    let held = app.write().await;
    let reply = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    drop(held);
    assert_busy(&reply, "a sign-in while the writer is held");
    assert_eq!(app.session_rows().await, 0, "sessions after a busy sign-in");

    app.signed_in(ADMIN, PASSWORD).await;
    assert_eq!(
        app.session_rows().await,
        1,
        "sessions after the sign-in that went through"
    );
}
