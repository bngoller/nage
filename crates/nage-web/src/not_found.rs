//! The page for an unknown address (SPEC 5.3, INV-27, decisions 102, 109 and 118): the
//! router's fallback, behind the same layers as every route, so the body limit, the Origin
//! check and the session rule cover it. Without a session such an address still leads to the
//! sign-in page, and to `/setup` while no user exists; an unknown path under `/static/` keeps
//! its bare 404, `/static/` itself included, and a known address asked with a method it does
//! not take its bare 405.

use axum::Extension;
use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use nage_core::sessions::SignedIn;

use crate::Ctx;

/// The answer to a request with a valid session for an address that no route answers,
/// whatever its method: 404 with the page that says in three parts that Nage has no page at
/// that address (`Refusal::NotFound`) and offers the home page, under the menu with no entry
/// marked. The page never holds the address, the query or anything else of the request. On a
/// GET the page shows and clears a one-time message (`flash::pending`); on any other method
/// it does neither. A request the session rule passed unread carries no signed-in user: the
/// one such address the fallback meets is `/static/` itself, which no asset route matches,
/// and it keeps the bare 404 of every unknown path under `/static/`.
pub(crate) async fn not_found(
    State(ctx): State<Ctx>,
    signed_in: Option<Extension<SignedIn>>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some(Extension(signed_in)) = signed_in else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let pending = flash::pending(&method, &headers);
    let page = NotFoundPage {
        layout: Layout {
            menu: Some(Menu {
                display_name: signed_in.user.display_name,
                current: None,
            }),
            message: pending.message(true),
            ..Layout::embedded()
        },
        message: Refusal::NotFound.to_string(),
    };
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    pending.settle(render_with(StatusCode::NOT_FOUND, &page), secure)
}

use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::messages::Refusal;
use crate::pages::{Layout, Menu, NotFoundPage, render_with};
use crate::{cookie, flash};
