#![allow(dead_code)]
//! Helpers shared by the nage-web integration tests. Requests go through the real
//! `nage_web::router` in process with `tower::ServiceExt::oneshot`; nothing binds a port.

use std::fmt::Write as _;
use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use nage_web::{AppState, Asset, HealthProbe};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;
use tracing::Level;

/// The version the tests put in the app state.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The three embedded files of decision 46, by file name.
pub const ASSET_NAMES: [&str; 3] = ["htmx.min.js", "pico.classless.min.css", "app.css"];

/// The `Content-Security-Policy` value of SPEC 5.3, copied from the spec text rather than
/// taken from `nage_web::CONTENT_SECURITY_POLICY`, so a drifted constant fails the tests.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'; base-uri 'self'";

/// The `Cache-Control` value of a content-hashed asset (decision 46).
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// App state whose health probe always answers `ok`.
pub fn state(ok: bool) -> AppState {
    AppState {
        version: VERSION,
        health: HealthProbe::always(ok),
    }
}

/// App state whose health probe answers `ok` and counts how often it runs.
pub fn counting_state(ok: bool) -> (AppState, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let health = HealthProbe::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        std::future::ready(ok)
    });
    let state = AppState {
        version: VERSION,
        health,
    };
    (state, calls)
}

/// Sends `GET uri` through a router built from `state`.
pub async fn get(state: AppState, uri: &str) -> Response {
    get_via(&nage_web::router(state), uri).await
}

/// Sends `GET uri` through an existing router, so several requests can share one instance.
pub async fn get_via(router: &Router, uri: &str) -> Response {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("cannot build GET {uri}: {error}"));
    match router.clone().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// The whole response body.
pub async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|error| panic!("cannot read the response body: {error}"))
        .to_vec()
}

/// The whole response body as UTF-8 text.
pub async fn body_text(response: Response) -> String {
    let bytes = body_bytes(response).await;
    String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("the response body is not UTF-8: {error}"))
}

/// The first value of header `name`, if present.
pub fn header(response: &Response, name: &str) -> Option<String> {
    response.headers().get(name).map(|value| {
        value
            .to_str()
            .unwrap_or_else(|error| panic!("header {name} is not visible ASCII: {error}"))
            .to_owned()
    })
}

/// Lowercase hex of the SHA-256 of `bytes`, encoded by hand.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest.iter() {
        write!(hex, "{byte:02x}").unwrap_or_else(|error| panic!("cannot write hex: {error}"));
    }
    hex
}

/// The first 16 lowercase hex characters of the SHA-256 of `bytes` (decision 46).
pub fn sha256_hex16(bytes: &[u8]) -> String {
    let mut hex = sha256_hex(bytes);
    hex.truncate(16);
    hex
}

/// The embedded asset called `name`.
pub fn asset(name: &str) -> &'static Asset {
    nage_web::assets()
        .iter()
        .find(|asset| asset.name == name)
        .unwrap_or_else(|| panic!("asset {name} is not embedded"))
}

/// The URL decision 46 gives file `name` with content `body`:
/// `/static/<stem>.<first 16 hex of SHA-256>.<ext>`, the stem being the name before its
/// last extension.
pub fn hashed_url(name: &str, body: &[u8]) -> String {
    let (stem, ext) = name
        .rsplit_once('.')
        .unwrap_or_else(|| panic!("asset name {name} has no extension"));
    format!("/static/{stem}.{}.{ext}", sha256_hex16(body))
}

/// The bytes of the source file in the repository that asset `name` embeds.
pub fn source_bytes(name: &str) -> Vec<u8> {
    let relative = if name == "app.css" {
        "static/app.css".to_owned()
    } else {
        format!("static/vendor/{name}")
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The `Content-Type` an asset is served with, by its extension.
pub fn content_type_for(name: &str) -> &'static str {
    if name.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if name.ends_with(".css") {
        "text/css; charset=utf-8"
    } else {
        panic!("no expected content type for {name}")
    }
}

/// Every start tag in `html` opening with `open` (for example `"<script"`), through its `>`.
pub fn start_tags<'a>(html: &'a str, open: &str) -> Vec<&'a str> {
    html.match_indices(open)
        .map(|(at, _)| {
            let rest = &html[at..];
            let end = rest.find('>').map_or(rest.len(), |index| index + 1);
            &rest[..end]
        })
        .collect()
}

/// Asserts the three response headers of SPEC 5.3, each exactly once with its exact value
/// (decision 40). `what` names the response in the failure message.
pub fn assert_security_headers(response: &Response, what: &str) {
    let expected = [
        ("content-security-policy", CSP),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
    ];
    for (name, value) in expected {
        assert_eq!(
            header(response, name).as_deref(),
            Some(value),
            "{name} on {what}"
        );
        assert_eq!(
            response.headers().get_all(name).iter().count(),
            1,
            "number of {name} headers on {what}"
        );
    }
}

/// Runs the future `request` builds on a fresh current-thread runtime, with a `fmt`
/// subscriber (ANSI off, at most `max_level`) as this thread's default for the duration, and
/// returns its output with everything the subscriber wrote. The subscriber is scoped to the
/// calling thread and no global subscriber is installed, but tracing caches each callsite's
/// interest process-wide: a callsite first reached on another thread while this subscriber
/// is the only registered one caches `never`. A capture is therefore reliable only in a test
/// binary where no other test runs (`tests/request_log.rs`).
pub fn capture_logs<F, Fut>(max_level: Level, request: F) -> (Fut::Output, String)
where
    F: FnOnce() -> Fut,
    Fut: Future,
{
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || SharedBuffer(Arc::clone(&sink)))
        .with_ansi(false)
        .with_max_level(max_level)
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"));
    let output = tracing::subscriber::with_default(subscriber, || runtime.block_on(request()));
    let bytes = buffer
        .lock()
        .unwrap_or_else(|error| panic!("log buffer poisoned: {error}"))
        .clone();
    let logs = String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("captured logs are not UTF-8: {error}"));
    (output, logs)
}

/// The writer `capture_logs` hands the subscriber: appends to a shared buffer.
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log buffer poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
