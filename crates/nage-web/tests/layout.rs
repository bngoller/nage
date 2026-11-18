//! The layout's bar and its menu (SPEC 5.3, decisions 46, 73, 105 and 116): which pages carry
//! the menu, what it lists and which entry it marks, and that no page of this slice carries
//! an inline script, an inline style or an htmx attribute.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::storage::StorageOptions;

use common::app::{ADMIN, App, OTHER, OTHER_PASSWORD, Options, PASSWORD, Reply, Req};
use common::start_tags;

/// The start tags of the links to `href` in `html`.
fn links_to<'a>(html: &'a str, href: &str) -> Vec<&'a str> {
    let needle = format!(r#"href="{href}""#);
    start_tags(html, "<a ")
        .into_iter()
        .filter(|tag| tag.contains(&needle))
        .collect()
}

/// Asserts that `page` carries the bar with Nage's name and the menu, once for a phone
/// (behind the Menu button, closed: the start tag of its `details` holds no `open` anywhere)
/// and once for a computer: in each, a link to the home page, a link to the change-password
/// page, who is signed in by display name, and the Sign out button in a form that posts to
/// `/logout`; and that exactly the entry `current` is marked as the page in view, in both.
fn assert_menu(page: &Reply, display_name: &str, current: Option<&str>, what: &str) {
    let html = &page.body;
    assert_eq!(
        html.matches(r#"<header class="bar">"#).count(),
        1,
        "bars on {what}:\n{html}"
    );
    let header = html
        .split(r#"<header class="bar">"#)
        .nth(1)
        .and_then(|rest| rest.split("</header>").next())
        .unwrap_or_default();
    assert!(
        header.contains("<strong>Nage</strong>"),
        "the bar of {what} does not carry Nage's name:\n{header}"
    );
    assert_eq!(
        header
            .matches(r#"<summary role="button">Menu</summary>"#)
            .count(),
        1,
        "Menu buttons on {what}:\n{header}"
    );
    assert_eq!(
        header.matches("<details").count(),
        1,
        "details elements in the bar of {what}:\n{header}"
    );
    let details = start_tags(header, "<details");
    assert!(
        details.iter().all(|tag| !tag.contains(" open")),
        "the menu of {what} is open before any tap: {details:?}"
    );
    for (href, label) in [("/", "Home"), ("/account/password", "Change password")] {
        let links = links_to(header, href);
        assert_eq!(links.len(), 2, "links to {href} in the bar of {what}");
        assert_eq!(
            header.matches(&format!(">{label}</a>")).count(),
            2,
            "entries called {label} in the bar of {what}:\n{header}"
        );
        let marked = links
            .iter()
            .filter(|tag| tag.contains(r#"aria-current="page""#))
            .count();
        let expected = if current == Some(href) { 2 } else { 0 };
        assert_eq!(
            marked, expected,
            "links to {href} marked as the page in view on {what}: {links:?}"
        );
    }
    assert_eq!(
        header
            .matches(&format!("Signed in as {display_name}<"))
            .count(),
        2,
        "entries saying who is signed in on {what}:\n{header}"
    );
    let forms = start_tags(header, "<form");
    assert_eq!(forms.len(), 2, "forms in the bar of {what}: {forms:?}");
    for form in forms {
        assert!(
            form.contains(r#"method="post""#) && form.contains(r#"action="/logout""#),
            "a form in the bar of {what} does not post to /logout: {form}"
        );
    }
    assert_eq!(
        header
            .matches(r#"<button type="submit">Sign out</button>"#)
            .count(),
        2,
        "Sign out buttons in the bar of {what}:\n{header}"
    );
    assert_eq!(
        html.matches("Sign out").count(),
        2,
        "Sign out on {what} outside the menu:\n{html}"
    );
}

/// Asserts that `page` carries no bar and no menu.
fn assert_no_menu(page: &Reply, what: &str) {
    for absent in ["<header", "<details", "<nav", "Sign out", "Signed in as"] {
        assert!(
            !page.body.contains(absent),
            "{what} carries {absent:?}:\n{}",
            page.body
        );
    }
}

/// The home page, the change-password page and the page for an unknown address carry the
/// bar with Nage's name and the menu: Home, Change password, who is signed in, by display
/// name and not by username, and the Sign out button, which posts to `/logout`. The menu is
/// in the page once behind the Menu button, closed, and once as a row for a computer, with
/// the same entries. The home page marks Home as the page in view, the change-password page
/// marks Change password, and the page for an unknown address marks neither. The
/// change-password page has no Sign out button of its own any more.
///
/// Proves: decision 116 (the bar with Nage's name and the Menu button, behind which the menu
/// is closed before any tap; the menu lists Home, Change password, who is signed in and Sign
/// out; which pages carry it; the page in view is marked; Sign out is a button that posts to
/// `/logout` and takes the place of the change-password page's own), decision 112.
#[tokio::test]
async fn the_pages_that_know_who_is_signed_in_carry_the_menu() {
    let app = App::with_book().await;
    app.add_named_user(OTHER, "Alexandra").await;
    let mut client = app.signed_in(OTHER, OTHER_PASSWORD).await;
    let home = client.get("/").await;
    assert_eq!(home.status, StatusCode::OK, "GET / status");
    assert_menu(&home, "Alexandra", Some("/"), "the home page");
    let password = client.get("/account/password").await;
    assert_eq!(
        password.status,
        StatusCode::OK,
        "GET /account/password status"
    );
    assert_menu(
        &password,
        "Alexandra",
        Some("/account/password"),
        "the change-password page",
    );
    let unknown = client.get("/no-such-page").await;
    assert_eq!(
        unknown.status,
        StatusCode::NOT_FOUND,
        "GET /no-such-page status"
    );
    assert_menu(
        &unknown,
        "Alexandra",
        None,
        "the page for an unknown address",
    );
    let refused = client
        .post("/account/password", &[("current_password", "wrong")])
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused change"
    );
    assert_menu(
        &refused,
        "Alexandra",
        Some("/account/password"),
        "the change-password page of a refused change",
    );
}

/// The pages that know no signed-in user carry no bar and no menu and keep their single
/// button or link: the sign-in page, empty and refused; the set-up page and, once a user
/// exists, its closed page; the 403 page of the Origin check; the 413 page of the body
/// limit; and the busy page, whose link reads "Back to the home page" and leads to `/`.
///
/// Proves: decision 116 (the sign-in page, the set-up page and its closed page, the 403 and
/// 413 refusal pages and the busy page carry no menu; the busy page keeps its link, which
/// reads "Back to the home page" and leads to the home page).
#[tokio::test]
async fn the_pages_without_a_signed_in_user_carry_no_menu() {
    let fresh = App::start().await;
    let setup = fresh.client().get("/setup").await;
    assert_eq!(setup.status, StatusCode::OK, "GET /setup on a fresh server");
    assert_no_menu(&setup, "the set-up page");

    let app = App::start_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await;
    app.set_up().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let closed = client.get("/setup").await;
    assert_eq!(
        closed.status,
        StatusCode::NOT_FOUND,
        "GET /setup once set up"
    );
    assert_no_menu(&closed, "the closed set-up page, asked for with a session");

    let sign_in = app.client().get("/login").await;
    assert_eq!(sign_in.status, StatusCode::OK, "GET /login status");
    assert_no_menu(&sign_in, "the sign-in page");
    let refused = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", "wrong")])
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused sign-in"
    );
    assert_no_menu(&refused, "the sign-in page of a refused sign-in");

    let cross_site = client
        .send(Req::form("/logout", &[]).header("origin", "http://elsewhere.test"))
        .await;
    assert_eq!(
        cross_site.status,
        StatusCode::FORBIDDEN,
        "status of a cross-site sign-out"
    );
    assert_no_menu(&cross_site, "the 403 page, sent with a session");
    let too_large = client
        .send(Req::form("/logout", &[]).header("content-length", "65537"))
        .await;
    assert_eq!(
        too_large.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "status of a form that declares 65,537 bytes"
    );
    assert_no_menu(&too_large, "the 413 page, sent with a session");

    let held = app.write().await;
    let busy = client.post("/logout", &[]).await;
    drop(held);
    assert_eq!(
        busy.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "status of a sign-out while the writer is held"
    );
    assert!(
        busy.shows("Nage is busy"),
        "the busy answer is not the busy page:\n{}",
        busy.body
    );
    assert_no_menu(&busy, "the busy page, sent with a session");
    assert!(
        busy.body
            .contains(r#"<a href="/">Back to the home page</a>"#),
        "the busy page does not keep its link to the home page:\n{}",
        busy.body
    );
}

/// No page this slice adds or changes carries an inline script, an inline style, an `hx-on`
/// attribute or any other htmx attribute, and each keeps the htmx configuration tag as
/// decision 40 set it: the home page with balances and in its empty state, the page for an
/// unknown address, the change-password page with its confirmation, the sign-in page with the
/// message of a sign-out, the 403 and 413 pages, and the set-up page and its closed page. The
/// busy page, whose link this slice rewords, is not looked at here: `busy.rs` keeps
/// milestone 1's own check of its scripts and styles.
///
/// Proves: SPEC 5.3 (no inline scripts, no inline styles, no `hx-on`), decision 40 (the htmx
/// meta configuration stays as it is), decision 116 (no element of this slice carries an
/// htmx attribute; the menu needs no script of its own).
#[tokio::test]
async fn the_pages_of_this_slice_carry_no_inline_code_and_no_htmx_attribute() {
    const HTMX_CONFIG: &str =
        r#"<meta name="htmx-config" content='{"allowEval":false,"includeIndicatorStyles":false}'>"#;
    let app = App::with_book().await;
    let checking = app.account("Assets:Checking").await;
    app.open_balance(checking, "2027-01-01", 248_015).await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let mut pages = vec![
        ("the home page", client.get("/").await),
        (
            "the page for an unknown address",
            client.get("/nowhere").await,
        ),
        (
            "the change-password page",
            client.get("/account/password").await,
        ),
    ];
    let changed = client
        .post(
            "/account/password",
            &[
                ("current_password", PASSWORD),
                ("new_password", "tulip window 99"),
                ("new_password_again", "tulip window 99"),
            ],
        )
        .await;
    assert_eq!(
        changed.status,
        StatusCode::SEE_OTHER,
        "status of the change"
    );
    pages.push((
        "the change-password page with its confirmation",
        client.get("/account/password").await,
    ));
    pages.push((
        "the 403 page",
        client
            .send(Req::form("/logout", &[]).header("origin", "http://elsewhere.test"))
            .await,
    ));
    pages.push((
        "the 413 page",
        client
            .send(Req::form("/logout", &[]).header("content-length", "65537"))
            .await,
    ));
    pages.push(("the closed set-up page", app.client().get("/setup").await));
    let signed_out = client.post("/logout", &[]).await;
    assert_eq!(
        signed_out.status,
        StatusCode::SEE_OTHER,
        "status of the sign-out"
    );
    pages.push((
        "the sign-in page after a sign-out",
        client.get("/login").await,
    ));
    let empty = App::start().await;
    empty.set_up_as(false).await;
    pages.push((
        "the home page in its empty state",
        empty.signed_in(ADMIN, PASSWORD).await.get("/").await,
    ));
    pages.push((
        "the set-up page",
        App::start().await.client().get("/setup").await,
    ));
    let statuses: Vec<_> = pages
        .iter()
        .map(|(what, page)| (*what, page.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("the home page", StatusCode::OK),
            ("the page for an unknown address", StatusCode::NOT_FOUND),
            ("the change-password page", StatusCode::OK),
            (
                "the change-password page with its confirmation",
                StatusCode::OK
            ),
            ("the 403 page", StatusCode::FORBIDDEN),
            ("the 413 page", StatusCode::PAYLOAD_TOO_LARGE),
            ("the closed set-up page", StatusCode::NOT_FOUND),
            ("the sign-in page after a sign-out", StatusCode::OK),
            ("the home page in its empty state", StatusCode::OK),
            ("the set-up page", StatusCode::OK),
        ],
        "the pages looked at"
    );
    for (what, page) in &pages {
        let html = &page.body;
        assert!(
            html.contains(HTMX_CONFIG),
            "{what} does not carry {HTMX_CONFIG}:\n{html}"
        );
        let lower = html.to_ascii_lowercase();
        let scripts = start_tags(&lower, "<script");
        assert_eq!(scripts.len(), 1, "script tags on {what}: {scripts:?}");
        assert!(
            scripts[0].contains(" src=\"/static/"),
            "{what} has an inline script `{}`",
            scripts[0]
        );
        for forbidden in [
            "<style",
            " style=",
            "hx-on",
            " hx-",
            "data-hx-",
            "javascript:",
        ] {
            assert!(
                !lower.contains(forbidden),
                "{what} carries {forbidden:?}:\n{html}"
            );
        }
    }
}
