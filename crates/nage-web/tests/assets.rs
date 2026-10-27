//! Embedded static files served at content-hashed URLs.

mod common;

use axum::http::StatusCode;

use common::{
    ASSET_NAMES, IMMUTABLE, asset, body_bytes, content_type_for, get, hashed_url, header,
    sha256_hex, sha256_hex16, source_bytes, state,
};

/// SHA-256 of `htmx.min.js` as recorded in `static/vendor/VENDORED.md` (htmx v2.0.11).
const HTMX_SHA256: &str = "d6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717";

/// SHA-256 of `pico.classless.min.css` as recorded in `static/vendor/VENDORED.md` (Pico v2.1.1).
const PICO_SHA256: &str = "61207a40ffc02a42d1e50143651c121beab70ed413c934c1ff84fa263ba436b0";

/// Whether `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Each of the three files is embedded under `/static/<stem>.<hash>.<ext>`, the hash being
/// the first 16 hex characters of its content's SHA-256, and `asset_url` agrees.
///
/// Cites: decision 46.
#[test]
fn assets_are_embedded_under_content_hashed_urls() {
    for name in ASSET_NAMES {
        let embedded = asset(name);
        let expected = hashed_url(name, embedded.body);
        assert_eq!(embedded.url, expected, "url of the embedded {name}");
        assert_eq!(
            nage_web::asset_url(name),
            Some(expected.as_str()),
            "asset_url({name:?})"
        );
    }
    assert_eq!(
        nage_web::assets().len(),
        ASSET_NAMES.len(),
        "number of embedded assets"
    );
    assert_eq!(
        nage_web::asset_url("missing.css"),
        None,
        "asset_url of a file that is not embedded"
    );
}

/// A hashed URL answers 200 with the content type of its extension, the immutable
/// cache header and exactly the bytes of the source file.
///
/// Cites: decision 46.
#[tokio::test]
async fn hashed_asset_urls_serve_the_bytes_with_cache_headers() {
    for name in ASSET_NAMES {
        let url = hashed_url(name, asset(name).body);
        let response = get(state(true), &url).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
        assert_eq!(
            header(&response, "content-type").as_deref(),
            Some(content_type_for(name)),
            "GET {url} content-type"
        );
        assert_eq!(
            header(&response, "cache-control").as_deref(),
            Some(IMMUTABLE),
            "GET {url} cache-control"
        );
        let served = body_bytes(response).await;
        let source = source_bytes(name);
        assert!(
            served == source,
            "GET {url} does not serve the bytes of {name}: {} bytes served, {} in the source file",
            served.len(),
            source.len()
        );
    }
}

/// Only the hashed URL serves an asset: the plain name, a stale hash, an unknown stem, the
/// full-length hash, the source path and `/static/` itself are 404.
///
/// Cites: decision 46.
#[tokio::test]
async fn only_the_hashed_url_serves_an_asset() {
    let body = asset("app.css").body;
    let hash = sha256_hex16(body);
    let hashed = format!("/static/app.{hash}.css");
    let response = get(state(true), &hashed).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {hashed} status");

    let changed = if hash.starts_with('0') { '1' } else { '0' };
    let stale = format!("/static/app.{changed}{}.css", &hash[1..]);
    let unknown = format!("/static/unknown.{hash}.css");
    let full = format!("/static/app.{}.css", sha256_hex(body));
    let not_found = [
        "/static/app.css",
        stale.as_str(),
        unknown.as_str(),
        full.as_str(),
        "/static/vendor/htmx.min.js",
        "/static/",
        "/static",
    ];
    for uri in not_found {
        let response = get(state(true), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
    }
}

/// The embedded vendored files are the ones `VENDORED.md` records: their SHA-256 matches, the
/// htmx file is on the 2.x line and the Pico file is Pico.
///
/// Cites: decision 46.
#[test]
fn vendored_assets_match_their_recorded_hashes() {
    let htmx = asset("htmx.min.js").body;
    assert_eq!(
        sha256_hex(htmx),
        HTMX_SHA256,
        "SHA-256 of the embedded htmx.min.js against VENDORED.md"
    );
    let pico = asset("pico.classless.min.css").body;
    assert_eq!(
        sha256_hex(pico),
        PICO_SHA256,
        "SHA-256 of the embedded pico.classless.min.css against VENDORED.md"
    );
    assert!(
        contains(htmx, br#"version:"2."#),
        "the embedded htmx.min.js does not contain `version:\"2.`"
    );
    assert!(
        contains(pico, b"--pico-"),
        "the embedded pico.classless.min.css does not contain `--pico-`"
    );
}
