//! `GET /static/<stem>.<hash>.<ext>`: the embedded files, cached forever (decision 46).

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

use crate::assets;

/// The `Cache-Control` of a content-hashed asset: its URL changes with its content.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Serves the embedded file whose hashed URL is exactly the request path; every other path
/// under `/static/` (the plain name, a stale hash, an unknown file) is 404.
pub(crate) async fn get_static(uri: Uri) -> Response {
    match assets::by_url(uri.path()) {
        Some(asset) => (
            [
                (header::CONTENT_TYPE, asset.content_type),
                (header::CACHE_CONTROL, IMMUTABLE),
            ],
            asset.body,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
