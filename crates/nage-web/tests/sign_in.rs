//! Signing in and out (SPEC 5.3, decisions 106 and 108): the page, an accepted sign-in, the
//! one message of every refused one, what an attempt costs, `next`, and `POST /logout`.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use nage_core::currency::Currency;
use nage_core::password;
use nage_core::setup::{self, Setup};
use nage_core::test_support::{CHEAP_HASH_PARAMS, TEST_BOOK_NAME};
use nage_core::users::NewUser;
use nage_web::Hashing;
use nage_web::messages::Refusal;

use common::app::{
    ADMIN, App, DAY, Gate, OTHER, OTHER_PASSWORD, Options, PASSWORD, Reply, Req, WRONG_PASSWORD,
    attribute, control, percent,
};
use common::start_tags;

/// Asserts that `reply` is the sign-in form again with 422 and the one message, with the
/// typed username kept, no password written back and no cookie set.
fn assert_refused(reply: &Reply, typed_username: &str, typed_password: &str, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of {what}"
    );
    assert!(
        reply.shows(&Refusal::SignIn.to_string()),
        "{what} does not show the one message:\n{}",
        reply.body
    );
    let username = control(&reply.body, "username");
    assert_eq!(
        attribute(username, "value").as_deref(),
        Some(typed_username),
        "the username {what} keeps: {username}"
    );
    let password = control(&reply.body, "password");
    assert_eq!(
        attribute(password, "value"),
        None,
        "{what} writes a password back: {password}"
    );
    if !typed_password.is_empty() {
        assert!(
            !reply.body.contains(typed_password),
            "{what} holds the typed password:\n{}",
            reply.body
        );
    }
    assert!(
        reply.set_cookies().is_empty(),
        "{what} sets a cookie: {:?}",
        reply.set_cookies()
    );
}

/// `GET /login` answers 200 with a plain form that posts to `/login`: the username
/// (`autocomplete="username"`) and the password (`autocomplete="current-password"`, of type
/// `password`, without a value), and a hidden `next`. The page names Nage and nowhere the
/// book, here called Household; it keeps the title `Nage`.
///
/// Proves: decision 106 (only Nage's name before sign-in), decision 105 (the `autocomplete`
/// values, a plain POST), decision 73 (the title).
#[tokio::test]
async fn the_sign_in_page_shows_only_nage() {
    let app = App::with_book().await;
    let reply = app.client().get("/login").await;
    assert_eq!(reply.status, StatusCode::OK, "GET /login status");
    let html = &reply.body;
    assert!(
        html.contains("<title>Nage</title>"),
        "the title of the sign-in page:\n{html}"
    );
    assert!(
        reply.shows("Sign in to Nage"),
        "the sign-in page does not name Nage:\n{html}"
    );
    assert!(
        !html.contains(TEST_BOOK_NAME),
        "the sign-in page names the book before anyone signed in:\n{html}"
    );
    let forms = start_tags(html, "<form");
    assert_eq!(forms.len(), 1, "forms on the sign-in page: {forms:?}");
    assert!(
        forms[0].contains(r#"method="post""#) && forms[0].contains(r#"action="/login""#),
        "the sign-in form does not post to /login: {}",
        forms[0]
    );
    assert!(
        !html.contains("hx-"),
        "the sign-in page uses an htmx attribute:\n{html}"
    );
    let username = control(html, "username");
    assert_eq!(
        attribute(username, "autocomplete").as_deref(),
        Some("username"),
        "autocomplete of the username box: {username}"
    );
    assert_eq!(
        attribute(username, "value").as_deref(),
        Some(""),
        "the username box is not empty: {username}"
    );
    let password = control(html, "password");
    assert_eq!(
        attribute(password, "autocomplete").as_deref(),
        Some("current-password"),
        "autocomplete of the password box: {password}"
    );
    assert_eq!(
        attribute(password, "type").as_deref(),
        Some("password"),
        "type of the password box: {password}"
    );
    assert_eq!(
        attribute(password, "value"),
        None,
        "the password box holds a value: {password}"
    );
    let next = control(html, "next");
    assert_eq!(
        attribute(next, "type").as_deref(),
        Some("hidden"),
        "type of the next field: {next}"
    );
}

/// A sign-in with the right password answers 303 to `/` with the session cookie and creates
/// one session, the cookie's, for that user at the clock's instant. The user is found by the
/// fold of what was typed: the stored username `sam` is found as `SAM` and with spaces around
/// it. The page behind the sign-in then answers 200 with that cookie. A username stored as 33
/// precomposed letters is found when typed in 66 characters, each letter followed by its
/// combining mark, and a wrong password in that spelling counts toward the pause like any
/// other: after five of them the sixth attempt is paused.
///
/// Proves: decision 106 (the user found by the fold of the typed username, the 303, the
/// cookie), decision 95, INV-27, decision 111 (a wrong password counts whenever the typed
/// username finds a user, however long its typed spelling).
#[tokio::test]
async fn a_sign_in_finds_the_user_by_the_fold_and_starts_a_session() {
    let app = App::with_book().await;
    let admin = app.credentials(ADMIN).await.user.id;
    for (typed, sessions) in [("sam", 1), ("SAM", 2), ("  Sam ", 3)] {
        let mut client = app.client();
        let reply = client
            .post("/login", &[("username", typed), ("password", PASSWORD)])
            .await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the sign-in typed as {typed:?}:\n{}",
            reply.body
        );
        assert_eq!(
            reply.location(),
            Some("/"),
            "where the sign-in typed as {typed:?} leads"
        );
        let (token, _attributes) = reply.session_cookie();
        let signed_in = app.session_of(&token).await.unwrap_or_else(|| {
            panic!("the cookie of the sign-in typed as {typed:?} names no session")
        });
        assert_eq!(signed_in.user.id, admin, "whose session it is");
        assert_eq!(
            signed_in.session.created_at,
            app.clock.now(),
            "when the session began"
        );
        assert_eq!(
            app.session_rows().await,
            sessions,
            "sessions after the sign-in typed as {typed:?}"
        );
        let home = client.get("/").await;
        assert_eq!(
            home.status,
            StatusCode::OK,
            "GET / after the sign-in typed as {typed:?}"
        );
    }

    let stored = "\u{e9}".repeat(33);
    let typed = "e\u{301}".repeat(33);
    assert_eq!(
        (stored.chars().count(), typed.chars().count()),
        (33, 66),
        "characters of the stored and of the typed spelling"
    );
    let long = app.add_user(&stored, OTHER_PASSWORD).await;
    let reply = app
        .client()
        .post(
            "/login",
            &[("username", typed.as_str()), ("password", OTHER_PASSWORD)],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the sign-in typed in 66 characters:\n{}",
        reply.body
    );
    let (token, _attributes) = reply.session_cookie();
    assert_eq!(
        app.session_of(&token)
            .await
            .map(|signed_in| signed_in.user.id),
        Some(long.id),
        "whose session the sign-in typed in 66 characters started"
    );
    for number in 1..=5 {
        let reply = app
            .client()
            .post(
                "/login",
                &[("username", typed.as_str()), ("password", WRONG_PASSWORD)],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of wrong password {number} typed in 66 characters"
        );
    }
    let sixth = app
        .client()
        .post(
            "/login",
            &[("username", typed.as_str()), ("password", OTHER_PASSWORD)],
        )
        .await;
    assert_eq!(
        sixth.status,
        StatusCode::TOO_MANY_REQUESTS,
        "status of the sixth attempt typed in 66 characters"
    );
}

/// A sign-in sent by a browser that still holds a cookie which names no session any more
/// sets the new session's cookie and nothing else: the answer carries exactly one
/// `Set-Cookie`, the new token, and no clearing cookie after it, which would sign the browser
/// out again. A refused sign-in sent with such a cookie clears it.
///
/// Proves: decision 108 (the cookie is sent when the session is created; a cookie that names
/// no session is cleared, and one that is replaced is not cleared after it).
#[tokio::test]
async fn a_sign_in_replaces_a_cookie_that_names_no_session() {
    let app = App::with_book().await;
    let stale = format!("nage_session={}", "0123456789abcdef".repeat(4));
    let reply = app
        .client()
        .send(
            Req::form("/login", &[("username", ADMIN), ("password", PASSWORD)])
                .header("cookie", &stale),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of a sign-in sent with a stale cookie"
    );
    let (token, _attributes) = reply.session_cookie();
    assert!(
        app.session_of(&token).await.is_some(),
        "the one cookie set is not the new session's: {:?}",
        reply.set_cookies()
    );

    let refused = app
        .client()
        .send(
            Req::form(
                "/login",
                &[("username", ADMIN), ("password", WRONG_PASSWORD)],
            )
            .header("cookie", &stale),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused sign-in sent with a stale cookie"
    );
    assert!(
        refused.clears_the_cookie(),
        "a refused sign-in does not clear the stale cookie: {:?}",
        refused.set_cookies()
    );
}

/// Every sign-in refused for its username or its password reads the same: an unknown
/// username, a wrong password (with the username as stored and in another spelling), a
/// deactivated user who types the right password, an empty password and a form with nothing
/// in it all answer 422 with the form again and exactly the same page text, the one message,
/// which says neither whether the username exists nor which of the two was wrong. The
/// username is kept as it was typed, never as it is stored, the password is never written
/// back, no cookie is set and no session is written.
///
/// Proves: decision 106 (one message for every miss; a deactivated user reads the same),
/// decision 105 (422, what was typed is kept, the password never written back), INV-27 (a
/// deactivated user's sign-in is refused), INV-28 (a refused sign-in leaves no row).
#[tokio::test]
async fn every_refused_sign_in_reads_the_same_and_writes_nothing() {
    let app = App::with_book().await;
    let other = app.add_user(OTHER, OTHER_PASSWORD).await;
    app.deactivate(other.id).await;

    let mut texts = Vec::new();
    for (username, password, what) in [
        ("nobody", PASSWORD, "an unknown username"),
        (ADMIN, WRONG_PASSWORD, "a wrong password"),
        (
            "  SAM ",
            WRONG_PASSWORD,
            "a wrong password with the username in another spelling",
        ),
        (
            OTHER,
            OTHER_PASSWORD,
            "a deactivated user with the right password",
        ),
        (ADMIN, "", "an empty password"),
        ("", "", "an empty form"),
    ] {
        let reply = app
            .client()
            .post("/login", &[("username", username), ("password", password)])
            .await;
        assert_refused(&reply, username, password, what);
        texts.push((what, reply.text()));
    }
    let (_, first) = &texts[0];
    for (what, text) in &texts[1..] {
        assert_eq!(
            text, first,
            "the page of {what} differs from the page of an unknown username"
        );
    }
    assert_eq!(
        app.session_rows().await,
        0,
        "sessions after six refused sign-ins"
    );
}

/// What an attempt costs is counted through the hashing double, never timed: a wrong
/// password costs one verification, against the user's stored hash; an unknown username
/// costs one too, against a hash that is no user's, the dummy hash made with the same set; a
/// deactivated user's attempt costs one; and a sign-in that is accepted costs one. No
/// sign-in makes a hash. Twelve wrong passwords for one username sent together get no more
/// checked answers than twelve sent one after another, since the pause is decided again when
/// each verification ends: five are answered 422, and seven 429 with a `Retry-After` of 60
/// seconds and the form saying how long to wait.
///
/// Proves: SPEC 5.3 (the dummy verification for unknown usernames), decisions 106 and 107
/// (every attempt that is not paused costs one verification), decision 115 (counted, never
/// timed), decision 111 (attempts sent together are held to the same number of checked answers
/// as attempts sent one after another).
#[tokio::test]
async fn every_checked_attempt_costs_one_verification() {
    let app = App::with_book().await;
    let other = app.add_user(OTHER, OTHER_PASSWORD).await;
    app.deactivate(other.id).await;
    let admin_hash = app.credentials(ADMIN).await.password_hash;
    let other_hash = app.credentials(OTHER).await.password_hash;
    let hashes_before = app.work.hashes();

    app.client()
        .post(
            "/login",
            &[("username", ADMIN), ("password", WRONG_PASSWORD)],
        )
        .await;
    assert_eq!(
        app.work.verified_against(),
        std::slice::from_ref(&admin_hash),
        "the verifications of a wrong password"
    );

    app.client()
        .post("/login", &[("username", "nobody"), ("password", PASSWORD)])
        .await;
    let verified = app.work.verified_against();
    assert_eq!(
        verified.len(),
        2,
        "verifications after an unknown username's attempt"
    );
    let dummy = &verified[1];
    assert!(
        *dummy != admin_hash && *dummy != other_hash,
        "an unknown username was verified against a user's hash"
    );
    assert!(
        dummy.starts_with("$argon2id$v=19$m=8,t=1,p=1$"),
        "the dummy hash was not made with the set handed in: {dummy}"
    );

    app.client()
        .post(
            "/login",
            &[("username", OTHER), ("password", OTHER_PASSWORD)],
        )
        .await;
    assert_eq!(
        app.work.verifications(),
        3,
        "verifications after a deactivated user's attempt"
    );

    app.signed_in(ADMIN, PASSWORD).await;
    let verified = app.work.verified_against();
    assert_eq!(verified.len(), 4, "verifications after an accepted sign-in");
    assert_eq!(
        verified[3], admin_hash,
        "what an accepted sign-in was verified against"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before,
        "hashes made by four sign-ins"
    );

    let wrong = || {
        let mut client = app.client();
        async move {
            client
                .post(
                    "/login",
                    &[("username", ADMIN), ("password", WRONG_PASSWORD)],
                )
                .await
        }
    };
    let (a, b, c, d, e, f, g, h, i, j, k, l) = tokio::join!(
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong(),
        wrong()
    );
    let replies = [a, b, c, d, e, f, g, h, i, j, k, l];
    let wait = Refusal::SignInPaused { seconds: 60 }.to_string();
    let checked = replies
        .iter()
        .filter(|reply| reply.status == StatusCode::UNPROCESSABLE_ENTITY)
        .count();
    let paused = replies
        .iter()
        .filter(|reply| {
            reply.status == StatusCode::TOO_MANY_REQUESTS
                && reply.header("retry-after") == Some("60")
                && reply.shows(&wait)
        })
        .count();
    assert_eq!(
        (checked, paused),
        (5, 7),
        "answers 422 and 429 to twelve wrong passwords sent together"
    );
}

/// A user whose stored hash cannot be read (here the text `not a hash`, stored through the
/// core's set-up, which takes the hash as opaque text) is refused like any miss, with the one
/// message, and the attempt is verified against the dummy hash, so it costs what any other
/// attempt costs.
///
/// Proves: decisions 106 and 107 (the dummy hash for a stored hash that cannot be read).
#[tokio::test]
async fn a_stored_hash_that_cannot_be_read_is_met_with_the_dummy_hash() {
    let app = App::start().await;
    let damaged = Setup {
        book_name: String::from(TEST_BOOK_NAME),
        currency: Currency::from_code("CAD")
            .unwrap_or_else(|| panic!("CAD is not in the currency table")),
        admin: NewUser {
            username: String::from(ADMIN),
            display_name: String::from(ADMIN),
            password_hash: String::from("not a hash"),
            is_admin: true,
        },
        starter_chart: false,
    };
    let mut tx = app.write().await;
    setup::initialize(&mut tx, app.clock.now(), &damaged)
        .await
        .unwrap_or_else(|error| panic!("cannot set the damaged book up: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the damaged book: {error}"));

    let reply = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    assert_refused(&reply, ADMIN, PASSWORD, "a sign-in against a damaged hash");
    let verified = app.work.verified_against();
    let last = verified
        .last()
        .unwrap_or_else(|| panic!("a sign-in against a damaged hash verified nothing"));
    assert!(
        last.starts_with("$argon2id$"),
        "the attempt was not verified against the dummy hash: {verified:?}"
    );
    assert_eq!(app.session_rows().await, 0, "sessions after the attempt");
}

/// Each sign-in deletes the sessions that have expired, every user's, and no other: with one
/// session thirty-one days old and one two days old, a refused sign-in deletes nothing, and
/// an accepted one leaves the two-day-old session and its own.
///
/// Proves: SPEC 5.3 (expired sessions are deleted at each login), decision 106.
#[tokio::test]
async fn a_sign_in_deletes_the_expired_sessions() {
    let app = App::with_book().await;
    app.add_user(OTHER, OTHER_PASSWORD).await;
    let old = app.signed_in(OTHER, OTHER_PASSWORD).await;
    app.clock.advance(29 * DAY);
    let recent = app.signed_in(ADMIN, PASSWORD).await;
    app.clock.advance(2 * DAY);
    assert_eq!(app.session_rows().await, 2, "sessions before the sign-in");

    let refused = app
        .client()
        .post(
            "/login",
            &[("username", ADMIN), ("password", WRONG_PASSWORD)],
        )
        .await;
    assert_refused(&refused, ADMIN, WRONG_PASSWORD, "a wrong password");
    assert_eq!(
        app.session_rows().await,
        2,
        "sessions after a refused sign-in"
    );

    let new = app.signed_in(ADMIN, PASSWORD).await;
    assert_eq!(
        app.session_rows().await,
        2,
        "sessions after an accepted sign-in: the expired one is gone"
    );
    for (client, valid, what) in [
        (&old, false, "the session of thirty-one days ago"),
        (&recent, true, "the session of two days ago"),
        (&new, true, "the new session"),
    ] {
        let token = client
            .cookie
            .as_deref()
            .unwrap_or_else(|| panic!("{what} has no cookie"));
        assert_eq!(
            app.session_of(token).await.is_some(),
            valid,
            "whether {what} is valid"
        );
    }
}

/// After a sign-in `next` is followed only when it stays on this site: a path with its query
/// is followed as it stands; `/`, nothing, a protocol-relative address, an absolute URL, a
/// path with a backslash or a control character, a text without a leading slash and a path
/// longer than 2 KiB all lead to `/`. The sign-in page carries `next` in its hidden field
/// only when it passes the same test.
///
/// Proves: SPEC 5.3 (never an absolute URL), decision 109 (`next` is tested again after
/// sign-in and `/` takes its place when it fails).
#[tokio::test]
async fn a_sign_in_follows_next_only_when_it_stays_on_the_site() {
    let app = App::with_book().await;
    let longest = format!("/{}", "a".repeat(2047));
    let too_long = format!("/{}", "a".repeat(2048));
    for (next, leads_to) in [
        ("/accounts?kind=asset&page=2", "/accounts?kind=asset&page=2"),
        ("/account/password", "/account/password"),
        (longest.as_str(), longest.as_str()),
        ("/", "/"),
        ("", "/"),
        ("//evil.example/", "/"),
        ("https://evil.example/", "/"),
        ("/\\evil.example", "/"),
        ("/accounts\n", "/"),
        ("accounts", "/"),
        (too_long.as_str(), "/"),
    ] {
        let reply = app
            .client()
            .post(
                "/login",
                &[("username", ADMIN), ("password", PASSWORD), ("next", next)],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the sign-in with next {:?}",
            &next[..next.len().min(40)]
        );
        assert_eq!(
            reply.location(),
            Some(leads_to),
            "where the sign-in with next {:?} leads",
            &next[..next.len().min(40)]
        );
    }

    for (next, field) in [
        ("/accounts?kind=asset", "/accounts?kind=asset"),
        ("//evil.example/", ""),
        ("https://evil.example/", ""),
    ] {
        let reply = app
            .client()
            .get(&format!("/login?next={}", percent(next)))
            .await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "GET /login with next {next:?}"
        );
        assert_eq!(
            attribute(control(&reply.body, "next"), "value").as_deref(),
            Some(field),
            "the hidden next of the page asked with next {next:?}"
        );
    }
    let plain = app.client().get("/login").await;
    assert_eq!(
        attribute(control(&plain.body, "next"), "value").as_deref(),
        Some(""),
        "the hidden next of the page asked without one"
    );
}

/// The write half of a sign-in reads the user again: a user who is deactivated while the
/// password is being verified (the verification is held open by the hashing double until the
/// deactivation is committed) is refused with the one message, and no session is written.
///
/// Proves: decision 106 (one write transaction reads the user again: a user deactivated meanwhile
/// is refused), INV-27, INV-28.
#[tokio::test]
async fn a_sign_in_is_refused_when_the_user_changed_while_the_password_was_verified() {
    let gate = Gate::new();
    let held = Arc::clone(&gate);
    let hashing = Hashing::new(
        |password| password::hash(CHEAP_HASH_PARAMS, password),
        move |password, stored_hash| {
            held.pass();
            password::verify(password, stored_hash)
        },
    );
    let app = App::start_with(Options {
        hashing: Some(hashing),
        ..Options::default()
    })
    .await;
    let admin = app.set_up().await.admin.id;
    let mut client = app.client();
    let (reply, ()) = tokio::join!(
        client.post("/login", &[("username", ADMIN), ("password", PASSWORD)]),
        async {
            gate.wait_entered(1).await;
            app.deactivate(admin).await;
            gate.open();
        }
    );
    assert_refused(
        &reply,
        ADMIN,
        PASSWORD,
        "a sign-in whose user was deactivated meanwhile",
    );
    assert_eq!(
        app.session_rows().await,
        0,
        "sessions after the refused sign-in"
    );
}

/// `POST /logout` deletes the session in use and no other, clears the cookie with
/// `Max-Age=0` and answers 303 to the sign-in page. The cookie it cleared no longer opens a
/// page, and the user's session on another device stays signed in.
///
/// Proves: SPEC 5.3 (logout is a POST), decision 106 (sign-out deletes the session, clears
/// the cookie and leads to the sign-in page), decision 108.
#[tokio::test]
async fn sign_out_ends_the_session_and_clears_the_cookie() {
    let app = App::with_book().await;
    let mut here = app.signed_in(ADMIN, PASSWORD).await;
    let mut elsewhere = app.signed_in(ADMIN, PASSWORD).await;
    let token = here
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));

    let reply = here.post("/logout", &[]).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "POST /logout status");
    assert_eq!(reply.location(), Some("/login"), "where sign-out leads");
    assert!(
        reply.clears_the_cookie(),
        "sign-out does not clear the cookie: {:?}",
        reply.set_cookies()
    );
    assert_eq!(app.session_rows().await, 1, "sessions after sign-out");
    assert!(
        app.session_of(&token).await.is_none(),
        "the session signed out of is still valid"
    );

    let again = app
        .client()
        .send(Req::get("/").header("cookie", &format!("nage_session={token}")))
        .await;
    assert_eq!(
        again.status,
        StatusCode::SEE_OTHER,
        "GET / with the cookie that was signed out"
    );
    assert_eq!(
        again.location(),
        Some("/login"),
        "where the signed-out cookie leads"
    );
    let other = elsewhere.get("/").await;
    assert_eq!(
        other.status,
        StatusCode::OK,
        "GET / on the user's other device after the sign-out"
    );
}

/// Sign-out is a POST only: `GET /logout` with a session answers 405 and ends nothing.
///
/// Proves: SPEC 5.3 (logout is a POST).
#[tokio::test]
async fn sign_out_is_a_post_only() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reply = client.get("/logout").await;
    assert_eq!(
        reply.status,
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /logout status"
    );
    assert_eq!(app.session_rows().await, 1, "sessions after GET /logout");
    let home = client.get("/").await;
    assert_eq!(home.status, StatusCode::OK, "GET / after GET /logout");
}
