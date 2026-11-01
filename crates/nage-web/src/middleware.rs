//! Middleware applied to every response, the 404 fallback included.

use std::time::Instant;

use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::CONTENT_SECURITY_POLICY;

/// Sets the three response headers of SPEC 5.3 on every response, replacing any value a
/// handler set, so each appears exactly once (decision 40).
pub(crate) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
}

/// Logs one `request` event per response with `method`, `path`, `status` and `latency_ms`
/// (SPEC 5.2, decision 47). The path is the URI path without the query string; headers,
/// cookies and bodies are never read. The event is at `info`, except that a 2xx response to
/// `/health` is at `debug`, so a passing container health check stays out of the default
/// logs while a failing one shows.
pub(crate) async fn request_log(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    let status = response.status();
    let code = status.as_u16();
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if path == "/health" && status.is_success() {
        tracing::debug!(method = %method, path = %path, status = code, latency_ms, "request");
    } else {
        tracing::info!(method = %method, path = %path, status = code, latency_ms, "request");
    }
    response
}
