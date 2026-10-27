//! HTML pages on the askama base layout (SPEC 11, decision 50).

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::assets;

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

/// `GET /`.
pub(crate) async fn get_index() -> Response {
    render(&IndexPage {
        layout: Layout::embedded(),
    })
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
