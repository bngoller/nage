//! The embedded static files and their content-hashed URLs (decision 46).

use std::fmt::Write as _;
use std::sync::LazyLock;

use sha2::{Digest, Sha256};

/// The `Content-Type` of the JavaScript file.
const JAVASCRIPT: &str = "text/javascript; charset=utf-8";

/// The `Content-Type` of the stylesheets.
const CSS: &str = "text/css; charset=utf-8";

/// An embedded static file served at a content-hashed URL (decision 46).
pub struct Asset {
    /// The file name in the source tree, for example `app.css`.
    pub name: &'static str,
    /// `/static/<stem>.<hash>.<ext>`.
    pub url: String,
    /// The `Content-Type` the file is served with.
    pub content_type: &'static str,
    /// The file's bytes, exactly as in the source tree.
    pub body: &'static [u8],
}

impl Asset {
    /// Embeds `body` under its content-hashed URL.
    fn embed(name: &'static str, content_type: &'static str, body: &'static [u8]) -> Self {
        Self {
            name,
            url: hashed_url(name, body),
            content_type,
            body,
        }
    }
}

/// The three files, in the order htmx, Pico CSS, `app.css`; `embedded` relies on it.
static ASSETS: LazyLock<[Asset; 3]> = LazyLock::new(|| {
    [
        Asset::embed(
            "htmx.min.js",
            JAVASCRIPT,
            include_bytes!("../static/vendor/htmx.min.js"),
        ),
        Asset::embed(
            "pico.classless.min.css",
            CSS,
            include_bytes!("../static/vendor/pico.classless.min.css"),
        ),
        Asset::embed("app.css", CSS, include_bytes!("../static/app.css")),
    ]
});

/// The embedded static files.
pub fn assets() -> &'static [Asset] {
    embedded()
}

/// The content-hashed URL of an embedded file, by file name: `"app.css"` gives
/// `Some("/static/app.<hash>.css")`.
pub fn asset_url(name: &str) -> Option<&'static str> {
    assets()
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.url.as_str())
}

/// The embedded files as htmx, Pico CSS and `app.css`, for the base layout.
pub(crate) fn embedded() -> &'static [Asset; 3] {
    &ASSETS
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
