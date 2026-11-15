//! The body limit (SPEC 5.3, decision 110): a form larger than 64 KiB is refused with 413,
//! before the Origin check looks at it.

mod common;

use axum::http::StatusCode;
use nage_web::MAX_FORM_BYTES;
use nage_web::messages::Refusal;

use common::app::{ADMIN, App, Req};

/// A sign-in form whose body is exactly `bytes` long: the password is padded.
fn form_of(bytes: usize) -> Req {
    let head = format!("username={ADMIN}&password=");
    let mut body = head.into_bytes();
    body.resize(bytes, b'x');
    Req::form("/login", &[]).body(body)
}

/// A form of exactly 64 KiB is read and answered by its page; one byte more is refused with
/// 413 and the page that says what was refused, why and what to do, and its password is
/// never verified.
///
/// Proves: SPEC 5.3 (request bodies are limited to 64 KiB), decision 110 (413), decision 102.
#[tokio::test]
async fn a_form_larger_than_64_kib_is_refused_with_413() {
    assert_eq!(MAX_FORM_BYTES, 64 * 1024, "the limit in bytes");
    let app = App::with_book().await;
    let fits = app.client().send(form_of(MAX_FORM_BYTES)).await;
    assert_eq!(
        fits.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a sign-in form of exactly 64 KiB"
    );
    let verified = app.work.verifications();

    let too_large = app.client().send(form_of(MAX_FORM_BYTES + 1)).await;
    assert_eq!(
        too_large.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "status of a sign-in form of 64 KiB and a byte"
    );
    assert!(
        too_large.shows(&Refusal::FormTooLarge.to_string()),
        "the 413 does not show its page:\n{}",
        too_large.body
    );
    assert!(
        too_large.body.contains("<title>Nage</title>"),
        "the 413 page is not on the base layout:\n{}",
        too_large.body
    );
    assert_eq!(
        app.work.verifications(),
        verified,
        "passwords verified for a form that was too large"
    );

    // A body that declares no length is read up to the limit and no further, and gets the
    // same page.
    let undeclared = |bytes| form_of(bytes).header("transfer-encoding", "chunked");
    let fits = app.client().send(undeclared(MAX_FORM_BYTES)).await;
    assert_eq!(
        fits.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a sign-in form of exactly 64 KiB that declares no length"
    );
    let too_large = app.client().send(undeclared(MAX_FORM_BYTES + 1)).await;
    assert_eq!(
        too_large.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "status of a sign-in form of 64 KiB and a byte that declares no length"
    );
    assert!(
        too_large.shows(&Refusal::FormTooLarge.to_string()),
        "the 413 of a form that declares no length does not show its page:\n{}",
        too_large.body
    );
}

/// The body limit is the outermost of the three rules: a form that is both too large and
/// from another site gets the 413, not the Origin check's 403, and one that is too large and
/// has no session gets the 413, not the 303 to the sign-in page.
///
/// Proves: decision 110 (the body limit is the outermost of the three layers, before the Origin
/// check and the session rule).
#[tokio::test]
async fn the_body_limit_comes_before_the_origin_check_and_the_session_rule() {
    let app = App::with_book().await;
    let cross_site = app
        .client()
        .send(form_of(MAX_FORM_BYTES + 1).header("origin", "http://evil.example"))
        .await;
    assert_eq!(
        cross_site.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "status of a cross-site form that is too large"
    );
    let no_session = app
        .client()
        .send(Req::form("/account/password", &[]).body(vec![b'x'; MAX_FORM_BYTES + 1]))
        .await;
    assert_eq!(
        no_session.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "status of a form without a session that is too large"
    );
}
