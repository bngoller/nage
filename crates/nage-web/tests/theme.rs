//! The appearance switch through the router (milestone 2 slice 6a): `POST /theme` behind the
//! session rule and the Origin check, the `nage_theme` cookie it sets or removes, the way back
//! to the page the form came from, the refusal of a choice the menu does not offer, and the
//! palette every page wears from that cookie.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use nage_core::storage::StorageOptions;
use nage_web::messages::Refusal;
use tower::ServiceExt as _;

use common::app::{
    ADMIN, App, Client, HOST, Options, PASSWORD, Reply, Req, SESSION_COOKIE, attribute, form_body,
};
use common::start_tags;

/// The name of the appearance's cookie, written out here rather than taken from nage-web, so
/// that a renamed cookie fails the tests.
const THEME_COOKIE: &str = "nage_theme";

/// The attributes of a kept choice, sorted as `Reply::cookies_named` sorts them.
const KEPT: [&str; 4] = ["HttpOnly", "Max-Age=31536000", "Path=/", "SameSite=Lax"];

/// The attributes of a removed choice, sorted as `Reply::cookies_named` sorts them.
const REMOVED: [&str; 4] = ["HttpOnly", "Max-Age=0", "Path=/", "SameSite=Lax"];

/// The heading of the 403 page, which the refusal of an unknown appearance shares.
const NOT_ACCEPTED: &str = "This form was not accepted";

/// A text no page of Nage contains, to look for in a page after sending it in the cookie.
const MARKER: &str = "nwq7zmarker";

/// A signed-in client of an application over a book.
async fn signed_in() -> (App, Client) {
    let app = App::with_book().await;
    let client = app.signed_in(ADMIN, PASSWORD).await;
    (app, client)
}

/// Sends `request` through `client` with one `Cookie` header that holds `nage_theme=<theme>`
/// first, when a theme is given, and then the client's session cookie, when it holds one.
async fn send_with(client: &mut Client, request: Req, theme: Option<&str>) -> Reply {
    let mut cookies = Vec::new();
    if let Some(value) = theme {
        cookies.push(format!("{THEME_COOKIE}={value}"));
    }
    if let Some(token) = &client.cookie {
        cookies.push(format!("{SESSION_COOKIE}={token}"));
    }
    let request = if cookies.is_empty() {
        request
    } else {
        request.header("cookie", &cookies.join("; "))
    };
    client.send(request).await
}

/// `GET uri` through `client` with the cookie `nage_theme=<theme>`, when a theme is given.
async fn get_with(client: &mut Client, uri: &str, theme: Option<&str>) -> Reply {
    send_with(client, Req::get(uri), theme).await
}

/// `GET uri` through `app`'s router with one `Cookie` header of exactly the bytes `cookie`,
/// which need not be UTF-8, and no session.
async fn get_with_raw_cookie(app: &App, uri: &str, cookie: &[u8]) -> Reply {
    let value = HeaderValue::from_bytes(cookie)
        .unwrap_or_else(|error| panic!("cannot build the Cookie header {cookie:?}: {error}"));
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", HOST)
        .header("cookie", value)
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("cannot build GET {uri}: {error}"));
    let response = match app.router.clone().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|error| panic!("cannot read the answer to GET {uri}: {error}"));
    let body = String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|error| panic!("the answer to GET {uri} is not UTF-8: {error}"));
    Reply {
        status,
        headers,
        body,
    }
}

/// Posts the appearance form with `theme` and `back`, as the menu's buttons send it.
async fn choose(client: &mut Client, theme: &str, back: &str) -> Reply {
    client
        .post("/theme", &[("theme", theme), ("back", back)])
        .await
}

/// Every `Set-Cookie` of `nage_theme` on `reply`, each as its value and sorted attributes.
fn theme_cookies(reply: &Reply) -> Vec<(String, Vec<String>)> {
    reply.cookies_named(THEME_COOKIE)
}

/// `attributes` as owned strings.
fn owned(attributes: &[&str]) -> Vec<String> {
    attributes.iter().map(|text| (*text).to_owned()).collect()
}

/// The value of `data-theme` on the page's `html` element, if it has one; panics unless the
/// page has exactly one `html` start tag.
fn palette(reply: &Reply) -> Option<String> {
    let tags = start_tags(&reply.body, "<html");
    assert_eq!(tags.len(), 1, "html start tags in:\n{}", reply.body);
    attribute(tags[0], "data-theme")
}

/// The value of every hidden field `back` of the page, unescaped, in the order of the page.
fn backs(reply: &Reply) -> Vec<String> {
    start_tags(&reply.body, "<input")
        .into_iter()
        .filter(|tag| tag.contains(r#"name="back""#))
        .map(|tag| {
            attribute(tag, "value").unwrap_or_else(|| panic!("the field back has no value: {tag}"))
        })
        .collect()
}

/// The menu's appearance buttons, in the order of the page: each button's `value` and its
/// `aria-pressed`.
fn appearance_buttons(reply: &Reply) -> Vec<(String, String)> {
    start_tags(&reply.body, "<button")
        .into_iter()
        .filter(|tag| tag.contains(r#"name="theme""#))
        .map(|tag| {
            (
                attribute(tag, "value").unwrap_or_default(),
                attribute(tag, "aria-pressed").unwrap_or_default(),
            )
        })
        .collect()
}

/// Asserts that `reply` is a 303 to `location`.
fn assert_sent_to(reply: &Reply, location: &str, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of {what}:\n{}",
        reply.body
    );
    assert_eq!(reply.location(), Some(location), "Location of {what}");
}

/// Asserts that `reply` sets no appearance cookie.
fn assert_no_theme_cookie(reply: &Reply, what: &str) {
    assert!(
        theme_cookies(reply).is_empty(),
        "{what} sets the appearance cookie: {:?}",
        reply.set_cookies()
    );
}

/// An application whose requests wait a tenth of a second for the writer, so that a form sent
/// while the test holds it gets the busy page.
async fn impatient() -> App {
    App::start_with(Options {
        storage: Some(StorageOptions {
            write_queue: Duration::from_millis(100),
            ..StorageOptions::default()
        }),
        ..Options::default()
    })
    .await
}

/// Light and Dark, each chosen from a page, are kept in `nage_theme` holding exactly that word
/// with `Path=/`, `HttpOnly`, `SameSite=Lax` and a year's `Max-Age`, and no `Secure` over
/// plain HTTP; the answer is a 303 to the page named in `back`, which then wears the palette;
/// nothing is written to the database, so a choice made while another write holds the
/// database, on an application that waits only a tenth of a second for it, still gets its 303
/// and its cookie, not the busy page.
///
/// Proves: decision 124: Light and Dark kept in nage_theme with Path=/, HttpOnly, SameSite=Lax and a year Max-Age, and a 303 to the page named in back
#[tokio::test]
async fn choosing_light_or_dark_keeps_it_in_a_cookie_and_returns_to_the_page() {
    let (app, mut client) = signed_in().await;
    let sessions = app.session_rows().await;
    for (choice, back) in [
        ("light", "/account/password"),
        ("dark", "/"),
        ("light", "/?view=all"),
    ] {
        let what = format!("choosing {choice} from {back}");
        let reply = choose(&mut client, choice, back).await;
        assert_sent_to(&reply, back, &what);
        assert_eq!(
            theme_cookies(&reply),
            [(choice.to_owned(), owned(&KEPT))],
            "the appearance cookie of {what}"
        );
        assert!(
            reply.cookies_named(SESSION_COOKIE).is_empty(),
            "{what} sets the session cookie: {:?}",
            reply.set_cookies()
        );
        let page = get_with(&mut client, back, Some(choice)).await;
        assert_eq!(page.status, StatusCode::OK, "status of {back} after {what}");
        assert_eq!(
            palette(&page).as_deref(),
            Some(choice),
            "data-theme of {back} after {what}"
        );
    }
    assert_eq!(
        app.session_rows().await,
        sessions,
        "sessions after the choices"
    );

    let busy = impatient().await;
    busy.set_up().await;
    let mut chooser = busy.signed_in(ADMIN, PASSWORD).await;
    let held = busy.write().await;
    let reply = choose(&mut chooser, "dark", "/").await;
    drop(held);
    let what = "a choice made while another write holds the database";
    assert_sent_to(&reply, "/", what);
    assert_eq!(
        theme_cookies(&reply),
        [(String::from("dark"), owned(&KEPT))],
        "the appearance cookie of {what}"
    );
}

/// Automatic removes the cookie Nage set: an empty `nage_theme` with `Max-Age=0` under the
/// same attributes, and a page asked for afterwards without the cookie, or with it empty,
/// carries no `data-theme`.
///
/// Proves: decision 124: Automatic removes the nage_theme Nage set, with Max-Age=0 under the same attributes
#[tokio::test]
async fn choosing_automatic_removes_the_cookie() {
    let (_app, mut client) = signed_in().await;
    let reply = choose(&mut client, "automatic", "/account/password").await;
    assert_sent_to(&reply, "/account/password", "choosing automatic");
    assert_eq!(
        theme_cookies(&reply),
        [(String::new(), owned(&REMOVED))],
        "the appearance cookie of choosing automatic"
    );
    for theme in [None, Some("")] {
        let page = get_with(&mut client, "/account/password", theme).await;
        assert_eq!(
            page.status,
            StatusCode::OK,
            "status of the page ({theme:?})"
        );
        assert_eq!(palette(&page), None, "data-theme of the page ({theme:?})");
    }
}

/// The appearance cookie, kept or removed, carries `Secure` exactly when the session cookie
/// would: trust-proxy on and the first `X-Forwarded-Proto` `https`; not over plain HTTP behind
/// the proxy, and not when trust-proxy is off whatever the header says.
///
/// Proves: decision 124 with decision 108: Secure on the appearance cookie, set or removed, exactly when trust-proxy is on and X-Forwarded-Proto is https
#[tokio::test]
async fn the_appearance_cookie_is_secure_by_the_session_cookies_rule() {
    for trust_proxy in [true, false] {
        let app = App::start_with(Options {
            trust_proxy,
            ..Options::default()
        })
        .await;
        app.set_up().await;
        let mut client = app.signed_in(ADMIN, PASSWORD).await;
        for proto in [Some("https"), Some("HTTPS, http"), Some("http"), None] {
            for (choice, value, attributes) in [
                ("light", "light", KEPT),
                ("dark", "dark", KEPT),
                ("automatic", "", REMOVED),
            ] {
                let what = format!("choosing {choice}, trust-proxy {trust_proxy}, proto {proto:?}");
                let mut request = Req::form("/theme", &[("theme", choice), ("back", "/")]);
                if let Some(proto) = proto {
                    request = request.header("x-forwarded-proto", proto);
                }
                let reply = client.send(request).await;
                assert_sent_to(&reply, "/", &what);
                let mut expected = owned(&attributes);
                let secure = trust_proxy
                    && proto.is_some_and(|proto| proto.starts_with("HTTPS") || proto == "https");
                if secure {
                    expected.push(String::from("Secure"));
                    expected.sort();
                }
                assert_eq!(
                    theme_cookies(&reply),
                    [(value.to_owned(), expected)],
                    "the appearance cookie of {what}"
                );
            }
        }
    }
}

/// A readable value of `theme` other than the three, an empty or missing one, another case
/// of letters and extra spaces included, is refused with 422 and the refusal page: the 403's
/// heading, the sentence of `Refusal::UnknownTheme` and the button back to Nage, the mark and
/// the name not a link, and no menu; it sets no appearance cookie and writes nothing. A body
/// that cannot be read as a form, the field `theme` named twice or another content type, gets
/// the bare answer the older forms give for the same fault, and sets no appearance cookie.
///
/// Proves: decision 124, decision 102 and INV-28: a readable value of theme other than the three is refused with 422 and the refusal page, which carries the mark and the name, not a link, and no menu; a form that cannot be read (the field theme named twice, another content type) gets the bare answer of forms::unreadable; neither sets an appearance cookie
#[tokio::test]
async fn a_choice_the_menu_does_not_offer_is_refused_and_changes_nothing() {
    let (app, mut client) = signed_in().await;
    let sessions = app.session_rows().await;
    let refused = [
        "",
        " ",
        "Light",
        "DARK",
        "Automatic",
        " light",
        "light ",
        "dark\n",
        "sepia",
        "auto",
        "light,dark",
        "<b>dark</b>",
    ];
    for value in refused {
        let what = format!("the choice {value:?}");
        let reply = choose(&mut client, value, "/").await;
        assert_refused(&reply, &what);
        assert!(
            !reply.body.contains("<b>dark"),
            "{what} writes the value into the page:\n{}",
            reply.body
        );
    }
    let missing = client.post("/theme", &[("back", "/")]).await;
    assert_refused(&missing, "a form without the field theme");

    let twice =
        client
            .send(Req::form("/theme", &[]).body(
                form_body(&[("theme", "light"), ("theme", "dark"), ("back", "/")]).into_bytes(),
            ))
            .await;
    let twice_older = client
        .send(
            Req::form("/account/password", &[]).body(
                form_body(&[
                    ("current_password", PASSWORD),
                    ("current_password", PASSWORD),
                    ("new_password", "a new password 9"),
                    ("new_password_again", "a new password 9"),
                ])
                .into_bytes(),
            ),
        )
        .await;
    assert_bare(&twice, &twice_older, "the field theme named twice");
    assert_eq!(
        twice.header("content-type"),
        twice_older.header("content-type"),
        "content type of the answer to a field named twice"
    );

    let other_type = |uri: &str| {
        Req::form(uri, &[])
            .header("content-type", "text/plain")
            .body(b"theme=light&back=/".to_vec())
    };
    let typed = client.send(other_type("/theme")).await;
    let typed_older = client.send(other_type("/account/password")).await;
    assert_bare(&typed, &typed_older, "another content type");
    assert_eq!(
        typed.body, typed_older.body,
        "the answer to another content type differs from the older form's"
    );

    assert_eq!(
        app.session_rows().await,
        sessions,
        "sessions after the refused forms"
    );
    let page = client.get("/").await;
    assert_eq!(palette(&page), None, "data-theme after the refused forms");
}

/// Asserts that `reply` is the 422 refusal of an appearance the menu does not offer.
fn assert_refused(reply: &Reply, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of {what}:\n{}",
        reply.body
    );
    assert!(
        reply.shows(NOT_ACCEPTED),
        "{what} does not show the heading {NOT_ACCEPTED:?}:\n{}",
        reply.body
    );
    assert!(
        reply.shows(&Refusal::UnknownTheme.to_string()),
        "{what} does not say why:\n{}",
        reply.body
    );
    assert!(
        reply.shows("Back to Nage"),
        "{what} has no way back to Nage:\n{}",
        reply.body
    );
    assert!(
        reply.body.contains(r#"<span class="brand">"#)
            && !reply.body.contains(r#"<a class="brand""#),
        "{what} does not carry the mark and the name as no link:\n{}",
        reply.body
    );
    for menu in [r#"action="/theme""#, r#"action="/logout""#, "Signed in as"] {
        assert!(
            !reply.body.contains(menu),
            "{what} carries the menu ({menu}):\n{}",
            reply.body
        );
    }
    assert_no_theme_cookie(reply, what);
    assert!(
        reply.location().is_none(),
        "{what} leads somewhere: {:?}",
        reply.location()
    );
}

/// Asserts that `reply` is the bare answer `older` is: the same status, not 2xx or 3xx, no
/// page of Nage, and no appearance cookie.
fn assert_bare(reply: &Reply, older: &Reply, what: &str) {
    assert!(
        reply.status.is_client_error(),
        "status of {what}: {}",
        reply.status
    );
    assert_eq!(
        reply.status, older.status,
        "status of {what} against the older form's"
    );
    assert!(
        !reply.body.contains("<html"),
        "{what} answers a page:\n{}",
        reply.body
    );
    assert_no_theme_cookie(reply, what);
}

/// `back` is followed only when the sign-in's test for `next` accepts it as a path on this
/// site: one leading slash, no backslash, no control character, at most 2 KiB; another site,
/// a scheme-relative address, a relative one, an empty one, one too long, one with a control
/// character and a form without the field all lead to `/`. A path on this site with a letter outside ASCII, which a
/// page's own address never holds but the test for `next` accepts, is followed as it stands.
///
/// Proves: decision 124 with decision 109: back is followed only when the test for next accepts it as a path on this site, and a back naming another site, or not a plain local path, leads to /
#[tokio::test]
async fn the_way_back_is_a_path_on_this_site_or_the_home_page() {
    let (_app, mut client) = signed_in().await;
    let longest = format!("/{}", "a".repeat(2047));
    let too_long = format!("/{}", "a".repeat(2048));
    let cases: [(&str, &str); 15] = [
        ("/account/password", "/account/password"),
        (
            "/account/password?a=1&b=%3Cx%3E",
            "/account/password?a=1&b=%3Cx%3E",
        ),
        ("/", "/"),
        ("/no/such/page", "/no/such/page"),
        (&longest, &longest),
        (&too_long, "/"),
        ("https://elsewhere.test/", "/"),
        ("//elsewhere.test/x", "/"),
        ("/\\elsewhere.test", "/"),
        ("\\\\elsewhere.test", "/"),
        ("elsewhere.test", "/"),
        ("account/password", "/"),
        ("", "/"),
        ("/a\u{7}b", "/"),
        ("/a\nLocation: //elsewhere.test", "/"),
    ];
    for (back, expected) in cases {
        let reply = choose(&mut client, "dark", back).await;
        let what = format!("a choice with back {back:?}");
        assert_sent_to(&reply, expected, &what);
        assert_eq!(
            reply.headers.get_all("location").iter().count(),
            1,
            "Location headers of {what}"
        );
        assert_eq!(
            theme_cookies(&reply),
            [(String::from("dark"), owned(&KEPT))],
            "the appearance cookie of {what}"
        );
    }

    let accented = "/caf\u{e9}";
    let reply = choose(&mut client, "dark", accented).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of a choice with back {accented:?}"
    );
    assert_eq!(
        reply.headers.get("location").map(|value| value.as_bytes()),
        Some(accented.as_bytes()),
        "Location of a choice with back {accented:?}"
    );

    let without_back = client.post("/theme", &[("theme", "dark")]).await;
    assert_sent_to(&without_back, "/", "a choice without the field back");
    assert_eq!(
        theme_cookies(&without_back),
        [(String::from("dark"), owned(&KEPT))],
        "the appearance cookie of a choice without the field back"
    );
}

/// The appearance form on a page asked for at an address with a query holds that path and
/// query in `back`, escaped like any text, in both copies of the menu; the choice sent from it
/// lands on that address again. The 422 page of a refused change of password and the 429 page
/// of a paused one hold the change-password page's own address in `back` in both copies,
/// never the home page.
///
/// Proves: decision 124: the form on a page asked for at an address with a query holds that path and query in back, in both copies of the menu, and the choice sent from it lands on that address again
#[tokio::test]
async fn the_choice_returns_to_the_page_the_form_stands_on() {
    let (_app, mut client) = signed_in().await;
    for address in [
        "/account/password?a=1&b=%3Cx%3E",
        "/?view=all",
        "/",
        "/account/password",
    ] {
        let page = client.get(address).await;
        assert_eq!(page.status, StatusCode::OK, "status of {address}");
        assert_eq!(
            backs(&page),
            [address, address],
            "the field back on {address}"
        );
        if address.contains('&') {
            assert!(
                !page.body.contains("?a=1&b="),
                "the field back on {address} is not escaped:\n{}",
                page.body
            );
        }
        let back = backs(&page).remove(0);
        let reply = choose(&mut client, "light", &back).await;
        assert_sent_to(&reply, address, &format!("the choice sent from {address}"));
    }

    let address = "/account/password";
    let wrong = [
        ("current_password", "not the password 1"),
        ("new_password", "tulip window 99"),
        ("new_password_again", "tulip window 99"),
    ];
    for number in 1..=6 {
        let reply = client.post(address, &wrong).await;
        let (status, what) = if number <= 5 {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("the 422 page of refused change {number}"),
            )
        } else {
            (
                StatusCode::TOO_MANY_REQUESTS,
                String::from("the 429 page of a paused change"),
            )
        };
        assert_eq!(reply.status, status, "status of {what}:\n{}", reply.body);
        assert_eq!(
            backs(&reply),
            [address, address],
            "the field back on {what}"
        );
    }
}

/// On the page for an unknown address `back` holds `/` in both copies of the menu, the page
/// holds neither the address nor its query, and the choice sent from it lands on `/`.
///
/// Proves: decision 124 with decision 118: on the page for an unknown address back holds / in both copies of the menu and the page holds neither the address nor its query, and the choice sent from it lands on /
#[tokio::test]
async fn the_form_on_the_page_for_an_unknown_address_leads_home() {
    let (_app, mut client) = signed_in().await;
    let page = client.get("/no/such/nwqplace?nwqquery=1").await;
    assert_eq!(
        page.status,
        StatusCode::NOT_FOUND,
        "status of the unknown address"
    );
    assert_eq!(
        backs(&page),
        ["/", "/"],
        "the field back on the unknown address"
    );
    for part in ["nwqplace", "nwqquery", "no/such"] {
        assert!(
            !page.body.contains(part),
            "the page for an unknown address holds {part:?}:\n{}",
            page.body
        );
    }
    let back = backs(&page).remove(0);
    let reply = choose(&mut client, "dark", &back).await;
    assert_sent_to(&reply, "/", "the choice sent from the unknown address");
}

/// `POST /theme` sits behind the session rule and the Origin check. As a control, the form
/// with a valid session and Nage's own `Origin` is accepted with 303 and sets `nage_theme`;
/// without a session it is sent to the sign-in page, and from another site, or with no
/// `Origin` at all, it gets the 403 page; neither sets an appearance cookie. The route answers
/// a POST only: `GET /theme` with the choice in its query, with a valid session, gets the bare
/// 405 that names POST, and sets no appearance cookie.
///
/// Proves: INV-27, INV-28 and decision 110 for POST /theme: as a control, the same form with a valid session and Nage's own Origin is accepted with 303 and sets nage_theme, which reaches the route's handler; then without a session it leads to the sign-in page and with another site's Origin it gets the 403, and neither sets an appearance cookie
#[tokio::test]
async fn the_appearance_form_needs_a_session_and_passes_the_origin_check() {
    let (app, mut client) = signed_in().await;
    let fields = [("theme", "light"), ("back", "/account/password")];

    let control = client.post("/theme", &fields).await;
    assert_sent_to(&control, "/account/password", "the control");
    assert_eq!(
        theme_cookies(&control),
        [(String::from("light"), owned(&KEPT))],
        "the appearance cookie of the control"
    );

    let typed = client.get("/theme?theme=dark&back=/").await;
    assert_eq!(
        typed.status,
        StatusCode::METHOD_NOT_ALLOWED,
        "status of GET /theme with the choice in its query:\n{}",
        typed.body
    );
    assert_eq!(typed.body, "", "the body of the 405 of GET /theme");
    assert_eq!(
        typed.header("allow"),
        Some("POST"),
        "the Allow header of the 405 of GET /theme"
    );
    assert_no_theme_cookie(&typed, "GET /theme with the choice in its query");

    let mut stranger = app.client();
    let unsigned = stranger.post("/theme", &fields).await;
    assert_sent_to(&unsigned, "/login", "the form without a session");
    assert_no_theme_cookie(&unsigned, "the form without a session");

    let mut forged = app.client();
    forged.cookie = Some("0".repeat(64));
    let unknown = forged.post("/theme", &fields).await;
    assert_sent_to(&unknown, "/login", "the form with an unknown session");
    assert_no_theme_cookie(&unknown, "the form with an unknown session");

    for (origin, what) in [
        (Some("http://elsewhere.test"), "the form from another site"),
        (None, "the form with no Origin"),
    ] {
        let mut request = Req::form("/theme", &fields);
        request = match origin {
            Some(origin) => request.header("origin", origin),
            None => request.without("origin"),
        };
        let reply = client.send(request).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "status of {what}:\n{}",
            reply.body
        );
        assert!(
            reply.shows(&Refusal::CrossSite.to_string()),
            "{what} does not show the 403 page:\n{}",
            reply.body
        );
        assert_no_theme_cookie(&reply, what);
    }
}

/// Every page wears `data-theme` from the cookie's two values: the sign-in page, the set-up
/// page and its closed page, the 403, 413 and 422 refusal pages, the home page, the
/// change-password page, the page for an unknown address and the busy page; the sign-in page
/// and the closed set-up page wear it too when another cookie in the same `Cookie` header holds
/// a letter outside ASCII, whether written as UTF-8 or as a lone byte that is not UTF-8.
///
/// Proves: decision 124: data-theme light or dark on every page, the sign-in, set-up, refusal, unknown-address and busy pages included
#[tokio::test]
async fn every_page_wears_the_palette_its_cookie_names() {
    for choice in ["light", "dark"] {
        let theme = Some(choice);
        let mut seen: Vec<(String, Reply)> = Vec::new();

        let fresh = App::start().await;
        let mut nobody = fresh.client();
        seen.push((
            String::from("the set-up page"),
            get_with(&mut nobody, "/setup", theme).await,
        ));

        let busy = impatient().await;
        let mut early = busy.client();
        let form = [
            ("book_name", "Household"),
            ("currency", "CAD"),
            ("username", ADMIN),
            ("display_name", "Sam"),
            ("password", PASSWORD),
            ("password_again", PASSWORD),
        ];
        let held = busy.write().await;
        let reply = send_with(&mut early, Req::form("/setup", &form), theme).await;
        drop(held);
        assert_eq!(
            reply.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "status of the busy set-up:\n{}",
            reply.body
        );
        seen.push((String::from("the busy page"), reply));

        let (app, mut client) = signed_in().await;
        let mut stranger = app.client();
        seen.push((
            String::from("the sign-in page"),
            get_with(&mut stranger, "/login", theme).await,
        ));
        seen.push((
            String::from("the closed set-up page"),
            get_with(&mut stranger, "/setup", theme).await,
        ));
        seen.push((
            String::from("the 403 page"),
            send_with(
                &mut stranger,
                Req::form("/login", &[("username", ADMIN), ("password", PASSWORD)])
                    .header("origin", "http://elsewhere.test"),
                theme,
            )
            .await,
        ));
        let large = "x".repeat(70_000);
        seen.push((
            String::from("the 413 page"),
            send_with(
                &mut stranger,
                Req::form("/login", &[("username", &large), ("password", PASSWORD)]),
                theme,
            )
            .await,
        ));
        seen.push((
            String::from("the 422 page"),
            send_with(
                &mut client,
                Req::form("/theme", &[("theme", "sepia"), ("back", "/")]),
                theme,
            )
            .await,
        ));
        seen.push((
            String::from("the home page"),
            get_with(&mut client, "/", theme).await,
        ));
        seen.push((
            String::from("the change-password page"),
            get_with(&mut client, "/account/password", theme).await,
        ));
        seen.push((
            String::from("the page for an unknown address"),
            get_with(&mut client, "/no/such/page", theme).await,
        ));

        let expected = [
            ("the set-up page", StatusCode::OK),
            ("the busy page", StatusCode::SERVICE_UNAVAILABLE),
            ("the sign-in page", StatusCode::OK),
            ("the closed set-up page", StatusCode::NOT_FOUND),
            ("the 403 page", StatusCode::FORBIDDEN),
            ("the 413 page", StatusCode::PAYLOAD_TOO_LARGE),
            ("the 422 page", StatusCode::UNPROCESSABLE_ENTITY),
            ("the home page", StatusCode::OK),
            ("the change-password page", StatusCode::OK),
            ("the page for an unknown address", StatusCode::NOT_FOUND),
        ];
        assert_eq!(seen.len(), expected.len(), "pages seen");
        for ((what, reply), (named, status)) in seen.iter().zip(expected) {
            assert_eq!(what, named, "the order of the pages");
            assert_eq!(
                reply.status, status,
                "status of {what} ({choice}):\n{}",
                reply.body
            );
            assert_eq!(
                palette(reply).as_deref(),
                Some(choice),
                "data-theme of {what} with the cookie {choice}"
            );
            assert_no_theme_cookie(reply, what);
        }

        let accented = format!("lang=caf\u{e9}; {THEME_COOKIE}={choice}");
        for address in ["/login", "/setup"] {
            let reply = app
                .client()
                .send(Req::get(address).header("cookie", &accented))
                .await;
            let what =
                format!("{address} with {THEME_COOKIE}={choice} after a cookie outside ASCII");
            assert!(
                matches!(reply.status, StatusCode::OK | StatusCode::NOT_FOUND),
                "status of {what}:\n{}",
                reply.body
            );
            assert_eq!(
                palette(&reply).as_deref(),
                Some(choice),
                "data-theme of {what}"
            );

            let mut raw = b"lang=caf\xe9; ".to_vec();
            raw.extend_from_slice(format!("{THEME_COOKIE}={choice}").as_bytes());
            let reply = get_with_raw_cookie(&app, address, &raw).await;
            let what = format!(
                "{address} with {THEME_COOKIE}={choice} beside a cookie holding the lone byte 0xE9, not UTF-8"
            );
            assert!(
                matches!(reply.status, StatusCode::OK | StatusCode::NOT_FOUND),
                "status of {what}:\n{}",
                reply.body
            );
            assert_eq!(
                palette(&reply).as_deref(),
                Some(choice),
                "data-theme of {what}"
            );
        }
    }
}

/// A cookie value that is a marker no page contains, and one with markup characters, appear
/// nowhere in the page; those and `automatic`, the empty value, `Light`, `DARK`, `light`
/// followed by a space, and a missing cookie set no `data-theme`, and the menu marks Automatic.
///
/// Proves: decision 124: a cookie value that is a marker no page contains, and one with markup characters, appear nowhere in the page; automatic, the empty value, Light, DARK, light followed by a space and a missing cookie set no data-theme
#[tokio::test]
async fn an_unknown_or_missing_palette_means_automatic_and_is_never_echoed() {
    let (_app, mut client) = signed_in().await;
    let markup = format!("\"><i>{MARKER}</i>");
    let values = [
        Some(MARKER),
        Some(markup.as_str()),
        Some("automatic"),
        Some(""),
        Some("Light"),
        Some("DARK"),
        Some("light "),
        None,
    ];
    for value in values {
        let mut replies = Vec::new();
        for address in ["/", "/account/password", "/no/such/page"] {
            replies.push((address, get_with(&mut client, address, value).await));
        }
        replies.push((
            "the 403 page",
            send_with(
                &mut client,
                Req::form("/logout", &[]).header("origin", "http://elsewhere.test"),
                value,
            )
            .await,
        ));
        for (what, reply) in &replies {
            assert!(
                matches!(
                    reply.status,
                    StatusCode::OK | StatusCode::NOT_FOUND | StatusCode::FORBIDDEN
                ),
                "status of {what} with {value:?}:\n{}",
                reply.body
            );
            assert_eq!(
                palette(reply),
                None,
                "data-theme of {what} with the cookie {value:?}"
            );
            assert!(
                !reply.body.contains(MARKER),
                "{what} echoes the cookie {value:?}:\n{}",
                reply.body
            );
            assert!(
                !reply.body.contains("<i>"),
                "{what} holds the cookie's markup with {value:?}:\n{}",
                reply.body
            );
            assert_no_theme_cookie(reply, what);
            for (choice, pressed) in appearance_buttons(reply) {
                let expected = if choice == "automatic" {
                    "true"
                } else {
                    "false"
                };
                assert_eq!(
                    pressed, expected,
                    "aria-pressed of {choice} on {what} with {value:?}"
                );
            }
        }
    }
}

/// The menu's three buttons, in both copies of the menu, carry `aria-pressed="true"` on the
/// choice in force and `"false"` on the other two: Automatic without a cookie, Light and Dark
/// with theirs, on every page that carries the menu. The cookie is found by its whole name
/// wherever it stands in the `Cookie` header, after the session's or after a cookie whose
/// name only begins with `nage_theme`, and in a second `Cookie` header beside the one that
/// holds the session's, after a tab as after a space; a cookie whose name only begins or only
/// ends with `nage_theme`, whose name is `nage_theme` only after or only before a no-break
/// space, or whose value holds `nage_theme=dark`, decides nothing; of two `nage_theme` cookies
/// in one header the first decides the palette and the pressed button (decision 124's known
/// limit), even when its value is unknown or empty, which means Automatic; and of two `Cookie`
/// headers that each name `nage_theme`, the first header's cookie decides.
///
/// Proves: decision 124: aria-pressed true on the choice in force and false on the other two, in both copies of the menu
#[tokio::test]
async fn the_menu_marks_the_choice_in_force() {
    let (_app, mut client) = signed_in().await;
    for (cookie, in_force) in [
        (None, "automatic"),
        (Some("light"), "light"),
        (Some("dark"), "dark"),
        (Some("automatic"), "automatic"),
    ] {
        for address in ["/", "/account/password", "/no/such/page"] {
            let reply = get_with(&mut client, address, cookie).await;
            let what = format!("{address} with the cookie {cookie:?}");
            assert_eq!(
                appearance_buttons(&reply),
                pressed_in_order(in_force),
                "the appearance buttons of {what}"
            );
        }
    }

    let token = client
        .cookie
        .clone()
        .expect("the signed-in client holds a session");
    for (cookies, in_force) in [
        (
            format!("{SESSION_COOKIE}={token}; {THEME_COOKIE}=dark"),
            "dark",
        ),
        (
            format!(
                "other=1; {THEME_COOKIE}_old=dark; {THEME_COOKIE}=light; {SESSION_COOKIE}={token}"
            ),
            "light",
        ),
        (
            format!("{THEME_COOKIE}s=dark; {SESSION_COOKIE}={token}"),
            "automatic",
        ),
        (
            format!("x_{THEME_COOKIE}=dark; {SESSION_COOKIE}={token}"),
            "automatic",
        ),
        (
            format!("other={THEME_COOKIE}=dark; {SESSION_COOKIE}={token}"),
            "automatic",
        ),
        (
            format!("{SESSION_COOKIE}={token};\t{THEME_COOKIE}=dark"),
            "dark",
        ),
        (
            format!("{THEME_COOKIE}=dark; {SESSION_COOKIE}={token}; {THEME_COOKIE}=light"),
            "dark",
        ),
        (
            format!("{THEME_COOKIE}=light; {THEME_COOKIE}=dark; {SESSION_COOKIE}={token}"),
            "light",
        ),
        (
            format!("{THEME_COOKIE}=sepia; {THEME_COOKIE}=dark; {SESSION_COOKIE}={token}"),
            "automatic",
        ),
        (
            format!("{THEME_COOKIE}=; {THEME_COOKIE}=dark; {SESSION_COOKIE}={token}"),
            "automatic",
        ),
    ] {
        let reply = client.send(Req::get("/").header("cookie", &cookies)).await;
        let what = format!("/ with the Cookie header {cookies:?}");
        assert_marks(&reply, in_force, &what);
    }

    for (theme, in_force) in [("dark", "dark"), ("light", "light")] {
        let reply = client
            .send(
                Req::get("/")
                    .header("cookie", &format!("{SESSION_COOKIE}={token}"))
                    .also("cookie", &format!("{THEME_COOKIE}={theme}")),
            )
            .await;
        let what = format!("/ with {THEME_COOKIE}={theme} in a second Cookie header");
        assert_marks(&reply, in_force, &what);
    }

    let reply = client
        .send(
            Req::get("/")
                .header(
                    "cookie",
                    &format!("{SESSION_COOKIE}={token}; {THEME_COOKIE}=dark"),
                )
                .also("cookie", &format!("{THEME_COOKIE}=light")),
        )
        .await;
    let what = format!(
        "/ with {THEME_COOKIE}=dark in the first Cookie header and {THEME_COOKIE}=light in a later one"
    );
    assert_marks(&reply, "dark", &what);

    for disguised in [
        format!("\u{a0}{THEME_COOKIE}=light; {THEME_COOKIE}=dark"),
        format!("{THEME_COOKIE}\u{a0}=light; {THEME_COOKIE}=dark"),
    ] {
        let reply = client
            .send(
                Req::get("/")
                    .header("cookie", &format!("{SESSION_COOKIE}={token}"))
                    .also("cookie", &disguised),
            )
            .await;
        let what = format!("/ with the second Cookie header {disguised:?}");
        assert_marks(&reply, "dark", &what);
    }
}

/// Asserts that `reply` is a page of the menu that wears `in_force` (`data-theme`, none for
/// Automatic) and marks it as the choice in force in both copies of the menu.
fn assert_marks(reply: &Reply, in_force: &str, what: &str) {
    assert_eq!(reply.status, StatusCode::OK, "status of {what}");
    assert_eq!(
        palette(reply).as_deref(),
        (in_force != "automatic").then_some(in_force),
        "data-theme of {what}"
    );
    assert_eq!(
        appearance_buttons(reply),
        pressed_in_order(in_force),
        "the appearance buttons of {what}"
    );
}

/// The menu's appearance buttons as both copies of the menu carry them with `in_force` the
/// choice in force: each value with its `aria-pressed`, in the order of the page.
fn pressed_in_order(in_force: &str) -> Vec<(String, String)> {
    ["automatic", "light", "dark"]
        .iter()
        .cycle()
        .take(6)
        .map(|choice| {
            let pressed = if *choice == in_force { "true" } else { "false" };
            ((*choice).to_owned(), pressed.to_owned())
        })
        .collect()
}
