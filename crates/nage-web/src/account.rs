//! Changing one's own password (SPEC 5.3, W14, decision 112): `/account/password`, behind the
//! sign-in. The page asks for the current password, then the new one twice, and carries the
//! Sign out button until slice 6's navigation does.

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, Query, State};
use axum::response::Response;
use nage_core::sessions::SignedIn;
use serde::Deserialize;

use crate::pages::PasswordProblems;
use crate::{AppError, Ctx};

#[cfg(test)]
mod tests;

/// Where an accepted change leads: the page again, which then confirms the change.
pub(crate) const CHANGED_LOCATION: &str = "/account/password?changed";

/// Where a change leads that a reset or another change overtook between the check of the
/// current password and the save (decision 112): the sign-in page, whose marker
/// `changed-elsewhere` makes it say that the password was not changed
/// (`Refusal::PasswordChangedElsewhere`). The session in use was ended by the reset, and the
/// session rule clears its cookie at that next request.
pub(crate) const CHANGED_ELSEWHERE_LOCATION: &str = "/login?changed-elsewhere";

/// The query of `GET /account/password`: `changed` is present after an accepted change.
#[derive(Deserialize)]
pub(crate) struct PasswordQuery {
    #[serde(default)]
    pub(crate) changed: Option<String>,
}

/// The change-password form as posted. A field that is missing is empty.
#[derive(Deserialize)]
pub(crate) struct PasswordForm {
    #[serde(default)]
    pub(crate) current_password: String,
    #[serde(default)]
    pub(crate) new_password: String,
    #[serde(default)]
    pub(crate) new_password_again: String,
}

/// The problems of the new password alone, each for its own box: the new password by the
/// core's rules (`PasswordError`), as typed, and the second box against the first
/// (`Refusal::NewPasswordsDiffer`). Whether the current password is right is the handler's
/// to find out, since that costs a verification.
pub(crate) fn check_new(form: &PasswordForm) -> PasswordProblems {
    PasswordProblems {
        new: password::check(&form.new_password)
            .err()
            .map(|error| error.to_string()),
        again: (form.new_password_again != form.new_password)
            .then(|| Refusal::NewPasswordsDiffer.to_string()),
        ..PasswordProblems::default()
    }
}

/// `GET /account/password`: the form for the signed-in user, with the confirmation
/// (`Notice::PasswordChanged`) when the query holds `changed`.
pub(crate) async fn get_password(
    Extension(signed_in): Extension<SignedIn>,
    Query(query): Query<PasswordQuery>,
) -> Response {
    let confirmation = query.changed.map(|_| Notice::PasswordChanged.to_string());
    render(&page(&signed_in, confirmation, PasswordProblems::default()))
}

/// `POST /account/password` (decision 112). A pause for the user's username refuses first,
/// with 429, a `Retry-After` and the form saying how long to wait, nothing checked. Otherwise
/// the current password is verified, in a blocking thread, whatever else the form holds, and
/// the pause is decided again (decision 111): when a pause for the user's username has begun
/// meanwhile, the change is answered as one during the pause, whatever the current password,
/// and neither counted nor forgetting; otherwise a miss is remembered as a failure for the
/// user's username and a match forgets its failures. A wrong current password and the
/// problems of the new one are shown together, each under its box, with 422, and nothing is
/// written. An accepted change hashes the new password, then one write transaction stores it,
/// stamps the user and ends the user's other sessions while keeping the one in use
/// (`nage_core::users::change_own_password`), which checks again inside the save that the
/// stored hash is still the one verified. When it is not (`ChangedMeanwhile`: the password was
/// changed or reset after the current one was checked), the other change or the reset stands and
/// the answer is a 303 to [`CHANGED_ELSEWHERE_LOCATION`], the cookie left as it is; the core's
/// other refusals are shown above the form with 422. Success is a 303 to [`CHANGED_LOCATION`].
pub(crate) async fn post_password(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    form: Result<Form<PasswordForm>, FormRejection>,
) -> Result<Response, AppError> {
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => return Ok(forms::unreadable(rejection)),
    };
    let now = ctx.state.clock.now()?;
    let paused = |seconds: u64| {
        let problems = PasswordProblems {
            notice: Some(Refusal::PasswordCheckPaused { seconds }.to_string()),
            ..PasswordProblems::default()
        };
        let mut response = render_with(
            StatusCode::TOO_MANY_REQUESTS,
            &page(&signed_in, None, problems),
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        response
    };
    let username = signed_in.user.username.as_str();
    if let Err(seconds) = ctx.lockout.check(username, now) {
        return Ok(paused(seconds));
    }
    let refused = |problems: PasswordProblems| {
        let problems = PasswordProblems {
            notice: problems
                .notice
                .or_else(|| Some(Refusal::PasswordNotChanged.to_string())),
            ..problems
        };
        render_with(
            StatusCode::UNPROCESSABLE_ENTITY,
            &page(&signed_in, None, problems),
        )
    };
    // The signed-in user with the stored hash, read by the username as stored.
    let found = {
        let mut read = ctx.state.storage.begin_read().await?;
        users::user_by_username(&mut read, username)
            .await
            .map_err(storage::Error::Sqlx)?
            .filter(|credentials| credentials.user.id == signed_in.user.id)
    };
    let stored_hash = found
        .as_ref()
        .map(|credentials| credentials.password_hash.clone());
    let verified = ctx
        .state
        .hashing
        .verify(form.current_password.clone(), stored_hash)
        .await?;
    let mut problems = check_new(&form);
    if verified {
        if let Err(seconds) = ctx.lockout.settle(username, now, Attempt::Succeeded) {
            return Ok(paused(seconds));
        }
    } else {
        let user_found = found.is_some();
        let failed = Attempt::Failed { user_found };
        if let Err(seconds) = ctx.lockout.settle(username, now, failed) {
            return Ok(paused(seconds));
        }
        problems.current = Some(Refusal::CurrentPasswordWrong.to_string());
    }
    let credentials = match found {
        Some(credentials) if problems == PasswordProblems::default() => credentials,
        _ => return Ok(refused(problems)),
    };
    let new_hash = match ctx.state.hashing.hash(form.new_password).await? {
        Ok(hash) => hash,
        Err(error) => {
            return Ok(refused(PasswordProblems {
                new: Some(error.to_string()),
                ..PasswordProblems::default()
            }));
        }
    };
    let mut tx = ctx.state.storage.begin_write().await?;
    match users::change_own_password(&mut tx, &credentials, now, &new_hash, signed_in.session.id)
        .await
    {
        Ok(_) => {}
        Err(PasswordChangeError::ChangedMeanwhile { .. }) => {
            // The other change or the reset stands; this transaction wrote nothing.
            return Ok(Redirect::to(CHANGED_ELSEWHERE_LOCATION).into_response());
        }
        Err(PasswordChangeError::Database(error)) => {
            return Err(storage::Error::Sqlx(error).into());
        }
        Err(refusal) => {
            return Ok(refused(PasswordProblems {
                notice: Some(refusal.to_string()),
                ..PasswordProblems::default()
            }));
        }
    }
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    Ok(Redirect::to(CHANGED_LOCATION).into_response())
}

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use nage_core::password;
use nage_core::storage;
use nage_core::users::{self, PasswordChangeError};

use crate::forms;
use crate::lockout::Attempt;
use crate::messages::{Notice, Refusal};
use crate::pages::{Layout, PasswordPage, render, render_with};

/// The change-password page for the signed-in user, with `confirmation` above the form and
/// `problems` under its boxes.
fn page(
    signed_in: &SignedIn,
    confirmation: Option<String>,
    problems: PasswordProblems,
) -> PasswordPage {
    PasswordPage {
        layout: Layout::embedded(),
        display_name: signed_in.user.display_name.clone(),
        username: signed_in.user.username.clone(),
        confirmation,
        problems,
    }
}
