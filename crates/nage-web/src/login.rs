//! Signing in and out (SPEC 5.3, decisions 106 and 111): `/login` and `POST /logout`.

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use nage_core::sessions::SignedIn;
use serde::Deserialize;

use crate::{AppError, Ctx};

/// The query of `GET /login`: where the sign-in leads afterwards, and `changed-elsewhere`,
/// present when a change of one's own password was overtaken (decision 112).
#[derive(Deserialize)]
pub(crate) struct LoginQuery {
    #[serde(default)]
    pub(crate) next: String,
    #[serde(default, rename = "changed-elsewhere")]
    pub(crate) changed_elsewhere: Option<String>,
}

/// The sign-in form as posted. A field that is missing is empty.
#[derive(Deserialize)]
pub(crate) struct LoginForm {
    #[serde(default)]
    pub(crate) username: String,
    #[serde(default)]
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) next: String,
}

/// `GET /login`: the form, with `next` in a hidden field when `session::safe_next` accepts
/// it and nothing otherwise, and, when the query holds `changed-elsewhere` (the marker of
/// [`crate::account::CHANGED_ELSEWHERE_LOCATION`]), the notice
/// `Refusal::PasswordChangedElsewhere` above the form; without the marker there is no
/// notice. The page shows Nage's name and never the book's.
pub(crate) async fn get_login(Query(query): Query<LoginQuery>) -> Response {
    let notice = query
        .changed_elsewhere
        .map(|_| Refusal::PasswordChangedElsewhere.to_string());
    render(&LoginPage {
        notice,
        ..page(String::new(), &query.next, None)
    })
}

/// `POST /login`, decided in this order (decision 106): a paused username is refused first,
/// with 429, a `Retry-After` of the seconds left and the form saying how long to wait,
/// nothing checked and nothing counted; the user is found by the fold of the typed username,
/// with the stored hash; the password is verified in a blocking thread outside any
/// transaction, against the dummy hash when the username is unknown or the stored hash cannot
/// be read; then the pause is decided again (decision 111): when a pause for the username has
/// begun meanwhile, the attempt is answered as one during the pause, whatever its password,
/// and neither counted nor forgetting; otherwise a miss (an unknown username, a wrong
/// password, a deactivated user whatever the password) is remembered as a failure, however
/// long the typed spelling of a username that found a user, and answers the form again with
/// 422 and the one message, the typed username kept; the right password of an active user
/// forgets the username's failures, and one write transaction then reads the user again,
/// deletes the expired sessions and creates the session (`nage_core::sessions::sign_in`), a
/// user who changed meanwhile getting the one message too. Success is a 303 to `next`, or to
/// `/` when `next` is not safe, with the session cookie.
pub(crate) async fn post_login(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    form: Result<Form<LoginForm>, FormRejection>,
) -> Result<Response, AppError> {
    let Form(LoginForm {
        username,
        password,
        next,
    }) = match form {
        Ok(form) => form,
        Err(rejection) => return Ok(forms::unreadable(rejection)),
    };
    let now = ctx.state.clock.now()?;
    let paused = |username: String, seconds: u64| {
        let problem = Refusal::SignInPaused { seconds }.to_string();
        let mut response = render_with(
            StatusCode::TOO_MANY_REQUESTS,
            &page(username, &next, Some(problem)),
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        response
    };
    if let Err(seconds) = ctx.lockout.check(&username, now) {
        return Ok(paused(username, seconds));
    }
    let refused = |username: String| {
        render_with(
            StatusCode::UNPROCESSABLE_ENTITY,
            &page(username, &next, Some(Refusal::SignIn.to_string())),
        )
    };
    let found = {
        let mut read = ctx.state.storage.begin_read().await?;
        users::user_by_username(&mut read, &username)
            .await
            .map_err(storage::Error::Sqlx)?
    };
    let stored_hash = found
        .as_ref()
        .map(|credentials| credentials.password_hash.clone());
    let verified = ctx.state.hashing.verify(password, stored_hash).await?;
    let user_found = found.is_some();
    let Some(credentials) = found.filter(|credentials| verified && credentials.user.is_enabled())
    else {
        let failed = Attempt::Failed { user_found };
        if let Err(seconds) = ctx.lockout.settle(&username, now, failed) {
            return Ok(paused(username, seconds));
        }
        return Ok(refused(username));
    };
    if let Err(seconds) = ctx.lockout.settle(&username, now, Attempt::Succeeded) {
        return Ok(paused(username, seconds));
    }
    let token = cookie::new_token()?;
    let mut tx = ctx.state.storage.begin_write().await?;
    let Some(session) = sessions::sign_in(&mut tx, &credentials, now, &token.hash)
        .await
        .map_err(storage::Error::Sqlx)?
    else {
        // Deactivated, or given another password, while the password was being verified.
        return Ok(refused(username));
    };
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    let max_age = session
        .expires_at
        .as_second()
        .saturating_sub(now.as_second());
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut response = Redirect::to(safe_next(&next).unwrap_or(START_PATH)).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie::set(&token.token, max_age, secure),
    );
    Ok(response)
}

/// `POST /logout`: deletes the session in use, clears the cookie and answers 303 to the
/// sign-in page. The session rule has already sent a request without a session there.
pub(crate) async fn post_logout(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    Extension(signed_in): Extension<SignedIn>,
) -> Result<Response, AppError> {
    let mut tx = ctx.state.storage.begin_write().await?;
    sessions::delete_session(&mut tx, signed_in.session.id)
        .await
        .map_err(storage::Error::Sqlx)?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let mut response = Redirect::to(LOGIN_PATH).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie::clear(secure));
    Ok(response)
}

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use nage_core::sessions;
use nage_core::storage;
use nage_core::users;

use crate::lockout::Attempt;
use crate::messages::Refusal;
use crate::pages::{Layout, LoginPage, render, render_with};
use crate::session::safe_next;
use crate::{cookie, forms};

/// Where a sign-in leads when `next` is not safe: the start page.
const START_PATH: &str = "/";

/// The sign-in page's address, where a sign-out leads.
const LOGIN_PATH: &str = "/login";

/// The sign-in page with `username` in its box, `next` in its hidden field when
/// [`safe_next`] accepts it, and `problem` above the form; no notice.
fn page(username: String, next: &str, problem: Option<String>) -> LoginPage {
    LoginPage {
        layout: Layout::embedded(),
        username,
        next: safe_next(next).unwrap_or_default().to_owned(),
        problem,
        notice: None,
    }
}
