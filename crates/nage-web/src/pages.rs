//! HTML pages on the askama base layout (SPEC 11, decision 50): the busy page, the pages of
//! milestone 2 slice 5 (decisions 105, 106, 110 and 112), whose handlers live in `setup`,
//! `login` and `account`, and the pages of slice 6 (decisions 116 to 120): the home page and
//! the page for an unknown address, whose handlers live in `home` and `not_found`. The layout
//! carries the bar with its menu on a page that knows who is signed in, and a one-time message
//! at the top of the page that shows one. Every page keeps the title `Nage` (decision 73).

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use nage_core::currency::Currency;
use nage_core::home::{GroupHeading, Home, Line};

use crate::assets;

#[cfg(test)]
mod tests;

/// What the base layout holds: the content-hashed asset URLs it links (decision 46), the menu
/// of a page that knows who is signed in (decision 116) and the one-time message of a page
/// that shows one (decision 117).
pub(crate) struct Layout {
    pub(crate) htmx_js: &'static str,
    pub(crate) pico_css: &'static str,
    pub(crate) app_css: &'static str,
    /// The bar with Nage's name and the menu. `Some` on the home page, the change-password
    /// page and the page for an unknown address; `None` on the sign-in page, the set-up page
    /// and its closed page, the 403 and 413 pages and the busy page, none of which knows a
    /// signed-in user.
    pub(crate) menu: Option<Menu>,
    /// The one-time message, shown above the page's heading.
    pub(crate) message: Option<Message>,
}

impl Layout {
    /// The URLs of the embedded files, with no menu and no message.
    pub(crate) fn embedded() -> Self {
        let [htmx, pico, app] = assets::embedded();
        Self {
            htmx_js: &htmx.url,
            pico_css: &pico.url,
            app_css: &app.url,
            menu: None,
            message: None,
        }
    }
}

/// The menu (decision 116): Home, Change password, who is signed in, and the Sign out button,
/// which posts to `/logout`. It lists only pages that exist; a later page adds its entry in its
/// own slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Menu {
    /// The display name of the signed-in user.
    pub(crate) display_name: String,
    /// The entry of the page in view, which the menu marks; `None` on a page that has no
    /// entry, such as the page for an unknown address.
    pub(crate) current: Option<Entry>,
}

/// The pages the menu links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// `/`.
    Home,
    /// `/account/password`.
    ChangePassword,
}

/// A one-time message as a page shows it (decision 117).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) text: String,
    /// Whether the message says that something was not done (shown as an alert) rather than
    /// that it was (shown as a status).
    pub(crate) alert: bool,
}

/// The home page at `/` (decision 119): the day its balances are for, the Assets and
/// Liabilities sections with their totals, groups and account lines, and the Net worth line,
/// or the empty state in words. The template only renders the core's model (decision 120).
#[derive(Template)]
#[template(path = "home.html")]
pub(crate) struct HomePage {
    pub(crate) layout: Layout,
    pub(crate) home: Home,
}

/// The page of the 404 for an address no route answers (decision 118): what was not shown, why
/// and what to do, with a button to the home page. It never holds the address.
#[derive(Template)]
#[template(path = "not_found.html")]
pub(crate) struct NotFoundPage {
    pub(crate) layout: Layout,
    pub(crate) message: String,
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

/// The change-password page at `/account/password` (decision 112): the current password and
/// the new one twice, under the menu, whose Sign out button takes the place of the page's own
/// (decision 116). The confirmation after a change is the layout's one-time message
/// (decision 117).
#[derive(Template)]
#[template(path = "password.html")]
pub(crate) struct PasswordPage {
    pub(crate) layout: Layout,
    /// Who is signed in.
    pub(crate) display_name: String,
    pub(crate) username: String,
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
