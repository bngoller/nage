//! The body limit (SPEC 5.3, decision 110): the outermost of the three inner layers, so a
//! body that is too large is refused before the Origin check and the session rule look at
//! the request.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Refuses a request whose `Content-Length` is larger than `MAX_FORM_BYTES` with 413 and the
/// page that says what was refused, why and what to do (decision 102), without reading the
/// body; every other request goes on. A body that declares no length is cut off at the limit
/// by the form extractor (`forms::unreadable`).
pub(crate) async fn body_limit(request: Request, next: Next) -> Response {
    if declared_length(request.headers()).is_some_and(|length| length > MAX_FORM_BYTES) {
        return too_large();
    }
    next.run(request).await
}

use axum::http::{HeaderMap, StatusCode, header};

use crate::MAX_FORM_BYTES;
use crate::messages::Refusal;
use crate::pages::{Layout, RefusalPage, TOO_LARGE_HEADING, render_with};

/// The body length `Content-Length` declares, or `None` when there is none or it is not a
/// number; such a body is left to the form extractor's own limit. A length beyond what the
/// machine can count is larger than any limit.
fn declared_length(headers: &HeaderMap) -> Option<usize> {
    let text = headers.get(header::CONTENT_LENGTH)?.to_str().ok()?;
    let length: u64 = text.trim().parse().ok()?;
    Some(usize::try_from(length).unwrap_or(usize::MAX))
}

/// The answer to a form larger than [`MAX_FORM_BYTES`]: 413 with the page that says what was
/// refused, why and what to do (decisions 102 and 110).
pub(crate) fn too_large() -> Response {
    render_with(
        StatusCode::PAYLOAD_TOO_LARGE,
        &RefusalPage {
            layout: Layout::embedded(),
            heading: TOO_LARGE_HEADING,
            message: Refusal::FormTooLarge.to_string(),
        },
    )
}
