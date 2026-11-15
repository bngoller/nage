//! HTML pages on the askama base layout (SPEC 11, decision 50): the placeholder `/`, the busy
//! page, and the pages of milestone 2 slice 5 (decisions 105, 106, 110 and 112), whose handlers
//! live in `setup`, `login` and `account`. Every page keeps the title `Nage` (decision 73).

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use nage_core::currency::Currency;

use crate::assets;

#[cfg(test)]
mod tests;

/// What the base layout links: the content-hashed asset URLs (decision 46).
pub(crate) struct Layout {
    pub(crate) htmx_js: &'static str,
    pub(crate) pico_css: &'static str,
    pub(crate) app_css: &'static str,
}

impl Layout {
    /// The URLs of the embedded files.
    pub(crate) fn embedded() -> Self {
        let [htmx, pico, app] = assets::embedded();
        Self {
            htmx_js: &htmx.url,
            pico_css: &pico.url,
            app_css: &app.url,
        }
    }
}

/// The placeholder `/` (PLAN milestone 1 slice 2).
#[derive(Template)]
#[template(path = "index.html")]
pub(crate) struct IndexPage {
    pub(crate) layout: Layout,
}

/// The 503 busy page (SPEC 5.4, decision 53): neutral about who holds the database, with a
/// link back to `/`. Answered by [`crate::AppError`], never by a route of its own.
#[derive(Template)]
#[template(path = "busy.html")]
pub(crate) struct BusyPage {
    pub(crate) layout: Layout,
}

/// What the set-up form shows in its boxes (decision 105): on a refused form, everything that
/// was typed except the passwords, which are never written back into a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SetupView {
    pub(crate) book_name: String,
    /// The currency code selected in the list.
    pub(crate) currency: String,
    pub(crate) username: String,
    pub(crate) display_name: String,
    /// Whether the starter-chart tick box is ticked.
    pub(crate) starter_chart: bool,
}

/// The problems of a refused set-up form, each shown under its own box, all at once
/// (decision 105). `notice` is the line above the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SetupProblems {
    pub(crate) notice: Option<String>,
    pub(crate) book_name: Option<String>,
    pub(crate) currency: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) display_name: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) password_again: Option<String>,
}

/// The set-up page (W1, decision 105): one plain form, a full-page POST to `/setup`.
#[derive(Template)]
#[template(path = "setup.html")]
pub(crate) struct SetupPage {
    pub(crate) layout: Layout,
    /// The currency table, for the list of codes.
    pub(crate) currencies: &'static [Currency],
    pub(crate) form: SetupView,
    pub(crate) problems: SetupProblems,
}

/// The page `/setup` answers with 404 once a user exists (decision 105): Nage is already set
/// up, with a button to the sign-in page.
#[derive(Template)]
#[template(path = "setup_closed.html")]
pub(crate) struct SetupClosedPage {
    pub(crate) layout: Layout,
    pub(crate) message: String,
}

/// The sign-in page (decision 106): Nage's name and never the book's.
#[derive(Template)]
#[template(path = "login.html")]
pub(crate) struct LoginPage {
    pub(crate) layout: Layout,
    /// The username as typed, kept on a refused form.
    pub(crate) username: String,
    /// Where a sign-in leads, already checked: a path on this site, or empty for `/`.
    pub(crate) next: String,
    /// The one message of a refused sign-in, or the wait of a paused one.
    pub(crate) problem: Option<String>,
    /// Why the person was sent here, above the form: a change of their own password that a
    /// reset or another change overtook (`Refusal::PasswordChangedElsewhere`, decision 112),
    /// shown only when the address holds the marker of
    /// [`crate::account::CHANGED_ELSEWHERE_LOCATION`].
    pub(crate) notice: Option<String>,
}

/// The problems of a refused change of password, each under its own box (decision 112).
/// `notice` is the line above the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PasswordProblems {
    pub(crate) notice: Option<String>,
    pub(crate) current: Option<String>,
    pub(crate) new: Option<String>,
    pub(crate) again: Option<String>,
}

/// The change-password page at `/account/password` (decision 112): the current password, the
/// new one twice, and the Sign out button until slice 6's navigation carries it.
#[derive(Template)]
#[template(path = "password.html")]
pub(crate) struct PasswordPage {
    pub(crate) layout: Layout,
    /// Who is signed in.
    pub(crate) display_name: String,
    pub(crate) username: String,
    /// The confirmation after a change.
    pub(crate) confirmation: Option<String>,
    pub(crate) problems: PasswordProblems,
}

/// A page that only refuses: the 403 of the Origin check and the 413 of the body limit
/// (decision 110), each with a button back to Nage.
#[derive(Template)]
#[template(path = "refusal.html")]
pub(crate) struct RefusalPage {
    pub(crate) layout: Layout,
    pub(crate) heading: &'static str,
    pub(crate) message: String,
}

/// The heading of the Origin check's 403 page.
pub(crate) const CROSS_SITE_HEADING: &str = "This form was not accepted";

/// The heading of the body limit's 413 page.
pub(crate) const TOO_LARGE_HEADING: &str = "This form is too large";

/// `GET /`.
pub(crate) async fn get_index() -> Response {
    render(&IndexPage {
        layout: Layout::embedded(),
    })
}

/// Renders `page` as [`render`] does and answers it with `status`; a failed render keeps its
/// 500.
pub(crate) fn render_with<T: Template>(status: StatusCode, page: &T) -> Response {
    let rendered = render(page);
    if rendered.status() != StatusCode::OK {
        return rendered;
    }
    (status, rendered).into_response()
}

/// Renders `page` to a `String` and answers it as HTML; a failed render is logged and
/// answers 500 (decision 50).
pub(crate) fn render<T: Template>(page: &T) -> Response {
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => {
            tracing::error!(%error, "cannot render a page");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
