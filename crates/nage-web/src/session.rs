//! The session rule (SPEC 5.3, INV-27, decisions 108 and 109): the innermost layer, which
//! every request the router answers passes, one for an unknown address included.
//!
//! - `/health` and every address under `/static/` always pass, unread: no cookie is looked
//!   at and the database is not asked. `/static` without the slash is like any other address.
//! - While no user exists, `/setup` passes and every other request is sent to `/setup`,
//!   `/login` included. Once a user exists the router remembers it.
//! - Otherwise the `nage_session` cookie is looked up: a valid session (its hash known, not
//!   expired, its user active) reaches the route as the signed-in user, who is put into the
//!   request's extensions; `/setup` and `/login` pass without one; any other request is sent
//!   to the sign-in page.
//! - A cookie that is sent and is unknown, expired, a deactivated user's or of another shape
//!   is cleared with `Max-Age=0` on the answer, unless the answer sets the session cookie
//!   itself (a sign-in that replaces it). An answer that sets or clears only another cookie,
//!   the one-time message's (decision 117), does not count: `cookie::sets_session` looks for
//!   `nage_session` by name.
//! - "Sent to" is a 303; for an htmx request (`HX-Request: true`) it is a 401 with
//!   `HX-Redirect`, except for htmx's history-restore request
//!   (`HX-History-Restore-Request: true`), which gets the 303.

use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{Method, Uri};
use axum::middleware::Next;
use axum::response::Response;
use nage_core::users::UserId;

use crate::Ctx;

#[cfg(test)]
mod tests;

/// How old `last_seen_at` must be before a request rewrites it, in seconds (SPEC 5.3: at
/// most once an hour per session).
pub const TOUCH_AFTER_SECONDS: i64 = 3600;

/// How long a touch waits for the writer before it is skipped and the page served
/// (decision 108): a page view never waits behind a long write.
pub const TOUCH_WAIT: Duration = Duration::from_millis(100);

/// The longest `next` that is followed, in bytes (decision 109: 2 KiB).
pub(crate) const MAX_NEXT_BYTES: usize = 2048;

/// The signed-in user's id as the session rule found it, put into the response's extensions
/// for the request log (decision 109); absent without a valid session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoggedUser(pub(crate) UserId);

/// The rule above. A request with a valid session carries
/// `nage_core::sessions::SignedIn` in its extensions and its response carries
/// [`LoggedUser`]. On a GET or HEAD whose session was last seen [`TOUCH_AFTER_SECONDS`] ago
/// or more, the session is touched in a short write transaction of its own, waited for at
/// most [`TOUCH_WAIT`], and the cookie is sent again with the new `Max-Age`, unless the
/// route set the session cookie itself (`cookie::sets_session`; a `Set-Cookie` for the
/// one-time message does not count, decision 117); a mutating request never touches, so it
/// stays one transaction (INV-28). A database that is busy or fails is `AppError`'s answer.
pub(crate) async fn session_rule(State(ctx): State<Ctx>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path == HEALTH_PATH || path.starts_with(STATIC_PREFIX) {
        return next.run(request).await;
    }
    apply(&ctx, request, next)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

/// `next` when it may be followed after a sign-in, `None` when it may not (SPEC 5.3: never an
/// absolute URL; decision 109): it starts with exactly one `/`, holds no backslash and no
/// control character, and is at most [`MAX_NEXT_BYTES`] long. The caller goes to `/` instead
/// of a `next` that fails.
pub(crate) fn safe_next(next: &str) -> Option<&str> {
    let one_slash = next.starts_with('/') && !next.starts_with("//");
    let safe = one_slash
        && next.len() <= MAX_NEXT_BYTES
        && !next.contains('\\')
        && !next.chars().any(char::is_control);
    safe.then_some(next)
}

/// Where a request without a session is sent: `/login`, with `?next=` and the request's path
/// and query when the request is a GET whose path and query pass [`safe_next`] and are not
/// `/` alone. The value is percent-encoded into one query value: every byte except ASCII
/// letters, digits, `-`, `.`, `_` and `~`.
pub(crate) fn login_location(method: &Method, uri: &Uri) -> String {
    let target = uri.path_and_query().map_or("/", PathAndQuery::as_str);
    match safe_next(target) {
        Some(next) if *method == Method::GET && next != "/" => {
            let mut location = String::from(LOGIN_PATH);
            location.push_str("?next=");
            for byte in next.bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    location.push(char::from(byte));
                } else {
                    location.push('%');
                    for nibble in [byte >> 4, byte & 0x0f] {
                        location.push(char::from(UPPER_DIGITS[usize::from(nibble)]));
                    }
                }
            }
            location
        }
        _ => String::from(LOGIN_PATH),
    }
}

use axum::http::uri::PathAndQuery;

/// The sign-in page's address.
const LOGIN_PATH: &str = "/login";

/// The hexadecimal digits of a percent-encoded byte, by value (RFC 3986 prefers capitals).
const UPPER_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

use std::sync::atomic::Ordering;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use nage_core::sessions::{self, Session, SessionId, SignedIn};
use nage_core::storage;
use nage_core::timestamp::Timestamp;
use nage_core::users;

use crate::{AppError, cookie};

/// The address the rule never reads a request for.
const HEALTH_PATH: &str = "/health";

/// The start of every address the rule never reads a request for: the static files.
const STATIC_PREFIX: &str = "/static/";

/// The set-up page's address.
const SETUP_PATH: &str = "/setup";

/// The header htmx marks its requests with, and the one it adds to its history-restore
/// request (its value is `true` in both).
const HX_REQUEST: &str = "hx-request";
const HX_HISTORY_RESTORE_REQUEST: &str = "hx-history-restore-request";

/// The header that tells htmx where to send the whole page.
const HX_REDIRECT: &str = "hx-redirect";

/// The rule for every request it reads, in the order of the module's list; a database that
/// is busy or fails, and a clock outside the years a stamp holds, are the error.
async fn apply(ctx: &Ctx, request: Request, next: Next) -> Result<Response, AppError> {
    let headers = request.headers();
    let htmx = is_true(headers, HX_REQUEST) && !is_true(headers, HX_HISTORY_RESTORE_REQUEST);
    let secure = cookie::is_secure(headers, ctx.state.trust_proxy);
    let sent = cookie::session_cookie(headers);
    let path = request.uri().path();
    let (setup, login) = (path == SETUP_PATH, path == LOGIN_PATH);
    let mut response = if !user_exists(ctx).await? {
        if setup {
            next.run(request).await
        } else {
            send_to(SETUP_PATH, SETUP_PATH, htmx)
        }
    } else if let Some((signed_in, token, now)) = valid_session(ctx, sent.as_deref()).await? {
        return signed_in_answer(ctx, request, next, signed_in, token, now, secure).await;
    } else if setup || login {
        next.run(request).await
    } else {
        send_to(
            &login_location(request.method(), request.uri()),
            LOGIN_PATH,
            htmx,
        )
    };
    if sent.is_some() && !cookie::sets_session(response.headers()) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie::clear(secure));
    }
    Ok(response)
}

/// Whether a user exists: known once true, since users are never deleted (decision 109);
/// until then the users are counted at every request, so that a book set up by `nage init`
/// from another process is seen at the next one.
async fn user_exists(ctx: &Ctx) -> Result<bool, AppError> {
    // The flag guards no other memory: a request that sees it late counts the users again.
    if ctx.has_user.load(Ordering::Relaxed) {
        return Ok(true);
    }
    let mut read = ctx.state.storage.begin_read().await?;
    let exists = users::count_users(&mut read)
        .await
        .map_err(storage::Error::Sqlx)?
        > 0;
    if exists {
        ctx.has_user.store(true, Ordering::Relaxed);
    }
    Ok(exists)
}

/// The valid session the cookie value `sent` names, with that value and the instant it was
/// valid at; `None` without a cookie, for a value of another shape, and for a session that
/// is unknown, expired or a deactivated user's.
async fn valid_session(
    ctx: &Ctx,
    sent: Option<&str>,
) -> Result<Option<(SignedIn, String, Timestamp)>, AppError> {
    let Some((token, hash)) = sent.and_then(|token| Some((token, cookie::token_hash(token)?)))
    else {
        return Ok(None);
    };
    let now = ctx.state.clock.now()?;
    let mut read = ctx.state.storage.begin_read().await?;
    let found = sessions::valid_session(&mut read, &hash, now)
        .await
        .map_err(storage::Error::Sqlx)?;
    Ok(found.map(|signed_in| (signed_in, token.to_owned(), now)))
}

/// A request with a valid session: touched when it is a GET or HEAD and the session was last
/// seen [`TOUCH_AFTER_SECONDS`] ago or more, then handed to its route with the signed-in user
/// in its extensions; the answer carries [`LoggedUser`], and the cookie again with its new
/// `Max-Age` after a touch, unless the route set the session cookie itself
/// (`cookie::sets_session`). `token` is the cookie's value as sent and `now` the instant the
/// session was found valid at.
async fn signed_in_answer(
    ctx: &Ctx,
    mut request: Request,
    next: Next,
    mut signed_in: SignedIn,
    token: String,
    now: Timestamp,
    secure: bool,
) -> Result<Response, AppError> {
    let reads = matches!(*request.method(), Method::GET | Method::HEAD);
    let idle = now
        .as_second()
        .saturating_sub(signed_in.session.last_seen_at.as_second());
    let touched = if reads && idle >= TOUCH_AFTER_SECONDS {
        touch(ctx, signed_in.session.id, now).await?
    } else {
        None
    };
    if let Some(session) = &touched {
        signed_in.session = session.clone();
    }
    let user = signed_in.user.id;
    request.extensions_mut().insert(signed_in);
    let mut response = next.run(request).await;
    response.extensions_mut().insert(LoggedUser(user));
    if let Some(session) = touched
        && !cookie::sets_session(response.headers())
    {
        let max_age = session
            .expires_at
            .as_second()
            .saturating_sub(now.as_second());
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie::set(&token, max_age, secure));
    }
    Ok(response)
}

/// Touches the session `id` at `now` in a write transaction of its own, waiting at most
/// [`TOUCH_WAIT`] for the writer (decision 108). `None` when the writer does not come in time
/// or is busy, and when the session is gone meanwhile: the page is then served untouched. Any
/// other database failure is the error.
async fn touch(ctx: &Ctx, id: SessionId, now: Timestamp) -> Result<Option<Session>, AppError> {
    let mut tx = match tokio::time::timeout(TOUCH_WAIT, ctx.state.storage.begin_write()).await {
        Ok(Ok(tx)) => tx,
        Ok(Err(error)) if error.is_busy() => return Ok(None),
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => return Ok(None),
    };
    let session = sessions::touch_session(&mut tx, id, now)
        .await
        .map_err(storage::Error::Sqlx)?;
    tx.commit().await.map_err(storage::Error::Sqlx)?;
    Ok(session)
}

/// Whether header `name` is `true`, as htmx writes its flags.
fn is_true(headers: &HeaderMap, name: &str) -> bool {
    headers.get(name).is_some_and(|value| value == "true")
}

/// Sends a request to `location`: a 303, or for htmx a 401 with `HX-Redirect` to
/// `htmx_location` and no `Location`, since htmx would follow a 303 itself and put the page
/// it leads to inside the current one.
fn send_to(location: &str, htmx_location: &'static str, htmx: bool) -> Response {
    if htmx {
        (
            StatusCode::UNAUTHORIZED,
            [(HX_REDIRECT, HeaderValue::from_static(htmx_location))],
        )
            .into_response()
    } else {
        Redirect::to(location).into_response()
    }
}
