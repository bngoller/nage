//! The Origin check (SPEC 5.3, decision 110): every request whose method is not a safe one
//! (GET, HEAD, OPTIONS, TRACE and QUERY, the methods the `http` crate counts as safe), each
//! mutating request (POST, PUT, PATCH, DELETE) among them, must say that it comes from one of
//! Nage's own pages. It runs after the body limit and before the session rule, and covers
//! `/setup` and `/login` too, since a cross-site form post could otherwise set the book up or
//! sign a browser in under another account.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;

use crate::Ctx;

#[cfg(test)]
mod tests;

/// Lets every GET, HEAD and other safe request through, and a mutating one only when
/// [`from_own_page`] accepts its headers; any other mutating request gets 403 with the page
/// that says what was refused, why and what to do (decision 102), and nothing behind this
/// layer runs.
pub(crate) async fn origin_check(State(ctx): State<Ctx>, request: Request, next: Next) -> Response {
    if request.method().is_safe() || from_own_page(request.headers(), ctx.state.trust_proxy) {
        next.run(request).await
    } else {
        cross_site()
    }
}

/// Whether a mutating request with these headers comes from one of Nage's own pages: the host
/// and port of its `Origin` header, or of its `Referer` when it has no `Origin`, are those of
/// its `Host` header or, when `trust_proxy` is on, of the first value of `X-Forwarded-Host`;
/// either of the two matching is enough. They are compared as written, the host ignoring
/// ASCII case: the scheme is ignored, an absent port matches only an absent port, and nothing
/// is resolved or normalized. `Origin: null`, a missing `Origin` and `Referer`, and a value
/// that is not a scheme, `://` and a host (whatever path, query or fragment follows the host)
/// never match; a `Referer` counts only when the request has no `Origin` at all.
pub(crate) fn from_own_page(headers: &HeaderMap, trust_proxy: bool) -> bool {
    let text = |name: HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
    // A `Referer` counts only when there is no `Origin` header at all, whatever its value.
    let sender = match headers.get(header::ORIGIN) {
        Some(origin) => origin.to_str().ok().and_then(authority),
        None => text(header::REFERER).and_then(authority),
    };
    let Some(sender) = sender else {
        return false;
    };
    let names = |host: &str| host.eq_ignore_ascii_case(sender);
    text(header::HOST).is_some_and(names)
        || (trust_proxy
            && text(HeaderName::from_static(FORWARDED_HOST))
                .and_then(|value| value.split(',').next())
                .is_some_and(|host| names(host.trim())))
}

use axum::http::{HeaderName, header};

/// The header a reverse proxy names the host the browser asked for in.
const FORWARDED_HOST: &str = "x-forwarded-host";

/// The authority of a URL as an `Origin` or `Referer` header writes it: the text between `://`
/// and the next `/`, `?` or `#`, or the end. `None` unless a scheme comes before `://` and the
/// authority is not empty, so `null` and a bare host never match anything.
fn authority(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    (!scheme.is_empty() && !authority.is_empty()).then_some(authority)
}

use axum::http::StatusCode;

use crate::messages::Refusal;
use crate::pages::{CROSS_SITE_HEADING, Layout, RefusalPage, render_with};

/// The answer to a mutating request that does not come from Nage's own page: 403 with the
/// page that says what was refused, why and what to do (decisions 102 and 110).
fn cross_site() -> Response {
    render_with(
        StatusCode::FORBIDDEN,
        &RefusalPage {
            layout: Layout::embedded(),
            heading: CROSS_SITE_HEADING,
            message: Refusal::CrossSite.to_string(),
        },
    )
}
