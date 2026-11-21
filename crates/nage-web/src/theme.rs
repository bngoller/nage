//! The appearance switch (milestone 2 slice 6a): the pages follow the device's light or dark
//! setting unless the person chose one for this browser. The menu of every signed-in page
//! carries a group labelled Appearance with three buttons, Automatic, Light and Dark, in a plain
//! form that posts the field `theme` and the field `back` (the page to return to) to
//! `POST /theme`; no script is involved. The choice is kept in the cookie `nage_theme`, which
//! holds `light` or `dark` and nothing else (Automatic removes it), with `Path=/`, `HttpOnly`,
//! `SameSite=Lax` and `Secure` as the session cookie has them (`cookie::is_secure`) and a
//! lifetime of one year.
//!
//! Every page, the sign-in and set-up pages included, reads the cookie through [`wear`], an
//! outer layer, which makes the palette it names the one [`current`] gives while the request is
//! answered, so the base layout marks the page with `data-theme="light"` or `"dark"` on its
//! `html` element, or with no such attribute for Automatic. The mark comes only from the
//! cookie's two known values and is never written from what was sent: an unknown, empty or
//! missing value means Automatic.

use axum::extract::rejection::FormRejection;
use axum::extract::{Form, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::messages::Refusal;
use crate::pages::{CROSS_SITE_HEADING, Layout, RefusalPage, render_with};
use crate::session::safe_next;
use crate::{Ctx, THEME_COOKIE, cookie, forms};

#[cfg(test)]
mod tests;

/// The appearance of the pages for one browser: the device's own setting, or light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Theme {
    /// The device's setting (`prefers-color-scheme`); no cookie.
    Automatic,
    /// The light palette, whatever the device says.
    Light,
    /// The dark palette, whatever the device says.
    Dark,
}

impl Theme {
    /// The choice the menu's buttons send in the field `theme`: exactly `automatic`, `light`
    /// or `dark`. Any other value, an empty one, another spelling or another case of letters,
    /// is `None`, which the route refuses.
    pub(crate) fn from_form(value: &str) -> Option<Self> {
        match value {
            "automatic" => Some(Self::Automatic),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    /// The palette the cookie's value names: exactly `light` or `dark`. Any other value,
    /// `automatic` and an empty one included, is Automatic.
    pub(crate) fn from_cookie(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Automatic,
        }
    }

    /// The value of `data-theme` on a page's `html` element: `light` or `dark`, and none for
    /// Automatic, so that the style file follows the device's setting.
    pub(crate) fn data_theme(self) -> Option<&'static str> {
        match self {
            Self::Automatic => None,
            Self::Light => Some("light"),
            Self::Dark => Some("dark"),
        }
    }

    /// The `aria-pressed` value of the menu's button for `button`: `true` on the one in force
    /// and `false` on the other two.
    pub(crate) fn pressed(self, button: Self) -> &'static str {
        if self == button { "true" } else { "false" }
    }
}

tokio::task_local! {
    /// The appearance of the request being answered, set by [`wear`].
    static THEME: Theme;
}

/// The appearance of the request being answered, as [`wear`] read it from its cookie;
/// Automatic outside a request, as when a test renders a page by itself.
pub(crate) fn current() -> Theme {
    THEME.try_with(|theme| *theme).unwrap_or(Theme::Automatic)
}

/// The outer layer that reads the request's `nage_theme` cookie and answers the request with
/// the palette it names in force ([`current`]), so that every page this request renders, a
/// refusal or the busy page included, wears it. No cookie is Automatic.
pub(crate) async fn wear(request: Request, next: Next) -> Response {
    let theme =
        sent(request.headers()).map_or(Theme::Automatic, |value| Theme::from_cookie(&value));
    THEME.scope(theme, next.run(request)).await
}

/// The appearance form as posted. A field that is missing is empty.
#[derive(Deserialize)]
pub(crate) struct ThemeForm {
    /// `automatic`, `light` or `dark`.
    #[serde(default)]
    pub(crate) theme: String,
    /// The page to return to, as the menu wrote it (`pages::Menu::back`).
    #[serde(default)]
    pub(crate) back: String,
}

/// `POST /theme`, behind the session rule and the Origin check like every form. A choice the
/// menu offers is kept in the cookie, or the cookie removed for Automatic, and the answer is
/// a 303 to the page named in `back` when it is a path on this site (`session::safe_next`),
/// and to `/` otherwise. Any other choice is refused with 422 and the page that says what to
/// do, setting no cookie; a body that is not a readable form gets `forms::unreadable`'s answer.
pub(crate) async fn post_theme(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    form: Result<Form<ThemeForm>, FormRejection>,
) -> Response {
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => return forms::unreadable(rejection),
    };
    let Some(theme) = Theme::from_form(&form.theme) else {
        return refused();
    };
    let secure = cookie::is_secure(&headers, ctx.state.trust_proxy);
    let back = safe_next(&form.back).unwrap_or("/");
    let mut response = Redirect::to(back).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, set_cookie(theme, secure));
    response
}

/// The `Set-Cookie` value that keeps `theme`: for Light or Dark
/// `nage_theme=<light|dark>; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000`, and for
/// Automatic the same cookie emptied with `Max-Age=0`, which removes it; each followed by
/// `; Secure` when `secure` is true.
pub(crate) fn set_cookie(theme: Theme, secure: bool) -> HeaderValue {
    let value = match (theme, secure) {
        (Theme::Light, false) => {
            "nage_theme=light; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000"
        }
        (Theme::Light, true) => {
            "nage_theme=light; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000; Secure"
        }
        (Theme::Dark, false) => "nage_theme=dark; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000",
        (Theme::Dark, true) => {
            "nage_theme=dark; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000; Secure"
        }
        (Theme::Automatic, false) => "nage_theme=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        (Theme::Automatic, true) => {
            "nage_theme=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure"
        }
    };
    HeaderValue::from_static(value)
}

/// The answer to a choice the menu does not offer: 422 with the refusal page, which says what
/// was refused, why and what to do (decision 102).
fn refused() -> Response {
    render_with(
        StatusCode::UNPROCESSABLE_ENTITY,
        &RefusalPage {
            layout: Layout::embedded(),
            heading: CROSS_SITE_HEADING,
            message: Refusal::UnknownTheme.to_string(),
        },
    )
}

/// The optional white space HTTP allows before a cookie in a `Cookie` header: the space and the
/// tab, and no other white space.
const COOKIE_SPACE: [char; 2] = [' ', '\t'];

/// The value of the request's `nage_theme` cookie, as sent: the first cookie of that name in
/// the `Cookie` headers, whatever other cookies stand beside it, a cookie whose value holds a
/// byte outside ASCII included (each header is read with such bytes replaced, so that it is
/// never dropped whole). `None` when there is none. Only the optional white space HTTP allows
/// before a cookie, spaces and tabs, is dropped: a name after any other white space, a
/// no-break space say, is another cookie's, and a value with a space after it, `light ` say,
/// is not `light`, and so means Automatic.
fn sent(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
        .find_map(|cookies| {
            cookies.split(';').find_map(|pair| {
                pair.trim_start_matches(COOKIE_SPACE)
                    .strip_prefix(THEME_COOKIE)
                    .and_then(|rest| rest.strip_prefix('='))
                    .map(str::to_owned)
            })
        })
}
