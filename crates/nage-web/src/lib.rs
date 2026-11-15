//! The axum application: routes, auth middleware, templates, htmx partials and
//! embedded static assets (SPEC 11). Milestone 1 slice 2 served `/`, `/health` and
//! `/static/*`; milestone 2 slice 5 adds the way in (SPEC 5.3, decisions 105 to 112): the
//! set-up page, signing in and out, the change of one's own password, and the three rules
//! every request meets first (the body limit, the Origin check, the session rule). Only the
//! `nage` binary reads the environment and the clock; this crate is handed values
//! (decisions 42, 88 and 113).

mod account;
mod assets;
mod clock;
mod cookie;
mod error;
mod forms;
mod hashing;
mod health;
mod limits;
mod lockout;
mod login;
pub mod messages;
mod middleware;
mod origin;
mod pages;
mod session;
mod setup;
mod static_files;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef};
use axum::routing::{get, post};
use nage_core::storage::Storage;

pub use assets::{Asset, asset_url, assets};
pub use clock::Clock;
pub use error::AppError;
pub use hashing::{Hashing, MAX_CONCURRENT_HASHES};
pub use health::HealthProbe;
pub use session::{TOUCH_AFTER_SECONDS, TOUCH_WAIT};

/// The `Content-Security-Policy` header value of SPEC 5.3, set on every response (decision 40).
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'; base-uri 'self'";

/// The name of the session cookie (SPEC 5.3).
pub const SESSION_COOKIE: &str = "nage_session";

/// The largest request body a form may have, in bytes (SPEC 5.3: 64 KiB); a larger one is
/// refused with 413 (decision 110). Upload routes and their own limit arrive with the import.
pub const MAX_FORM_BYTES: usize = 65_536;

/// What `serve` hands the application (decisions 42, 88 and 113): nage-web reads no
/// environment and no clock of its own. Tests hand a storage in a temporary directory, a clock
/// they set and a hashing double that counts.
#[derive(Clone)]
pub struct AppState {
    /// The workspace version, reported by `/health`.
    pub version: &'static str,
    /// The check behind `/health`.
    pub health: HealthProbe,
    /// The database, opened and migrated by the caller.
    pub storage: Storage,
    /// The instant of every stamp, session and pause (decisions 88 and 108).
    pub clock: Clock,
    /// How passwords are hashed and verified, with the dummy hash (decision 107).
    pub hashing: Hashing,
    /// Whether the reverse proxy's `X-Forwarded-Proto` and `X-Forwarded-Host` are believed
    /// (SPEC 5.2; decisions 108 and 110).
    pub trust_proxy: bool,
}

/// What every handler and layer can reach: the handed values and what one router remembers
/// between requests, the pauses after wrong passwords and whether a user exists. A new router
/// starts with neither, as a restart does (decisions 109 and 111).
#[derive(Clone)]
pub(crate) struct Ctx {
    pub(crate) state: AppState,
    /// The failed attempts per username (decision 111).
    pub(crate) lockout: lockout::Lockout,
    /// Set once a user is known to exist and never cleared, since users are never deleted
    /// (decision 109).
    pub(crate) has_user: Arc<AtomicBool>,
}

impl Ctx {
    /// The context of a new router over `state`: no pause, no user known yet.
    pub(crate) fn new(state: AppState) -> Self {
        Self {
            state,
            lockout: lockout::Lockout::new(),
            has_user: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl FromRef<Ctx> for AppState {
    fn from_ref(ctx: &Ctx) -> Self {
        ctx.state.clone()
    }
}

/// The whole application (SPEC 5.3, decisions 105 to 112): `/setup`, `/login`, `/logout`
/// (POST), `/account/password`, the placeholder `/`, `/health` and `/static/{*name}`. Every
/// request, one for an unknown address included, passes five layers, outermost first: the
/// request log (decision 47), the security headers (decision 40), the body limit, the Origin
/// check (decision 110) and the session rule (decision 109); so a 404, a 405 and every refusal
/// carry the headers and are logged, a cross-site form post is refused before the session rule
/// looks at it, and only `/setup`, `/login`, `/health` and `/static/*` answer without a
/// session (INV-27). Ready for `axum::serve` or `tower::ServiceExt::oneshot`.
pub fn router(state: AppState) -> Router {
    let ctx = Ctx::new(state);
    let routes = guard(
        Router::new()
            .route("/", get(pages::get_index))
            .route("/health", get(health::get_health))
            .route("/static/{*name}", get(static_files::get_static))
            .route("/setup", get(setup::get_setup).post(setup::post_setup))
            .route("/login", get(login::get_login).post(login::post_login))
            .route("/logout", post(login::post_logout))
            .route(
                "/account/password",
                get(account::get_password).post(account::post_password),
            ),
        &ctx,
    );
    routes
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(axum::middleware::from_fn(middleware::request_log))
        .with_state(ctx)
}

/// Puts the three inner layers around `routes`: the body limit outermost, then the Origin
/// check, then the session rule (decision 110). [`MAX_FORM_BYTES`] is also what a form
/// extractor reads at most, for a body that does not declare its length.
fn guard(routes: Router<Ctx>, ctx: &Ctx) -> Router<Ctx> {
    routes
        .layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            session::session_rule,
        ))
        .layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            origin::origin_check,
        ))
        .layer(axum::middleware::from_fn(limits::body_limit))
        .layer(DefaultBodyLimit::max(MAX_FORM_BYTES))
}
