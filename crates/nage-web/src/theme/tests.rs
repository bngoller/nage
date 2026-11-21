//! The appearance switch without a server: which form values are a choice, which cookie values
//! name a palette, and the cookie that keeps a choice or removes it.

use super::{Theme, set_cookie};

/// The form's field `theme` names a choice only as exactly `automatic`, `light` or `dark`;
/// another case of letters, a space before or after, an empty value, another word and the
/// cookie's own spellings with anything added are no choice at all.
///
/// Proves: decision 124: automatic, light and dark are the choices, exactly as written, and nothing else is
#[test]
fn the_form_offers_exactly_three_choices() {
    assert_eq!(
        Theme::from_form("automatic"),
        Some(Theme::Automatic),
        "automatic"
    );
    assert_eq!(Theme::from_form("light"), Some(Theme::Light), "light");
    assert_eq!(Theme::from_form("dark"), Some(Theme::Dark), "dark");
    for refused in [
        "",
        " ",
        "Automatic",
        "AUTOMATIC",
        "Light",
        "LIGHT",
        "Dark",
        "DARK",
        " light",
        "light ",
        "dark\n",
        "\tdark",
        "auto",
        "system",
        "lightdark",
        "light,dark",
        "dark;",
        "sepia",
        "none",
        "0",
        "light\u{0}",
    ] {
        assert_eq!(
            Theme::from_form(refused),
            None,
            "the form value {refused:?} is taken as a choice"
        );
    }
}

/// Only the cookie values `light` and `dark`, exactly as written, name a palette; every other
/// value, `automatic`, an empty one, another case of letters, one with spaces or markup
/// characters, is Automatic.
///
/// Proves: decision 124: only light and dark name a palette; every other cookie value is Automatic
#[test]
fn the_cookie_names_light_or_dark_and_nothing_else() {
    assert_eq!(Theme::from_cookie("light"), Theme::Light, "light");
    assert_eq!(Theme::from_cookie("dark"), Theme::Dark, "dark");
    for automatic in [
        "",
        "automatic",
        "Automatic",
        "Light",
        "LIGHT",
        "Dark",
        "DARK",
        "light ",
        " dark",
        "\"light\"",
        "light;",
        "darker",
        "lighter",
        "<b>dark</b>",
        "\"><script>",
        "sepia",
    ] {
        assert_eq!(
            Theme::from_cookie(automatic),
            Theme::Automatic,
            "the cookie value {automatic:?} names a palette"
        );
    }
}

/// The `Set-Cookie` that keeps Light or Dark holds exactly that word with `Path=/`, `HttpOnly`,
/// `SameSite=Lax` and a year's `Max-Age`; the one for Automatic empties the cookie with
/// `Max-Age=0` and the same attributes; each ends with `Secure` exactly when asked to.
///
/// Proves: decision 124: the Set-Cookie values for Light, Dark and Automatic, with and without Secure
#[test]
fn the_cookie_keeps_a_choice_or_removes_it() {
    let cases = [
        (
            Theme::Light,
            false,
            "nage_theme=light; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000",
        ),
        (
            Theme::Light,
            true,
            "nage_theme=light; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000; Secure",
        ),
        (
            Theme::Dark,
            false,
            "nage_theme=dark; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000",
        ),
        (
            Theme::Dark,
            true,
            "nage_theme=dark; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000; Secure",
        ),
        (
            Theme::Automatic,
            false,
            "nage_theme=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        ),
        (
            Theme::Automatic,
            true,
            "nage_theme=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure",
        ),
    ];
    for (theme, secure, expected) in cases {
        let value = set_cookie(theme, secure);
        assert_eq!(
            value.to_str().ok(),
            Some(expected),
            "the Set-Cookie for {theme:?}, secure {secure}"
        );
    }
}
