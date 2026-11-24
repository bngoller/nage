//! The Origin check (SPEC 5.3, decision 110 as PLAN M2 slice 8 amends it): every request whose
//! method is not GET, HEAD, OPTIONS or TRACE, each mutating request (POST, PUT, PATCH, DELETE),
//! QUERY and every extension method among them, must say that it comes from one of Nage's own
//! pages. The four are listed here by name rather than taken from the `http` crate's safe
//! methods, which count QUERY too: a QUERY carries a body that a cross-site page could send. It
//! runs after the body limit and before the session rule, and covers `/setup` and `/login` too,
//! since a cross-site form post could otherwise set the book up or sign a browser in under
//! another account.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method};
use axum::middleware::Next;
use axum::response::Response;

use crate::Ctx;

#[cfg(test)]
mod tests;

/// Lets every GET, HEAD, OPTIONS and TRACE request through, and any other only when
/// [`from_own_page`] accepts its headers; any other request gets 403 with the page that says
/// what was refused, why and what to do (decision 102), and nothing behind this layer runs.
pub(crate) async fn origin_check(State(ctx): State<Ctx>, request: Request, next: Next) -> Response {
    if passes_without_check(request.method())
        || from_own_page(request.headers(), ctx.state.trust_proxy)
    {
        next.run(request).await
    } else {
        cross_site()
    }
}

/// Whether a request of `method` passes without the Origin check: GET, HEAD, OPTIONS and TRACE
/// alone (decision 110 as PLAN M2 slice 8 amends it, the maintainer's pick "Here, in this
/// slice"); QUERY, every extension method and every mutating one need it.
pub(crate) fn passes_without_check(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

/// Whether a request with these headers comes from one of Nage's own pages, asked of every
/// request whose method is not GET, HEAD, OPTIONS or TRACE (QUERY and the extension methods
/// included): the host and port of its `Origin` header, or of its `Referer` when it has no
/// `Origin`, are those of
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

/// The authority of a URL as an `Origin` or `Referer` header writes it: the text between the
/// first `://` and the next `/`, `?` or `#`, or the end. `None` unless a scheme comes before
/// `://` (a letter, then letters, digits, `+`, `-` or `.`, as RFC 3986 writes one) and the
/// authority is not empty, so `null`, a bare host and a host with a path or query that holds
/// `://` never match anything.
fn authority(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    (is_scheme(scheme) && !authority.is_empty()).then_some(authority)
}

/// Whether `text` is a URL's scheme: a letter, then letters, digits, `+`, `-` or `.` (RFC 3986,
/// section 3.1).
fn is_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

use axum::http::StatusCode;

use crate::messages::Refusal;
use crate::pages::{CROSS_SITE_HEADING, Layout, RefusalPage, render_with};

/// The answer to a request the Origin check meets (any method but GET, HEAD, OPTIONS and TRACE)
/// that does not come from Nage's own page: 403 with the page that says what was refused, why and
/// what to do (decisions 102 and 110).
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
