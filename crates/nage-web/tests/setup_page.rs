//! The set-up page (SPEC W1 steps 1 to 3, 5.3, decision 105): the form, an accepted set-up, a
//! refused one, and the closed page once a user exists.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use nage_core::accounts;
use nage_core::book::{self, BookError};
use nage_core::currency::CURRENCIES;
use nage_core::fold::NameError;
use nage_core::password::{self, PasswordError, Verification};
use nage_core::setup::STARTER_CHART;
use nage_core::test_support::CHEAP_HASH_PARAMS;
use nage_core::users::UserError;
use nage_web::Hashing;
use nage_web::messages::Refusal;

use common::app::{
    ADMIN, App, Gate, Options, PASSWORD, Reply, Req, attribute, control, form_body, unescape,
};
use common::start_tags;

/// A set-up form that is accepted: the book `Household` in CAD, the admin `sam` shown as
/// `Sam`, the password twice and the tick box ticked.
fn good_form() -> Vec<(&'static str, &'static str)> {
    vec![
        ("book_name", "Household"),
        ("currency", "CAD"),
        ("username", ADMIN),
        ("display_name", "Sam"),
        ("password", PASSWORD),
        ("password_again", PASSWORD),
        ("starter_chart", "on"),
    ]
}

/// `form` with the field `name` set to `value`.
fn with(
    mut form: Vec<(&'static str, &'static str)>,
    name: &'static str,
    value: &'static str,
) -> Vec<(&'static str, &'static str)> {
    for field in &mut form {
        if field.0 == name {
            field.1 = value;
        }
    }
    form
}

/// How many accounts the database holds.
async fn account_rows(app: &App) -> usize {
    let mut read = app.read().await;
    accounts::accounts(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the accounts: {error}"))
        .len()
}

/// Whether the database holds a book.
async fn has_book(app: &App) -> bool {
    let mut read = app.read().await;
    book::book(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"))
        .is_some()
}

/// Asserts that nothing was set up: no book, no user, no account, no session.
async fn assert_nothing_written(app: &App, when: &str) {
    assert!(!has_book(app).await, "a book exists {when}");
    assert_eq!(app.users().await, 0, "users {when}");
    assert_eq!(account_rows(app).await, 0, "accounts {when}");
    assert_eq!(app.session_rows().await, 0, "sessions {when}");
}

/// Asserts that `reply` is the closed set-up page: 404, the page's text, a button that leads
/// to the sign-in page, and no cookie.
fn assert_closed_page(reply: &Reply, what: &str) {
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "status of {what}");
    assert!(
        reply.shows(&Refusal::SetupClosed.to_string()),
        "{what} does not show the closed set-up page:\n{}",
        reply.body
    );
    assert!(
        reply.shows("Nage is already set up"),
        "{what} does not say that Nage is already set up:\n{}",
        reply.body
    );
    let forms = start_tags(&reply.body, "<form");
    assert!(
        forms
            .iter()
            .any(|tag| tag.contains(r#"action="/login""#) && tag.contains(r#"method="get""#)),
        "{what} has no button to the sign-in page: {forms:?}"
    );
    assert!(
        reply.set_cookies().is_empty(),
        "{what} sets a cookie: {:?}",
        reply.set_cookies()
    );
}

/// While no user exists, `GET /setup` answers 200 with one plain form that posts to
/// `/setup`: the book name, the currency as a list of the table's 155 codes with CAD alone
/// selected, the username, the display name, the password twice and the starter-chart tick
/// box, ticked. The inputs carry the `autocomplete` values password managers read
/// (`username`, `new-password` twice), the password boxes are of type `password` and hold no
/// value, the form is not sent by htmx, and the page keeps the title `Nage`.
///
/// Proves: SPEC W1 step 2, decision 105 (the form, the `autocomplete` values, CAD preselected,
/// the tick box ticked by default), decision 73 (the title).
#[tokio::test]
async fn the_set_up_page_is_the_form_of_w1_while_no_user_exists() {
    let app = App::start().await;
    let reply = app.client().get("/setup").await;
    assert_eq!(reply.status, StatusCode::OK, "GET /setup status");
    let html = &reply.body;
    assert!(
        html.contains("<title>Nage</title>"),
        "the set-up page's title:\n{html}"
    );

    let forms = start_tags(html, "<form");
    assert_eq!(forms.len(), 1, "forms on the set-up page: {forms:?}");
    assert!(
        forms[0].contains(r#"method="post""#) && forms[0].contains(r#"action="/setup""#),
        "the set-up form does not post to /setup: {}",
        forms[0]
    );
    assert!(
        !html.contains("hx-"),
        "the set-up page uses an htmx attribute:\n{html}"
    );

    for (name, kind, autocomplete) in [
        ("book_name", "text", None),
        ("username", "text", Some("username")),
        ("display_name", "text", None),
        ("password", "password", Some("new-password")),
        ("password_again", "password", Some("new-password")),
    ] {
        let tag = control(html, name);
        assert_eq!(
            attribute(tag, "type").as_deref(),
            Some(kind),
            "type of the box {name}: {tag}"
        );
        if let Some(expected) = autocomplete {
            assert_eq!(
                attribute(tag, "autocomplete").as_deref(),
                Some(expected),
                "autocomplete of the box {name}: {tag}"
            );
        }
        if kind == "password" {
            assert_eq!(
                attribute(tag, "value"),
                None,
                "the password box {name} holds a value: {tag}"
            );
        } else {
            assert_eq!(
                attribute(tag, "value").as_deref(),
                Some(""),
                "the box {name} is not empty: {tag}"
            );
        }
    }

    control(html, "currency");
    let options = start_tags(html, "<option");
    assert_eq!(options.len(), 155, "currencies in the list");
    assert_eq!(options.len(), CURRENCIES.len(), "currencies in the table");
    for currency in CURRENCIES {
        assert!(
            html.contains(&format!(">{}</option>", currency.code())),
            "the list lacks {}",
            currency.code()
        );
    }
    let selected: Vec<&str> = html
        .match_indices("<option selected>")
        .map(|(at, _)| &html[at + 17..at + 20])
        .collect();
    assert_eq!(selected, ["CAD"], "the currencies selected in the list");

    let tick = control(html, "starter_chart");
    assert_eq!(
        attribute(tick, "type").as_deref(),
        Some("checkbox"),
        "the starter-chart box is no tick box: {tick}"
    );
    assert!(
        tick.contains(" checked"),
        "the starter-chart box is not ticked: {tick}"
    );
}

/// An accepted set-up answers 303 to `/` with the session cookie, so the admin is signed in,
/// and has written, in the chosen currency, the book with that currency's digits, the admin
/// (an admin, created by nobody, at the clock's instant), the system accounts with the
/// starter chart, and one session that is the cookie's. The stored hash is argon2id made
/// with the set the application was handed and verifies the password as typed; exactly one
/// hash was made for it, and the page behind the sign-in then answers 200.
///
/// Proves: SPEC W1 step 3, decision 105 (set-up signs the admin in), decision 107 (the
/// parameters handed in), INV-5's Core half (the currency and its digits written by set-up),
/// INV-26 (the first admin's row), INV-27 (the session reaches `/`).
#[tokio::test]
async fn set_up_creates_the_book_the_admin_and_the_chart_and_signs_the_admin_in() {
    let app = App::start().await;
    let hashes_before = app.work.hashes();
    let mut client = app.client();
    let reply = client
        .post("/setup", &with(good_form(), "currency", "JPY"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "POST /setup status:\n{}",
        reply.body
    );
    assert_eq!(
        reply.location(),
        Some("/"),
        "where an accepted set-up leads"
    );
    let (token, attributes) = reply.session_cookie();
    assert!(
        attributes.contains(&String::from("Max-Age=2592000")),
        "the set-up cookie does not live thirty days: {attributes:?}"
    );

    let mut read = app.read().await;
    let book = book::book(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"))
        .unwrap_or_else(|| panic!("set-up wrote no book"));
    drop(read);
    assert_eq!(book.name, "Household", "the book's name");
    assert_eq!(book.currency_code, "JPY", "the book's currency");
    assert_eq!(book.minor_digits.get(), 0, "the book's minor digits");
    assert_eq!(
        book.created_at,
        app.clock.now(),
        "when the book was created"
    );

    assert_eq!(app.users().await, 1, "users after set-up");
    let admin = app.credentials(ADMIN).await;
    assert!(admin.user.is_admin, "the first user is no admin");
    assert_eq!(admin.user.display_name, "Sam", "the admin's display name");
    assert_eq!(admin.user.created_by, None, "who created the first admin");
    assert_eq!(
        admin.user.created_at,
        app.clock.now(),
        "when the admin was created"
    );
    assert!(
        admin
            .password_hash
            .starts_with("$argon2id$v=19$m=8,t=1,p=1$"),
        "the stored hash was not made with the set handed in: {}",
        admin.password_hash
    );
    assert_eq!(
        password::verify(PASSWORD, &admin.password_hash),
        Verification::Verified,
        "the stored hash against the password as typed"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before + 1,
        "hashes made by one set-up"
    );

    let starter: usize = STARTER_CHART.iter().map(|(_, names)| names.len()).sum();
    assert_eq!(
        account_rows(&app).await,
        8 + starter,
        "accounts after a set-up with the starter chart"
    );

    assert_eq!(app.session_rows().await, 1, "sessions after set-up");
    let signed_in = app
        .session_of(&token)
        .await
        .unwrap_or_else(|| panic!("the cookie names no valid session"));
    assert_eq!(signed_in.user.id, admin.user.id, "whose session it is");
    assert_eq!(
        signed_in.session.created_at,
        app.clock.now(),
        "when the session began"
    );

    let home = client.get("/").await;
    assert_eq!(home.status, StatusCode::OK, "GET / right after set-up");
}

/// A set-up whose tick box is not ticked (the field is absent, as a browser sends it)
/// creates the five roots and the three other system accounts and no starter account.
///
/// Proves: SPEC W1 step 2 (the starter chart is a choice), decision 105.
#[tokio::test]
async fn set_up_without_the_tick_creates_no_starter_account() {
    let app = App::start().await;
    let mut form = good_form();
    form.retain(|(name, _)| *name != "starter_chart");
    let reply = app.client().post("/setup", &form).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "POST /setup status");
    assert_eq!(
        account_rows(&app).await,
        8,
        "accounts after a set-up without the starter chart"
    );
}

/// A form with a problem in every box is refused whole, with 422 and the form again: the notice and
/// each box's own problem, all at once, in the core's words for the names and the password and in
/// the page's words for the second password. No password is written back into the page, no cookie
/// is set, no hash is made, and nothing is written: no book, no user, no account, no session.
///
/// Proves: decision 105 (checked whole, every problem at once, 422, the passwords never
/// written back), decision 114 (the core's messages shown as they stand), INV-28 (a refused
/// set-up leaves no row).
#[tokio::test]
async fn a_refused_set_up_shows_every_problem_at_once_and_writes_nothing() {
    let app = App::start().await;
    let hashes_before = app.work.hashes();
    let reply = app
        .client()
        .post(
            "/setup",
            &[
                ("book_name", "   "),
                ("currency", "CAD"),
                ("username", ""),
                ("display_name", "Sam\tthe admin"),
                ("password", "Zq7!xw-p9"),
                ("password_again", "Vb3?other-pw"),
                ("starter_chart", "on"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a refused set-up"
    );
    for (what, message) in [
        ("the notice", Refusal::SetupNotDone.to_string()),
        (
            "the book name's problem",
            BookError::Name(NameError::Empty).to_string(),
        ),
        (
            "the username's problem",
            UserError::Username(NameError::Empty).to_string(),
        ),
        (
            "the display name's problem",
            UserError::DisplayName(NameError::Control).to_string(),
        ),
        (
            "the password's problem",
            PasswordError::TooShort.to_string(),
        ),
        (
            "the second password's problem",
            Refusal::SetupPasswordsDiffer.to_string(),
        ),
    ] {
        assert!(
            reply.shows(&message),
            "the refused form does not show {what} ({message}):\n{}",
            reply.body
        );
    }
    for secret in ["Zq7!xw-p9", "Vb3?other-pw"] {
        assert!(
            !reply.body.contains(secret),
            "the refused form writes the password {secret:?} back:\n{}",
            reply.body
        );
    }
    for name in ["password", "password_again"] {
        let tag = control(&reply.body, name);
        assert_eq!(
            attribute(tag, "value"),
            None,
            "the password box {name} holds a value: {tag}"
        );
    }
    assert!(
        reply.set_cookies().is_empty(),
        "a refused set-up sets a cookie: {:?}",
        reply.set_cookies()
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before,
        "hashes made by a refused set-up"
    );
    assert_nothing_written(&app, "after a refused set-up").await;
}

/// A form with one problem (a password of nine characters, typed the same twice) is refused with
/// that one problem and no other, and keeps everything else that was typed: the book name, the
/// currency chosen, the username, the display name and the tick box as it was sent, here not
/// ticked. A currency that is not in the list, which only a changed form can send, is named, and
/// the list is back on CAD.
///
/// Proves: decision 105 (a refused form names only the problem it has; what was typed is kept),
/// decision 107 (a password of nine characters is refused).
#[tokio::test]
async fn a_refused_set_up_keeps_what_was_typed() {
    let app = App::start().await;
    let reply = app
        .client()
        .post(
            "/setup",
            &[
                ("book_name", "Our \"home\" book"),
                ("currency", "EUR"),
                ("username", "Sam"),
                ("display_name", "Sam & Co"),
                ("password", "123456789"),
                ("password_again", "123456789"),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a set-up with a short password"
    );
    assert!(
        reply.shows(&PasswordError::TooShort.to_string()),
        "the short password's problem is not shown:\n{}",
        reply.body
    );
    for absent in [
        BookError::Name(NameError::Empty).to_string(),
        UserError::Username(NameError::Empty).to_string(),
        UserError::DisplayName(NameError::Empty).to_string(),
        Refusal::SetupPasswordsDiffer.to_string(),
    ] {
        assert!(
            !reply.shows(&absent),
            "a box without a problem shows one ({absent}):\n{}",
            reply.body
        );
    }
    let html = &reply.body;
    for (name, typed) in [
        ("book_name", "Our \"home\" book"),
        ("username", "Sam"),
        ("display_name", "Sam & Co"),
    ] {
        let tag = control(html, name);
        assert_eq!(
            attribute(tag, "value").as_deref(),
            Some(typed),
            "what the box {name} holds on the refused form: {tag}"
        );
    }
    assert!(
        html.contains("<option selected>EUR</option>"),
        "the refused form does not keep the currency chosen:\n{html}"
    );
    assert_eq!(
        html.matches("<option selected>").count(),
        1,
        "currencies selected on the refused form"
    );
    assert!(
        !control(html, "starter_chart").contains(" checked"),
        "the refused form ticks a box that was sent unticked"
    );

    let reply = app
        .client()
        .post("/setup", &with(good_form(), "currency", "cad"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a set-up with a currency that is not in the list"
    );
    let unknown = Refusal::UnknownCurrency {
        typed: String::from("cad"),
    };
    assert!(
        reply.shows(&unknown.to_string()),
        "the unknown currency's problem is not shown:\n{}",
        reply.body
    );
    assert!(
        reply.body.contains("<option selected>CAD</option>"),
        "the list is not back on CAD:\n{}",
        reply.body
    );
    assert_nothing_written(&app, "after two refused set-ups").await;
}

/// The password is taken exactly as typed: one made of a space, eight characters (`eight ch`)
/// and a space has ten characters and is accepted with its spaces, and the admin then signs in
/// with the spaces and not without them.
///
/// Proves: SPEC 5.3 and decision 107 (never trimmed; no composition rule), through the page.
#[tokio::test]
async fn the_set_up_password_is_taken_as_typed() {
    let app = App::start().await;
    let spaced = " eight ch ";
    let form = with(
        with(good_form(), "password", spaced),
        "password_again",
        spaced,
    );
    let reply = app.client().post("/setup", &form).await;
    assert_eq!(
        reply.status,
        StatusCode::SEE_OTHER,
        "status of a set-up whose password has a space at each end:\n{}",
        reply.body
    );
    let trimmed = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", "eight ch")])
        .await;
    assert_eq!(
        trimmed.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a sign-in with the password without its spaces"
    );
    let as_typed = app
        .client()
        .post("/login", &[("username", ADMIN), ("password", spaced)])
        .await;
    assert_eq!(
        as_typed.status,
        StatusCode::SEE_OTHER,
        "status of a sign-in with the password as typed"
    );
}

/// Once a user exists, `/setup` answers 404 with the short page that says Nage is already
/// set up and has a button to the sign-in page: to a GET and to a POST of a form that would
/// otherwise be accepted, without a session and with one. The POST changes nothing: the
/// book keeps its name and its currency, no second user appears, and no session is created.
///
/// Proves: SPEC 5.3 (`/setup` returns 404 once any user exists), decision 105 (the closed
/// page), INV-5 (a second set-up cannot change the currency).
#[tokio::test]
async fn set_up_is_closed_once_a_user_exists() {
    let app = App::with_book().await;
    let accounts_before = account_rows(&app).await;
    let hashes_before = app.work.hashes();
    let other_book = with(
        with(with(good_form(), "book_name", "Another"), "currency", "USD"),
        "username",
        "intruder",
    );

    let mut anonymous = app.client();
    assert_closed_page(
        &anonymous.get("/setup").await,
        "GET /setup without a session",
    );
    assert_closed_page(
        &anonymous.post("/setup", &other_book).await,
        "POST /setup without a session",
    );

    let mut admin = app.signed_in(ADMIN, PASSWORD).await;
    assert_closed_page(&admin.get("/setup").await, "GET /setup with a session");
    assert_closed_page(
        &admin.post("/setup", &other_book).await,
        "POST /setup with a session",
    );

    let mut read = app.read().await;
    let book = book::book(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"))
        .unwrap_or_else(|| panic!("the book is gone"));
    drop(read);
    assert_eq!(
        book.name, "Household",
        "the book's name after a second set-up"
    );
    assert_eq!(
        book.currency_code, "CAD",
        "the book's currency after a second set-up"
    );
    assert_eq!(app.users().await, 1, "users after a second set-up");
    assert_eq!(
        account_rows(&app).await,
        accounts_before,
        "accounts after a second set-up"
    );
    assert_eq!(
        app.session_rows().await,
        1,
        "sessions after a second set-up (the admin's own)"
    );
    assert_eq!(
        app.work.hashes(),
        hashes_before,
        "passwords hashed for a set-up once a user exists"
    );
}

/// Two set-ups sent at the same moment, each held inside its hash until both have passed the
/// rule that no user exists yet: the writer serializes them, one is accepted (303 and the
/// cookie) and the other is refused inside its write transaction by the core's check, writes
/// nothing and gets the closed page with 404 and no session. One book, one user and one
/// session exist afterwards, and the user is the accepted form's.
///
/// Proves: SPEC W1 step 3 (two simultaneous submissions, serialized by the writer), decision
/// 105 (the losing submission gets the closed page and no session), INV-28.
#[tokio::test]
async fn the_second_of_two_set_ups_gets_the_closed_page_and_no_session() {
    let gate = Gate::new();
    let held = Arc::clone(&gate);
    let hashing = Hashing::new(
        move |password| {
            // The dummy hash, made before any request, passes freely.
            if password == PASSWORD {
                held.pass();
            }
            password::hash(CHEAP_HASH_PARAMS, password)
        },
        password::verify,
    );
    let app = App::start_with(Options {
        hashing: Some(hashing),
        ..Options::default()
    })
    .await;
    let mut first = app.client();
    let mut second = app.client();
    let first_form = good_form();
    let second_form = with(good_form(), "username", "alex");
    let (first_reply, second_reply, ()) = tokio::join!(
        first.post("/setup", &first_form),
        second.post("/setup", &second_form),
        async {
            gate.wait_entered(2).await;
            gate.open();
        }
    );

    let (won, lost, winner) = if first_reply.status == StatusCode::SEE_OTHER {
        (first_reply, second_reply, ADMIN)
    } else {
        (second_reply, first_reply, "alex")
    };
    assert_eq!(
        won.status,
        StatusCode::SEE_OTHER,
        "status of the set-up that won:\n{}",
        won.body
    );
    assert_eq!(
        won.set_cookies().len(),
        1,
        "cookies set for the set-up that won"
    );
    assert_closed_page(&lost, "the set-up that lost");
    assert_eq!(app.users().await, 1, "users after two simultaneous set-ups");
    assert_eq!(
        app.credentials(winner).await.user.username,
        winner,
        "the user of the set-up that won"
    );
    assert_eq!(
        app.session_rows().await,
        1,
        "sessions after two simultaneous set-ups"
    );
    assert!(
        has_book(&app).await,
        "no book after two simultaneous set-ups"
    );
}

/// A set-up form sent with none of its fields, and with one Nage does not know, is refused
/// with 422 and the problems of its empty boxes, the tick box not ticked, never with a bare
/// error: a field that is missing is an empty box.
///
/// Proves: decision 105 (a submission is checked whole; a missing field is an empty box).
#[tokio::test]
async fn an_empty_set_up_form_is_refused_like_empty_boxes() {
    let app = App::start().await;
    let reply = app
        .client()
        .send(Req::form("/setup", &[]).body(form_body(&[("extra", "1")]).into_bytes()))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a set-up form without its fields:\n{}",
        reply.body
    );
    for message in [
        BookError::Name(NameError::Empty).to_string(),
        UserError::Username(NameError::Empty).to_string(),
        UserError::DisplayName(NameError::Empty).to_string(),
        PasswordError::TooShort.to_string(),
    ] {
        assert!(
            reply.shows(&message),
            "the empty form does not show {message}:\n{}",
            reply.body
        );
    }
    assert!(
        !control(&reply.body, "starter_chart").contains(" checked"),
        "the empty form ticks the starter-chart box"
    );
    assert_nothing_written(&app, "after an empty set-up form").await;
}

/// A colon in any of the three names refuses the form with 422, the notice and that name's
/// problem in the core's words under its own box, between it and the next box: a colon as typed
/// in the book name, the username and the display name, a fullwidth colon in the username and a
/// small colon in the book name. Nothing is written.
///
/// Proves: decision 200 (no colon in a book name, a username or a display name),
/// decision 105 (a problem with one box under that box), INV-28 (a refused set-up leaves no row).
#[tokio::test]
async fn a_colon_in_any_name_is_refused_under_its_box() {
    let app = App::start().await;
    let cases = [
        (
            "book_name",
            "Our: Household",
            "currency",
            BookError::Name(NameError::Colon).to_string(),
        ),
        (
            "book_name",
            "Our\u{FE55} Household",
            "currency",
            BookError::Name(NameError::Colon).to_string(),
        ),
        (
            "username",
            "sam:home",
            "display_name",
            UserError::Username(NameError::Colon).to_string(),
        ),
        (
            "username",
            "sam\u{FF1A}home",
            "display_name",
            UserError::Username(NameError::Colon).to_string(),
        ),
        (
            "display_name",
            "Sam: admin",
            "password",
            UserError::DisplayName(NameError::Colon).to_string(),
        ),
    ];
    for (name, typed, next, message) in cases {
        let reply = app
            .client()
            .post("/setup", &with(good_form(), name, typed))
            .await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "status of a set-up with {typed:?} in {name}:\n{}",
            reply.body
        );
        assert!(
            reply.shows(&Refusal::SetupNotDone.to_string()),
            "the form with {typed:?} in {name} does not show the notice:\n{}",
            reply.body
        );
        let html = &reply.body;
        let box_at = html
            .find(control(html, name))
            .unwrap_or_else(|| panic!("no box {name}:\n{html}"));
        let next_at = html
            .find(control(html, next))
            .unwrap_or_else(|| panic!("no box {next}:\n{html}"));
        assert!(
            unescape(&html[box_at..next_at]).contains(&message),
            "the problem of {typed:?} is not under the box {name} ({message}):\n{html}"
        );
        assert_eq!(
            unescape(html).matches(&message).count(),
            1,
            "the problem of {typed:?} in {name} is not shown once:\n{html}"
        );
    }
    assert_nothing_written(&app, "after set-ups refused for a colon").await;
}

/// A password the hasher refuses by one of its rules, although the page's own check let it
/// pass, is the form's refusal: 422 with the rule's message under the password box, between
/// it and the second password box and nowhere after that box, and nothing written; never a
/// failure of the server.
///
/// Proves: decision 107 (a password that breaks a rule is the form's refusal), decision 105
/// (a problem with one box under that box).
#[tokio::test]
async fn a_password_the_hasher_refuses_is_shown_under_its_box() {
    let refused = "refused by the hasher";
    let hashing = Hashing::new(
        move |typed| {
            if typed == refused {
                Err(password::HashError::Password(PasswordError::TooShort))
            } else {
                password::hash(CHEAP_HASH_PARAMS, typed)
            }
        },
        password::verify,
    );
    let app = App::start_with(Options {
        hashing: Some(hashing),
        ..Options::default()
    })
    .await;
    let form = with(
        with(good_form(), "password", refused),
        "password_again",
        refused,
    );
    let reply = app.client().post("/setup", &form).await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "status of a set-up whose password the hasher refuses:\n{}",
        reply.body
    );
    assert!(
        reply.shows(&PasswordError::TooShort.to_string()),
        "the refusal of the hasher is not shown:\n{}",
        reply.body
    );
    let html = &reply.body;
    let message = PasswordError::TooShort.to_string();
    let password_at = html
        .find(control(html, "password"))
        .unwrap_or_else(|| panic!("no password box:\n{html}"));
    let again_at = html
        .find(control(html, "password_again"))
        .unwrap_or_else(|| panic!("no second password box:\n{html}"));
    assert!(
        unescape(&html[password_at..again_at]).contains(&message),
        "the refusal of the hasher is not under the password box:\n{html}"
    );
    assert!(
        !unescape(&html[again_at..]).contains(&message),
        "the refusal of the hasher is under the second password box:\n{html}"
    );
    assert_nothing_written(&app, "after a set-up whose password the hasher refuses").await;
}
