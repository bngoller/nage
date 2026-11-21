//! Changing one's own password (SPEC 5.3, W14, decisions 111, 112, 116 and 117): the page at
//! `/account/password`, an accepted change, the refusals, its share of the pause after wrong
//! passwords, and the one-time message of a change that another change or a reset overtook.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use nage_core::password::{self, PasswordError, Verification};
use nage_core::test_support::{CHEAP_HASH_PARAMS, cheap_hash};
use nage_core::users::{self, NewUser, PasswordChangeError};
use nage_web::Hashing;
use nage_web::messages::{Notice, Refusal};

use common::app::{
    ADMIN, App, Client, Gate, OTHER, OTHER_PASSWORD, Options, PASSWORD, Reply, Req, WRONG_PASSWORD,
    attribute, control, main_of,
};
use common::start_tags;

/// The password the tests change to.
const NEW_PASSWORD: &str = "tulip window 99";

/// A change-password form.
async fn change(client: &mut Client, current: &str, new: &str, again: &str) -> Reply {
    client
        .post(
            "/account/password",
            &[
                ("current_password", current),
                ("new_password", new),
                ("new_password_again", again),
            ],
        )
        .await
}

/// Asserts that `reply` writes no password back: the three boxes hold no value and none of
/// `typed` is anywhere in the page.
fn assert_no_password_written_back(reply: &Reply, typed: &[&str], what: &str) {
    for name in ["current_password", "new_password", "new_password_again"] {
        let tag = control(&reply.body, name);
        assert_eq!(
            attribute(tag, "value"),
            None,
            "the box {name} of {what} holds a value: {tag}"
        );
    }
    for password in typed {
        assert!(
            !reply.body.contains(password),
            "{what} holds the typed password {password:?}:\n{}",
            reply.body
        );
    }
}

/// `GET /account/password` with a session answers 200 with a plain form that posts to
/// `/account/password` and asks for the current password first, then the new one twice: three
/// boxes of type `password` without a value, with the `autocomplete` values `current-password`,
/// `new-password` and `new-password`. The page says who is signed in, the display name and
/// then the username, each in its place, for a user whose display name is not the username;
/// it has no Sign out button of its own, since the menu above it carries one, and keeps the
/// title `Nage`. Any user reaches it, admin or not. The page goes by the username, not the display
/// name: five wrong current passwords pause a sign-in for that username, and a minute later
/// the right current password changes the password.
///
/// Proves: decision 112 (the page at `/account/password`; the current password first, then
/// the new one twice; the page says who is signed in and checks the current password for the
/// signed-in user's username), decision 116 (the page has no Sign out button of its
/// own), decision 111 (the page's
/// failures count for the username), decision 105 (the `autocomplete` values), SPEC W14
/// (every user changes their own password).
#[tokio::test]
async fn the_change_password_page_asks_for_the_current_password_first() {
    let app = App::with_book().await;
    let admin = app.credentials(ADMIN).await.user.id;
    let alexandra = NewUser {
        username: String::from(OTHER),
        display_name: String::from("Alexandra"),
        password_hash: cheap_hash(OTHER_PASSWORD),
        is_admin: false,
    };
    let mut tx = app.write().await;
    users::create_user(&mut tx, Some(admin), app.clock.now(), &alexandra)
        .await
        .unwrap_or_else(|error| panic!("cannot add {OTHER}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit adding {OTHER}: {error}"));
    let mut client = app.signed_in(OTHER, OTHER_PASSWORD).await;
    let reply = client.get("/account/password").await;
    assert_eq!(reply.status, StatusCode::OK, "GET /account/password status");
    let html = &reply.body;
    assert!(
        html.contains("<title>Nage</title>"),
        "the title of the change-password page:\n{html}"
    );
    assert!(
        reply.shows("You are signed in as Alexandra (alex)."),
        "the page does not say who is signed in:\n{html}"
    );
    let main = main_of(html);
    let forms = start_tags(main, "<form");
    assert_eq!(
        forms.len(),
        1,
        "forms of the change-password page itself, under the menu: {forms:?}"
    );
    assert!(
        forms[0].contains(r#"method="post""#) && forms[0].contains(r#"action="/account/password""#),
        "the page's form does not post to /account/password: {}",
        forms[0]
    );
    assert!(
        !main.contains("Sign out"),
        "the change-password page has a Sign out button of its own:\n{main}"
    );
    assert!(
        !html.contains("hx-"),
        "the change-password page uses an htmx attribute:\n{html}"
    );
    let mut positions = Vec::new();
    for (name, autocomplete) in [
        ("current_password", "current-password"),
        ("new_password", "new-password"),
        ("new_password_again", "new-password"),
    ] {
        let tag = control(html, name);
        assert_eq!(
            attribute(tag, "type").as_deref(),
            Some("password"),
            "type of the box {name}: {tag}"
        );
        assert_eq!(
            attribute(tag, "autocomplete").as_deref(),
            Some(autocomplete),
            "autocomplete of the box {name}: {tag}"
        );
        assert_eq!(
            attribute(tag, "value"),
            None,
            "the box {name} holds a value: {tag}"
        );
        positions.push(html.find(tag).unwrap_or_default());
    }
    assert!(
        positions[0] < positions[1] && positions[1] < positions[2],
        "the boxes are not in the order current, new, new again: {positions:?}"
    );
    assert!(
        !reply.shows(&Notice::PasswordChanged.to_string()),
        "the page confirms a change nobody made:\n{html}"
    );

    for number in 1..=5 {
        let reply = change(&mut client, WRONG_PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of wrong current password {number}"
        );
    }
    let sign_in = app
        .client()
        .post(
            "/login",
            &[("username", OTHER), ("password", OTHER_PASSWORD)],
        )
        .await;
    assert_eq!(
        sign_in.status,
        StatusCode::TOO_MANY_REQUESTS,
        "status of a sign-in for alex after five wrong current passwords"
    );
    app.clock.advance(60);
    let reply = change(&mut client, OTHER_PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the change a minute later:\n{}",
        reply.body
    );
}

/// An accepted change answers 303 back to the page, which then confirms the change and says
/// that the other devices were signed out. The new hash is stored (one hash made, the
/// current password verified once), `updated_by` is the user and `updated_at` the clock's
/// instant; the old password no longer signs in and the new one does; the user's session on
/// another device is refused from then on, while the one that made the change stays signed
/// in; another user's session is untouched.
///
/// Proves: SPEC 5.3 (changing one's own password revokes the user's other sessions),
/// decision 112 (the 303 back to the page, the confirmation, the session in use kept),
/// INV-26's Core half for the change (`updated_by`, `updated_at`), decision 93.
#[tokio::test]
async fn a_change_stores_the_new_hash_stamps_the_user_and_signs_the_other_devices_out() {
    let app = App::with_book().await;
    app.add_user(OTHER, OTHER_PASSWORD).await;
    let mut here = app.signed_in(ADMIN, PASSWORD).await;
    let mut elsewhere = app.signed_in(ADMIN, PASSWORD).await;
    let mut other = app.signed_in(OTHER, OTHER_PASSWORD).await;
    let before = app.credentials(ADMIN).await;
    assert_eq!(before.user.updated_at, None, "updated_at before the change");
    let hashes_before = app.work.hashes();
    let verifications_before = app.work.verifications();

    app.clock.advance(100);
    let reply = change(&mut here, PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the change:\n{}",
        reply.body
    );
    assert_eq!(
        reply.location(),
        Some("/account/password"),
        "where an accepted change leads"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before + 1,
        "hashes made by a change"
    );
    assert_eq!(
        app.work.verifications(),
        verifications_before + 1,
        "verifications of a change"
    );

    let page = here.get("/account/password").await;
    assert_eq!(
        page.status,
        StatusCode::OK,
        "GET of the page after the change, on the device that made it"
    );
    assert!(
        page.shows(&Notice::PasswordChanged.to_string()),
        "the page does not confirm the change:\n{}",
        page.body
    );

    let after = app.credentials(ADMIN).await;
    assert_eq!(
        after.user.updated_by,
        Some(after.user.id),
        "who changed the user"
    );
    assert_eq!(
        after.user.updated_at,
        Some(app.clock.now()),
        "when the user was changed"
    );
    assert_ne!(after.password_hash, before.password_hash, "the stored hash");
    assert_eq!(
        password::verify(NEW_PASSWORD, &after.password_hash),
        Verification::Verified,
        "the stored hash against the new password"
    );

    let reply = elsewhere.get("/").await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "GET / on the user's other device after the change"
    );
    assert_eq!(
        other.get("/").await.status,
        StatusCode::OK,
        "GET / of another user after the change"
    );
    assert_eq!(app.session_rows().await, 2, "sessions after the change");

    let old = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    assert_eq!(
        old.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a sign-in with the old password"
    );
    app.signed_in(ADMIN, NEW_PASSWORD).await;
}

/// A refused change shows every problem at once, with 422: a wrong current password, a new password
/// of nine characters and a second box that differs give the notice and three problems together.
/// The current password is verified whatever else the form holds; nothing is hashed and nothing is
/// written: the stored hash, the stamps and the user's other session stay as they were, and no
/// password is written back. With the right current password and a second box that differs, only
/// that one problem is shown.
///
/// Proves: decision 112 (the current password first; the new one meets decision 107's rules;
/// the form follows decision 105's rules), decision 105 (every problem at once, 422, the
/// passwords never written back), INV-28.
#[tokio::test]
async fn a_refused_change_shows_every_problem_at_once_and_writes_nothing() {
    let app = App::with_book().await;
    let mut here = app.signed_in(ADMIN, PASSWORD).await;
    let mut elsewhere = app.signed_in(ADMIN, PASSWORD).await;
    let before = app.credentials(ADMIN).await;
    let hashes_before = app.work.hashes();
    let verifications_before = app.work.verifications();

    let reply = change(&mut here, WRONG_PASSWORD, "Zq7!xw-p9", "Vb3?other-pw").await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a change with three problems"
    );
    for (what, message) in [
        ("the notice", Refusal::PasswordNotChanged.to_string()),
        (
            "the current password's problem",
            Refusal::CurrentPasswordWrong.to_string(),
        ),
        (
            "the new password's problem",
            PasswordError::TooShort.to_string(),
        ),
        (
            "the second box's problem",
            Refusal::NewPasswordsDiffer.to_string(),
        ),
    ] {
        assert!(
            reply.shows(&message),
            "the refused change does not show {what} ({message}):\n{}",
            reply.body
        );
    }
    assert_no_password_written_back(
        &reply,
        &[WRONG_PASSWORD, "Zq7!xw-p9", "Vb3?other-pw"],
        "the refused change",
    );
    assert_eq!(
        app.work.verifications(),
        verifications_before + 1,
        "verifications of a refused change"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before,
        "hashes made by a refused change"
    );

    let reply = change(&mut here, PASSWORD, NEW_PASSWORD, "tulip window 98").await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a change whose second box differs"
    );
    assert!(
        reply.shows(&Refusal::NewPasswordsDiffer.to_string()),
        "the second box's problem is not shown:\n{}",
        reply.body
    );
    for absent in [
        Refusal::CurrentPasswordWrong.to_string(),
        PasswordError::TooShort.to_string(),
    ] {
        assert!(
            !reply.shows(&absent),
            "a box without a problem shows one ({absent}):\n{}",
            reply.body
        );
    }
    assert_no_password_written_back(
        &reply,
        &[PASSWORD, NEW_PASSWORD, "tulip window 98"],
        "the change whose second box differs",
    );

    let after = app.credentials(ADMIN).await;
    assert_eq!(after, before, "the user after two refused changes");
    assert_eq!(
        elsewhere.get("/").await.status,
        StatusCode::OK,
        "GET / on the user's other device after two refused changes"
    );
    assert_eq!(
        app.session_rows().await,
        2,
        "sessions after two refused changes"
    );
}

/// A wrong current password counts toward the pause for the user's username, with the wrong
/// passwords typed at the sign-in page: after three there and two here, the next change is
/// refused with 429, a `Retry-After` of 60 seconds and the form saying how long to wait,
/// without the current password being checked, though it is right this time; a sign-in for
/// that username is paused as well. A minute later the change is accepted. Twelve wrong
/// current passwords sent together after that get no more checked answers than twelve sent
/// one after another, since the pause is decided again when each verification ends: five are
/// answered 422, and seven 429 with a `Retry-After` of 60 seconds and the form saying how
/// long to wait.
///
/// Proves: decision 111 (a wrong current password on the change-password page counts for
/// the signed-in user's username; a paused attempt is refused unchecked with 429; attempts
/// sent together are held to the same number of checked answers as attempts sent one after
/// another), decision 112, SPEC 5.3's rate limit as decision 111 amends it.
#[tokio::test]
async fn a_wrong_current_password_counts_toward_the_pause() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    for number in 1..=3 {
        let reply = app
            .client()
            .post(
                "/login",
                &[("username", "SAM"), ("password", WRONG_PASSWORD)],
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of wrong sign-in {number}"
        );
    }
    for number in 1..=2 {
        let reply = change(&mut client, WRONG_PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of wrong current password {number}"
        );
        assert!(
            reply.shows(&Refusal::CurrentPasswordWrong.to_string()),
            "wrong current password {number} is not named:\n{}",
            reply.body
        );
    }
    let before = app.credentials(ADMIN).await;
    let verifications_before = app.work.verifications();

    let paused = change(&mut client, PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(
        paused.status,
        StatusCode::TOO_MANY_REQUESTS,
        "status of a change during the pause"
    );
    assert_eq!(
        paused.header("retry-after"),
        Some("60"),
        "Retry-After of a change during the pause"
    );
    assert!(
        paused.shows(&Refusal::PasswordCheckPaused { seconds: 60 }.to_string()),
        "the paused change does not say how long to wait:\n{}",
        paused.body
    );
    assert_no_password_written_back(&paused, &[PASSWORD, NEW_PASSWORD], "the paused change");
    assert_eq!(
        app.work.verifications(),
        verifications_before,
        "verifications of a change during the pause"
    );
    assert_eq!(
        app.credentials(ADMIN).await,
        before,
        "the user after a change during the pause"
    );
    let sign_in = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    assert_eq!(
        sign_in.status,
        StatusCode::TOO_MANY_REQUESTS,
        "status of a sign-in for the username during the pause"
    );

    app.clock.advance(60);
    let reply = change(&mut client, PASSWORD, NEW_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the change a minute later:\n{}",
        reply.body
    );

    let wrong = || {
        let mut device = app.client();
        device.cookie = client.cookie.clone();
        async move {
            change(
                &mut device,
                WRONG_PASSWORD,
                "tulip window 98",
                "tulip window 98",
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
    let wait = Refusal::PasswordCheckPaused { seconds: 60 }.to_string();
    let checked = replies
        .iter()
        .filter(|reply| reply.status == StatusCode::UNPROCESSABLE_ENTITY)
        .count();
    let waiting = replies
        .iter()
        .filter(|reply| {
            reply.status == StatusCode::TOO_MANY_REQUESTS
                && reply.header("retry-after") == Some("60")
                && reply.shows(&wait)
        })
        .count();
    assert_eq!(
        (checked, waiting),
        (5, 7),
        "answers 422 and 429 to twelve wrong current passwords sent together"
    );
}

/// A right current password forgets the username's failures, as a sign-in that succeeds
/// does, even when the change itself is refused for its new password: after four wrong
/// sign-ins and such a change, four more wrong sign-ins leave four remembered, so the right
/// password then signs in.
///
/// Proves: decision 111 (a success forgets the username's failures; the change-password
/// page's check counts like the sign-in's).
#[tokio::test]
async fn a_right_current_password_forgets_the_failures() {
    let app = App::with_book().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let wrong_sign_ins = |app: &App| {
        let mut anonymous = app.client();
        async move {
            for number in 1..=4 {
                let reply = anonymous
                    .post(
                        "/login",
                        &[("username", ADMIN), ("password", WRONG_PASSWORD)],
                    )
                    .await;
                assert_eq!(
                    reply.status,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "status of wrong sign-in {number}"
                );
            }
        }
    };
    wrong_sign_ins(&app).await;
    let reply = change(&mut client, PASSWORD, "123456789", "123456789").await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a change with the right current password and a short new one"
    );
    assert!(
        !reply.shows(&Refusal::CurrentPasswordWrong.to_string()),
        "the right current password is called wrong:\n{}",
        reply.body
    );
    wrong_sign_ins(&app).await;
    app.signed_in(ADMIN, PASSWORD).await;
}

/// One's own change is refused when the password was reset while the current one was being
/// verified (the verification is held open by the hashing double until an admin's reset is
/// committed, so the reset lands in the same second): the core's second check inside the save
/// refuses it and the hash the reset stored stays. The change is sent by HTTPS through a
/// trusted proxy. The answer is a 303 to the sign-in page, with no marker in the address; it
/// sets one cookie, the one-time message's, `nage_message=password-changed-elsewhere` with
/// `HttpOnly`, `Max-Age=60`, `Path=/`, `SameSite=Lax` and `Secure`, and leaves the session
/// cookie as it is. The sign-in page then says once, at its top and as an alert, in the second
/// person, that the password was not changed because it was changed or reset somewhere else,
/// never in the core's words and never in the words for a person still signed in; the same
/// answer clears both cookies, the message's and the session's, whose session the reset
/// ended. A reload says nothing, and neither does the old marker in the address.
///
/// Proves: decision 112 (the current password is verified before the change's one write
/// transaction; the second check inside the save, the reset standing; the person is sent to
/// the sign-in page, which says so), decision 117 (the refusal of an overtaken change travels
/// as a one-time message, its cookie carrying `Path=/`, `HttpOnly`, `SameSite=Lax`,
/// `Max-Age=60` and `Secure` by decision 108's rule, and shows on the sign-in page when the
/// reset ended the session in use; an answer that clears the message's cookie still has a
/// cookie that names no valid session cleared), decision 108 (an ended session's cookie is
/// cleared at the next request), INV-28.
#[tokio::test]
async fn a_change_overtaken_by_a_reset_leads_to_the_sign_in_page_which_says_so() {
    let gate = Gate::unarmed();
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
        trust_proxy: true,
        ..Options::default()
    })
    .await;
    let admin = app.set_up().await.admin.id;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let reset_hash = cheap_hash(OTHER_PASSWORD);
    gate.arm();
    let (reply, ()) = tokio::join!(
        client.send(
            Req::form(
                "/account/password",
                &[
                    ("current_password", PASSWORD),
                    ("new_password", NEW_PASSWORD),
                    ("new_password_again", NEW_PASSWORD),
                ],
            )
            .header("x-forwarded-proto", "https"),
        ),
        async {
            gate.wait_entered(1).await;
            let mut tx = app.write().await;
            users::reset_password(&mut tx, admin, app.clock.now(), admin, &reset_hash)
                .await
                .unwrap_or_else(|error| panic!("cannot reset the password: {error}"));
            tx.commit()
                .await
                .unwrap_or_else(|error| panic!("cannot commit the reset: {error}"));
            gate.open();
        }
    );
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of a change overtaken by a reset:\n{}",
        reply.body
    );
    assert_eq!(
        reply.location(),
        Some("/login"),
        "where a change overtaken by a reset leads"
    );
    assert_eq!(
        reply.message_cookie(),
        Some((
            String::from("password-changed-elsewhere"),
            ["HttpOnly", "Max-Age=60", "Path=/", "SameSite=Lax", "Secure"]
                .map(str::to_owned)
                .to_vec()
        )),
        "the message cookie of the overtaken change"
    );
    assert_eq!(
        reply.set_cookies().len(),
        1,
        "the overtaken change sets or clears another cookie: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        app.credentials(ADMIN).await.password_hash,
        reset_hash,
        "the stored hash after the overtaken change"
    );

    let notice = Refusal::PasswordChangedElsewhere.to_string();
    let core = PasswordChangeError::ChangedMeanwhile {
        username: String::from(ADMIN),
    }
    .to_string();
    let page = client.get("/login").await;
    assert_eq!(
        page.status,
        StatusCode::OK,
        "status of the sign-in page after the overtaken change"
    );
    assert_eq!(
        page.notices(),
        [(String::from("alert"), notice.clone())],
        "the notices of the sign-in page after the overtaken change:\n{}",
        page.body
    );
    for (what, other) in [
        ("the core's message", core),
        (
            "the message for a person still signed in",
            Refusal::PasswordChangedElsewhereSignedIn.to_string(),
        ),
    ] {
        assert!(
            !page.shows(&other),
            "the sign-in page shows {what}:\n{}",
            page.body
        );
    }
    assert!(
        page.clears_the_message(),
        "the sign-in page does not clear the message's cookie: {:?}",
        page.set_cookies()
    );
    assert!(
        page.clears_the_cookie(),
        "the sign-in page does not clear the cookie of the ended session: {:?}",
        page.set_cookies()
    );
    for uri in ["/login", "/login?changed-elsewhere"] {
        let again = client.get(uri).await;
        assert!(
            again.notices().is_empty(),
            "GET {uri} afterwards says something:\n{}",
            again.body
        );
    }
}

/// One's own change is refused as well when another change of the same session overtook it,
/// as when the same form is sent twice in the same moment, by a double tap on the button or
/// from two tabs (the verification is held open
/// until the other change, which keeps the session in use, is committed). The answer is the
/// same 303 to the sign-in page with the one-time message; but the session is still valid,
/// so the sign-in page sends the person on to the home page without touching the message,
/// and the home page says once, at its top and as an alert, that the password was not
/// changed, in the words for a person still signed in: nothing tells them to sign in. A
/// reload says nothing.
///
/// Proves: decision 117 (the refusal of an overtaken change shows on the home page when the
/// session in use stayed valid, as when the same form is sent twice in the same moment),
/// decision 118 (a person already signed in is sent on from the sign-in page),
/// decision 112.
#[tokio::test]
async fn a_change_overtaken_in_another_tab_says_so_on_the_home_page() {
    let gate = Gate::unarmed();
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
    app.set_up().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let token = client
        .cookie
        .clone()
        .unwrap_or_else(|| panic!("the signed-in client holds no cookie"));
    let session = app
        .session_of(&token)
        .await
        .unwrap_or_else(|| panic!("the client's session is not valid"))
        .session
        .id;
    let credentials = app.credentials(ADMIN).await;
    let other_hash = cheap_hash(OTHER_PASSWORD);
    gate.arm();
    let (reply, ()) = tokio::join!(
        change(&mut client, PASSWORD, NEW_PASSWORD, NEW_PASSWORD),
        async {
            gate.wait_entered(1).await;
            let mut tx = app.write().await;
            users::change_own_password(
                &mut tx,
                &credentials,
                app.clock.now(),
                &other_hash,
                session,
            )
            .await
            .unwrap_or_else(|error| panic!("cannot change the password in the other tab: {error}"));
            tx.commit()
                .await
                .unwrap_or_else(|error| panic!("cannot commit the other tab's change: {error}"));
            gate.open();
        }
    );
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of a change overtaken in another tab:\n{}",
        reply.body
    );
    assert_eq!(
        reply.location(),
        Some("/login"),
        "where a change overtaken in another tab leads"
    );
    assert_eq!(
        app.credentials(ADMIN).await.password_hash,
        other_hash,
        "the stored hash after the overtaken change"
    );

    let sent_on = client.get("/login").await;
    assert_eq!(
        sent_on.status,
        StatusCode::SEE_OTHER,
        "GET /login with the session that stayed valid"
    );
    assert_eq!(
        sent_on.location(),
        Some("/"),
        "where the sign-in page sends a person already signed in"
    );
    assert!(
        sent_on.set_cookies().is_empty(),
        "the 303 from the sign-in page sets or clears a cookie: {:?}",
        sent_on.set_cookies()
    );
    let home = client.get("/").await;
    assert_eq!(
        home.status,
        StatusCode::OK,
        "GET / after the overtaken change"
    );
    assert_eq!(
        home.notices(),
        [(
            String::from("alert"),
            Refusal::PasswordChangedElsewhereSignedIn.to_string()
        )],
        "the notices of the home page after the overtaken change:\n{}",
        home.body
    );
    assert!(
        !home.shows(&Refusal::PasswordChangedElsewhere.to_string()),
        "the home page tells a person who is signed in to sign in:\n{}",
        home.body
    );
    assert!(
        home.clears_the_message(),
        "the home page does not clear the message's cookie: {:?}",
        home.set_cookies()
    );
    let reload = client.get("/").await;
    assert!(
        reload.notices().is_empty(),
        "the home page, reloaded, says something:\n{}",
        reload.body
    );
}
