//! Embedded static files served at content-hashed URLs.

mod common;

use std::path::Path;

use axum::http::StatusCode;

use common::app::{ADMIN, App, PASSWORD};
use common::{
    ASSET_NAMES, IMMUTABLE, assert_one_mark, asset, body_bytes, content_type_for, crate_file, get,
    hashed_url, header, sha256_hex, sha256_hex16, source_bytes, state,
};

/// SHA-256 of `htmx.min.js` as recorded in `static/vendor/VENDORED.md` (htmx v2.0.11).
const HTMX_SHA256: &str = "d6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717";

/// The four IBM Plex files of decision 126: the name each is served under, the literal
/// address the style file names it at, its SHA-256 and its byte count, as `VENDORED.md`
/// records them.
const FONTS: [(&str, &str, &str, usize); 4] = [
    (
        "IBMPlexSans-Regular-Latin1.woff2",
        "/static/IBMPlexSans-Regular-Latin1.b5ad7bd39f996144.woff2",
        "b5ad7bd39f996144915f0ad9849a90183b27d8c28ad97ed98af5b1bebc51f6b1",
        20984,
    ),
    (
        "IBMPlexSans-SemiBold-Latin1.woff2",
        "/static/IBMPlexSans-SemiBold-Latin1.fff0ab3a88b0b4aa.woff2",
        "fff0ab3a88b0b4aa0b693e4f0201359a15183b08e3fa5696d1918d8f0ade8ad5",
        22260,
    ),
    (
        "IBMPlexMono-Regular-Latin1.woff2",
        "/static/IBMPlexMono-Regular-Latin1.10d3c7fa7eaf48e7.woff2",
        "10d3c7fa7eaf48e78db24f317b64f008a75e00f63a68bb3c2afc6ef51e58674f",
        17268,
    ),
    (
        "IBMPlexMono-SemiBold-Latin1.woff2",
        "/static/IBMPlexMono-SemiBold-Latin1.1ce95cff1c5056cb.woff2",
        "1ce95cff1c5056cb0fed049c2912823293b158b816e193a6f937f2d92b1e0f39",
        17712,
    ),
];

/// The four fonts' bytes in all (decision 126).
const FONT_BYTES: usize = 78_224;

/// The fonts' licence text: the name it is served under, its address, its SHA-256 and its
/// byte count (decision 126).
const LICENCE_NAME: &str = "LICENSE-ibm-plex.txt";
const LICENCE_URL: &str = "/static/LICENSE-ibm-plex.7e6b2818edbd8f6a.txt";
const LICENCE_SHA256: &str = "7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da";
const LICENCE_BYTES: usize = 4456;

/// The address the bytes of the home-screen icon's SVG source would give it, were it served
/// (decision 125: it is not).
const ICON_SOURCE_URL: &str = "/static/home-screen.e7520456121f12d4.svg";

/// The section of `VENDORED.md` that records htmx: from its heading to the next heading.
fn htmx_record(vendored: &str) -> &str {
    let start = vendored
        .find("## htmx.min.js\n")
        .unwrap_or_else(|| panic!("VENDORED.md has no section ## htmx.min.js:\n{vendored}"));
    let rest = &vendored[start..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3);
    &rest[..end]
}

/// Whether `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Each of the nine served files is embedded under `/static/<stem>.<hash>.<ext>`, the hash
/// being the first 16 hex characters of its content's SHA-256, and `asset_url` agrees.
///
/// Proves: decision 46.
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
/// Proves: decision 46.
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
/// full-length hash, the source path and `/static/` itself are 404, and so are a served address
/// with a slash or more text after it, the served address with its hash in capitals or its stem
/// in capitals, and the plain names and source paths of both icons and of the fonts' licence
/// text. (`/static` without the slash is no address under `/static/`: like any other address it
/// needs a session since milestone 2 slice 5, which `tests/session_rule.rs` proves.)
///
/// Proves: decision 46.
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
    let slash = format!("{hashed}/");
    let below = format!("{hashed}/x");
    let longer = format!("{hashed}.map");
    let capital_hash = format!("/static/app.{}.css", hash.to_ascii_uppercase());
    assert_ne!(
        capital_hash, hashed,
        "the hash of app.css has no letter to write in capitals"
    );
    let capital_stem = format!("/static/APP.{hash}.css");
    let not_found = [
        "/static/app.css",
        stale.as_str(),
        unknown.as_str(),
        full.as_str(),
        slash.as_str(),
        below.as_str(),
        longer.as_str(),
        capital_hash.as_str(),
        capital_stem.as_str(),
        "/static/vendor/htmx.min.js",
        "/static/",
        "/static/mark.svg",
        "/static/icons/mark.svg",
        "/static/home-screen.png",
        "/static/icons/home-screen.png",
        "/static/LICENSE-ibm-plex.txt",
        "/static/LICENSE-ibm-plex",
        "/static/vendor/LICENSE-ibm-plex",
        "/static/IBMPlexSans-Regular-Latin1.woff2",
    ];
    for uri in not_found {
        let response = get(state(true), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
    }
}

/// The embedded vendored files are the ones `VENDORED.md` records: their SHA-256 matches, the
/// htmx file's SHA-256 and byte count stand in htmx's record in `VENDORED.md` itself, the htmx
/// file is on the 2.x line, each font is a woff2 file and the licence text is the Open Font
/// License's.
///
/// Proves: decision 46, decision 126.
#[test]
fn vendored_assets_match_their_recorded_hashes() {
    let htmx = asset("htmx.min.js").body;
    assert_eq!(
        sha256_hex(htmx),
        HTMX_SHA256,
        "SHA-256 of the embedded htmx.min.js against VENDORED.md"
    );
    let vendored = String::from_utf8(crate_file("static/vendor/VENDORED.md"))
        .unwrap_or_else(|error| panic!("VENDORED.md is not UTF-8: {error}"));
    let htmx_record = htmx_record(&vendored);
    for row in [
        format!("| SHA-256 | `{}` |", sha256_hex(htmx)),
        format!("| Bytes | {} |", htmx.len()),
    ] {
        assert!(
            htmx_record.lines().any(|line| line == row),
            "the embedded htmx.min.js is not as VENDORED.md records it: no row {row} in its record:\n{htmx_record}"
        );
    }
    assert!(
        contains(htmx, br#"version:"2."#),
        "the embedded htmx.min.js does not contain `version:\"2.`"
    );
    for (name, _, sha, _) in FONTS {
        let font = asset(name).body;
        assert_eq!(
            sha256_hex(font),
            sha,
            "SHA-256 of the embedded {name} against VENDORED.md"
        );
        assert!(
            font.starts_with(b"wOF2"),
            "the embedded {name} does not start with `wOF2`"
        );
    }
    let licence = asset(LICENCE_NAME).body;
    assert_eq!(
        sha256_hex(licence),
        LICENCE_SHA256,
        "SHA-256 of the embedded {LICENCE_NAME} against VENDORED.md"
    );
    assert!(
        contains(licence, b"SIL Open Font License, Version 1.1")
            && contains(licence, br#"Reserved Font Name "Plex""#),
        "the embedded {LICENCE_NAME} is not the Open Font License with the Reserved Font Name Plex"
    );
}

/// The four IBM Plex files are served at exactly the addresses the style file names them at,
/// as `font/woff2`, cached forever, byte for byte the maker's files with their recorded sums,
/// 78,224 bytes in all.
///
/// Proves: decision 126 and decision 46: the four Plex files at their literal hashed
/// addresses, as font/woff2, immutable, byte for byte.
#[tokio::test]
async fn the_fonts_are_served_at_their_pinned_addresses_as_woff2() {
    let mut total = 0;
    for (name, url, sha, bytes) in FONTS {
        let source = source_bytes(name);
        assert_eq!(
            sha256_hex(&source),
            sha,
            "SHA-256 of the source file {name}"
        );
        assert_eq!(source.len(), bytes, "bytes of the source file {name}");
        assert_eq!(
            nage_web::asset_url(name),
            Some(url),
            "the address Nage gives {name}"
        );
        let response = get(state(true), url).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
        let content_type = header(&response, "content-type");
        assert_eq!(
            content_type.as_deref(),
            Some("font/woff2"),
            "{name} at {url} is not served as font/woff2"
        );
        assert_eq!(
            header(&response, "cache-control").as_deref(),
            Some(IMMUTABLE),
            "GET {url} cache-control"
        );
        let served = body_bytes(response).await;
        assert!(
            served == source,
            "GET {url} does not serve the bytes of {name}: {} bytes served, {} in the source file",
            served.len(),
            source.len()
        );
        total += served.len();
    }
    assert_eq!(total, FONT_BYTES, "bytes of the four fonts in all");
}

/// The tab icon is served as `image/svg+xml` and the home-screen icon, a 180 by 180 PNG, as
/// `image/png`, each at its hashed address, cached forever and byte for byte; the PNG's SVG
/// source is in the tree and served nowhere, not even at the address its bytes would give it;
/// the tab icon, that source and the pages' `templates/mark.html` draw the same tile and the
/// same tilde; and the served list holds nine files.
///
/// Proves: decision 125 and decision 46: the tab icon as image/svg+xml, the home-screen icon
/// as image/png, the PNG's SVG source not served, not even at
/// /static/home-screen.e7520456121f12d4.svg, the address its bytes would give it, and the
/// served list holds nine files.
#[tokio::test]
async fn the_icons_are_served_as_images_and_their_source_is_not() {
    for (name, expected) in [
        ("mark.svg", "image/svg+xml"),
        ("home-screen.png", "image/png"),
    ] {
        let source = source_bytes(name);
        let url = hashed_url(name, &source);
        assert_eq!(
            nage_web::asset_url(name),
            Some(url.as_str()),
            "the address Nage gives {name}"
        );
        let response = get(state(true), &url).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {url} status");
        let content_type = header(&response, "content-type");
        assert_eq!(
            content_type.as_deref(),
            Some(expected),
            "{name} at {url} is not served as an image ({expected})"
        );
        assert_eq!(
            header(&response, "cache-control").as_deref(),
            Some(IMMUTABLE),
            "GET {url} cache-control"
        );
        let served = body_bytes(response).await;
        assert!(
            served == source,
            "GET {url} does not serve the bytes of {name}"
        );
    }

    let png = source_bytes("home-screen.png");
    assert!(
        png.starts_with(b"\x89PNG\r\n\x1a\n") && png.get(12..16) == Some(b"IHDR".as_slice()),
        "home-screen.png is not a PNG"
    );
    let width = png.get(16..20).map(|bytes| bytes.to_vec());
    let height = png.get(20..24).map(|bytes| bytes.to_vec());
    assert_eq!(
        (width, height),
        (Some(vec![0, 0, 0, 180]), Some(vec![0, 0, 0, 180])),
        "home-screen.png is not 180 by 180"
    );
    let svg = source_bytes("mark.svg");
    assert!(svg.starts_with(b"<svg"), "mark.svg is not an SVG drawing");
    assert_one_mark("the icons do not draw the mark of the pages");

    let icon_source = source_bytes("home-screen.svg");
    assert!(
        icon_source.starts_with(b"<svg"),
        "the home-screen icon's source is not an SVG drawing in the tree"
    );
    assert_eq!(
        hashed_url("home-screen.svg", &icon_source),
        ICON_SOURCE_URL,
        "the address the bytes of the home-screen icon's source would give it"
    );
    assert_eq!(
        nage_web::asset_url("home-screen.svg"),
        None,
        "the home-screen icon's source is among the served files"
    );
    for uri in [
        ICON_SOURCE_URL,
        "/static/home-screen.svg",
        "/static/icons/home-screen.svg",
    ] {
        let response = get(state(true), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {uri} status");
    }

    let names: Vec<&str> = nage_web::assets().iter().map(|asset| asset.name).collect();
    assert_eq!(names.len(), 9, "the served files: {names:?}");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    let mut expected = ASSET_NAMES.to_vec();
    expected.sort_unstable();
    assert_eq!(sorted, expected, "the served files");
}

/// The fonts' licence text is served under the name `LICENSE-ibm-plex.txt` at its hashed
/// address as plain text, cached forever, 4456 bytes with its recorded SHA-256, byte for byte
/// the file `vendor/LICENSE-ibm-plex`, which keeps its name on disk; and no page links it.
///
/// Proves: decision 126 and decision 46: LICENSE-ibm-plex at
/// /static/LICENSE-ibm-plex.7e6b2818edbd8f6a.txt as text/plain, immutable, 4456 bytes with
/// SHA-256 7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da, and no page links
/// it.
#[tokio::test]
async fn the_font_licence_is_served_as_plain_text() {
    let source = crate_file("static/vendor/LICENSE-ibm-plex");
    assert_eq!(
        sha256_hex(&source),
        LICENCE_SHA256,
        "SHA-256 of vendor/LICENSE-ibm-plex"
    );
    assert_eq!(
        source.len(),
        LICENCE_BYTES,
        "bytes of vendor/LICENSE-ibm-plex"
    );
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("static/vendor/LICENSE-ibm-plex.txt")
            .exists(),
        "the licence text is kept on disk under its served name"
    );
    assert_eq!(
        nage_web::asset_url(LICENCE_NAME),
        Some(LICENCE_URL),
        "the address Nage gives {LICENCE_NAME}"
    );

    let response = get(state(true), LICENCE_URL).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET {LICENCE_URL} status"
    );
    let content_type = header(&response, "content-type");
    assert_eq!(
        content_type.as_deref(),
        Some("text/plain; charset=utf-8"),
        "{LICENCE_NAME} at {LICENCE_URL} is not served as plain text"
    );
    assert_eq!(
        header(&response, "cache-control").as_deref(),
        Some(IMMUTABLE),
        "GET {LICENCE_URL} cache-control"
    );
    let served = body_bytes(response).await;
    assert!(
        served == source,
        "GET {LICENCE_URL} does not serve the bytes of vendor/LICENSE-ibm-plex"
    );

    let fresh = App::start().await;
    let mut nobody = fresh.client();
    let mut pages = vec![("the set-up page", nobody.get("/setup").await.body)];
    let app = App::with_book().await;
    let mut visitor = app.client();
    pages.push(("the sign-in page", visitor.get("/login").await.body));
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    pages.push(("the home page", client.get("/").await.body));
    pages.push((
        "the change-password page",
        client.get("/account/password").await.body,
    ));
    pages.push((
        "the page for an unknown address",
        client.get("/no-such-page").await.body,
    ));
    let style = String::from_utf8(asset("app.css").body.to_vec())
        .unwrap_or_else(|error| panic!("app.css is not UTF-8: {error}"));
    pages.push(("the style file", style));
    for (what, body) in pages {
        assert!(
            body.contains("<html") || what == "the style file",
            "{what} is not a page:\n{body}"
        );
        assert!(
            !body.contains("LICENSE-ibm-plex"),
            "{what} links the fonts' licence text:\n{body}"
        );
    }
}

/// `VENDORED.md` records each font as it records htmx, with its source address, SHA-256, byte
/// count and the address it is served at on one line, the project, the version, the four
/// fonts' bytes in all, and the fonts' licence text with its sum, its byte count and the address
/// it is served at; htmx's record, the model the fonts follow, holds its project, version, tag,
/// SHA-256 and bytes; Pico is gone from it, from the tree and from the served files.
///
/// Proves: decision 46 and decision 126: VENDORED.md records each font with its SHA-256 and
/// bytes and LICENSE-ibm-plex with its served address, and names no Pico file.
#[test]
fn vendored_md_records_the_fonts_and_no_longer_pico() {
    let vendored = String::from_utf8(crate_file("static/vendor/VENDORED.md"))
        .unwrap_or_else(|error| panic!("VENDORED.md is not UTF-8: {error}"));
    let records = |needles: &[&str]| {
        vendored
            .lines()
            .any(|line| needles.iter().all(|needle| line.contains(needle)))
    };
    for (name, url, sha, bytes) in FONTS {
        let bytes = format!("| {bytes} |");
        let served = format!("| `{url}` |");
        let family = if name.starts_with("IBMPlexMono-") {
            "mono"
        } else {
            "sans"
        };
        let source = format!(
            "https://github.com/IBM/plex/raw/%40ibm%2Fplex-{family}%401.1.0/packages/plex-{family}/fonts/split/woff2/{name}"
        );
        assert!(
            records(&[name, &source, sha, &bytes, &served]),
            "VENDORED.md does not record {name} with its source address {source}, its SHA-256 {sha}, its bytes and the address it is served at, {url}, on one line:\n{vendored}"
        );
    }
    for row in [
        "| Project | IBM Plex, https://github.com/IBM/plex |",
        "| Version | IBM Plex Sans 1.1.0 and IBM Plex Mono 1.1.0, ",
    ] {
        assert!(
            vendored.lines().any(|line| line.starts_with(row)),
            "VENDORED.md does not record the fonts' row {row}:\n{vendored}"
        );
    }
    let htmx_record = htmx_record(&vendored);
    for row in [
        "| Project | htmx, https://github.com/bigskysoftware/htmx |".to_owned(),
        "| Version | 2.0.11 ".to_owned(),
        "| Tag | `v2.0.11` |".to_owned(),
        format!("| SHA-256 | `{HTMX_SHA256}` |"),
        format!("| Bytes | {} |", asset("htmx.min.js").body.len()),
    ] {
        assert!(
            htmx_record.lines().any(|line| line.starts_with(&row)),
            "VENDORED.md does not record htmx's row {row}:\n{htmx_record}"
        );
    }
    let total = format!("| Bytes | {FONT_BYTES} in all |");
    assert!(
        records(&[&total]),
        "VENDORED.md does not record the fonts' {total}:\n{vendored}"
    );
    let licence_bytes = format!("| {LICENCE_BYTES} |");
    assert!(
        records(&[
            "`LICENSE-ibm-plex`",
            LICENCE_SHA256,
            &licence_bytes,
            LICENCE_URL
        ]),
        "VENDORED.md does not record LICENSE-ibm-plex with its SHA-256, its bytes and {LICENCE_URL} on one line:\n{vendored}"
    );
    for field in [
        "https://github.com/IBM/plex",
        "`@ibm/plex-sans@1.1.0`",
        "`@ibm/plex-mono@1.1.0`",
        "1da12f02587b630c07e92692d21492d722f53614",
        "https://unpkg.com/@ibm/plex-sans@1.1.0/",
        "https://unpkg.com/@ibm/plex-mono@1.1.0/",
        "SIL Open Font License 1.1",
    ] {
        assert!(
            vendored.contains(field),
            "VENDORED.md does not record the fonts' {field}:\n{vendored}"
        );
    }
    assert!(
        !vendored.to_ascii_lowercase().contains("pico"),
        "VENDORED.md still names Pico:\n{vendored}"
    );
    for gone in [
        "static/vendor/pico.classless.min.css",
        "static/vendor/LICENSE-pico",
    ] {
        assert!(
            !Path::new(env!("CARGO_MANIFEST_DIR")).join(gone).exists(),
            "{gone} is still in the tree"
        );
    }
    for served in nage_web::assets() {
        assert!(
            !served.name.to_ascii_lowercase().contains("pico"),
            "{} is still served",
            served.name
        );
    }
}
