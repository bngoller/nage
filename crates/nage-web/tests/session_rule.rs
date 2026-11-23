//! The session rule and the cookie (SPEC 5.3, INV-27, decisions 108 and 109): where a request
//! without a session goes, what a valid session reaches, expiry, the hourly touch and the
//! cookie's attributes.

mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use nage_core::sessions::Session;
use nage_web::{TOUCH_AFTER_SECONDS, TOUCH_WAIT};

use common::app::{
    ADMIN, App, DAY, HOUR, OTHER, OTHER_PASSWORD, Options, PASSWORD, Reply, Req, START, stamp,
};
use common::{asset, hashed_url};

/// The text `encoded` stands for, percent-decoded; panics on a malformed escape.
fn percent_decoded(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded
                .get(index + 1..index + 3)
                .unwrap_or_else(|| panic!("a cut-off escape in {encoded:?}"));
            decoded.push(
                u8::from_str_radix(hex, 16)
                    .unwrap_or_else(|error| panic!("a bad escape in {encoded:?}: {error}")),
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|error| panic!("{encoded:?} is not UTF-8: {error}"))
}

/// Beside another app's cookie that holds a byte outside ASCII in the same `Cookie` header, as a
/// browser sends every app's cookies to every port of one host name, the session is read and
/// the home page answers, the one-time message shows and is cleared, and the appearance is worn.
///
/// Proves: Decision 147: beside another app's cookie holding a byte outside ASCII in the same header, the session is read and the page answers, a one-time message shows and is cleared, and the appearance is worn.
#[tokio::test]
async fn a_cookie_of_another_app_with_an_accented_letter_hides_nothing() {
    use axum::body::Body;
    use axum::http::{HeaderValue, Request};
    use tower::ServiceExt as _;

    let app = App::with_book().await;
    let sam = app.signed_in(ADMIN, PASSWORD).await;
    let token = sam
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the sign-in set no session cookie"));
    for other in [
        b"other_app=caf\xe9".to_vec(),
        "other_app=caf\u{e9}".as_bytes().to_vec(),
    ] {
        let mut cookie = other.clone();
        cookie.extend_from_slice(
            format!("; nage_session={token}; nage_message=password-changed; nage_theme=dark")
                .as_bytes(),
        );
        let request = Request::builder()
            .method("GET")
            .uri("/")
            .header("host", "nage.test")
            .header(
                "cookie",
                HeaderValue::from_bytes(&cookie)
                    .unwrap_or_else(|error| panic!("{cookie:?} is no header value: {error}")),
            )
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("cannot build the request: {error}"));
        let response = match app.router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| panic!("cannot read the answer: {error}"));
        let reply = Reply {
            status,
            headers,
            body: String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|error| panic!("the answer is not UTF-8: {error}")),
        };
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "web: one cookie reader: the home page beside {other:?} is not answered:\n{}",
            reply.body
        );
        assert_eq!(
            reply.notices(),
            [(
                String::from("status"),
                nage_web::messages::Notice::PasswordChanged.to_string()
            )],
            "web: one cookie reader: the message beside {other:?}"
        );
        assert!(
            reply.clears_the_message(),
            "web: one cookie reader: the message beside {other:?} is not cleared: {:?}",
            reply.set_cookies()
        );
        assert!(
            reply.body.contains(r#"data-theme="dark""#),
            "web: one cookie reader: the appearance beside {other:?} is not worn"
        );
    }
}

/// Asserts that `reply` is a 303 to the sign-in page whose `next` stands for `next`, or to
/// `/login` alone when `next` is `None`.
fn assert_sent_to_login(reply: &Reply, next: Option<&str>, what: &str) {
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "status of {what}");
    let location = reply
        .location()
        .unwrap_or_else(|| panic!("{what} has no Location"));
    match next {
        None => assert_eq!(location, "/login", "where {what} leads"),
        Some(next) => {
            let value = location
                .strip_prefix("/login?next=")
                .unwrap_or_else(|| panic!("{what} leads to {location}, not to /login?next="));
            assert!(
                !value.contains(['&', '=', '#', '?', '+', ' ']),
                "the next of {what} is not one encoded query value: {value}"
            );
            assert_eq!(percent_decoded(value), next, "the next of {what}");
        }
    }
}

/// The session the cookie value `token` names, which must be valid at the clock's instant.
async fn stored_session(app: &App, token: &str) -> Session {
    app.session_of(token)
        .await
        .unwrap_or_else(|| panic!("the session is no longer valid"))
        .session
}

/// Asserts that `reply` is a 303 to `/setup`.
fn assert_sent_to_setup(reply: &Reply, what: &str) {
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "status of {what}");
    assert_eq!(reply.location(), Some("/setup"), "where {what} leads");
}

/// While no user exists, every request but `/setup`, `/health` and `/static/*` is sent to
/// `/setup` with a 303: the start page, the sign-in page, a page behind the sign-in, an
/// unknown address and the forms of sign-in and sign-out. `/setup` shows its form, `/health`
/// answers and a static file is served.
///
/// Proves: SPEC W1 step 1, SPEC 5.3 (with no user every other route redirects to `/setup`,
/// `/login` included), decision 109, INV-27.
#[tokio::test]
async fn without_a_user_every_request_is_sent_to_set_up() {
    let app = App::start().await;
    let mut client = app.client();
    for uri in ["/", "/login", "/account/password", "/no-such-page?x=1"] {
        let reply = client.get(uri).await;
        assert_sent_to_setup(&reply, &format!("GET {uri} without a user"));
        assert!(
            reply.set_cookies().is_empty(),
            "GET {uri} without a user sets a cookie"
        );
    }
    for uri in ["/login", "/logout", "/account/password"] {
        let reply = client
            .post(uri, &[("username", ADMIN), ("password", PASSWORD)])
            .await;
        assert_sent_to_setup(&reply, &format!("POST {uri} without a user"));
    }
    assert_eq!(app.session_rows().await, 0, "sessions without a user");

    assert_eq!(
        client.get("/setup").await.status,
        StatusCode::OK,
        "GET /setup without a user"
    );
    assert_eq!(
        client.get("/health").await.status,
        StatusCode::OK,
        "GET /health without a user"
    );
    let css = hashed_url("app.css", asset("app.css").body);
    assert_eq!(
        client.get(&css).await.status,
        StatusCode::OK,
        "GET {css} without a user"
    );
}

/// A book that is set up while the server runs, as `nage init` does from another process, is
/// seen at the next request: the start page then leads to the sign-in page and `/setup` is
/// closed.
///
/// Proves: decision 109 (a book set up by `nage init` while the server runs is seen at the
/// next request).
#[tokio::test]
async fn a_book_set_up_from_outside_is_seen_at_the_next_request() {
    let app = App::start().await;
    let mut client = app.client();
    assert_sent_to_setup(&client.get("/").await, "GET / before the set-up");
    app.set_up().await;
    assert_sent_to_login(&client.get("/").await, None, "GET / after the set-up");
    assert_eq!(
        client.get("/setup").await.status,
        StatusCode::NOT_FOUND,
        "GET /setup after the set-up"
    );
}

/// Once a user exists, a request without a session is sent to the sign-in page with a 303:
/// a GET carries its path and query as `next`, the start page alone carries none, an unknown
/// address is sent there like any page, and a request that is not a GET carries no `next`.
/// `/login`, `/health` and `/static/*` answer without a session, and `/setup` is closed.
///
/// Proves: SPEC 5.3 (a 303 to `/login?next=<relative path>`), decision 109 (the rule covers
/// unknown addresses; `next` only for a GET; `/` left out), INV-27.
#[tokio::test]
async fn without_a_session_every_page_leads_to_the_sign_in_page() {
    let app = App::with_book().await;
    let mut client = app.client();
    for (uri, next) in [
        ("/", None),
        ("/account/password", Some("/account/password")),
        (
            "/account/password?changed",
            Some("/account/password?changed"),
        ),
        (
            "/no-such-page?kind=asset&name=a%20b",
            Some("/no-such-page?kind=asset&name=a%20b"),
        ),
        // Only `/health` itself and the addresses under `/static/` need no session.
        ("/static", Some("/static")),
        ("/healthz", Some("/healthz")),
        ("/health/", Some("/health/")),
    ] {
        let reply = client.get(uri).await;
        assert_sent_to_login(&reply, next, &format!("GET {uri} without a session"));
        assert!(
            reply.set_cookies().is_empty(),
            "GET {uri} without a cookie sets one: {:?}",
            reply.set_cookies()
        );
    }
    for uri in ["/logout", "/account/password", "/no-such-page"] {
        let reply = client.post(uri, &[("current_password", PASSWORD)]).await;
        assert_sent_to_login(&reply, None, &format!("POST {uri} without a session"));
    }
    let head = client.send(Req::new("HEAD", "/account/password")).await;
    assert_sent_to_login(&head, None, "HEAD /account/password without a session");

    assert_eq!(
        client.get("/login").await.status,
        StatusCode::OK,
        "GET /login without a session"
    );
    assert_eq!(
        client.get("/health").await.status,
        StatusCode::OK,
        "GET /health without a session"
    );
    let css = hashed_url("app.css", asset("app.css").body);
    assert_eq!(
        client.get(&css).await.status,
        StatusCode::OK,
        "GET {css} without a session"
    );
    assert_eq!(
        client.get("/setup").await.status,
        StatusCode::NOT_FOUND,
        "GET /setup once a user exists"
    );
    assert_eq!(app.session_rows().await, 0, "sessions after these requests");
}

/// A `next` that could lead off the site is never written: an address that starts with two
/// slashes and one longer than 2 KiB are sent to `/login` alone, while the longest address
/// that fits is carried.
///
/// Proves: SPEC 5.3 (never an absolute URL), decision 109 (exactly one leading `/`, at most
/// 2 KiB, no `next` otherwise).
#[tokio::test]
async fn next_is_left_out_when_it_could_lead_off_the_site() {
    let app = App::with_book().await;
    let mut client = app.client();
    let fits = format!("/{}", "a".repeat(2047));
    let too_long = format!("/{}", "a".repeat(2048));
    assert_sent_to_login(
        &client.get(&fits).await,
        Some(&fits),
        "GET of an address of 2,048 bytes",
    );
    assert_sent_to_login(
        &client.get(&too_long).await,
        None,
        "GET of an address of 2,049 bytes",
    );
    assert_sent_to_login(
        &client.get("//evil.example/accounts").await,
        None,
        "GET of an address that starts with two slashes",
    );
}

/// An htmx request without a session gets 401 with `HX-Redirect` and no `Location`, since
/// htmx would follow a 303 itself and put the sign-in page inside the page: to `/login`, or
/// to `/setup` while no user exists, for a GET and for a form. htmx's history-restore
/// request, which ignores `HX-Redirect`, gets the 303 like a plain request.
///
/// Proves: SPEC 5.3 and decision 109 (401 with `HX-Redirect` for htmx; the 303 for htmx's
/// history-restore request), INV-27.
#[tokio::test]
async fn an_htmx_request_without_a_session_gets_401_and_hx_redirect() {
    let empty = App::start().await;
    let reply = empty
        .client()
        .send(Req::get("/").header("hx-request", "true"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNAUTHORIZED,
        "status of an htmx GET without a user"
    );
    assert_eq!(
        reply.header("hx-redirect"),
        Some("/setup"),
        "HX-Redirect of an htmx GET without a user"
    );
    assert_eq!(
        reply.location(),
        None,
        "Location of an htmx GET without a user"
    );

    let app = App::with_book().await;
    let mut client = app.client();
    for request in [
        Req::get("/account/password").header("hx-request", "true"),
        Req::form("/account/password", &[("current_password", PASSWORD)])
            .header("hx-request", "true"),
        Req::get("/no-such-page").header("hx-request", "true"),
    ] {
        let reply = client.send(request).await;
        assert_eq!(
            reply.status,
            StatusCode::UNAUTHORIZED,
            "status of an htmx request without a session"
        );
        assert_eq!(
            reply.header("hx-redirect"),
            Some("/login"),
            "HX-Redirect of an htmx request without a session"
        );
        assert_eq!(
            reply.location(),
            None,
            "Location of an htmx request without a session"
        );
    }
    let restore = client
        .send(
            Req::get("/account/password")
                .header("hx-request", "true")
                .header("hx-history-restore-request", "true"),
        )
        .await;
    assert_sent_to_login(
        &restore,
        Some("/account/password"),
        "htmx's history-restore request without a session",
    );
    assert_eq!(
        restore.header("hx-redirect"),
        None,
        "HX-Redirect of htmx's history-restore request"
    );
    let not_htmx = client
        .send(Req::get("/account/password").header("hx-request", "false"))
        .await;
    assert_sent_to_login(
        &not_htmx,
        Some("/account/password"),
        "a request whose HX-Request is false, without a session",
    );
}

/// A request with a valid session reaches its route as the signed-in user: the home page
/// and the change-password page answer 200, the sign-in page sends the person on with a 303,
/// and an unknown address answers 404, to a GET and to a form. None of the GETs sets a
/// cookie.
///
/// Proves: decision 109 (a valid session reaches its route; an unknown address then answers
/// 404), decision 118 (the sign-in page sends a person already signed in on), INV-27.
#[tokio::test]
async fn a_valid_session_reaches_the_page_and_an_unknown_address_is_404() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for (uri, status) in [
        ("/", StatusCode::OK),
        ("/account/password", StatusCode::OK),
        ("/login", StatusCode::SEE_OTHER),
        ("/no-such-page", StatusCode::NOT_FOUND),
    ] {
        let reply = client.get(uri).await;
        assert_eq!(reply.status, status, "GET {uri} with a session");
        assert!(
            reply.set_cookies().is_empty(),
            "GET {uri} with a fresh session sets a cookie: {:?}",
            reply.set_cookies()
        );
    }
    let reply = client.post("/no-such-page", &[]).await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "POST /no-such-page with a session"
    );
}

/// A session is valid until thirty days after its last use and not a second longer: used a
/// second before, it opens the page; left alone for thirty days, it is refused with the 303
/// to the sign-in page and its cookie is cleared.
///
/// Proves: SPEC 5.3 (expiry thirty days after last use), decision 108 (an expired cookie is
/// cleared with `Max-Age=0`), INV-27 (an expired session is refused).
#[tokio::test]
async fn an_expired_session_is_refused_and_its_cookie_cleared() {
    let app = App::with_book().await;
    let mut used = app.signed_in(ADMIN, PASSWORD).await;
    let mut unused = app.signed_in(ADMIN, PASSWORD).await;

    app.clock.set(30 * DAY - 1);
    assert_eq!(
        used.get("/").await.status,
        StatusCode::OK,
        "GET / a second before the session expires"
    );

    app.clock.set(30 * DAY);
    let reply = unused.get("/account/password").await;
    assert_sent_to_login(
        &reply,
        Some("/account/password"),
        "GET with a session unused for thirty days",
    );
    assert!(
        reply.clears_the_cookie(),
        "the expired cookie is not cleared: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        used.get("/").await.status,
        StatusCode::OK,
        "GET / with the session that was used a second earlier"
    );
}

/// A cookie that names no session is refused and cleared: a token nobody was given, a text
/// of another length, and a real token written in capitals. A valid cookie is found among
/// other cookies in the same header; a request with other cookies only is sent to the
/// sign-in page with nothing to clear.
///
/// Proves: decision 108 (the cookie is 64 lowercase hexadecimal characters; an unknown one
/// is cleared with `Max-Age=0`), INV-27.
#[tokio::test]
async fn a_cookie_that_names_no_session_is_refused_and_cleared() {
    let app = App::with_book().await;
    let signed_in = app.signed_in(ADMIN, PASSWORD).await;
    let token = signed_in
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
    let unknown = "0123456789abcdef".repeat(4);
    for (cookie, what) in [
        (unknown.clone(), "a token nobody was given"),
        (String::from("abc"), "a cookie of three characters"),
        (token.to_ascii_uppercase(), "a real token in capitals"),
        (
            format!("{token}00"),
            "a real token with two characters more",
        ),
    ] {
        let reply = app
            .client()
            .send(Req::get("/").header("cookie", &format!("nage_session={cookie}")))
            .await;
        assert_sent_to_login(&reply, None, &format!("GET / with {what}"));
        assert!(
            reply.clears_the_cookie(),
            "{what} is not cleared: {:?}",
            reply.set_cookies()
        );
    }

    let among = app
        .client()
        .send(Req::get("/").header(
            "cookie",
            &format!("theme=dark; nage_session={token}; other=1"),
        ))
        .await;
    assert_eq!(
        among.status,
        StatusCode::OK,
        "GET / with the session cookie among others"
    );
    let others = app
        .client()
        .send(Req::get("/").header("cookie", "theme=dark; other=1"))
        .await;
    assert_sent_to_login(&others, None, "GET / with other cookies only");
    assert!(
        others.set_cookies().is_empty(),
        "a request without the session cookie gets one cleared: {:?}",
        others.set_cookies()
    );
}

/// A deactivated user's session is refused from the next request on, with the 303 to the
/// sign-in page and the cookie cleared; the deactivation is planted, since no operation
/// writes it before milestone 8.
///
/// Proves: SPEC 5.3 (a valid session for an active user), INV-27, decision 115 (the planted
/// deactivation).
#[tokio::test]
async fn a_deactivated_users_session_is_refused() {
    let app = App::with_book().await;
    let other = app.add_user(OTHER, OTHER_PASSWORD).await;
    let mut client = app.signed_in(OTHER, OTHER_PASSWORD).await;
    assert_eq!(
        client.get("/").await.status,
        StatusCode::OK,
        "GET / before the deactivation"
    );
    app.deactivate(other.id).await;
    let reply = client.get("/").await;
    assert_sent_to_login(&reply, None, "GET / after the deactivation");
    assert!(
        reply.clears_the_cookie(),
        "the deactivated user's cookie is not cleared: {:?}",
        reply.set_cookies()
    );
}

/// `last_seen_at` is rewritten at most once an hour, and only by a GET or HEAD: a GET a
/// second short of the hour changes nothing and sets no cookie; a GET at the hour rewrites
/// `last_seen_at` to that instant and the expiry to thirty days later, and sends the cookie
/// again with the new `Max-Age`; a form sent two hours later does not touch; a HEAD then
/// does.
///
/// Proves: SPEC 5.3 (`last_seen_at` written at most once an hour; `Max-Age` refreshed as
/// the session slides), decision 108 (a GET or HEAD touches, a mutating request never does),
/// INV-28.
#[tokio::test]
async fn a_session_is_touched_at_most_once_an_hour_and_only_by_a_get_or_head() {
    assert_eq!(TOUCH_AFTER_SECONDS, HOUR, "the hour between two touches");
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let token = client
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
    app.clock.set(HOUR - 1);
    let early = client.get("/").await;
    assert_eq!(
        early.status,
        StatusCode::OK,
        "GET / a second short of the hour"
    );
    assert!(
        early.set_cookies().is_empty(),
        "a GET within the hour sends the cookie again: {:?}",
        early.set_cookies()
    );
    assert_eq!(
        stored_session(&app, &token).await.last_seen_at,
        stamp(START),
        "last_seen_at after a GET within the hour"
    );

    app.clock.set(HOUR);
    let touched = client.get("/").await;
    assert_eq!(touched.status, StatusCode::OK, "GET / at the hour");
    let (value, attributes) = touched.session_cookie();
    assert_eq!(value, token, "the token of the cookie sent again");
    assert!(
        attributes.contains(&format!("Max-Age={}", 30 * DAY)),
        "the cookie sent again does not last thirty days: {attributes:?}"
    );
    let stored = stored_session(&app, &token).await;
    assert_eq!(
        stored.last_seen_at,
        stamp(START + HOUR),
        "last_seen_at after the touch"
    );
    assert_eq!(
        stored.expires_at,
        stamp(START + HOUR + 30 * DAY),
        "the expiry after the touch"
    );

    app.clock.set(3 * HOUR);
    let form = client
        .post("/account/password", &[("current_password", "")])
        .await;
    assert_eq!(
        form.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused change-password form"
    );
    assert!(
        form.set_cookies().is_empty(),
        "a form sends the cookie again: {:?}",
        form.set_cookies()
    );
    assert_eq!(
        stored_session(&app, &token).await.last_seen_at,
        stamp(START + HOUR),
        "last_seen_at after a form two hours later"
    );

    let head = client.send(Req::new("HEAD", "/")).await;
    assert_eq!(head.status, StatusCode::OK, "HEAD / three hours in");
    assert_eq!(
        head.set_cookies().len(),
        1,
        "cookies sent again by the HEAD: {:?}",
        head.set_cookies()
    );
    assert_eq!(
        stored_session(&app, &token).await.last_seen_at,
        stamp(START + 3 * HOUR),
        "last_seen_at after the HEAD"
    );
}

/// However much a session is used, it ends ninety days after it began: used every
/// twenty-nine days, its cookie's `Max-Age` shrinks to what is left of the ninety days; a
/// second before the ninety days it still opens the page, and at ninety days it is refused.
///
/// Proves: SPEC 5.3 (expiry the earlier of 30 days after last use and 90 days after
/// creation), decisions 92 and 108.
#[tokio::test]
async fn a_session_ends_ninety_days_after_it_began() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for (day, max_age) in [(29, 30 * DAY), (58, 30 * DAY), (87, 3 * DAY)] {
        app.clock.set(day * DAY);
        let reply = client.get("/").await;
        assert_eq!(reply.status, StatusCode::OK, "GET / on day {day}");
        let (_value, attributes) = reply.session_cookie();
        assert!(
            attributes.contains(&format!("Max-Age={max_age}")),
            "the cookie sent again on day {day}: {attributes:?}"
        );
    }
    app.clock.set(90 * DAY - 1);
    assert_eq!(
        client.get("/").await.status,
        StatusCode::OK,
        "GET / a second before ninety days"
    );
    app.clock.set(90 * DAY);
    let reply = client.get("/").await;
    assert_sent_to_login(&reply, None, "GET / ninety days after the sign-in");
}

/// A touch never makes a page view wait behind a long write: while another write holds the
/// writer, a GET an hour after the last use is served at once, without the cookie being sent
/// again, and `last_seen_at` stays as it was; the touch is simply skipped.
///
/// Proves: decision 108 (when the writer does not come quickly the touch is skipped and the
/// page served).
#[tokio::test]
async fn a_touch_is_skipped_while_the_writer_is_held() {
    assert!(
        TOUCH_WAIT <= Duration::from_millis(500),
        "the touch waits {TOUCH_WAIT:?} for the writer"
    );
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let token = client
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
    app.clock.set(HOUR);
    let held = app.write().await;
    let started = Instant::now();
    let reply = client.get("/").await;
    let elapsed = started.elapsed();
    drop(held);
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "GET / while the writer is held"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "GET / took {elapsed:?} while the writer was held: the touch waited for it"
    );
    assert!(
        reply.set_cookies().is_empty(),
        "a skipped touch sends the cookie again: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        stored_session(&app, &token).await.last_seen_at,
        stamp(START),
        "last_seen_at after a skipped touch"
    );
}

/// A writer that is busy before the touch's own wait has passed skips the touch too: with the
/// writer held and a write queue shorter than [`TOUCH_WAIT`], a GET an hour after the last use
/// is served at once without the cookie being sent again, never with the busy page.
///
/// Proves: decision 108 (when the writer does not come quickly the touch is skipped and the
/// page served), decision 53 (a busy writer).
#[tokio::test]
async fn a_touch_is_skipped_when_the_writer_is_busy_before_the_wait_ends() {
    let app = App::start_with(Options {
        storage: Some(nage_core::storage::StorageOptions {
            write_queue: Duration::from_millis(10),
            ..nage_core::storage::StorageOptions::default()
        }),
        ..Options::default()
    })
    .await;
    app.set_up().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    app.clock.set(HOUR);
    let held = app.write().await;
    let reply = client.get("/").await;
    drop(held);
    assert_eq!(
        reply.status,
        StatusCode::OK,
        "GET / while the writer is busy:\n{}",
        reply.body
    );
    assert!(
        reply.set_cookies().is_empty(),
        "a skipped touch sends the cookie again: {:?}",
        reply.set_cookies()
    );
}

/// The session cookie is `nage_session` with a token of 64 lowercase hexadecimal characters,
/// `Path=/`, `HttpOnly`, `SameSite=Lax` and a `Max-Age` of thirty days; each sign-in draws a
/// new token, and the database holds its SHA-256, never the token. It carries `Secure`
/// exactly when trust-proxy is on and the first value of `X-Forwarded-Proto` is `https`:
/// not with trust-proxy off whatever the header says, not for `http`, not without the
/// header. The cookie sent again after the hourly touch carries the same attributes, `Secure`
/// included or not as on the sign-in's cookie.
///
/// Proves: SPEC 5.3 (the cookie's attributes; Secure if and only if trust-proxy is on and
/// the request carries `X-Forwarded-Proto: https`; only the SHA-256 of the token is stored),
/// decision 108 (the same attributes on the cookie sent again when the expiry is rewritten).
#[tokio::test]
async fn the_session_cookie_has_its_attributes_and_is_secure_only_behind_a_trusted_https_proxy() {
    let mut tokens = Vec::new();
    for (trust_proxy, forwarded_proto, secure) in [
        (false, None, false),
        (false, Some("https"), false),
        (true, None, false),
        (true, Some("http"), false),
        (true, Some("https"), true),
        (true, Some("HTTPS"), true),
        (true, Some("https, http"), true),
        (true, Some("http, https"), false),
    ] {
        let app = App::start_with(Options {
            trust_proxy,
            ..Options::default()
        })
        .await;
        app.set_up().await;
        let mut request = Req::form("/login", &[("username", ADMIN), ("password", PASSWORD)]);
        if let Some(value) = forwarded_proto {
            request = request.header("x-forwarded-proto", value);
        }
        let what = format!("trust-proxy {trust_proxy}, X-Forwarded-Proto {forwarded_proto:?}");
        let reply = app.client().send(request).await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the sign-in with {what}"
        );
        let (token, attributes) = reply.session_cookie();
        assert!(
            token.len() == 64
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the token is not 64 lowercase hexadecimal characters: {token:?}"
        );
        let mut expected = vec![
            String::from("HttpOnly"),
            format!("Max-Age={}", 30 * DAY),
            String::from("Path=/"),
            String::from("SameSite=Lax"),
        ];
        if secure {
            expected.push(String::from("Secure"));
        }
        assert_eq!(attributes, expected, "the cookie's attributes with {what}");
        assert!(
            app.session_of(&token).await.is_some(),
            "the database does not hold the SHA-256 of the token's 32 bytes ({what})"
        );
        app.clock.set(HOUR);
        let mut again = Req::get("/").header("cookie", &format!("nage_session={token}"));
        if let Some(value) = forwarded_proto {
            again = again.header("x-forwarded-proto", value);
        }
        let touched = app.client().send(again).await;
        let (value, attributes) = touched.session_cookie();
        assert_eq!(
            value, token,
            "the token of the cookie sent again with {what}"
        );
        assert_eq!(
            attributes, expected,
            "the attributes of the cookie sent again after a touch with {what}"
        );
        tokens.push(token);
    }
    let mut distinct = tokens.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        tokens.len(),
        "distinct tokens of eight sign-ins"
    );
}
