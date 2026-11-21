//! The base layout as the home page `/` carries it (decision 119, in place of milestone 1's
//! placeholder), behind the sign-in since milestone 2 slice 5 (decision 106): each test signs
//! in first and asks for the page with its session.

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
/// `lang="en"`, the title and a viewport meta tag; and it is the home page, whose heading is
/// Home, not milestone 1's placeholder.
///
/// Proves: PLAN M1 slice 2, SPEC 11, decision 119 (`/` is the home page, on the base layout,
/// asked for here with a session), decision 73 (the title).
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
    assert!(
        html.contains("<h1>Home</h1>"),
        "GET / is not the home page:\n{html}"
    );
}

/// The page loads htmx and `app.css` from their content-hashed URLs and names the tab icon
/// (`rel="icon"`, `type="image/svg+xml"`) and the home-screen icon (`rel="apple-touch-icon"`)
/// at theirs in its head, and no other icon; it links nothing of Pico. The sign-in page and the
/// set-up page, which carry no menu, have the same head.
///
/// Proves: decision 46, decision 125 (the icons named in every page's head), decision 126
/// (Pico gone from the head).
#[tokio::test]
async fn index_links_its_hashed_files_and_icons() {
    let htmx = hashed_url("htmx.min.js", asset("htmx.min.js").body);
    let app_css = hashed_url("app.css", asset("app.css").body);
    let icon = hashed_url("mark.svg", asset("mark.svg").body);
    let touch = hashed_url("home-screen.png", asset("home-screen.png").body);

    let fresh = App::start().await;
    let setup = fresh.client().get("/setup").await;
    let app = App::with_book().await;
    let login = app.client().get("/login").await;
    let pages = [
        ("GET /", index().await),
        ("GET /login", login),
        ("GET /setup", setup),
    ];
    for (what, reply) in pages {
        assert_eq!(reply.status, StatusCode::OK, "{what} status");
        let html = reply.body;
        let head = html.split_once("</head>").map_or_else(
            || panic!("{what} has no </head>:\n{html}"),
            |(head, _)| head,
        );

        let src = format!(r#"src="{htmx}""#);
        assert!(
            start_tags(head, "<script")
                .iter()
                .any(|tag| tag.contains(&src)),
            "{what} has no <script {src}> in its head:\n{html}"
        );
        let href = format!(r#"href="{app_css}""#);
        assert!(
            start_tags(head, "<link")
                .iter()
                .any(|tag| tag.contains(r#"rel="stylesheet""#) && tag.contains(&href)),
            "{what} has no stylesheet <link {href}> in its head:\n{html}"
        );
        let href = format!(r#"href="{icon}""#);
        assert!(
            start_tags(head, "<link").iter().any(|tag| {
                tag.contains(r#"rel="icon""#)
                    && tag.contains(r#"type="image/svg+xml""#)
                    && tag.contains(&href)
            }),
            "{what} names no tab icon <link rel=\"icon\" type=\"image/svg+xml\" {href}> in its head:\n{html}"
        );
        let href = format!(r#"href="{touch}""#);
        assert!(
            start_tags(head, "<link")
                .iter()
                .any(|tag| tag.contains(r#"rel="apple-touch-icon""#) && tag.contains(&href)),
            "{what} names no home-screen icon <link rel=\"apple-touch-icon\" {href}> in its head:\n{html}"
        );
        let icons: Vec<&str> = start_tags(&html, "<link")
            .into_iter()
            .filter(|tag| tag.to_ascii_lowercase().contains("icon"))
            .collect();
        assert_eq!(
            icons.len(),
            2,
            "{what} names an icon other than the tab icon and the home-screen icon at their hashed addresses: {icons:?}"
        );
        assert!(
            !html.to_ascii_lowercase().contains("pico"),
            "{what} still names Pico:\n{html}"
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
