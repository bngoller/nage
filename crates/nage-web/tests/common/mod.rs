#![allow(dead_code)]
//! Helpers shared by the nage-web integration tests. Requests go through the real
//! `nage_web::router` in process with `tower::ServiceExt::oneshot`; nothing binds a port.
//! Temporary directories and storage opened in them come from `nage_core::test_support`
//! (decision 59). The states of this file and `get` serve the tests of `/health` and
//! `/static/*`, the two routes the session rule passes unread: it never reads the database for
//! them, and `/health` reads it only through the storage probe a test hands in (`storage_state`).

use std::fmt::Write as _;
use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use nage_core::storage::Storage;
use nage_core::test_support::{CHEAP_HASH_PARAMS, TestDir, open_fresh};
use nage_core::timestamp::Timestamp;
use nage_web::{AppState, Asset, Clock, Hashing, HealthProbe};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;
use tracing::Level;

pub mod app;

/// The version the tests put in the app state.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The nine served files of decisions 46, 125 and 126, by the name each is served under:
/// htmx, the style file, the two icons, the four IBM Plex files and the fonts' licence text.
pub const ASSET_NAMES: [&str; 9] = [
    "htmx.min.js",
    "app.css",
    "mark.svg",
    "home-screen.png",
    "IBMPlexSans-Regular-Latin1.woff2",
    "IBMPlexSans-SemiBold-Latin1.woff2",
    "IBMPlexMono-Regular-Latin1.woff2",
    "IBMPlexMono-SemiBold-Latin1.woff2",
    "LICENSE-ibm-plex.txt",
];

/// The `Content-Security-Policy` value of SPEC 5.3, copied from the spec text rather than
/// taken from `nage_web::CONTENT_SECURITY_POLICY`, so a drifted constant fails the tests.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'; base-uri 'self'";

/// The `Cache-Control` value of a content-hashed asset (decision 46).
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The deadline `serve` gives the storage probe (decision 55: one second).
pub const PROBE_DEADLINE: Duration = Duration::from_secs(1);

/// The instant the clock of a [`Given`] state always shows: 2027-01-15T08:00:00Z.
pub const GIVEN_NOW: i64 = 1_800_000_000;

/// What a test of `/health` or `/static/*` hands [`get`]: the health probe, and the storage
/// when the test opened one itself. The rest of the state is the same for all of them: a
/// clock that stands at [`GIVEN_NOW`], the cheap hashing set and trust-proxy off.
pub struct Given {
    health: HealthProbe,
    storage: Option<Storage>,
}

impl Given {
    /// The router over this state. Without a storage of the test's own, a database is opened
    /// in a fresh temporary directory, which is returned so that it lives as long as the
    /// router is used; nothing is migrated, since these routes never read it.
    pub async fn router(self) -> (Router, Option<TestDir>) {
        let (dir, storage) = match self.storage {
            Some(storage) => (None, storage),
            None => {
                let (dir, storage) = open_fresh().await;
                (Some(dir), storage)
            }
        };
        let router = nage_web::router(AppState {
            version: VERSION,
            health: self.health,
            storage,
            clock: Clock::new(|| Timestamp::from_second(GIVEN_NOW)),
            hashing: Hashing::argon2id(CHEAP_HASH_PARAMS),
            trust_proxy: false,
            zone: jiff::tz::TimeZone::UTC,
        });
        (router, dir)
    }
}

/// A state whose health probe is `health`.
pub fn probe_state(health: HealthProbe) -> Given {
    Given {
        health,
        storage: None,
    }
}

/// A state whose health probe always answers `ok`.
pub fn state(ok: bool) -> Given {
    probe_state(HealthProbe::always(ok))
}

/// A state whose health probe is the production one, `HealthProbe::storage` over `storage`
/// with the one-second deadline (decision 55), and whose storage is `storage`.
pub fn storage_state(storage: Storage) -> Given {
    Given {
        health: HealthProbe::storage(storage.clone(), PROBE_DEADLINE),
        storage: Some(storage),
    }
}

/// A state whose health probe answers `ok` and counts how often it runs.
pub fn counting_state(ok: bool) -> (Given, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let health = HealthProbe::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        std::future::ready(ok)
    });
    (probe_state(health), calls)
}

/// Sends `GET uri` through a router built from `given`.
pub async fn get(given: Given, uri: &str) -> Response {
    let (router, _dir) = given.router().await;
    get_via(&router, uri).await
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

/// The path under `crates/nage-web/` of the source file that the asset served as `name`
/// embeds: the style file under `static/`, the icons under `static/icons/`, the fonts' licence
/// text as `static/vendor/LICENSE-ibm-plex` (it keeps its name on disk, decision 126) and every
/// other file under `static/vendor/`.
pub fn source_path(name: &str) -> String {
    match name {
        "app.css" => "static/app.css".to_owned(),
        "mark.svg" | "home-screen.png" | "home-screen.svg" => format!("static/icons/{name}"),
        "LICENSE-ibm-plex.txt" => "static/vendor/LICENSE-ibm-plex".to_owned(),
        _ => format!("static/vendor/{name}"),
    }
}

/// The bytes of the file at `relative` under `crates/nage-web/`.
pub fn crate_file(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The three drawings of Nage's mark (decision 125): the template drawn inline in every page,
/// the browser tab's icon and the source of the home-screen icon, by their paths under
/// `crates/nage-web/`.
pub const MARK_DRAWINGS: [&str; 3] = [
    "templates/mark.html",
    "static/icons/mark.svg",
    "static/icons/home-screen.svg",
];

/// The tile every drawing of the mark holds, exactly as each draws it.
pub const MARK_TILE: &str = r#"<rect x="1" y="1" width="30" height="30" rx="8""#;

/// The path data (`d="..."`) of every path in `svg`, in order.
pub fn path_data(svg: &str) -> Vec<&str> {
    svg.match_indices(" d=\"")
        .map(|(at, open)| {
            let rest = &svg[at + open.len()..];
            rest.split('"').next().unwrap_or("")
        })
        .collect()
}

/// Asserts that the three drawings of the mark draw the same tile and the same tilde, one path
/// each with the same path data, so the tab, the home screen and the pages show one mark
/// (decision 125). `what` begins the failure message.
pub fn assert_one_mark(what: &str) {
    let mut tilde: Option<String> = None;
    for file in MARK_DRAWINGS {
        let text = String::from_utf8(crate_file(file))
            .unwrap_or_else(|error| panic!("{file} is not UTF-8: {error}"));
        assert!(
            text.contains(MARK_TILE),
            "{what}: {file} does not draw the mark's tile {MARK_TILE}:\n{text}"
        );
        let paths = path_data(&text);
        let [path] = paths.as_slice() else {
            panic!("{what}: {file} does not draw the tilde as one path: {paths:?}");
        };
        match &tilde {
            None => tilde = Some((*path).to_owned()),
            Some(first) => assert_eq!(
                path, first,
                "{what}: {file} draws another tilde than {}",
                MARK_DRAWINGS[0]
            ),
        }
    }
}

/// The bytes of the source file in the repository that asset `name` embeds.
pub fn source_bytes(name: &str) -> Vec<u8> {
    crate_file(&source_path(name))
}

/// The `Content-Type` an asset is served with, by the extension of the name it is served
/// under (decisions 46, 125 and 126).
pub fn content_type_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext {
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "txt" => "text/plain; charset=utf-8",
        _ => panic!("no expected content type for {name}"),
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

/// Asserts the three response headers of decision 40 (SPEC 5.3), each exactly once with its
/// exact value, and not the fourth, `Cache-Control`, which a served asset sets itself
/// (decision 119). `what` names the response in the failure message.
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

/// Lines of `logs` written at `level` (as the `fmt` subscriber prints it) that contain `needle`
/// (an empty `needle` matches every line at `level`).
pub fn lines_at<'a>(logs: &'a str, level: &str, needle: &str) -> Vec<&'a str> {
    let level = format!(" {level} ");
    logs.lines()
        .filter(|line| line.contains(&level) && line.contains(needle))
        .collect()
}

/// Runs the future `request` builds on a fresh current-thread runtime (with a time driver,
/// since the deadline probe uses `tokio::time::timeout` and sqlx's pools use timers), with a
/// `fmt` subscriber (ANSI off, at most `max_level`) as this thread's default for the duration,
/// and returns its output with everything the subscriber wrote. The subscriber is scoped to
/// the calling thread and no global subscriber is installed, but tracing caches each
/// callsite's interest process-wide: a callsite first reached on another thread while this
/// subscriber is the only registered one caches `never`. A capture is therefore reliable only
/// in a test binary where no other test runs (`tests/request_log.rs`, `tests/health_log.rs`, `tests/error_log.rs`).
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
        .enable_time()
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
