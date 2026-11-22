//! The axum application: routes, auth middleware, templates, htmx partials and
//! embedded static assets (SPEC 11). Milestone 1 slice 2 served `/`, `/health` and
//! `/static/*`; milestone 2 slice 5 adds the way in (SPEC 5.3, decisions 105 to 112): the
//! set-up page, signing in and out, the change of one's own password, and the three rules
//! every request meets first (the body limit, the Origin check, the session rule); slice 6
//! adds the frame (decisions 116 to 120): the layout's bar with its menu, one-time messages,
//! the page for an unknown address, the answer to a person already signed in who opens the
//! sign-in page, and the home page with the balances as of today; slice 6a adds the look: the
//! fonts and the icons Nage serves, and the appearance switch with its route `POST /theme` and
//! its cookie; slice 7 adds the account pages under `/accounts` (decisions 128 to 139): the
//! list, creating, an account's page, changing it with the clash of two saves, archiving and
//! unarchiving, the menu's Accounts entry and the account messages. Only the `nage` binary
//! reads the environment and the clock; this crate is handed
//! values (decisions 42, 88, 113 and 120).

mod account;
mod accounts;
mod assets;
mod clock;
mod cookie;
mod error;
mod flash;
mod forms;
mod hashing;
mod health;
mod home;
mod limits;
mod lockout;
mod login;
pub mod messages;
mod middleware;
mod not_found;
mod origin;
mod pages;
mod session;
mod setup;
mod static_files;
mod theme;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef};
use axum::routing::{get, post};
use jiff::tz::TimeZone;
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

/// The name of the cookie a one-time message travels in (decision 117): beside
/// [`SESSION_COOKIE`], holding a short code from a fixed set, never text.
pub const MESSAGE_COOKIE: &str = "nage_message";

/// The name of the cookie that keeps a browser's chosen appearance: `light` or `dark`, never
/// any other value; without it the pages follow the device's setting.
pub const THEME_COOKIE: &str = "nage_theme";

/// The largest request body a form may have, in bytes (SPEC 5.3: 64 KiB); a larger one is
/// refused with 413 (decision 110). Upload routes and their own limit arrive with the import.
pub const MAX_FORM_BYTES: usize = 65_536;

/// What `serve` hands the application (decisions 42, 88, 113 and 120): nage-web reads no
/// environment and no clock of its own. Tests hand a storage in a temporary directory, a clock
/// they set, a hashing double that counts and a zone they name.
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
    /// The configured time zone (SPEC 5.2: `--tz`, `NAGE_TZ`, `TZ`, else UTC; decision 43),
    /// in which today is read from the clock at each request that needs it (decision 120).
    pub zone: TimeZone,
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

/// The whole application (SPEC 5.3, decisions 105 to 112, 116 to 120 and 128): `/setup`,
/// `/login`, `/logout` (POST), `/account/password`, `/theme` (POST, the appearance switch), the
/// home page `/`, the account pages under `/accounts` (`accounts`), `/health` and
/// `/static/{*name}`, and for every other address the fallback, the
/// page of the 404 (decision 118). Every request, one for an unknown address included, passes
/// six layers, outermost first: the request log (decision 47), the security headers
/// (decision 40), the appearance (`theme::wear`, which reads the `nage_theme` cookie for every
/// page the request renders), the body limit, the Origin check (decision 110) and the session
/// rule (decision 109); the fallback is added before the three inner layers, so they cover it
/// like any route. So a 404, a 405 and every refusal carry the headers and are logged, every
/// page wears the chosen palette, a cross-site form post is refused before the session rule
/// looks at it, and only `/setup`, `/login`, `/health` and `/static/*` answer without a
/// session (INV-27). Ready for `axum::serve` or `tower::ServiceExt::oneshot`.
pub fn router(state: AppState) -> Router {
    let ctx = Ctx::new(state);
    let routes = guard(
        Router::new()
            .route("/", get(home::get_home))
            .route("/health", get(health::get_health))
            .route("/static/{*name}", get(static_files::get_static))
            .route("/setup", get(setup::get_setup).post(setup::post_setup))
            .route("/login", get(login::get_login).post(login::post_login))
            .route("/logout", post(login::post_logout))
            .route(
                "/account/password",
                get(account::get_password).post(account::post_password),
            )
            .route("/theme", post(theme::post_theme))
            .route("/accounts", get(accounts::get_list))
            .route(
                "/accounts/new",
                get(accounts::get_new).post(accounts::post_new),
            )
            .route("/accounts/{id}", get(accounts::get_account))
            .route(
                "/accounts/{id}/edit",
                get(accounts::get_edit).post(accounts::post_edit),
            )
            .route(
                "/accounts/{id}/archive",
                get(accounts::get_archive).post(accounts::post_archive),
            )
            .route(
                "/accounts/{id}/unarchive",
                get(accounts::get_unarchive).post(accounts::post_unarchive),
            )
            .fallback(not_found::not_found),
        &ctx,
    );
    routes
        .layer(axum::middleware::from_fn(theme::wear))
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
