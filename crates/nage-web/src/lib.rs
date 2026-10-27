//! The axum application: routes, auth middleware, templates, htmx partials and
//! embedded static assets (SPEC 11). Milestone 1 slice 2 serves `/`, `/health` and
//! `/static/*`. Only the `nage` binary reads the environment; this crate is handed
//! values (decision 42).

mod assets;
mod health;
mod middleware;
mod pages;
mod static_files;

use axum::Router;
use axum::routing::get;

pub use assets::{Asset, asset_url, assets};
pub use health::HealthProbe;

/// The `Content-Security-Policy` header value of SPEC 5.3, set on every response (decision 40).
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'; base-uri 'self'";

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub version: &'static str,
    pub health: HealthProbe,
}

/// Routes `/`, `/health`, `/static/{*name}` with the security-headers and request-log
/// middlewares applied after the routes (so the 404 fallback gets both). The request log is
/// outermost, so its latency covers the headers layer too. Ready for `axum::serve` or
/// `tower::ServiceExt::oneshot`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::get_index))
        .route("/health", get(health::get_health))
        .route("/static/{*name}", get(static_files::get_static))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(axum::middleware::from_fn(middleware::request_log))
        .with_state(state)
}
