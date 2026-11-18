//! The home page (SPEC 6.2, 6.3, W1, decisions 119 and 120): `/`, behind the sign-in, in place
//! of milestone 1's placeholder.

use axum::Extension;
use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::Response;
use nage_core::sessions::SignedIn;

use crate::{AppError, Ctx};

/// `GET /`: the home page for the signed-in user, under the menu with Home marked as the page
/// in view. Today is the date in the configured zone at the clock's instant
/// (`clock::today`); one read transaction reads the core's model as of that day
/// (`nage_core::home::home_page`), taken through `Storage::begin_read` (a snapshot, so the
/// lines and the totals are of one moment), never through `Storage::reader()`, which hands
/// out the pool without one. The template only renders the model: the day the balances are
/// for, the Assets and Liabilities sections with their totals, groups and lines, each amount
/// as SPEC 6.10 displays it with the book's minor digits, and the Net worth line; or, when
/// Assets and Liabilities hold no account to list, the empty state in words and no figure.
/// The page shows and clears a one-time message (`flash::pending`); a HEAD does neither. A
/// date outside the range and an amount that does not fit in 64 bits answer 500, a busy
/// database the busy page.
pub(crate) async fn get_home(
    State(ctx): State<Ctx>,
    Extension(signed_in): Extension<SignedIn>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let today = clock::today(&ctx.state.clock, &ctx.state.zone)?;
    let home = {
        let mut read = ctx.state.storage.begin_read().await?;
        home::home_page(&mut read, today).await?
    };
    let pending = flash::pending(&method, &headers);
    let page = HomePage {
        layout: Layout {
            menu: Some(Menu {
                display_name: signed_in.user.display_name,
                current: Some(Entry::Home),
            }),
            message: pending.message(true),
            ..Layout::embedded()
        },
        home,
    };
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    Ok(pending.settle(render(&page), secure))
}

use nage_core::home;

use crate::pages::{Entry, HomePage, Layout, Menu, render};
use crate::{clock, cookie, flash};
