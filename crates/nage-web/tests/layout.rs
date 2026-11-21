//! The layout's bar and its menu (SPEC 5.3, decisions 46, 73, 105, 116 and 123 to 125): which
//! pages carry the menu and which the masthead alone, what the menu lists and which entry it
//! marks, the mark and Nage's name in both, and that no page of this slice carries an inline
//! script, an inline style or an htmx attribute.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::storage::StorageOptions;
use nage_web::messages::Refusal;

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

/// The brand of the top of a page, `<a class="brand" href="/">` on a page with the menu or
/// `<span class="brand">` on a page without it, from its start tag to its end tag, which must
/// hold the mark (the one inline `svg`, of class `mark`) and then Nage's name; panics unless
/// `top` holds the brand `open` exactly once.
fn brand<'a>(top: &'a str, open: &str, close: &str, what: &str) -> &'a str {
    assert_eq!(
        top.matches(open).count(),
        1,
        "brands {open} at the top of {what}:\n{top}"
    );
    let start = top.find(open).unwrap_or_default();
    let brand = top[start..].split(close).next().unwrap_or_default();
    let mark = brand.find(r#"<svg class="mark""#);
    let name = brand.find(r#"<span class="brand-name">Nage</span>"#);
    assert!(
        matches!((mark, name), (Some(mark), Some(name)) if mark < name),
        "the brand of {what} does not hold the mark and then Nage's name:\n{brand}"
    );
    assert_eq!(
        top.matches("<svg").count(),
        1,
        "drawings at the top of {what}:\n{top}"
    );
    brand
}

/// Asserts that `page` carries the bar with the mark and Nage's name, together one link to
/// the home page, and the menu, once for a phone (behind the Menu button, closed: the start
/// tag of its `details` holds no `open` anywhere) and once for a computer: in each, a link to
/// the home page, a link to the change-password page, the Appearance group in a form that
/// posts to `/theme`, who is signed in by display name, and the Sign out button in a form
/// that posts to `/logout`; that exactly the entry `current` is marked as the page in view, in
/// both; that each Appearance group holds its buttons in `div.choices`; and that the page
/// stands in `div.shell` with `main.content`, never in the lone column.
fn assert_menu(page: &Reply, display_name: &str, current: Option<&str>, what: &str) {
    let html = &page.body;
    assert_eq!(
        html.matches(r#"<header class="bar">"#).count(),
        1,
        "bars on {what}:\n{html}"
    );
    assert!(
        !html.contains(r#"<header class="masthead">"#),
        "{what} carries the masthead of a page without a menu:\n{html}"
    );
    let header = html
        .split(r#"<header class="bar">"#)
        .nth(1)
        .and_then(|rest| rest.split("</header>").next())
        .unwrap_or_default();
    let link = brand(header, r#"<a class="brand" href="/">"#, "</a>", what);
    assert!(
        !link.contains("aria-current"),
        "the brand of {what} is marked as the page in view: {link}"
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
        let links: Vec<&str> = links_to(header, href)
            .into_iter()
            .filter(|tag| !tag.contains(r#"class="brand""#))
            .collect();
        assert_eq!(links.len(), 2, "menu links to {href} in the bar of {what}");
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
    assert_eq!(forms.len(), 4, "forms in the bar of {what}: {forms:?}");
    for action in ["/theme", "/logout"] {
        let posting = forms
            .iter()
            .filter(|form| {
                form.contains(r#"method="post""#) && form.contains(&format!(r#"action="{action}""#))
            })
            .count();
        assert_eq!(
            posting, 2,
            "forms in the bar of {what} that post to {action}: {forms:?}"
        );
    }
    assert_eq!(
        header.matches("<legend>Appearance</legend>").count(),
        2,
        "Appearance groups in the bar of {what}:\n{header}"
    );
    assert_eq!(
        header.matches(r#"<div class="choices">"#).count(),
        2,
        "the Appearance groups' rows of three buttons (class choices) in the bar of {what}:\n{header}"
    );
    for (wrapper, count) in [
        (r#"<div class="shell">"#, 1),
        (r#"<main class="content">"#, 1),
        (r#"<div class="lone">"#, 0),
        (r#"<main class="panel">"#, 0),
    ] {
        assert_eq!(
            html.matches(wrapper).count(),
            count,
            "{wrapper} on {what}, the signed-in page the style file lays out beside the rail:\n{html}"
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

/// Asserts that `page` carries no bar and no menu: only the masthead, with the mark and
/// Nage's name, which are no link, and no Menu button, no `nav`, no `details`, no Appearance
/// group and no Sign out; and that the page stands in the lone column, `div.lone` with
/// `main.panel`, never in `div.shell` with `main.content`.
fn assert_no_menu(page: &Reply, what: &str) {
    for absent in [
        r#"<header class="bar""#,
        "<details",
        "<summary",
        "<nav",
        ">Menu<",
        "Appearance",
        "Sign out",
        "Signed in as",
    ] {
        assert!(
            !page.body.contains(absent),
            "{what} carries {absent:?}:\n{}",
            page.body
        );
    }
    assert_eq!(
        page.body.matches("<header").count(),
        1,
        "headers on {what}:\n{}",
        page.body
    );
    let masthead = page
        .body
        .split(r#"<header class="masthead">"#)
        .nth(1)
        .and_then(|rest| rest.split("</header>").next())
        .unwrap_or_else(|| panic!("{what} carries no masthead:\n{}", page.body));
    brand(masthead, r#"<span class="brand">"#, "</header>", what);
    assert!(
        !masthead.contains("<a "),
        "the masthead of {what} carries a link:\n{masthead}"
    );
    for (wrapper, count) in [
        (r#"<div class="lone">"#, 1),
        (r#"<main class="panel">"#, 1),
        (r#"<div class="shell">"#, 0),
        (r#"<main class="content">"#, 0),
    ] {
        assert_eq!(
            page.body.matches(wrapper).count(),
            count,
            "{wrapper} on {what}, the lone column the style file draws as one panel:\n{}",
            page.body
        );
    }
}

/// The home page, the change-password page and the page for an unknown address carry the
/// bar with the mark and Nage's name, together one link to the home page, and the menu:
/// Home, Change password, the Appearance group, which posts to `/theme`, who is signed in, by
/// display name and not by username, and the Sign out button, which posts to `/logout`. The
/// menu is in the page once behind the Menu button, closed, and once in the rail for a
/// computer, with the same entries. The home page marks Home as the page in view, the
/// change-password page marks Change password, and the page for an unknown address marks
/// neither. The change-password page has no Sign out button of its own any more.
///
/// Proves: decision 116 (the bar with Nage's name and the Menu button, behind which the menu
/// is closed before any tap; the menu lists Home, Change password, the Appearance group, who
/// is signed in and Sign out; which pages carry it; the page in view is marked; Sign out is a
/// button that posts to `/logout` and takes the place of the change-password page's own),
/// decision 125 (the mark and the name are one link to the home page), decision 112.
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

/// The pages that know no signed-in user carry no bar and no menu, only the masthead with
/// the mark and Nage's name, which are no link, and keep their single button or link: the
/// sign-in page, empty and refused; the set-up page and, once a user exists, its closed page;
/// the 403 page of the Origin check; the 413 page of the body limit; the 422 page that
/// refuses an appearance the menu does not offer, although a signed-in person sees it; and
/// the busy page, whose one link reads "Back to the home page", leads to `/` and has the
/// class `button`.
///
/// Proves: decision 116 and decision 125 (the sign-in page, the set-up page and its closed
/// page, the 403, 413 and 422 refusal pages and the busy page carry no menu, and the mark
/// and the name are no link there; the busy page keeps its link, which reads "Back to the
/// home page" and leads to the home page), decision 124 (the 422 page is a refusal page like
/// the others, without the menu), decision 123 (the busy page's way back looks like a
/// button).
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
    let links = start_tags(&busy.body, "<a ");
    assert_eq!(
        links,
        [r#"<a class="button" href="/">"#],
        "the links of the busy page:\n{}",
        busy.body
    );
    assert!(
        busy.body
            .contains(r#"<a class="button" href="/">Back to the home page</a>"#),
        "the busy page does not keep its link to the home page, as a button:\n{}",
        busy.body
    );

    let unknown_appearance = client
        .post("/theme", &[("theme", "sepia"), ("back", "/")])
        .await;
    assert_eq!(
        unknown_appearance.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of an appearance the menu does not offer"
    );
    assert!(
        unknown_appearance.shows(&Refusal::UnknownTheme.to_string()),
        "the 422 answer is not the page that refuses an appearance:\n{}",
        unknown_appearance.body
    );
    assert_no_menu(
        &unknown_appearance,
        "the 422 page that refuses an appearance, sent with a session",
    );
}

/// No page this slice adds or changes carries an inline script, an inline style, a style
/// attribute, an `hx-on` attribute or any other htmx attribute, and each keeps the htmx
/// configuration tag as decision 40 set it and the title Nage, the one title of the page,
/// and draws the mark once, hidden from screen readers: the home page with balances and in
/// its empty state, the page for an unknown address, the change-password page with its
/// confirmation, the sign-in page with the message of a sign-out, the 403, 413 and 422
/// pages, and the set-up page and its closed page. The busy page is not looked at here:
/// `busy.rs` keeps milestone 1's own check of its scripts and styles.
///
/// Proves: SPEC 5.3 (no inline scripts, no inline styles, no `hx-on`), decision 40 (the htmx
/// meta configuration stays as it is), decision 73 (the title), decision 116 and decision 124
/// (no element of this slice carries an htmx attribute; the menu and the appearance switch
/// need no script of their own), decision 125 (the mark, drawn inline with no title and no
/// style attribute, hidden from screen readers).
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
    pages.push((
        "the 422 page that refuses an appearance",
        client
            .post("/theme", &[("theme", "sepia"), ("back", "/")])
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
            (
                "the 422 page that refuses an appearance",
                StatusCode::UNPROCESSABLE_ENTITY
            ),
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
        assert_eq!(
            (
                html.matches("<title").count(),
                html.contains("<title>Nage</title>")
            ),
            (1, true),
            "the titles of {what}, the mark's none among them:\n{html}"
        );
        let drawings = start_tags(html, "<svg");
        assert!(
            drawings.len() == 1
                && drawings[0].contains(r#"class="mark""#)
                && drawings[0].contains(r#"aria-hidden="true""#),
            "{what} does not draw the one mark, hidden from screen readers: {drawings:?}"
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
