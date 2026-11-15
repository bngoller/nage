//! The placeholder `/` on the base layout, behind the sign-in since milestone 2 slice 5
//! (decision 106): each test signs in first and asks for the page with its session.

mod common;

use axum::http::StatusCode;

use common::app::{ADMIN, App, PASSWORD, Reply};
use common::{asset, hashed_url, start_tags};

/// `GET /` as a signed-in user gets it.
async fn index() -> Reply {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    client.get("/").await
}

/// The htmx configuration tag of SPEC 5.3, exactly as the base layout carries it.
const HTMX_CONFIG: &str =
    r#"<meta name="htmx-config" content='{"allowEval":false,"includeIndicatorStyles":false}'>"#;

/// `GET /` with a session answers 200 with an HTML page on the base layout: doctype,
/// `lang="en"`, the title and a viewport meta tag.
///
/// Proves: PLAN M1 slice 2, SPEC 11, decision 106 (`/` stays the placeholder, asked for here
/// with a session).
#[tokio::test]
async fn index_renders_the_base_layout() {
    let reply = index().await;
    assert_eq!(reply.status, StatusCode::OK, "GET / status");
    assert_eq!(
        reply.header("content-type"),
        Some("text/html; charset=utf-8"),
        "GET / content-type"
    );
    let html = reply.body;
    assert!(
        html.trim_start()
            .to_ascii_lowercase()
            .starts_with("<!doctype html>"),
        "GET / does not start with the HTML doctype:\n{html}"
    );
    assert!(
        html.contains(r#"<html lang="en""#),
        "GET / has no <html lang=\"en\">:\n{html}"
    );
    assert!(
        html.contains("<title>Nage</title>"),
        "GET / has no <title>Nage</title>:\n{html}"
    );
    let viewport = start_tags(&html, "<meta")
        .into_iter()
        .any(|tag| tag.contains(r#"name="viewport""#) && tag.contains("width=device-width"));
    assert!(viewport, "GET / has no viewport meta tag:\n{html}");
}

/// The page loads htmx, Pico CSS and `app.css` from their content-hashed URLs.
///
/// Proves: decision 46.
#[tokio::test]
async fn index_links_the_three_hashed_assets() {
    let htmx = hashed_url("htmx.min.js", asset("htmx.min.js").body);
    let pico = hashed_url(
        "pico.classless.min.css",
        asset("pico.classless.min.css").body,
    );
    let app = hashed_url("app.css", asset("app.css").body);

    let reply = index().await;
    assert_eq!(reply.status, StatusCode::OK, "GET / status");
    let html = reply.body;

    let src = format!(r#"src="{htmx}""#);
    assert!(
        start_tags(&html, "<script")
            .iter()
            .any(|tag| tag.contains(&src)),
        "GET / has no <script {src}>:\n{html}"
    );
    for url in [&pico, &app] {
        let href = format!(r#"href="{url}""#);
        assert!(
            start_tags(&html, "<link")
                .iter()
                .any(|tag| tag.contains(r#"rel="stylesheet""#) && tag.contains(&href)),
            "GET / has no stylesheet <link {href}>:\n{html}"
        );
    }
}

/// htmx is configured through its meta tag with `allowEval` and `includeIndicatorStyles`
/// off, and the page has no inline script, no inline style and no `hx-on` attribute.
///
/// Proves: SPEC 5.3, decision 40.
#[tokio::test]
async fn index_configures_htmx_without_eval_and_has_no_inline_code() {
    let reply = index().await;
    assert_eq!(reply.status, StatusCode::OK, "GET / status");
    let html = reply.body;
    assert!(
        html.contains(HTMX_CONFIG),
        "GET / does not carry {HTMX_CONFIG}:\n{html}"
    );

    let lower = html.to_ascii_lowercase();
    let scripts = start_tags(&lower, "<script");
    assert!(!scripts.is_empty(), "GET / has no <script> tag:\n{html}");
    for tag in scripts {
        assert!(
            tag.contains(" src="),
            "GET / has an inline script `{tag}`:\n{html}"
        );
    }
    assert!(
        !lower.contains("<style"),
        "GET / has an inline <style> element:\n{html}"
    );
    assert!(
        !lower.contains(" style="),
        "GET / has an inline style attribute:\n{html}"
    );
    assert!(
        !lower.contains("hx-on"),
        "GET / uses an hx-on attribute:\n{html}"
    );
}
