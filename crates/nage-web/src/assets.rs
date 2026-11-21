//! The embedded static files and their content-hashed URLs (decision 46).
//!
//! Every file lives under `crates/nage-web/static/` and is listed once in [`ASSETS`], one line
//! each. It is served at `/static/<stem>.<hash>.<ext>`, where `<stem>.<ext>` is the name it is
//! served under: its file name (the last segment of its path, so file names must be unique
//! across folders), or the name its line gives after `as` (the fonts' licence text,
//! `vendor/LICENSE-ibm-plex`, is served as `LICENSE-ibm-plex.txt`), and `<hash>` the first 16
//! lowercase hex characters of its content's SHA-256. The address therefore changes
//! only when the bytes change, and the style file states a font's address literally:
//! `printf '%.16s\n' "$(sha256sum vendor/x.woff2)"` gives the hash.

use std::fmt::Write as _;
use std::sync::LazyLock;

use sha2::{Digest, Sha256};

/// An embedded static file served at a content-hashed URL (decision 46).
pub struct Asset {
    /// The name the file is served under, for example `app.css`: its file name in the source
    /// tree, but `LICENSE-ibm-plex.txt` for `vendor/LICENSE-ibm-plex`.
    pub name: &'static str,
    /// `/static/<stem>.<hash>.<ext>`.
    pub url: String,
    /// The `Content-Type` the file is served with.
    pub content_type: &'static str,
    /// The file's bytes, exactly as in the source tree.
    pub body: &'static [u8],
}

impl Asset {
    /// Embeds `body`, read from `path` under `static/`, under its content-hashed URL.
    fn embed(path: &'static str, body: &'static [u8]) -> Self {
        let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
        Self::named(name, body)
    }

    /// Embeds `body` under the served name `name`, which gives its address and its content
    /// type, whatever the file's name on disk.
    fn named(name: &'static str, body: &'static [u8]) -> Self {
        Self {
            name,
            url: hashed_url(name, body),
            content_type: content_type(name),
            body,
        }
    }
}

/// One embedded file, by its path under `crates/nage-web/static/`, served under its own file
/// name, or under the name after `as` when the file on disk keeps a name without the extension
/// it is served with.
macro_rules! embed {
    ($path:literal) => {
        Asset::embed($path, include_bytes!(concat!("../static/", $path)))
    };
    ($path:literal as $name:literal) => {
        Asset::named($name, include_bytes!(concat!("../static/", $path)))
    };
}

/// Every file Nage serves: htmx, the style file, the two icons of the mark (the browser tab's
/// SVG and the home screen's 180 by 180 PNG), the four IBM Plex files the style file names and
/// their licence text, `vendor/LICENSE-ibm-plex`, served as `LICENSE-ibm-plex.txt` in plain
/// text and linked from no page (decision 126). Adding a file is one line here; its address is
/// then `/static/<stem>.<first 16 hex of its SHA-256>.<ext>`.
static ASSETS: LazyLock<Vec<Asset>> = LazyLock::new(|| {
    vec![
        embed!("vendor/htmx.min.js"),
        embed!("app.css"),
        embed!("icons/mark.svg"),
        embed!("icons/home-screen.png"),
        embed!("vendor/IBMPlexSans-Regular-Latin1.woff2"),
        embed!("vendor/IBMPlexSans-SemiBold-Latin1.woff2"),
        embed!("vendor/IBMPlexMono-Regular-Latin1.woff2"),
        embed!("vendor/IBMPlexMono-SemiBold-Latin1.woff2"),
        embed!("vendor/LICENSE-ibm-plex" as "LICENSE-ibm-plex.txt"),
    ]
});

/// The `Content-Type` of a file, by its extension. A new kind of file adds its line here.
fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The embedded static files.
pub fn assets() -> &'static [Asset] {
    &ASSETS
}

/// The content-hashed URL of an embedded file, by the name it is served under: `"app.css"`
/// gives `Some("/static/app.<hash>.css")`, `"LICENSE-ibm-plex.txt"` the fonts' licence text's.
pub fn asset_url(name: &str) -> Option<&'static str> {
    assets()
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.url.as_str())
}

/// The URL of a file the base layout links; its name is one of [`ASSETS`].
pub(crate) fn linked(name: &str) -> &'static str {
    asset_url(name).unwrap_or_else(|| panic!("{name} is not in the embedded files"))
}

/// The embedded file served at exactly `path`, if any.
pub(crate) fn by_url(path: &str) -> Option<&'static Asset> {
    assets().iter().find(|asset| asset.url == path)
}

/// `/static/<stem>.<hash>.<ext>`: the stem is the name before its last extension, the hash
/// the first 16 lowercase hex characters of the content's SHA-256.
fn hashed_url(name: &str, body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    let mut hash = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        // Writing to a String cannot fail.
        let _ = write!(hash, "{byte:02x}");
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("/static/{stem}.{hash}.{ext}"),
        None => format!("/static/{name}.{hash}"),
    }
}
