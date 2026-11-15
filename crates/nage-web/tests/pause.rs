//! The pause after wrong passwords (SPEC 5.3's login rate limit as decision 111 amends it),
//! through the sign-in page: when it starts, how it grows, what it forgets and what it is
//! keyed by. The change-password page's share is in `tests/password_page.rs`.

mod common;

use axum::http::StatusCode;
use nage_web::messages::Refusal;
use nage_web::{AppState, HealthProbe};

use common::VERSION;
use common::app::{
    ADMIN, App, Client, HOUR, OTHER, OTHER_PASSWORD, PASSWORD, Reply, WRONG_PASSWORD, attribute,
    control,
};

/// One sign-in attempt for `username` with `password`.
async fn attempt(app: &App, username: &str, password: &str) -> Reply {
    app.client()
        .post("/login", &[("username", username), ("password", password)])
        .await
}

/// Asserts that `reply` is a checked attempt that failed: 422.
fn assert_checked(reply: &Reply, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of {what}"
    );
}

/// Asserts that `reply` is an attempt refused during a pause: 429, `Retry-After` of
/// `seconds`, the form again saying how long to wait and not the message of a checked
/// attempt, and no cookie.
fn assert_paused(reply: &Reply, seconds: u64, what: &str) {
    assert_eq!(
        reply.status,
        StatusCode::TOO_MANY_REQUESTS,
        "status of {what}"
    );
    assert_eq!(
        reply.header("retry-after"),
        Some(seconds.to_string().as_str()),
        "Retry-After of {what}"
    );
    assert!(
        reply.shows(&Refusal::SignInPaused { seconds }.to_string()),
        "{what} does not say to wait {seconds} seconds:\n{}",
        reply.body
    );
    assert!(
        !reply.shows(&Refusal::SignIn.to_string()),
        "{what} also shows the message of a checked attempt:\n{}",
        reply.body
    );
    assert!(
        reply.set_cookies().is_empty(),
        "{what} sets a cookie: {:?}",
        reply.set_cookies()
    );
}

/// Five wrong passwords are each checked and refused; the sixth attempt, made right after
/// the fifth failure, is refused with 429, a `Retry-After` of 60 seconds and the form saying
/// how long to wait, without the password being checked: even the right password is
/// refused, an attempt with the username in another spelling is refused the same way and
/// keeps that spelling in its box, no verification is counted and no session is written.
/// A second before the minute is up an attempt is still refused, with one second left; when
/// the minute has passed the right password signs the user in.
///
/// Proves: SPEC 5.3 (after five failures the sixth attempt is refused with 429 and a
/// `Retry-After`; an attempt made while a lock lasts does not check the password), SPEC
/// 10.3 (the lockout engages on the sixth failure), decision 111 (the fifth failure starts
/// a pause of 60 seconds), decision 106 (the paused form keeps what was typed).
#[tokio::test]
async fn the_sixth_attempt_is_refused_unchecked_for_a_minute_after_the_fifth_failure() {
    let app = App::with_book().await;
    for number in 1..=5 {
        let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
        assert_checked(&reply, &format!("wrong password {number}"));
    }
    assert_eq!(
        app.work.verifications(),
        5,
        "verifications of five attempts"
    );

    let sixth = attempt(&app, ADMIN, PASSWORD).await;
    assert_paused(&sixth, 60, "the sixth attempt, with the right password");
    assert_eq!(
        attribute(control(&sixth.body, "username"), "value").as_deref(),
        Some(ADMIN),
        "the username the paused form keeps"
    );
    let seventh = attempt(&app, ADMIN, WRONG_PASSWORD).await;
    assert_paused(&seventh, 60, "the seventh attempt");
    let spelled = attempt(&app, "  SAM ", PASSWORD).await;
    assert_paused(
        &spelled,
        60,
        "an attempt with the username in another spelling",
    );
    assert_eq!(
        attribute(control(&spelled.body, "username"), "value").as_deref(),
        Some("  SAM "),
        "the username the paused form keeps, as it was typed"
    );
    assert_eq!(
        app.work.verifications(),
        5,
        "verifications after three attempts during the pause"
    );
    assert_eq!(app.session_rows().await, 0, "sessions during the pause");

    app.clock.advance(59);
    assert_paused(
        &attempt(&app, ADMIN, PASSWORD).await,
        1,
        "an attempt a second before the minute is up",
    );
    app.clock.advance(1);
    let reply = attempt(&app, ADMIN, PASSWORD).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the right password a minute later"
    );
    assert_eq!(app.work.verifications(), 6, "verifications after the pause");
}

/// Each further failure lengthens the pause, up to ten minutes: after the sixth failure it
/// lasts 120 seconds, after the seventh 300, after the eighth and every later one 600.
/// During a pause the seconds left count down, and an attempt made then, however often,
/// neither lengthens the pause nor counts as a failure. The count is forgotten an hour after
/// the last failure and not before: a failure 3,599 seconds after the tenth is the eleventh
/// and pauses for 600 seconds; once an hour has passed after that one, four failures are
/// checked without a pause and the fifth starts the pause of one minute again.
///
/// Proves: decision 111 (the pauses of 60, 120, 300 and 600 seconds that amend SPEC 5.3's
/// rate limit; a pause lasts its whole length; a refused attempt is not counted; the count is
/// forgotten when 3,600 seconds have passed since the last failure).
#[tokio::test]
async fn each_further_failure_lengthens_the_pause_to_ten_minutes_and_an_hour_forgets_the_count() {
    let app = App::with_book().await;
    for number in 1..=5 {
        let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
        assert_checked(&reply, &format!("wrong password {number}"));
    }
    // The fifth failure is at second 0 and pauses for 60 seconds; each later failure comes
    // when the pause before it has passed.
    let mut at: i64 = 0;
    let mut pause: i64 = 60;
    for (failure, length) in [(6, 120), (7, 300), (8, 600), (9, 600), (10, 600)] {
        at += pause;
        pause = length;
        app.clock.set(at);
        let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
        assert_checked(&reply, &format!("failure {failure}, at second {at}"));
        let seconds = u64::try_from(pause).unwrap_or(u64::MAX);
        assert_paused(
            &attempt(&app, ADMIN, PASSWORD).await,
            seconds,
            &format!("an attempt right after failure {failure}"),
        );
        app.clock.set(at + pause / 2);
        assert_paused(
            &attempt(&app, ADMIN, WRONG_PASSWORD).await,
            seconds - seconds / 2,
            &format!("an attempt half-way through the pause after failure {failure}"),
        );
        app.clock.set(at + pause - 1);
        assert_paused(
            &attempt(&app, ADMIN, WRONG_PASSWORD).await,
            1,
            &format!("an attempt a second before the pause after failure {failure} ends"),
        );
    }
    assert_eq!(
        app.work.verifications(),
        10,
        "verifications: the ten failures and none of the attempts during a pause"
    );

    // 3,599 seconds after the tenth failure its count is still remembered.
    app.clock.set(at + HOUR - 1);
    assert_checked(
        &attempt(&app, ADMIN, WRONG_PASSWORD).await,
        "failure 11, 3,599 seconds after the tenth",
    );
    assert_paused(
        &attempt(&app, ADMIN, PASSWORD).await,
        600,
        "an attempt right after failure 11",
    );

    // An hour after the eleventh failure the count is forgotten.
    app.clock.set(at + HOUR - 1 + HOUR);
    for number in 1..=5 {
        let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
        assert_checked(
            &reply,
            &format!("failure {number} after the count was forgotten"),
        );
    }
    assert_paused(
        &attempt(&app, ADMIN, WRONG_PASSWORD).await,
        60,
        "the attempt after five failures again",
    );
}

/// A sign-in that succeeds forgets the username's failures: four wrong passwords, the right
/// one, and four wrong ones again leave a count of four, so the next attempt is checked.
///
/// Proves: SPEC 5.3 (reset on success), decision 111 (a success forgets the count at once).
#[tokio::test]
async fn a_success_forgets_the_failures() {
    let app = App::with_book().await;
    for round in 1..=2 {
        for number in 1..=4 {
            let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
            assert_checked(&reply, &format!("wrong password {number} of round {round}"));
        }
        let reply = attempt(&app, ADMIN, PASSWORD).await;
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "status of the right password after round {round}"
        );
    }
}

/// The pause is keyed by the fold of the typed username, whether or not a user has it: five
/// wrong passwords for a username nobody has, typed in five spellings the fold makes equal,
/// pause the sixth attempt for it, and the page shows the same wait as for a user who exists.
/// Another username is not paused, and neither is a second visitor's own username.
///
/// Proves: SPEC 5.3 (keyed by the case-folded username whether or not it exists), decision
/// 111, decision 95 (the fold).
#[tokio::test]
async fn the_pause_is_keyed_by_the_fold_of_the_username_whether_or_not_it_exists() {
    let app = App::with_book().await;
    for typed in ["ghost", "GHOST", " Ghost ", "gHoSt", "\u{ff47}host"] {
        let reply = attempt(&app, typed, WRONG_PASSWORD).await;
        assert_checked(&reply, &format!("a wrong password for {typed:?}"));
    }
    assert_paused(
        &attempt(&app, "Ghost", WRONG_PASSWORD).await,
        60,
        "the sixth attempt for a username nobody has",
    );
    let reply = attempt(&app, "ghost2", WRONG_PASSWORD).await;
    assert_checked(&reply, "a wrong password for another username");
    let reply = attempt(&app, ADMIN, PASSWORD).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the admin's sign-in while another username is paused"
    );
}

/// A deactivated user who types the right password is refused like any miss, and the
/// attempt counts toward the pause like any miss, so that a deactivated username behaves
/// exactly as one nobody has: five such attempts pause the sixth.
///
/// Proves: decision 106 (a deactivated user reads the same as an unknown username),
/// decision 111 (a failure is an attempt that was checked and refused), decision 115 (the
/// planted deactivation).
#[tokio::test]
async fn a_deactivated_users_right_password_counts_like_any_miss() {
    let app = App::with_book().await;
    let other = app.add_user(OTHER, OTHER_PASSWORD).await;
    app.deactivate(other.id).await;
    for number in 1..=5 {
        let reply = attempt(&app, OTHER, OTHER_PASSWORD).await;
        assert_checked(&reply, &format!("attempt {number} of the deactivated user"));
    }
    assert_paused(
        &attempt(&app, OTHER, OTHER_PASSWORD).await,
        60,
        "the sixth attempt of the deactivated user",
    );
}

/// The pause is kept in memory: a new router over the same database, as after a restart,
/// has forgotten it and checks the next attempt.
///
/// Proves: SPEC 5.3 (kept in memory), decision 111 (a restart forgets it).
#[tokio::test]
async fn a_restart_forgets_the_pause() {
    let app = App::with_book().await;
    for number in 1..=5 {
        let reply = attempt(&app, ADMIN, WRONG_PASSWORD).await;
        assert_checked(&reply, &format!("wrong password {number}"));
    }
    assert_paused(
        &attempt(&app, ADMIN, PASSWORD).await,
        60,
        "the sixth attempt before the restart",
    );
    let restarted = nage_web::router(AppState {
        version: VERSION,
        health: HealthProbe::always(true),
        storage: app.storage.clone(),
        clock: app.clock.clock(),
        hashing: app.work.hashing(),
        trust_proxy: false,
    });
    let reply = Client::over(restarted)
        .post("/login", &[("username", ADMIN), ("password", PASSWORD)])
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of the right password after the restart"
    );
}
