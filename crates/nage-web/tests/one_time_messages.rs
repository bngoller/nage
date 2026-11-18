//! One-time messages (SPEC 5.2, 5.3, INV-28, decision 117): the confirmation of a change of
//! one's own password and of a sign-out, each shown once at the top of the next page; the
//! cookie they travel in; and the session rule's check after a touch, which looks for the
//! session cookie by name. The overtaken change's message is in `password_page.rs`, beside
//! the change it follows, with the rule's other check, the clearing of a cookie that names
//! no valid session.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use nage_core::storage::StorageOptions;
use nage_web::messages::Notice;

use common::app::{ADMIN, App, Client, HOUR, Options, PASSWORD, Reply, Req, START, WRONG_PASSWORD};
use common::{asset, hashed_url};

/// The password the tests change to.
const NEW_PASSWORD: &str = "tulip window 99";

/// The attributes of the message's cookie, sorted, without `Secure`.
const ATTRIBUTES: [&str; 4] = ["HttpOnly", "Max-Age=60", "Path=/", "SameSite=Lax"];

/// The attributes of the cookie that clears the message, sorted, without `Secure`.
const CLEARED: [&str; 4] = ["HttpOnly", "Max-Age=0", "Path=/", "SameSite=Lax"];

/// 2199-12-31T12:00:00Z, at which it is already 2200 in `Pacific/Kiritimati`: the instant
/// whose home page has no today (`home_page.rs`).
const NOON_ON_THE_LAST_DAY: i64 = 7_258_075_200;

/// The first second after 2199-12-31T23:59:59Z, the last instant a stamp holds: a clock
/// that shows it cannot be read.
const PAST_THE_LAST_INSTANT: i64 = 7_258_118_400;

/// An accepted change of the password from [`PASSWORD`] to [`NEW_PASSWORD`].
async fn change(client: &mut Client) -> Reply {
    let reply = client
        .post(
            "/account/password",
            &[
                ("current_password", PASSWORD),
                ("new_password", NEW_PASSWORD),
                ("new_password_again", NEW_PASSWORD),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the change:\n{}",
        reply.body
    );
    reply
}

/// Asserts that `page` shows exactly the one-time message `text` as a status, above its
/// heading, and clears the message's cookie.
#[track_caller]
fn assert_confirms_once(page: &Reply, text: &str, what: &str) {
    assert_eq!(
        page.notices(),
        [(String::from("status"), text.to_owned())],
        "the notices of {what}:\n{}",
        page.body
    );
    let notice = page.body.find(r#"<p class="notice""#).unwrap_or_default();
    let heading = page.body.find("<h1>").unwrap_or_default();
    assert!(
        notice < heading,
        "the message of {what} does not stand above the heading:\n{}",
        page.body
    );
    assert_eq!(
        page.message_cookie(),
        Some((String::new(), CLEARED.map(str::to_owned).to_vec())),
        "the cookie {what} clears: {:?}",
        page.set_cookies()
    );
}

/// Asserts that `page` shows no notice and neither sets nor clears the message's cookie.
#[track_caller]
fn assert_says_nothing(page: &Reply, what: &str) {
    assert_eq!(
        page.notices(),
        Vec::<(String, String)>::new(),
        "the notices of {what}:\n{}",
        page.body
    );
    assert_eq!(
        page.message_cookie(),
        None,
        "the message cookie of {what}: {:?}",
        page.set_cookies()
    );
}

/// An accepted change of one's own password answers 303 back to the page, with no marker in
/// the address, and sets one cookie, the message's: `nage_message=password-changed` with
/// `Path=/`, `HttpOnly`, `SameSite=Lax` and `Max-Age=60`; the session cookie is left as it
/// is. The page then shows the confirmation once, at its top, as a status, and clears the
/// cookie. A reload shows no confirmation, and neither does any later page; the old marker
/// in the address shows nothing.
///
/// Proves: decision 117 (after an accepted change the change-password page shows decision
/// 112's confirmation once; a reload shows none of them again; the cookie holds a code and
/// carries `Path=/`, `HttpOnly`, `SameSite=Lax` and a short lifetime; the marker of decision
/// 112 is replaced), decision 112.
#[tokio::test]
async fn an_accepted_change_is_confirmed_once() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let session = client.cookie.clone();
    let reply = change(&mut client).await;
    assert_eq!(
        reply.location(),
        Some("/account/password"),
        "where an accepted change leads"
    );
    assert_eq!(
        reply.message_cookie(),
        Some((
            String::from("password-changed"),
            ATTRIBUTES.map(str::to_owned).to_vec()
        )),
        "the message cookie of an accepted change"
    );
    assert_eq!(
        reply.set_cookies().len(),
        1,
        "cookies set by an accepted change: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        client.cookie, session,
        "the session cookie after the change"
    );

    let page = client.get("/account/password").await;
    assert_eq!(
        page.status,
        StatusCode::OK,
        "GET of the page after the change"
    );
    assert_confirms_once(
        &page,
        &Notice::PasswordChanged.to_string(),
        "the change-password page after the change",
    );
    let reload = client.get("/account/password").await;
    assert_says_nothing(&reload, "the change-password page, reloaded");
    let home = client.get("/").await;
    assert_eq!(home.status, StatusCode::OK, "GET / after the change");
    assert_says_nothing(&home, "the home page after the confirmation was shown");
    let marker = client.get("/account/password?changed").await;
    assert_eq!(
        marker.status,
        StatusCode::OK,
        "GET of the page with the old marker"
    );
    assert_says_nothing(&marker, "the change-password page with the old marker");
}

/// Signing out answers 303 to the sign-in page with two cookies: the one that clears the
/// session, and `nage_message=signed-out`. The sign-in page then says once, at its top, that
/// the person is signed out, and clears the message's cookie; a reload says nothing.
///
/// Proves: decision 117 (after signing out the sign-in page says that the person is signed
/// out, once; the message is set on the 303 of a sign-out), decision 106.
#[tokio::test]
async fn a_sign_out_is_confirmed_once_on_the_sign_in_page() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reply = client.post("/logout", &[]).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "POST /logout status");
    assert_eq!(reply.location(), Some("/login"), "where sign-out leads");
    assert!(
        reply.clears_the_cookie(),
        "sign-out does not clear the session cookie: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        reply.message_cookie(),
        Some((
            String::from("signed-out"),
            ATTRIBUTES.map(str::to_owned).to_vec()
        )),
        "the message cookie of a sign-out"
    );
    assert_eq!(
        reply.set_cookies().len(),
        2,
        "cookies set by a sign-out: {:?}",
        reply.set_cookies()
    );

    let page = client.get("/login").await;
    assert_eq!(page.status, StatusCode::OK, "GET /login after the sign-out");
    assert_confirms_once(
        &page,
        "You are signed out.",
        "the sign-in page after a sign-out",
    );
    assert!(
        page.cookies_named("nage_session").is_empty(),
        "the sign-in page sets or clears a session cookie nobody sent: {:?}",
        page.set_cookies()
    );
    let reload = client.get("/login").await;
    assert_says_nothing(&reload, "the sign-in page, reloaded");
}

/// A message is shown by the next page a GET renders among those that read it, whatever page
/// that is, and by no page after it: after a change, the home page shows the confirmation when
/// it is opened instead of the change-password page, and so does the page for an unknown
/// address; after a sign-out, the closed set-up page shows that the person is signed out. An
/// answer that is no page, the 303 that leads a person without a session to the sign-in page,
/// leaves the message for the page it leads to.
///
/// Proves: decision 117 (the next page a GET renders with the layout shows the message and
/// clears the cookie, whatever page that is among those that read it, the page for an
/// unknown address and the closed set-up page among them; neither does any later page).
#[tokio::test]
async fn a_message_is_shown_by_whatever_page_comes_next() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    change(&mut client).await;
    let home = client.get("/").await;
    assert_eq!(home.status, StatusCode::OK, "GET / after the change");
    assert_confirms_once(
        &home,
        &Notice::PasswordChanged.to_string(),
        "the home page after the change",
    );
    let page = client.get("/account/password").await;
    assert_says_nothing(
        &page,
        "the change-password page after the home page showed it",
    );

    let mut other = app.signed_in(ADMIN, NEW_PASSWORD).await;
    other.message = Some(String::from("password-changed"));
    let unknown = other.get("/no-such-page").await;
    assert_eq!(
        unknown.status,
        StatusCode::NOT_FOUND,
        "GET /no-such-page with a message"
    );
    assert_confirms_once(
        &unknown,
        &Notice::PasswordChanged.to_string(),
        "the page for an unknown address",
    );

    let signed_out = client.post("/logout", &[]).await;
    assert_eq!(
        signed_out.status,
        StatusCode::SEE_OTHER,
        "POST /logout status"
    );
    let redirect = client.get("/account/password").await;
    assert_eq!(
        redirect.status,
        StatusCode::SEE_OTHER,
        "GET /account/password after the sign-out"
    );
    assert_eq!(
        redirect.message_cookie(),
        None,
        "the message cookie of the 303 to the sign-in page: {:?}",
        redirect.set_cookies()
    );
    let closed = client.get("/setup").await;
    assert_eq!(
        closed.status,
        StatusCode::NOT_FOUND,
        "GET /setup after the sign-out"
    );
    assert_confirms_once(&closed, "You are signed out.", "the closed set-up page");
    let sign_in = client.get("/login").await;
    assert_says_nothing(
        &sign_in,
        "the sign-in page after the closed set-up page showed it",
    );
}

/// A HEAD reads nothing, on each page that reads the message: after a change, a HEAD of the
/// change-password page, of the home page and of an unknown address neither shows nor clears
/// the message, and the GET that follows shows it; after a sign-out, a HEAD of the sign-in
/// page and of the closed set-up page leaves the sign-out's message for the sign-in page's
/// GET; and on a fresh server a HEAD of the set-up page leaves a waiting message as it is.
///
/// Proves: decision 117 (a HEAD reads nothing, having no body to show the message in, on each
/// page that reads it: the change-password page, the home page, the page for an unknown
/// address, the sign-in page, the set-up page and its closed page).
#[tokio::test]
async fn a_head_leaves_the_message_for_the_page_that_can_show_it() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    change(&mut client).await;
    for (uri, status) in [
        ("/account/password", StatusCode::OK),
        ("/", StatusCode::OK),
        ("/no-such-page", StatusCode::NOT_FOUND),
    ] {
        let head = client.head(uri).await;
        assert_eq!(head.status, status, "HEAD {uri} status");
        assert_eq!(
            head.message_cookie(),
            None,
            "the message cookie of HEAD {uri}: {:?}",
            head.set_cookies()
        );
    }
    assert_eq!(
        client.message.as_deref(),
        Some("password-changed"),
        "the message the client holds after the three HEADs"
    );
    let page = client.get("/account/password").await;
    assert_confirms_once(
        &page,
        &Notice::PasswordChanged.to_string(),
        "the change-password page after the three HEADs",
    );

    let signed_out = client.post("/logout", &[]).await;
    assert_eq!(
        signed_out.status,
        StatusCode::SEE_OTHER,
        "POST /logout status"
    );
    for (uri, status) in [
        ("/login", StatusCode::OK),
        ("/setup", StatusCode::NOT_FOUND),
    ] {
        let head = client.head(uri).await;
        assert_eq!(head.status, status, "HEAD {uri} status after the sign-out");
        assert_eq!(
            head.message_cookie(),
            None,
            "the message cookie of HEAD {uri} after the sign-out: {:?}",
            head.set_cookies()
        );
    }
    let page = client.get("/login").await;
    assert_confirms_once(
        &page,
        "You are signed out.",
        "the sign-in page after the two HEADs",
    );
    let fresh = App::start().await;
    let mut nobody = fresh.client();
    nobody.message = Some(String::from("signed-out"));
    let head = nobody.head("/setup").await;
    assert_eq!(head.status, StatusCode::OK, "HEAD /setup on a fresh server");
    assert_eq!(
        head.message_cookie(),
        None,
        "the message cookie of HEAD /setup on a fresh server: {:?}",
        head.set_cookies()
    );
}

/// A cookie that holds anything but one of Nage's codes puts nothing on a page and is
/// cleared: a made-up code, a sentence, markup and an empty value each show no notice, and
/// none of their text appears anywhere in the page, on the home page with a session and on
/// the sign-in page without one.
///
/// Proves: decision 117 (the cookie holds a code from a fixed set, never text; a code Nage
/// does not know is ignored and cleared; whoever can set cookies in that browser can only
/// make Nage's own fixed messages appear, never other words).
#[tokio::test]
async fn a_code_nage_does_not_know_shows_nothing_and_is_cleared() {
    let app = App::with_book().await;
    let mut signed_in = app.signed_in(ADMIN, PASSWORD).await;
    for value in [
        "zzyzx-made-up",
        "Your%20zzyzx%20account%20is%20locked",
        "%3Cb%3Ezzyzx%3C/b%3E",
        "PASSWORD-CHANGED",
        "",
    ] {
        signed_in.message = Some(value.to_owned());
        let home = signed_in.get("/").await;
        assert_eq!(
            home.status,
            StatusCode::OK,
            "GET / with the cookie {value:?}"
        );
        assert!(
            home.notices().is_empty() && !home.body.contains("zzyzx"),
            "the home page shows something of the cookie {value:?}:\n{}",
            home.body
        );
        assert!(
            home.clears_the_message(),
            "the home page does not clear the cookie {value:?}: {:?}",
            home.set_cookies()
        );

        let mut anonymous = app.client();
        anonymous.message = Some(value.to_owned());
        let sign_in = anonymous.get("/login").await;
        assert!(
            sign_in.notices().is_empty() && !sign_in.body.contains("zzyzx"),
            "the sign-in page shows something of the cookie {value:?}:\n{}",
            sign_in.body
        );
        assert!(
            sign_in.clears_the_message(),
            "the sign-in page does not clear the cookie {value:?}: {:?}",
            sign_in.set_cookies()
        );
    }
}

/// A message shows only on a page where it is true, and is cleared all the same where it is
/// not: the sign-out's confirmation on the home page of a person who is signed in, and the
/// change's confirmation on the sign-in page of a person who is not, each show no notice,
/// and each page clears the cookie.
///
/// Proves: decision 117 (a message shows only where it is true: the sign-out's on a page
/// without a signed-in user, the change's on a page with one; the cookie is cleared all the
/// same).
#[tokio::test]
async fn a_message_shows_only_on_a_page_where_it_is_true() {
    let app = App::with_book().await;
    let mut signed_in = app.signed_in(ADMIN, PASSWORD).await;
    signed_in.message = Some(String::from("signed-out"));
    let home = signed_in.get("/").await;
    assert_eq!(
        home.status,
        StatusCode::OK,
        "GET / with a sign-out's message"
    );
    assert!(
        home.notices().is_empty(),
        "the home page of a person signed in shows a sign-out's message:\n{}",
        home.body
    );
    assert!(
        home.clears_the_message(),
        "the home page does not clear the sign-out's message: {:?}",
        home.set_cookies()
    );

    let mut anonymous = app.client();
    anonymous.message = Some(String::from("password-changed"));
    let sign_in = anonymous.get("/login").await;
    assert_eq!(
        sign_in.status,
        StatusCode::OK,
        "GET /login with a change's message"
    );
    assert!(
        sign_in.notices().is_empty(),
        "the sign-in page shows a change's confirmation to nobody signed in:\n{}",
        sign_in.body
    );
    assert!(
        sign_in.clears_the_message(),
        "the sign-in page does not clear the change's message: {:?}",
        sign_in.set_cookies()
    );
}

/// An answer that is no page of the layout leaves a waiting message as it is: with the
/// change's confirmation waiting, `GET /health`, the `GET` of an asset, the bare 404 of
/// `/static/`, the bare 405 of `GET /logout` and the 500 of a home page that has no today each
/// answer without a `Set-Cookie` for the message, and the change-password page that follows
/// shows it once and clears it. On a fresh server the set-up page shows a waiting sign-out
/// message once and clears it as well.
///
/// Proves: decision 117 (`/health` and `/static/*` never read the message; a bare answer of
/// the router and a failure leave it as it is; the next page a GET renders among those that
/// read it shows and clears it, the set-up page among them).
#[tokio::test]
async fn an_answer_that_is_no_page_leaves_the_message_waiting() {
    let app = App::start_with(Options {
        zone: Some("Pacific/Kiritimati"),
        ..Options::default()
    })
    .await;
    app.clock.set(NOON_ON_THE_LAST_DAY - START);
    app.set_up().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    client.message = Some(String::from("password-changed"));
    let asset_url = hashed_url("app.css", asset("app.css").body);
    for (uri, status) in [
        ("/health", StatusCode::OK),
        (asset_url.as_str(), StatusCode::OK),
        ("/static/", StatusCode::NOT_FOUND),
        ("/logout", StatusCode::METHOD_NOT_ALLOWED),
        ("/", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let reply = client.get(uri).await;
        assert_eq!(reply.status, status, "GET {uri} with a waiting message");
        assert_eq!(
            reply.message_cookie(),
            None,
            "the message cookie of GET {uri}: {:?}",
            reply.set_cookies()
        );
    }
    assert_eq!(
        client.message.as_deref(),
        Some("password-changed"),
        "the message the client holds after those answers"
    );
    let page = client.get("/account/password").await;
    assert_eq!(page.status, StatusCode::OK, "GET /account/password");
    assert_confirms_once(
        &page,
        &Notice::PasswordChanged.to_string(),
        "the change-password page after those answers",
    );

    let fresh = App::start().await;
    let mut nobody = fresh.client();
    nobody.message = Some(String::from("signed-out"));
    let setup = nobody.get("/setup").await;
    assert_eq!(setup.status, StatusCode::OK, "GET /setup on a fresh server");
    assert_confirms_once(&setup, "You are signed out.", "the set-up page");
    let again = nobody.get("/setup").await;
    assert_says_nothing(&again, "the set-up page, reloaded");
}

/// The message's cookie carries `Secure` by the session cookie's rule: exactly when
/// trust-proxy is on and the request's `X-Forwarded-Proto` is `https`, where it is set and
/// where it is cleared; not with trust-proxy on over plain HTTP, and not with trust-proxy
/// off whatever the header says.
///
/// Proves: decision 117 (the message's cookie carries `Secure` by decision 108's rule where
/// a sign-out and an accepted change set it, and where the sign-in page, the change-password
/// page, the home page, the page for an unknown address and the closed set-up page clear
/// it), decision 108.
#[tokio::test]
async fn the_message_cookie_is_secure_only_behind_a_trusted_https_proxy() {
    for (trust_proxy, proto, secure) in [
        (true, Some("https"), true),
        (true, None, false),
        (true, Some("http"), false),
        (false, Some("https"), false),
    ] {
        let what = format!("trust-proxy {trust_proxy}, X-Forwarded-Proto {proto:?}");
        let app = App::start_with(Options {
            trust_proxy,
            ..Options::default()
        })
        .await;
        app.set_up().await;
        let mut client = app.signed_in(ADMIN, PASSWORD).await;
        let with_proto = |request: Req| match proto {
            Some(proto) => request.header("x-forwarded-proto", proto),
            None => request,
        };
        let reply = client.send(with_proto(Req::form("/logout", &[]))).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "sign-out with {what}");
        let (_code, set) = reply
            .message_cookie()
            .unwrap_or_else(|| panic!("the sign-out with {what} sets no message"));
        assert_eq!(
            set.iter().any(|attribute| attribute == "Secure"),
            secure,
            "whether the message cookie is Secure where it is set, with {what}: {set:?}"
        );
        let page = client.send(with_proto(Req::get("/login"))).await;
        let (_empty, cleared) = page
            .message_cookie()
            .unwrap_or_else(|| panic!("the sign-in page with {what} clears no message"));
        assert_eq!(
            cleared.iter().any(|attribute| attribute == "Secure"),
            secure,
            "whether the message cookie is Secure where it is cleared, with {what}: {cleared:?}"
        );
        let mut client = app.signed_in(ADMIN, PASSWORD).await;
        let changed = client
            .send(with_proto(Req::form(
                "/account/password",
                &[
                    ("current_password", PASSWORD),
                    ("new_password", NEW_PASSWORD),
                    ("new_password_again", NEW_PASSWORD),
                ],
            )))
            .await;
        assert_eq!(
            changed.status,
            StatusCode::SEE_OTHER,
            "the change with {what}"
        );
        let (_code, set) = changed
            .message_cookie()
            .unwrap_or_else(|| panic!("the change with {what} sets no message"));
        assert_eq!(
            set.iter().any(|attribute| attribute == "Secure"),
            secure,
            "whether the change's message cookie is Secure, with {what}: {set:?}"
        );
        for (uri, code) in [
            ("/account/password", "password-changed"),
            ("/", "password-changed"),
            ("/no-such-page", "password-changed"),
            ("/setup", "signed-out"),
        ] {
            client.message = Some(String::from(code));
            let page = client.send(with_proto(Req::get(uri))).await;
            let (_empty, cleared) = page
                .message_cookie()
                .unwrap_or_else(|| panic!("GET {uri} with {what} clears no message"));
            assert_eq!(
                cleared.iter().any(|attribute| attribute == "Secure"),
                secure,
                "whether GET {uri} clears the message Secure, with {what}: {cleared:?}"
            );
        }
    }
}

/// No answer but the three sets a message. The pages a GET renders set none when no message
/// was sent; an accepted sign-in, a sign-in refused for its password, a change refused for
/// what was typed, a change refused during a pause, a cross-site sign-out, a sign-out and a
/// change that find the database busy, a sign-out that fails with a 500 because the clock
/// shows an instant past the last one a stamp holds, and a set-up, accepted or refused, leave
/// none either.
///
/// Proves: decision 117 (the message is set on three answers only, never on any other: a
/// form refused for what was typed, a busy answer and a failure leave no message), INV-28.
#[tokio::test]
async fn no_other_answer_leaves_a_message() {
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
    let mut answers = vec![
        ("the home page", client.get("/").await),
        (
            "the page for an unknown address",
            client.get("/nowhere").await,
        ),
        (
            "the change-password page",
            client.get("/account/password").await,
        ),
        ("the sign-in page", app.client().get("/login").await),
        (
            "an accepted sign-in",
            app.client()
                .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
                .await,
        ),
        (
            "a refused sign-in",
            app.client()
                .post(
                    "/login",
                    &[("username", ADMIN), ("password", WRONG_PASSWORD)],
                )
                .await,
        ),
        (
            "a change with a wrong current password",
            client
                .post(
                    "/account/password",
                    &[
                        ("current_password", WRONG_PASSWORD),
                        ("new_password", NEW_PASSWORD),
                        ("new_password_again", NEW_PASSWORD),
                    ],
                )
                .await,
        ),
        (
            "a cross-site sign-out",
            client
                .send(Req::form("/logout", &[]).header("origin", "http://elsewhere.test"))
                .await,
        ),
    ];
    let held = app.write().await;
    answers.push(("a busy sign-out", client.post("/logout", &[]).await));
    answers.push((
        "a busy change",
        client
            .post(
                "/account/password",
                &[
                    ("current_password", PASSWORD),
                    ("new_password", NEW_PASSWORD),
                    ("new_password_again", NEW_PASSWORD),
                ],
            )
            .await,
    ));
    drop(held);
    // The busy change verified the right password, which forgot the two failures above.
    for _ in 0..5 {
        app.client()
            .post(
                "/login",
                &[("username", ADMIN), ("password", WRONG_PASSWORD)],
            )
            .await;
    }
    answers.push((
        "a change during a pause",
        client
            .post(
                "/account/password",
                &[
                    ("current_password", PASSWORD),
                    ("new_password", NEW_PASSWORD),
                    ("new_password_again", NEW_PASSWORD),
                ],
            )
            .await,
    ));
    app.clock.set(PAST_THE_LAST_INSTANT - START);
    answers.push(("a sign-out that fails", client.post("/logout", &[]).await));
    let fresh = App::start().await;
    let set_up = |password_again: &'static str| {
        [
            ("book_name", "Household"),
            ("currency", "CAD"),
            ("username", ADMIN),
            ("display_name", "Sam"),
            ("password", PASSWORD),
            ("password_again", password_again),
        ]
    };
    answers.push((
        "a refused set-up",
        fresh.client().post("/setup", &set_up(WRONG_PASSWORD)).await,
    ));
    answers.push((
        "an accepted set-up",
        fresh.client().post("/setup", &set_up(PASSWORD)).await,
    ));
    let expected = [
        StatusCode::OK,
        StatusCode::NOT_FOUND,
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::SEE_OTHER,
        StatusCode::UNPROCESSABLE_ENTITY,
        StatusCode::UNPROCESSABLE_ENTITY,
        StatusCode::FORBIDDEN,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::UNPROCESSABLE_ENTITY,
        StatusCode::SEE_OTHER,
    ];
    assert_eq!(answers.len(), expected.len(), "the answers looked at");
    for ((what, answer), status) in answers.iter().zip(expected) {
        assert_eq!(answer.status, status, "status of {what}:\n{}", answer.body);
        assert_eq!(
            answer.message_cookie(),
            None,
            "the message cookie of {what}: {:?}",
            answer.set_cookies()
        );
    }
}

/// A touched session's cookie is still sent again on an answer that clears the message's
/// cookie. A session last seen an hour ago changes its password, which touches nothing; the
/// GET of the change-password page that follows shows the confirmation, clears the message's
/// cookie and sends the session cookie again with its new `Max-Age`, two `Set-Cookie`
/// headers in one answer.
///
/// Proves: decision 117 (the session rule's check after a touch looks for `nage_session` by
/// name, so a touched session's cookie is still sent again on an answer that clears only the
/// message's cookie; an answer may set two cookies), decision 108 (the hourly touch).
#[tokio::test]
async fn a_touched_sessions_cookie_is_sent_again_beside_the_cleared_message() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let token = client
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
    app.clock.advance(HOUR);
    let reply = change(&mut client).await;
    assert!(
        reply.cookies_named("nage_session").is_empty(),
        "the change, a POST, sends the session cookie again: {:?}",
        reply.set_cookies()
    );

    let page = client.get("/account/password").await;
    assert_eq!(
        page.status,
        StatusCode::OK,
        "GET of the page after the change"
    );
    assert_confirms_once(
        &page,
        &Notice::PasswordChanged.to_string(),
        "the change-password page an hour after the sign-in",
    );
    let (value, attributes) = page.session_cookie();
    assert_eq!(value, token, "the session cookie sent again");
    assert!(
        attributes
            .iter()
            .any(|attribute| attribute == "Max-Age=2592000"),
        "the session cookie sent again does not carry its new Max-Age: {attributes:?}"
    );
    assert_eq!(
        page.set_cookies().len(),
        2,
        "cookies of the answer: {:?}",
        page.set_cookies()
    );
}
