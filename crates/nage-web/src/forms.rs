//! What the three forms of milestone 2 slice 5 share (decision 105): each is a plain POST,
//! read as a URL-encoded body of at most 64 KiB; a field that is missing counts as empty, so
//! that a submission is checked whole and every problem is shown at once.

use axum::extract::rejection::FormRejection;
use axum::response::Response;

/// The answer to a form body that could not be read at all: the 413 page of decision 110
/// when the body is larger than `MAX_FORM_BYTES` (a body that declared no length, since the
/// body limit refuses a declared one first), and axum's own answer otherwise (415 for
/// another content type, 400 or 422 for a body that is not a form).
pub(crate) fn unreadable(rejection: FormRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        too_large()
    } else {
        rejection.into_response()
    }
}

use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::limits::too_large;
