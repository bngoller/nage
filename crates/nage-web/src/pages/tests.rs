//! Every page of milestone 2 slice 5 in every state that shows a message, rendered from its
//! template with made-up details and written out as a phone shows it, in plain text: the
//! heading, then each label, help line, box and button in order. The renderings are file
//! snapshots beside this file (decision 60), so a change of wording shows as a changed
//! snapshot; the contract's MSG.md is made from them.

use askama::Template;
use nage_core::book::BookError;
use nage_core::currency::CURRENCIES;
use nage_core::fold::NameError;
use nage_core::password::PasswordError;
use nage_core::users::UserError;

use super::{
    CROSS_SITE_HEADING, Layout, LoginPage, PasswordPage, PasswordProblems, RefusalPage,
    SetupClosedPage, SetupPage, SetupProblems, SetupView, TOO_LARGE_HEADING,
};
use crate::messages::{Notice, Refusal};

/// How wide a box is drawn, in characters.
const BOX: usize = 24;

/// `text` with its white space collapsed and the character references askama writes turned
/// back into their characters.
fn plain(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("&#60;", "<")
        .replace("&#62;", ">")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#38;", "&")
}

/// The value of attribute `name` in the attributes of a start tag.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let needle = format!(r#"{name}=""#);
    let start = attributes.find(&needle)? + needle.len();
    let end = attributes[start..].find('"')? + start;
    Some(plain(&attributes[start..end]))
}

/// A box that holds `value`.
fn drawn_box(value: &str) -> String {
    let filler = "_".repeat(BOX.saturating_sub(value.chars().count()).max(2));
    format!("[ {value}{filler} ]")
}

/// The page `html` as plain text: the browser tab's title; `# ` before the heading; a notice
/// between `>>` and `<<`; each label on its own line, a box drawn as `[ ____ ]` with what it
/// holds (a password box never holds anything), the currency list as `[ CAD v ]`, a tick box
/// as `[x]` or `[ ]` before its label; a help line indented; a problem under its box after
/// `!`; a button as `( … )`. Hidden fields are not shown.
fn as_text(html: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut rest = html;
    let mut element: Option<(&str, &str)> = None;
    let mut text = String::new();
    let mut tick: Option<bool> = None;
    let mut options = 0_usize;
    let mut selected = String::new();
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let end = rest[start..]
            .find('>')
            .map_or(rest.len() - 1, |index| index + start);
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        let (name, attributes) = tag.split_once(' ').unwrap_or((tag, ""));
        match name {
            "title" | "h1" | "p" | "small" | "label" | "button" | "option" => {
                element = Some((name, attributes));
                text.clear();
                tick = None;
            }
            "input" => match attribute(attributes, "type").as_deref() {
                Some("hidden") => {}
                Some("checkbox") => tick = Some(attributes.contains(" checked")),
                Some("password") => lines.push(drawn_box("")),
                _ => lines.push(drawn_box(
                    &attribute(attributes, "value").unwrap_or_default(),
                )),
            },
            "select" => {
                options = 0;
                selected.clear();
            }
            "/select" => {
                lines.push(format!(
                    "[ {selected} v ]  (a list of {options} currency codes)"
                ));
            }
            "/title" | "/h1" | "/p" | "/small" | "/label" | "/button" | "/option" => {
                let words = plain(&text);
                if let Some((open, attributes)) = element.take() {
                    match open {
                        "title" => lines.push(format!("Browser tab: {words}")),
                        "h1" => lines.push(format!("\n# {words}\n")),
                        "p" if attributes.contains("notice") => {
                            lines.push(format!(">> {words} <<\n"));
                        }
                        "p" => lines.push(format!("{words}\n")),
                        "small" if attributes.contains("problem") => {
                            lines.push(format!("  ! {words}"));
                        }
                        "small" => lines.push(format!("  {words}")),
                        "label" => match tick {
                            Some(true) => lines.push(format!("\n[x] {words}")),
                            Some(false) => lines.push(format!("\n[ ] {words}")),
                            None => lines.push(format!("\n{words}")),
                        },
                        "button" => lines.push(format!("\n( {words} )\n")),
                        "option" => {
                            options += 1;
                            if attributes.contains("selected") {
                                selected = words;
                            }
                        }
                        _ => {}
                    }
                }
                text.clear();
            }
            _ => {}
        }
    }
    let mut page = lines.join("\n");
    while page.contains("\n\n\n") {
        page = page.replace("\n\n\n", "\n\n");
    }
    page.trim_end().to_owned() + "\n"
}

/// Renders `page` and writes it out as text.
fn rendered<T: Template>(page: &T) -> String {
    let html = page
        .render()
        .unwrap_or_else(|error| panic!("the page does not render: {error}"));
    assert!(
        html.contains("<title>Nage</title>"),
        "the page does not keep the title Nage:\n{html}"
    );
    as_text(&html)
}

/// The set-up form with `form` in its boxes and `problems` under them.
fn setup(form: SetupView, problems: SetupProblems) -> String {
    rendered(&SetupPage {
        layout: Layout::embedded(),
        currencies: CURRENCIES,
        form,
        problems,
    })
}

/// The change-password page of Sam with `confirmation` and `problems`.
fn password(confirmation: Option<String>, problems: PasswordProblems) -> String {
    rendered(&PasswordPage {
        layout: Layout::embedded(),
        display_name: String::from("Sam"),
        username: String::from("sam"),
        confirmation,
        problems,
    })
}

/// The set-up page as text: empty with CAD selected and the tick box ticked; refused with a
/// problem in every box, each under its own; refused with one problem, everything typed kept
/// except the passwords; and the page of the 404 once Nage is set up.
///
/// Proves: decision 105 (what the set-up page shows and says, in each state), decision 73
/// (the title), decision 102.
#[test]
fn the_set_up_page_in_each_state() {
    let empty = SetupView {
        book_name: String::new(),
        currency: String::from("CAD"),
        username: String::new(),
        display_name: String::new(),
        starter_chart: true,
    };
    insta::assert_snapshot!(
        "setup_1_form",
        setup(empty.clone(), SetupProblems::default())
    );

    let everything = SetupProblems {
        notice: Some(Refusal::SetupNotDone.to_string()),
        book_name: Some(BookError::Name(NameError::Empty).to_string()),
        currency: Some(
            Refusal::UnknownCurrency {
                typed: String::from("XXX"),
            }
            .to_string(),
        ),
        username: Some(UserError::Username(NameError::Empty).to_string()),
        display_name: Some(UserError::DisplayName(NameError::Empty).to_string()),
        password: Some(PasswordError::TooShort.to_string()),
        password_again: Some(Refusal::SetupPasswordsDiffer.to_string()),
    };
    insta::assert_snapshot!("setup_2_refused_every_box", setup(empty, everything));

    let typed = SetupView {
        book_name: String::from("Household"),
        currency: String::from("EUR"),
        username: String::from("sam"),
        display_name: String::from("Sam"),
        starter_chart: false,
    };
    let one = SetupProblems {
        notice: Some(Refusal::SetupNotDone.to_string()),
        password: Some(PasswordError::TooShort.to_string()),
        ..SetupProblems::default()
    };
    insta::assert_snapshot!("setup_3_refused_one_box", setup(typed, one));

    insta::assert_snapshot!(
        "setup_4_closed",
        rendered(&SetupClosedPage {
            layout: Layout::embedded(),
            message: Refusal::SetupClosed.to_string(),
        })
    );
}

/// The sign-in page as text: empty; refused with the one message and the typed username
/// kept; refused right after the fifth wrong password in a row, saying how long to wait; and
/// reached after a change of one's own password that a reset overtook, saying so above the
/// form.
///
/// Proves: decision 106 (what the sign-in page shows and says), decision 111 (the wait),
/// decision 112 (the sign-in page says that a change overtaken by a reset was not made),
/// decision 102.
#[test]
fn the_sign_in_page_in_each_state() {
    let page = |username: &str, problem: Option<Refusal>, notice: Option<Refusal>| {
        rendered(&LoginPage {
            layout: Layout::embedded(),
            username: username.to_owned(),
            next: String::new(),
            problem: problem.map(|refusal| refusal.to_string()),
            notice: notice.map(|refusal| refusal.to_string()),
        })
    };
    insta::assert_snapshot!("login_1_form", page("", None, None));
    insta::assert_snapshot!("login_2_refused", page("sam", Some(Refusal::SignIn), None));
    insta::assert_snapshot!(
        "login_3_paused",
        page("sam", Some(Refusal::SignInPaused { seconds: 60 }), None)
    );
    insta::assert_snapshot!(
        "login_4_changed_elsewhere",
        page("", None, Some(Refusal::PasswordChangedElsewhere))
    );
}

/// The change-password page as text: empty; after a change, with the confirmation; refused
/// with a problem under every box; and refused during a pause. In every state the sentence
/// about the other devices stands between the last box and the Change password button. A
/// change overtaken by a reset is never shown on this page: it leads to the sign-in page.
///
/// Proves: decision 112 (what the change-password page shows and says), decision 111,
/// decision 102.
#[test]
fn the_change_password_page_in_each_state() {
    insta::assert_snapshot!(
        "password_1_form",
        password(None, PasswordProblems::default())
    );
    insta::assert_snapshot!(
        "password_2_changed",
        password(
            Some(Notice::PasswordChanged.to_string()),
            PasswordProblems::default()
        )
    );
    insta::assert_snapshot!(
        "password_3_refused_every_box",
        password(
            None,
            PasswordProblems {
                notice: Some(Refusal::PasswordNotChanged.to_string()),
                current: Some(Refusal::CurrentPasswordWrong.to_string()),
                new: Some(PasswordError::TooShort.to_string()),
                again: Some(Refusal::NewPasswordsDiffer.to_string()),
            }
        )
    );
    insta::assert_snapshot!(
        "password_4_paused",
        password(
            None,
            PasswordProblems {
                notice: Some(Refusal::PasswordCheckPaused { seconds: 256 }.to_string()),
                ..PasswordProblems::default()
            }
        )
    );
}

/// The two pages that only refuse, as text: the Origin check's 403 and the body limit's 413,
/// each with its heading, its message and a button back to Nage.
///
/// Proves: decision 110 (the 403 page says what was refused, why and what to do; the 413),
/// decision 102.
#[test]
fn the_pages_that_only_refuse() {
    insta::assert_snapshot!(
        "refusal_1_cross_site",
        rendered(&RefusalPage {
            layout: Layout::embedded(),
            heading: CROSS_SITE_HEADING,
            message: Refusal::CrossSite.to_string(),
        })
    );
    insta::assert_snapshot!(
        "refusal_2_form_too_large",
        rendered(&RefusalPage {
            layout: Layout::embedded(),
            heading: TOO_LARGE_HEADING,
            message: Refusal::FormTooLarge.to_string(),
        })
    );
}
