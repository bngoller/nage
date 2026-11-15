//! The set-up page (SPEC W1 steps 1 to 3, 5.3, decision 105): `/setup` while no user exists.
//! An accepted form hashes the password first, outside any transaction, then in one write
//! transaction creates the book, the admin and the chart through `nage_core::setup::initialize`
//! and the admin's session, and answers 303 to `/` with the session cookie. Once a user
//! exists the address answers 404 with the page that says so.

use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::Response;
use nage_core::currency::Currency;
use serde::Deserialize;

use crate::pages::{SetupProblems, SetupView};
use crate::{AppError, Ctx};

#[cfg(test)]
mod tests;

/// The currency the list preselects (W1): the set-up page keeps CAD, while `nage init` makes
/// the currency be typed (decision 113).
pub(crate) const DEFAULT_CURRENCY: &str = "CAD";

/// The set-up form as posted. A field that is missing is empty; the tick box is present only
/// when it is ticked.
#[derive(Deserialize)]
pub(crate) struct SetupForm {
    #[serde(default)]
    pub(crate) book_name: String,
    #[serde(default)]
    pub(crate) currency: String,
    #[serde(default)]
    pub(crate) username: String,
    #[serde(default)]
    pub(crate) display_name: String,
    #[serde(default)]
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) password_again: String,
    #[serde(default)]
    pub(crate) starter_chart: Option<String>,
}

/// What an accepted form asks for: the names as typed (the core normalizes them), the
/// currency from the table, the password as typed and whether the starter chart is wanted.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Accepted {
    pub(crate) book_name: String,
    pub(crate) currency: Currency,
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) password: String,
    pub(crate) starter_chart: bool,
}

/// Checks the whole form before anything is written and returns every problem at once, each
/// for its own box (decision 105): the book name, the username and the display name by the
/// core's name rules, worded as the core words them (`BookError::Name`, `UserError::Username`,
/// `UserError::DisplayName`); the currency against the table; the password by the core's
/// rules (`PasswordError`), as typed; the second password against the first
/// (`Refusal::SetupPasswordsDiffer`). With any problem the notice above the form is
/// `Refusal::SetupNotDone`.
pub(crate) fn check(form: &SetupForm) -> Result<Accepted, Box<SetupProblems>> {
    let currency = Currency::from_code(&form.currency);
    let problems = SetupProblems {
        notice: None,
        book_name: checked_name(&form.book_name, Colon::Allowed)
            .err()
            .map(|error| BookError::Name(error).to_string()),
        currency: currency.is_none().then(|| {
            Refusal::UnknownCurrency {
                typed: form.currency.clone(),
            }
            .to_string()
        }),
        username: checked_name(&form.username, Colon::Allowed)
            .err()
            .map(|error| UserError::Username(error).to_string()),
        display_name: checked_name(&form.display_name, Colon::Allowed)
            .err()
            .map(|error| UserError::DisplayName(error).to_string()),
        password: password::check(&form.password)
            .err()
            .map(|error| error.to_string()),
        password_again: (form.password_again != form.password)
            .then(|| Refusal::SetupPasswordsDiffer.to_string()),
    };
    match currency {
        Some(currency) if problems == SetupProblems::default() => Ok(Accepted {
            book_name: form.book_name.clone(),
            currency,
            username: form.username.clone(),
            display_name: form.display_name.clone(),
            password: form.password.clone(),
            starter_chart: form.starter_chart.is_some(),
        }),
        _ => Err(Box::new(not_done(problems))),
    }
}

/// What a refused form shows again in its boxes: everything typed except the passwords, the
/// list back on [`DEFAULT_CURRENCY`] when the currency sent is not in the table.
pub(crate) fn view(form: &SetupForm) -> SetupView {
    SetupView {
        book_name: form.book_name.clone(),
        currency: Currency::from_code(&form.currency)
            .map_or(DEFAULT_CURRENCY, Currency::code)
            .to_owned(),
        username: form.username.clone(),
        display_name: form.display_name.clone(),
        starter_chart: form.starter_chart.is_some(),
    }
}

/// `GET /setup`: the empty form, [`DEFAULT_CURRENCY`] selected and the tick box ticked, while
/// no user exists; the 404 page once one does.
pub(crate) async fn get_setup(State(ctx): State<Ctx>) -> Result<Response, AppError> {
    if ctx.has_user.load(Ordering::Relaxed) {
        return Ok(closed());
    }
    let empty = SetupView {
        book_name: String::new(),
        currency: String::from(DEFAULT_CURRENCY),
        username: String::new(),
        display_name: String::new(),
        starter_chart: true,
    };
    Ok(render(&page(empty, SetupProblems::default())))
}

/// `POST /setup`. Once a user exists: the 404 page, and nothing is read or written. A form
/// with a problem: the form again with 422, every problem at once, nothing hashed and nothing
/// written. An accepted form: the password hashed, then one write transaction for
/// `initialize` and the session; a second submission that lost the race is refused there by
/// the core (`SetupError::AlreadyInitialized`), writes nothing and gets the 404 page with no
/// session; a core refusal the check did not foresee is shown under its box. Success is a 303
/// to `/` with the session cookie.
pub(crate) async fn post_setup(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    form: Result<Form<SetupForm>, FormRejection>,
) -> Result<Response, AppError> {
    if ctx.has_user.load(Ordering::Relaxed) {
        return Ok(closed());
    }
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => return Ok(forms::unreadable(rejection)),
    };
    let refused = |problems: SetupProblems| {
        render_with(
            StatusCode::UNPROCESSABLE_ENTITY,
            &page(view(&form), not_done(problems)),
        )
    };
    let accepted = match check(&form) {
        Ok(accepted) => accepted,
        Err(problems) => return Ok(refused(*problems)),
    };
    let password_hash = match ctx.state.hashing.hash(accepted.password).await? {
        Ok(hash) => hash,
        Err(error) => {
            return Ok(refused(SetupProblems {
                password: Some(error.to_string()),
                ..SetupProblems::default()
            }));
        }
    };
    let now = ctx.state.clock.now()?;
    let token = cookie::new_token()?;
    let setup = Setup {
        book_name: accepted.book_name,
        currency: accepted.currency,
        admin: NewUser {
            username: accepted.username,
            display_name: accepted.display_name,
            password_hash,
            is_admin: true,
        },
        starter_chart: accepted.starter_chart,
    };
    let mut tx = ctx.state.storage.begin_write().await?;
    let admin = match initialize(&mut tx, now, &setup).await {
        Ok(initialized) => initialized.admin,
        Err(SetupError::AlreadyInitialized | SetupError::Book(BookError::AlreadyInitialized)) => {
            // Another set-up won the race for the writer; this one writes nothing.
            drop(tx);
            ctx.has_user.store(true, Ordering::Relaxed);
            return Ok(closed());
        }
        Err(error) => return Ok(refused(unforeseen(error)?)),
    };
    let session = sessions::create_session(&mut tx, admin.id, now, &token.hash)
        .await
        .map_err(storage::Error::Sqlx)?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    ctx.has_user.store(true, Ordering::Relaxed);
    let max_age = session
        .expires_at
        .as_second()
        .saturating_sub(now.as_second());
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut response = Redirect::to(START_PATH).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie::set(&token.token, max_age, secure),
    );
    Ok(response)
}

use std::sync::atomic::Ordering;

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use nage_core::book::BookError;
use nage_core::currency::CURRENCIES;
use nage_core::fold::{Colon, checked_name};
use nage_core::password;
use nage_core::sessions;
use nage_core::setup::{Setup, SetupError, initialize};
use nage_core::storage;
use nage_core::users::{NewUser, UserError};

use crate::messages::Refusal;
use crate::pages::{Layout, SetupClosedPage, SetupPage, render, render_with};
use crate::{cookie, forms};

/// Where an accepted set-up leads: the start page, signed in.
const START_PATH: &str = "/";

/// The set-up page with `form` in its boxes and `problems` under them.
fn page(form: SetupView, problems: SetupProblems) -> SetupPage {
    SetupPage {
        layout: Layout::embedded(),
        currencies: CURRENCIES,
        form,
        problems,
    }
}

/// `problems` with the notice above the form, `Refusal::SetupNotDone`.
fn not_done(problems: SetupProblems) -> SetupProblems {
    SetupProblems {
        notice: Some(Refusal::SetupNotDone.to_string()),
        ..problems
    }
}

/// The answer once a user exists: 404 with the page that says Nage is already set up
/// (decision 105).
fn closed() -> Response {
    render_with(
        StatusCode::NOT_FOUND,
        &SetupClosedPage {
            layout: Layout::embedded(),
            message: Refusal::SetupClosed.to_string(),
        },
    )
}

/// A refusal of `initialize` that [`check`] did not foresee, under the box it is about. A
/// database failure, and a refusal no box of the form can be the cause of, are the error.
fn unforeseen(error: SetupError) -> Result<SetupProblems, AppError> {
    let mut problems = SetupProblems::default();
    match error {
        SetupError::Book(BookError::Database(error))
        | SetupError::User(UserError::Database(error))
        | SetupError::Database(error) => return Err(storage::Error::Sqlx(error).into()),
        SetupError::Book(error @ BookError::Name(_)) => {
            problems.book_name = Some(error.to_string())
        }
        SetupError::User(error @ (UserError::Username(_) | UserError::UsernameTaken)) => {
            problems.username = Some(error.to_string());
        }
        SetupError::User(error @ UserError::DisplayName(_)) => {
            problems.display_name = Some(error.to_string());
        }
        other => return Err(AppError::Internal(format!("set-up failed: {other}"))),
    }
    Ok(problems)
}
