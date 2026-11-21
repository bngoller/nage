//! Every page of milestone 2 slices 5, 6 and 6a in every state that shows a message, rendered
//! from its template with made-up details and written out as a phone shows it, in plain text:
//! Nage's name, with the Menu button on a page that has one, then the heading and each label,
//! help line, box and button in order, and on the home page each balance line with its amount
//! at the right. The menu is also written out open and as a computer shows it, in the rail.
//! The renderings are file snapshots beside this file (decisions 60 and 115), so a change of
//! wording shows as a changed snapshot; the contract's MSG.md is made from them. The structure
//! the look styles (the boxes' field wrappers, `aria-invalid` and the menu entries' classes)
//! is read from the HTML itself.

use askama::Template;
use nage_core::accounts::Kind;
use nage_core::book::BookError;
use nage_core::currency::CURRENCIES;
use nage_core::date::Date;
use nage_core::fold::NameError;
use nage_core::home::{
    AccountLine, Group, GroupHeading, Home, HomeBalances, Line, OtherLine, Section,
};
use nage_core::money::{MinorDigits, Money};
use nage_core::password::PasswordError;
use nage_core::users::UserError;

use super::{
    CROSS_SITE_HEADING, Entry, HomePage, Layout, LoginPage, Menu, Message, NotFoundPage,
    PasswordPage, PasswordProblems, RefusalPage, SetupClosedPage, SetupPage, SetupProblems,
    SetupView, TOO_LARGE_HEADING,
};
use crate::messages::{Notice, Refusal};
use crate::theme::Theme;

/// How wide a box is drawn, in characters.
const BOX: usize = 24;

/// How wide a phone's page is drawn, in characters: the bar and every balance line end here.
const WIDTH: usize = 46;

/// How a page is written out: as a phone shows it with the menu closed, as a phone shows it
/// after a tap on the Menu button, or as a computer's wider screen shows it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Phone,
    PhoneMenuOpen,
    Computer,
}

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

/// `left` at the left and `right` at the right of a line [`WIDTH`] wide, set in by `indent`.
/// A `left` too long for the line wraps onto further lines, as a long account name wraps on a
/// phone instead of widening the page; `right` stays on the first line.
fn row(indent: usize, left: &str, right: &str) -> String {
    let room = WIDTH
        .saturating_sub(indent + right.chars().count() + 2)
        .max(8);
    let mut pieces: Vec<String> = Vec::new();
    for word in left.split(' ') {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > room {
            pieces.push(word.drain(..room).collect());
        }
        let word: String = word.into_iter().collect();
        match pieces.last_mut() {
            Some(last) if last.chars().count() + 1 + word.chars().count() <= room => {
                last.push(' ');
                last.push_str(&word);
            }
            _ => pieces.push(word),
        }
    }
    let pad = " ".repeat(indent);
    let mut lines = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if index == 0 {
            let gap = " ".repeat(room.saturating_sub(piece.chars().count()) + 2);
            lines.push(format!("{pad}{piece}{gap}{right}"));
        } else {
            lines.push(format!("{pad}{piece}"));
        }
    }
    lines.join("\n")
}

/// A name with its amount, as a section's heading, an account's line and the Net worth line
/// carry them.
#[derive(Default)]
struct Figure {
    name: String,
    owner: Option<String>,
    amount: Option<String>,
}

/// The top of a page as the page carries it: Nage's name, and on a page with a menu the Menu
/// button and the menu's entries, once for a phone (inside the Menu button's `details`) and
/// once for a computer (the `nav` of the rail).
#[derive(Default)]
struct MenuText {
    brand: String,
    button: String,
    narrow: Vec<String>,
    wide: Vec<String>,
    /// Whether the top is the masthead of a page without a menu.
    lone: bool,
    /// The Appearance group's legend, while the renderer is inside the group.
    look: Option<String>,
    /// The Appearance group's buttons so far, each marked when it is the choice in force.
    choices: Vec<String>,
}

/// Where in the top of the page the renderer is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Page,
    Bar,
    Narrow,
    Wide,
}

/// The page `html` as plain text, as `view` shows it: the browser tab's title; Nage's name on
/// a line of its own on a page without a menu; on a page with a menu, Nage's name at the left
/// and the Menu button at the right on a phone (after a tap, the menu's entries follow, one
/// per line), or on a computer Nage's name with the entries of the rail under it, one per
/// line; the Appearance group as one entry, its legend with a colon and then its buttons,
/// `(chosen)` after the one in force; `# ` before the heading; a notice between `>>` and
/// `<<`; each label on its own line, a box drawn as `[ ____ ]` with what it holds (a password
/// box never holds anything), the currency list as `[ CAD v ]`, a tick box as `[x]` or `[ ]`
/// before its label; a help line indented; a problem under its box after `!`; a button as
/// `( … )`; a link as `-> …`, with `(this page)` after the menu's entry for the page in view;
/// a section's heading after `## ` with its total at the right; a group's heading with a
/// colon; an account's line, and an Other line, with its amount at the right, set in by its
/// level, the owner's name after the account's in the parentheses the page writes; the Net
/// worth line under a rule. Hidden fields and the mark, a drawing, are not shown.
fn as_text(html: &str, view: View) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut rest = html;
    let mut element: Option<(&str, &str)> = None;
    let mut text = String::new();
    let mut tick: Option<bool> = None;
    let mut options = 0_usize;
    let mut selected = String::new();
    let mut zone = Zone::Page;
    let mut menu = MenuText::default();
    let mut entry: Option<String> = None;
    let mut item: Option<&str> = None;
    let mut figure = Figure::default();
    let mut span = "";
    let mut current = false;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let end = rest[start..]
            .find('>')
            .map_or(rest.len() - 1, |index| index + start);
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        let (name, attributes) = tag.split_once(' ').unwrap_or((tag, ""));
        let in_menu = matches!(zone, Zone::Narrow | Zone::Wide);
        match name {
            "title" | "h1" | "h3" | "p" | "label" | "button" | "option" | "summary" | "legend" => {
                element = Some((name, attributes));
                text.clear();
                tick = None;
            }
            "header" => {
                zone = Zone::Bar;
                menu.lone = attributes.contains("masthead");
            }
            "span" if zone == Zone::Bar => {
                span = attributes;
                text.clear();
            }
            "/span" if zone == Zone::Bar => {
                if span.contains("brand-name") {
                    menu.brand = plain(&text);
                }
                span = "";
                text.clear();
            }
            "small" if attributes.contains("owner") => {
                figure.name = plain(&text);
                text.clear();
            }
            "small" => {
                element = Some((name, attributes));
                text.clear();
            }
            "h2" | "li" => {
                item = Some(attributes);
                figure = Figure::default();
                entry = None;
                text.clear();
            }
            "span" => {
                span = attributes;
                text.clear();
            }
            "a" if zone == Zone::Page => text.push_str("-> "),
            "a" if zone == Zone::Bar => text.clear(),
            "a" => {
                current = attributes.contains(r#"aria-current="page""#);
                text.clear();
            }
            "nav" if attributes.contains("menu-wide") => zone = Zone::Wide,
            "details" => zone = Zone::Narrow,
            "/nav" | "/details" => zone = Zone::Bar,
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
            "/small" if element.is_none() => {
                figure.owner = Some(plain(&text));
                text.clear();
            }
            "/span" => {
                let words = plain(&text);
                if span.contains("amount") {
                    figure.amount = Some(words);
                } else if !words.is_empty() || figure.name.is_empty() {
                    figure.name = words;
                }
                text.clear();
            }
            "/a" if in_menu => {
                let mark = if current { " (this page)" } else { "" };
                entry = Some(format!("-> {}{mark}", plain(&text)));
                text.clear();
            }
            "/li" => {
                let attributes = item.take().unwrap_or_default();
                if let Some(legend) = menu.look.take() {
                    entry = Some(format!("{legend} {}", menu.choices.join("  ")));
                    menu.choices.clear();
                }
                match zone {
                    Zone::Narrow => menu
                        .narrow
                        .push(entry.take().unwrap_or_else(|| plain(&text))),
                    Zone::Wide => menu.wide.push(entry.take().unwrap_or_else(|| plain(&text))),
                    Zone::Bar => {}
                    Zone::Page => {
                        let level: usize = attributes
                            .split("level-")
                            .nth(1)
                            .and_then(|rest| rest.trim_end_matches('"').parse().ok())
                            .unwrap_or(1);
                        let name = match &figure.owner {
                            Some(owner) => format!("{} {owner}", figure.name),
                            None => figure.name.clone(),
                        };
                        let amount = figure.amount.clone().unwrap_or_default();
                        lines.push(row(level * 2, &name, &amount));
                    }
                }
                text.clear();
            }
            "/h2" => {
                item = None;
                let amount = figure.amount.clone().unwrap_or_default();
                lines.push(format!(
                    "\n{}",
                    row(0, &format!("## {}", figure.name), &amount)
                ));
                text.clear();
            }
            "/header" => {
                zone = Zone::Page;
                assert_eq!(
                    menu.narrow, menu.wide,
                    "the menu's entries for a phone and for a computer differ"
                );
                if menu.lone {
                    lines.push(menu.brand.clone());
                } else {
                    match view {
                        View::Phone => lines.push(row(0, &menu.brand, &menu.button)),
                        View::PhoneMenuOpen => {
                            lines.push(row(0, &menu.brand, &menu.button));
                            lines.extend(menu.narrow.iter().map(|entry| format!("  {entry}")));
                        }
                        View::Computer => {
                            lines.push(menu.brand.clone());
                            lines.extend(menu.wide.iter().map(|entry| format!("  {entry}")));
                        }
                    }
                }
                lines.push(String::new());
            }
            "/title" | "/h1" | "/h3" | "/p" | "/small" | "/label" | "/button" | "/option"
            | "/summary" | "/legend" => {
                let words = plain(&text);
                if let Some((open, attributes)) = element.take() {
                    match open {
                        "title" => lines.push(format!("Browser tab: {words}")),
                        "h1" => lines.push(format!("\n# {words}\n")),
                        "h3" => lines.push(format!("\n{words}:")),
                        "summary" => menu.button = format!("( {words} )"),
                        "legend" => menu.look = Some(format!("{words}:")),
                        "button" if menu.look.is_some() => {
                            let chosen = if attributes.contains(r#"aria-pressed="true""#) {
                                " (chosen)"
                            } else {
                                ""
                            };
                            menu.choices.push(format!("( {words} ){chosen}"));
                        }
                        "p" if attributes.contains("notice") => {
                            lines.push(format!(">> {words} <<\n"));
                        }
                        "p" if attributes.contains("net-worth") => {
                            let amount = figure.amount.clone().unwrap_or_default();
                            lines.push(format!("\n{}", "-".repeat(WIDTH)));
                            lines.push(row(0, &figure.name, &amount));
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
                        "button" if in_menu => entry = Some(format!("( {words} )")),
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

/// Renders `page` and writes it out as `view` shows it.
fn rendered_as<T: Template>(page: &T, view: View) -> String {
    let html = page
        .render()
        .unwrap_or_else(|error| panic!("the page does not render: {error}"));
    assert!(
        html.contains("<title>Nage</title>"),
        "the page does not keep the title Nage:\n{html}"
    );
    as_text(&html, view)
}

/// Renders `page` and writes it out as a phone shows it, the menu closed.
fn rendered<T: Template>(page: &T) -> String {
    rendered_as(page, View::Phone)
}

/// The top of a page written out as text, down to its heading: the browser tab's title and
/// the bar with its menu.
fn bar(text: String) -> String {
    let end = text.find("\n\n# ").unwrap_or(text.len());
    format!("{}\n", &text[..end])
}

/// Renders `page` to its HTML.
fn html_of<T: Template>(page: &T) -> String {
    page.render()
        .unwrap_or_else(|error| panic!("the page does not render: {error}"))
}

/// The layout of a page that Sam is signed in on, with `current` marked in the menu and
/// `message` at the top.
fn sams(current: Option<Entry>, message: Option<Message>) -> Layout {
    Layout {
        menu: Some(Menu {
            display_name: String::from("Sam"),
            current,
            back: String::from(match current {
                Some(Entry::ChangePassword) => "/account/password",
                Some(Entry::Home) | None => "/",
            }),
        }),
        message,
        ..Layout::embedded()
    }
}

/// The layout of a page without a signed-in user, with `message` at the top.
fn nobodys(message: Option<Message>) -> Layout {
    Layout {
        message,
        ..Layout::embedded()
    }
}

/// `text` as a one-time message that confirms something.
fn status(text: impl ToString) -> Option<Message> {
    Some(Message {
        text: text.to_string(),
        alert: false,
    })
}

/// `text` as a one-time message that says something was not done.
fn alert(text: impl ToString) -> Option<Message> {
    Some(Message {
        text: text.to_string(),
        alert: true,
    })
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

/// The change-password page of Sam with the one-time message `message` and `problems`.
fn password(message: Option<Message>, problems: PasswordProblems) -> String {
    rendered(&password_page(message, problems))
}

/// The change-password page of Sam, as [`password`] writes it out.
fn password_page(message: Option<Message>, problems: PasswordProblems) -> PasswordPage {
    PasswordPage {
        layout: sams(Some(Entry::ChangePassword), message),
        display_name: String::from("Sam"),
        username: String::from("sam"),
        problems,
    }
}

/// The set-up page as text, under Nage's name: empty with CAD selected and the tick box
/// ticked; refused with a problem in every box, each under its own; refused with one problem,
/// everything typed kept except the passwords; and the page of the 404 once Nage is set up.
///
/// Proves: decision 105 (what the set-up page shows and says, in each state), decision 73
/// (the title), decision 102, decision 116 and decision 125 (with the layout of a page
/// without a signed-in user, which holds no menu, the set-up page and its closed page show
/// Nage's name alone, with no Menu button).
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
/// kept; refused right after the fifth wrong password in a row, saying how long to wait;
/// reached after a change of one's own password that a reset overtook, saying so once at the
/// top of the page; and reached after signing out, saying that the person is signed out. The
/// page carries Nage's name and no menu in any state.
///
/// Proves: decision 106 (what the sign-in page shows and says), decision 111 (the wait),
/// decision 112 (the sign-in page says that a change overtaken by a reset was not made),
/// decision 117 (the one-time messages of the sign-in page, at the top of the page, under
/// Nage's name), decision 116 and decision 125 (with the layout of a page without a signed-in
/// user, which holds no menu, the sign-in page shows Nage's name alone, with no Menu button),
/// decision 102.
#[test]
fn the_sign_in_page_in_each_state() {
    let page = |username: &str, problem: Option<Refusal>, message: Option<Message>| {
        rendered(&LoginPage {
            layout: nobodys(message),
            username: username.to_owned(),
            next: String::new(),
            problem: problem.map(|refusal| refusal.to_string()),
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
        page("", None, alert(Refusal::PasswordChangedElsewhere))
    );
    insta::assert_snapshot!(
        "login_5_signed_out",
        page("", None, status(Notice::SignedOut))
    );
}

/// The change-password page as text, under the bar with its Menu button: empty; after a
/// change, with the confirmation once at the top of the page; refused with a problem under
/// every box; and refused during a pause. In every state the sentence about the other devices
/// stands between the last box and the Change password button, and the page has no Sign out
/// button of its own: the menu carries it. A change overtaken by a reset is never shown on
/// this page.
///
/// Proves: decision 112 (what the change-password page shows and says), decision 111,
/// decision 116 (the page carries the bar with its Menu button and no Sign out button of
/// its own), decision 117 (the confirmation is a one-time message at the top of the page),
/// decision 102.
#[test]
fn the_change_password_page_in_each_state() {
    insta::assert_snapshot!(
        "password_1_form",
        password(None, PasswordProblems::default())
    );
    insta::assert_snapshot!(
        "password_2_changed",
        password(status(Notice::PasswordChanged), PasswordProblems::default())
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
/// each under Nage's name with its heading, its message and a button back to Nage, and no
/// menu.
///
/// Proves: decision 110 (the 403 page says what was refused, why and what to do; the 413),
/// decision 116 and decision 125 (with the layout of a page without a signed-in user, which
/// holds no menu, the refusal pages show Nage's name alone, with no Menu button), decision
/// 102.
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

/// An account's line at `level` with `minor` minor units.
fn line(name: &str, level: u8, minor: i64, owner: Option<&str>) -> Line {
    Line::Account(AccountLine {
        name: name.to_owned(),
        level,
        amount: Money::from_minor(minor),
        owner: owner.map(str::to_owned),
    })
}

/// The Other line at `level` with `minor` minor units: the own amount of the account above it.
fn other(level: u8, minor: i64) -> Line {
    Line::Other(OtherLine {
        level,
        amount: Money::from_minor(minor),
    })
}

/// The home page of Sam over `balances`, as of 2026-11-16 in a book with two minor digits,
/// with the one-time message `message`.
fn home_page(balances: Option<HomeBalances>, message: Option<Message>) -> HomePage {
    HomePage {
        layout: sams(Some(Entry::Home), message),
        home: Home {
            as_of: Date::new(2026, 11, 16)
                .unwrap_or_else(|error| panic!("2026-11-16 is no date: {error}")),
            minor_digits: MinorDigits::TWO,
            balances,
        },
    }
}

/// A book right after a set-up with the starter accounts: every account at zero, nobody
/// owning any, so no group has a heading.
fn starter_accounts() -> HomeBalances {
    HomeBalances {
        assets: Section {
            kind: Kind::Asset,
            name: String::from("Assets"),
            total: Money::ZERO,
            groups: vec![Group {
                heading: None,
                lines: vec![
                    line("Cash", 1, 0, None),
                    line("Checking", 1, 0, None),
                    line("Savings", 1, 0, None),
                ],
            }],
        },
        liabilities: Section {
            kind: Kind::Liability,
            name: String::from("Liabilities"),
            total: Money::ZERO,
            groups: vec![Group {
                heading: None,
                lines: vec![line("Credit Card", 1, 0, None)],
            }],
        },
        net_worth: Money::ZERO,
    }
}

/// A household's book in use: joint accounts and the accounts of Alex and of Sam, a joint
/// Savings account that holds a sub-account of Alex's, an account with nothing in it whose
/// long name wraps, a card that is owed on and a card that was overpaid.
fn household() -> HomeBalances {
    HomeBalances {
        assets: Section {
            kind: Kind::Asset,
            name: String::from("Assets"),
            total: Money::from_minor(1_591_233),
            groups: vec![
                Group {
                    heading: Some(GroupHeading::Joint),
                    lines: vec![
                        line("Checking", 1, 248_015, None),
                        line(
                            "Renovation fund for the kitchen and the upstairs bathroom",
                            1,
                            0,
                            None,
                        ),
                        line("Savings", 1, 1_200_000, None),
                        line("Emergency fund", 2, 900_000, None),
                        line("Holiday", 2, 300_000, Some("Alex")),
                    ],
                },
                Group {
                    heading: Some(GroupHeading::Owner(String::from("Alex"))),
                    lines: vec![line("Cash", 1, 8_218, None)],
                },
                Group {
                    heading: Some(GroupHeading::Owner(String::from("Sam"))),
                    lines: vec![line("Brokerage", 1, 135_000, None)],
                },
            ],
        },
        liabilities: Section {
            kind: Kind::Liability,
            name: String::from("Liabilities"),
            total: Money::from_minor(115_480),
            groups: vec![
                Group {
                    heading: Some(GroupHeading::Joint),
                    lines: vec![line("Credit Card", 1, 120_480, None)],
                },
                Group {
                    heading: Some(GroupHeading::Owner(String::from("Sam"))),
                    lines: vec![line("Store card", 1, -5_000, None)],
                },
            ],
        },
        net_worth: Money::from_minor(1_475_753),
    }
}

/// A book whose Savings account holds 1,005.00 of its own beside its sub-account Holiday's
/// 200.00: Savings shows 1,205.00, and under it Holiday and then the Other line with
/// Savings' own 1,005.00.
fn parent_with_entries_of_its_own() -> HomeBalances {
    HomeBalances {
        assets: Section {
            kind: Kind::Asset,
            name: String::from("Assets"),
            total: Money::from_minor(120_500),
            groups: vec![Group {
                heading: None,
                lines: vec![
                    line("Savings", 1, 120_500, None),
                    line("Holiday", 2, 20_000, None),
                    other(2, 100_500),
                ],
            }],
        },
        liabilities: Section {
            kind: Kind::Liability,
            name: String::from("Liabilities"),
            total: Money::ZERO,
            groups: vec![Group {
                heading: None,
                lines: vec![line("Credit Card", 1, 0, None)],
            }],
        },
        net_worth: Money::from_minor(120_500),
    }
}

/// A book whose only listed account is a card that is owed on: Assets has its heading and
/// its total and no line.
fn one_section_empty() -> HomeBalances {
    HomeBalances {
        assets: Section {
            kind: Kind::Asset,
            name: String::from("Assets"),
            total: Money::ZERO,
            groups: Vec::new(),
        },
        liabilities: Section {
            kind: Kind::Liability,
            name: String::from("Liabilities"),
            total: Money::from_minor(120_480),
            groups: vec![Group {
                heading: None,
                lines: vec![line("Credit Card", 1, 120_480, None)],
            }],
        },
        net_worth: Money::from_minor(-120_480),
    }
}

/// The home page as text, under the bar with its Menu button: right after a set-up with the
/// starter accounts, every account at zero and no group heading; a household's book in use,
/// with the joint accounts first and then each owner's, an account of another owner inside a
/// joint parent named with its owner, an account with nothing in it at 0.00 whose long name
/// the renderer draws wrapped, an overpaid card with a minus and the Net worth line; the
/// empty state, in words and with no figure; the home page reached after a change of one's
/// own password that was sent twice, saying so once at the top; a parent with entries of
/// its own, whose Other line stands last under it; and a book whose only listed account is
/// a card, whose Assets section has its heading and its total and no line. Every amount
/// stands at the right of its line.
///
/// Proves: decision 119 (what the home page shows and says in each state: the day named, the
/// two sections with their totals, the groups, the owner named on a line, zero lines, plain
/// card amounts, the Other line of a parent with entries of its own, a section with nothing
/// to list, the Net worth line, the empty state in words), decision 117 (the overtaken
/// change's message on the home page, in the words for a person still signed in), decision
/// 116 (the home page carries the menu), SPEC 6.10 (amounts as it displays them in a book
/// with two minor digits, and no symbol), decision 102.
#[test]
fn the_home_page_in_each_state() {
    insta::assert_snapshot!(
        "home_1_starter_accounts_at_zero",
        rendered(&home_page(Some(starter_accounts()), None))
    );
    insta::assert_snapshot!(
        "home_2_household",
        rendered(&home_page(Some(household()), None))
    );
    insta::assert_snapshot!("home_3_empty", rendered(&home_page(None, None)));
    insta::assert_snapshot!(
        "home_4_password_changed_elsewhere",
        rendered(&home_page(
            Some(starter_accounts()),
            alert(Refusal::PasswordChangedElsewhereSignedIn)
        ))
    );
    insta::assert_snapshot!(
        "home_5_parent_with_entries_of_its_own",
        rendered(&home_page(Some(parent_with_entries_of_its_own()), None))
    );
    insta::assert_snapshot!(
        "home_6_one_section_empty",
        rendered(&home_page(Some(one_section_empty()), None))
    );
}

/// The page for an unknown address as text, under the bar with its Menu button: its heading,
/// the message in three parts and the button to the home page. Nothing on the page comes from
/// a request.
///
/// Proves: decision 118 (what the page for an unknown address shows and says; it holds no
/// address), decision 116 (the page carries the menu), decision 102.
#[test]
fn the_page_for_an_unknown_address() {
    insta::assert_snapshot!(
        "not_found_1",
        rendered(&NotFoundPage {
            layout: sams(None, None),
            message: Refusal::NotFound.to_string(),
        })
    );
}

/// The bar and its menu as text, on the home page: on a phone, closed (Nage's name and the
/// Menu button); on a phone after a tap on the button (the entries below the bar, one per
/// line: Home, marked as the page in view, Change password, the Appearance group with
/// Automatic chosen, who is signed in and the Sign out button); on a computer (Nage's name
/// and under it the same entries in the rail, one per line, no Menu button). Then the open
/// menu on the change-password page, which marks Change password, and on the page for an
/// unknown address, which marks no entry. The page carries the entries once for a phone and
/// once for a computer, and the two lists are the same.
///
/// Proves: decision 116 and decision 123 (the bar with Nage's name and the Menu button; the
/// menu lists Home, Change password, the Appearance group, who is signed in and Sign out; the
/// page carries the same entries a second time for a computer's screen, where the style file,
/// which no cargo test of this file reads, sets them in the rail; the page in view is
/// marked).
#[test]
fn the_menu_closed_open_and_on_a_computer() {
    let home = home_page(Some(starter_accounts()), None);
    insta::assert_snapshot!(
        "menu_1_on_a_phone_closed",
        bar(rendered_as(&home, View::Phone))
    );
    insta::assert_snapshot!(
        "menu_2_on_a_phone_open",
        bar(rendered_as(&home, View::PhoneMenuOpen))
    );
    insta::assert_snapshot!(
        "menu_3_on_a_computer",
        bar(rendered_as(&home, View::Computer))
    );
    insta::assert_snapshot!(
        "menu_4_open_on_the_change_password_page",
        bar(rendered_as(
            &password_page(None, PasswordProblems::default()),
            View::PhoneMenuOpen
        ))
    );
    insta::assert_snapshot!(
        "menu_5_open_on_the_page_for_an_unknown_address",
        bar(rendered_as(
            &NotFoundPage {
                layout: sams(None, None),
                message: Refusal::NotFound.to_string(),
            },
            View::PhoneMenuOpen
        ))
    );
}

/// The Appearance group of the menu as text, with the choice in force marked: on the home
/// page on a phone after a tap on the Menu button, with Light chosen; and on the
/// change-password page on a computer, in the rail, with Dark chosen. With no choice made,
/// Automatic is marked, as the other renderings of the menu show.
///
/// Proves: decision 124 and decision 116: the Appearance group in the open menu and in the
/// rail, with the choice in force marked.
#[test]
fn the_appearance_switch_in_the_menu() {
    let mut home = home_page(Some(starter_accounts()), None);
    home.layout.theme = Theme::Light;
    insta::assert_snapshot!(
        "menu_6_open_with_light_chosen",
        bar(rendered_as(&home, View::PhoneMenuOpen))
    );
    let mut password = password_page(None, PasswordProblems::default());
    password.layout.theme = Theme::Dark;
    insta::assert_snapshot!(
        "menu_7_on_a_computer_with_dark_chosen",
        bar(rendered_as(&password, View::Computer))
    );
}

/// The page that refuses an appearance the menu does not offer, as text: under Nage's name,
/// which is no link there, and with no menu, the 403's heading, the sentence of the refusal
/// and the button back to Nage.
///
/// Proves: decision 124 and decision 102: the text of the refusal of an appearance the menu
/// does not offer, its heading, its sentence and its button, under Nage's name.
#[test]
fn the_page_that_refuses_an_unknown_appearance() {
    insta::assert_snapshot!(
        "refusal_3_unknown_appearance",
        rendered(&RefusalPage {
            layout: Layout::embedded(),
            heading: CROSS_SITE_HEADING,
            message: Refusal::UnknownTheme.to_string(),
        })
    );
}

/// The boxes of the set-up form, by name.
const SETUP_BOXES: [&str; 7] = [
    "book_name",
    "currency",
    "username",
    "display_name",
    "password",
    "password_again",
    "starter_chart",
];

/// The boxes of the change-password form, by name.
const PASSWORD_BOXES: [&str; 3] = ["current_password", "new_password", "new_password_again"];

/// The start tag of the box called `name` in `html` (an `input` or a `select`), which must
/// be there exactly once, with the place where it begins.
fn control<'a>(html: &'a str, name: &str) -> (usize, &'a str) {
    let needle = format!(r#"name="{name}""#);
    let found: Vec<(usize, &str)> = ["<input", "<select"]
        .into_iter()
        .flat_map(|open| html.match_indices(open))
        .map(|(at, _)| {
            let end = html[at..]
                .find('>')
                .map_or(html.len(), |index| at + index + 1);
            (at, &html[at..end])
        })
        .filter(|(_, tag)| tag.contains(&needle))
        .collect();
    assert_eq!(found.len(), 1, "boxes called {name} in:\n{html}");
    found[0]
}

/// The boxes among `boxes` whose start tag in `html` carries `aria-invalid`, in order; each
/// must carry it as `aria-invalid="true"`.
fn marked_invalid<'a>(html: &str, boxes: &[&'a str]) -> Vec<&'a str> {
    boxes
        .iter()
        .copied()
        .filter(|name| {
            let (_, tag) = control(html, name);
            let marked = tag.contains("aria-invalid");
            assert!(
                !marked || tag.contains(r#" aria-invalid="true""#),
                "aria-invalid is not on exactly the boxes with a problem: the box {name} carries another value: {tag}"
            );
            marked
        })
        .collect()
}

/// The set-up form's problems with a problem under the box `name` alone, as a refusal shows
/// it with the form's notice.
fn setup_problem_in(name: &str) -> SetupProblems {
    let problem = Some(String::from("This box has a problem."));
    let mut problems = SetupProblems {
        notice: Some(Refusal::SetupNotDone.to_string()),
        ..SetupProblems::default()
    };
    match name {
        "book_name" => problems.book_name = problem,
        "currency" => problems.currency = problem,
        "username" => problems.username = problem,
        "display_name" => problems.display_name = problem,
        "password" => problems.password = problem,
        "password_again" => problems.password_again = problem,
        _ => panic!("the set-up form has no box {name} that can have a problem"),
    }
    problems
}

/// The change-password form's problems with a problem under the box `name` alone, as a
/// refusal shows it with the form's notice.
fn password_problem_in(name: &str) -> PasswordProblems {
    let problem = Some(String::from("This box has a problem."));
    let mut problems = PasswordProblems {
        notice: Some(Refusal::PasswordNotChanged.to_string()),
        ..PasswordProblems::default()
    };
    match name {
        "current_password" => problems.current = problem,
        "new_password" => problems.new = problem,
        "new_password_again" => problems.again = problem,
        _ => panic!("the change-password form has no box {name}"),
    }
    problems
}

/// A refused form marks each box whose problem shows with `aria-invalid="true"` and no other
/// box: the set-up form with a problem under every box, under one box (the password, and
/// then each box in turn, so that no box answers to another box's problem) and under none,
/// and the change-password form the same way (its one box being the current password, and
/// then each box in turn). The tick box never has a problem and is never marked.
///
/// Proves: decision 123 and decision 105: on the set-up and change-password forms aria-invalid
/// true stands on each box whose problem shows and on no other box, with a problem in every
/// box, in one box and in none.
#[test]
fn a_refused_form_marks_exactly_the_boxes_with_a_problem() {
    let typed = SetupView {
        book_name: String::from("Household"),
        currency: String::from("EUR"),
        username: String::from("sam"),
        display_name: String::from("Sam"),
        starter_chart: true,
    };
    let every = SetupProblems {
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
    let one = SetupProblems {
        notice: Some(Refusal::SetupNotDone.to_string()),
        password: Some(PasswordError::TooShort.to_string()),
        ..SetupProblems::default()
    };
    let mut cases = vec![
        (
            String::from("a problem under every box"),
            every,
            vec![
                "book_name",
                "currency",
                "username",
                "display_name",
                "password",
                "password_again",
            ],
        ),
        (
            String::from("a problem under one box"),
            one,
            vec!["password"],
        ),
        (
            String::from("no problem"),
            SetupProblems::default(),
            Vec::new(),
        ),
    ];
    for name in SETUP_BOXES
        .into_iter()
        .filter(|name| *name != "starter_chart")
    {
        cases.push((
            format!("a problem under {name} alone"),
            setup_problem_in(name),
            vec![name],
        ));
    }
    for (state, problems, expected) in cases {
        let html = html_of(&SetupPage {
            layout: Layout::embedded(),
            currencies: CURRENCIES,
            form: typed.clone(),
            problems,
        });
        assert_eq!(
            marked_invalid(&html, &SETUP_BOXES),
            expected,
            "aria-invalid is not on exactly the boxes with a problem: the set-up form with {state}"
        );
    }

    let every = PasswordProblems {
        notice: Some(Refusal::PasswordNotChanged.to_string()),
        current: Some(Refusal::CurrentPasswordWrong.to_string()),
        new: Some(PasswordError::TooShort.to_string()),
        again: Some(Refusal::NewPasswordsDiffer.to_string()),
    };
    let one = PasswordProblems {
        notice: Some(Refusal::PasswordNotChanged.to_string()),
        current: Some(Refusal::CurrentPasswordWrong.to_string()),
        ..PasswordProblems::default()
    };
    let mut cases = vec![
        (
            String::from("a problem under every box"),
            every,
            vec!["current_password", "new_password", "new_password_again"],
        ),
        (
            String::from("a problem under one box"),
            one,
            vec!["current_password"],
        ),
        (
            String::from("no problem"),
            PasswordProblems::default(),
            Vec::new(),
        ),
    ];
    for name in PASSWORD_BOXES {
        cases.push((
            format!("a problem under {name} alone"),
            password_problem_in(name),
            vec![name],
        ));
    }
    for (state, problems, expected) in cases {
        let html = html_of(&password_page(None, problems));
        assert_eq!(
            marked_invalid(&html, &PASSWORD_BOXES),
            expected,
            "aria-invalid is not on exactly the boxes with a problem: the change-password form with {state}"
        );
    }
}

/// Asserts that each box of `boxes`, given as its name and whether a help line and a problem
/// line belong under it, stands in `html` in its own field wrapper `<div class="field">`,
/// which holds the one label for that box, then the box, then its help line if it has one,
/// then its problem line if it has one, and no other box; a tick box stands inside its label.
fn assert_fields(html: &str, page: &str, boxes: &[(&str, bool, bool)]) {
    const WRAPPER: &str = r#"<div class="field">"#;
    const HELP: &str = r#"<small class="help">"#;
    const PROBLEM: &str = r#"<small class="problem">"#;
    for &(name, help, problem) in boxes {
        let (at, tag) = control(html, name);
        let start = html[..at].rfind(WRAPPER);
        let close = html[at..].find("</div>").map(|index| at + index);
        let (Some(start), Some(close)) = (start, close) else {
            panic!("a box does not stand in its field wrapper: {name} on {page}:\n{html}");
        };
        let field = &html[start..close];
        assert!(
            !html[start..at].contains("</div>") && !html[at..close].contains(WRAPPER),
            "a box does not stand in its field wrapper: {name} on {page} stands outside it:\n{field}"
        );
        let boxes_in_field = field.matches("<input").count() + field.matches("<select").count();
        assert_eq!(
            boxes_in_field, 1,
            "a box does not stand in its field wrapper: the wrapper of {name} on {page} holds another box:\n{field}"
        );
        let labels: Vec<usize> = field.match_indices("<label").map(|(at, _)| at).collect();
        let id = attribute(tag, "id");
        let label_for = labels.first().and_then(|&label| {
            let end = field[label..]
                .find('>')
                .map_or(field.len(), |index| label + index);
            attribute(&field[label..end], "for")
        });
        assert!(
            labels.len() == 1 && id.is_some() && label_for == id,
            "a box does not stand in its field wrapper: the wrapper of {name} on {page} does not hold its one label:\n{field}"
        );
        assert_eq!(
            (field.matches(HELP).count(), field.matches(PROBLEM).count()),
            (usize::from(help), usize::from(problem)),
            "a box does not stand in its field wrapper: the help and problem lines of {name} on {page}:\n{field}"
        );
        let mut places = vec![labels[0], at - start];
        places.extend(field.find(HELP));
        places.extend(field.find(PROBLEM));
        assert!(
            places.windows(2).all(|pair| pair[0] < pair[1]),
            "a box does not stand in its field wrapper with its label, the box, its help line and its problem line in that order: {name} on {page}:\n{field}"
        );
        if tag.contains(r#"type="checkbox""#) {
            let label_end = field.find("</label>");
            assert!(
                label_end.is_some_and(|end| at - start < end),
                "a box does not stand in its field wrapper: the tick box {name} on {page} does not stand inside its label:\n{field}"
            );
        }
    }
}

/// Each box of the set-up form, empty and refused with a problem under every box, of the
/// sign-in form and of the change-password form, refused with a problem under every box,
/// stands in its own field wrapper with its label, the box, its help line and its problem
/// line in that order; the tick box stands inside its label, and its help line follows.
///
/// Proves: decision 123: on the set-up, sign-in and change-password forms each box stands in
/// its own field wrapper holding its label, the box, its help line and its problem line in
/// that order.
#[test]
fn every_box_stands_in_its_field_with_its_lines_in_order() {
    let empty = SetupView {
        book_name: String::new(),
        currency: String::from("CAD"),
        username: String::new(),
        display_name: String::new(),
        starter_chart: true,
    };
    let every = SetupProblems {
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
    for (state, problems) in [
        ("the empty set-up form", SetupProblems::default()),
        ("the set-up form refused in every box", every),
    ] {
        let refused = problems.book_name.is_some();
        let html = html_of(&SetupPage {
            layout: Layout::embedded(),
            currencies: CURRENCIES,
            form: empty.clone(),
            problems,
        });
        assert_fields(
            &html,
            state,
            &[
                ("book_name", false, refused),
                ("currency", true, refused),
                ("username", true, refused),
                ("display_name", true, refused),
                ("password", true, refused),
                ("password_again", false, refused),
                ("starter_chart", true, false),
            ],
        );
    }

    let html = html_of(&LoginPage {
        layout: nobodys(None),
        username: String::from("sam"),
        next: String::new(),
        problem: Some(Refusal::SignIn.to_string()),
    });
    assert_fields(
        &html,
        "the refused sign-in form",
        &[("username", false, false), ("password", false, false)],
    );

    let html = html_of(&password_page(
        None,
        PasswordProblems {
            notice: Some(Refusal::PasswordNotChanged.to_string()),
            current: Some(Refusal::CurrentPasswordWrong.to_string()),
            new: Some(PasswordError::TooShort.to_string()),
            again: Some(Refusal::NewPasswordsDiffer.to_string()),
        },
    ));
    assert_fields(
        &html,
        "the change-password form refused in every box",
        &[
            ("current_password", false, true),
            ("new_password", true, true),
            ("new_password_again", false, true),
        ],
    );
}

/// The entries of the copy of the menu in `html` that begins with `open` and ends with
/// `close`: each `li` as its class and what it holds, in order.
fn menu_entries<'a>(html: &'a str, open: &str, close: &str) -> Vec<(Option<String>, &'a str)> {
    let start = html
        .find(open)
        .unwrap_or_else(|| panic!("the page has no {open}:\n{html}"));
    let end = html[start..]
        .find(close)
        .map_or(html.len(), |index| start + index);
    html[start..end]
        .split("<li")
        .skip(1)
        .map(|chunk| {
            let (tag, rest) = chunk.split_once('>').unwrap_or((chunk, ""));
            let inner = rest.split("</li>").next().unwrap_or_default();
            (attribute(tag, "class"), inner)
        })
        .collect()
}

/// On the home page and on the change-password page, each entry of the menu carries a class
/// of its own, in the wide menu (the `nav`) and in the narrow one (inside the `details`)
/// alike: `home` on the link to the home page, `password` on the link to the
/// change-password page, `look` on the Appearance group, `who` on who is signed in and
/// `sign-out` on the Sign out button, in that order, and the two copies are the same. The
/// Appearance group holds one hidden field, `back`, with the address of the page it stands
/// on, and three buttons named `theme` that send `automatic`, `light` and `dark`, in that
/// order.
///
/// Proves: decision 123 and decision 116: Home, Change password, the Appearance group, who is
/// signed in and Sign out carry the classes home, password, look, who and sign-out, in the
/// wide menu and in the narrow one alike.
#[test]
fn every_menu_entry_carries_its_own_class_in_both_copies() {
    let expected = [
        ("home", r#"<a href="/""#, ">Home</a>"),
        (
            "password",
            r#"<a href="/account/password""#,
            ">Change password</a>",
        ),
        (
            "look",
            r#"<form method="post" action="/theme">"#,
            "<legend>Appearance</legend>",
        ),
        ("who", "Signed in as Sam", "Signed in as Sam"),
        (
            "sign-out",
            r#"<form method="post" action="/logout">"#,
            ">Sign out</button>",
        ),
    ];
    for (page, html, back) in [
        (
            "the home page",
            html_of(&home_page(Some(starter_accounts()), None)),
            "/",
        ),
        (
            "the change-password page",
            html_of(&password_page(None, PasswordProblems::default())),
            "/account/password",
        ),
    ] {
        let wide = menu_entries(&html, r#"<nav class="menu-wide""#, "</nav>");
        let narrow = menu_entries(&html, r#"<details class="menu-narrow""#, "</details>");
        for (copy, entries) in [("the wide menu", &wide), ("the narrow menu", &narrow)] {
            let classes: Vec<Option<&str>> =
                entries.iter().map(|(class, _)| class.as_deref()).collect();
            let wanted: Vec<Option<&str>> =
                expected.iter().map(|(class, _, _)| Some(*class)).collect();
            assert_eq!(
                classes, wanted,
                "a menu entry does not carry its own class in {copy} on {page}"
            );
            for ((class, inner), (_, first, second)) in entries.iter().zip(expected) {
                assert!(
                    inner.contains(first) && inner.contains(second),
                    "a menu entry does not carry its own class: the entry of class {class:?} in {copy} on {page} holds another entry: {inner}"
                );
            }
            let look = entries
                .iter()
                .find(|(class, _)| class.as_deref() == Some("look"))
                .map_or("", |(_, inner)| *inner);
            let hidden = format!(r#"<input type="hidden" name="back" value="{back}">"#);
            assert_eq!(
                look.matches(r#"type="hidden""#).count(),
                1,
                "the Appearance group in {copy} on {page} does not hold one hidden field: {look}"
            );
            assert!(
                look.contains(&hidden),
                "the Appearance group in {copy} on {page} does not return to {back} in its hidden field back: {look}"
            );
            let choices: Vec<Option<String>> = look
                .split("<button")
                .skip(1)
                .map(|button| {
                    let tag = button.split('>').next().unwrap_or_default();
                    assert_eq!(
                        attribute(tag, "name").as_deref(),
                        Some("theme"),
                        "a button of the Appearance group in {copy} on {page} is not named theme: {tag}"
                    );
                    attribute(tag, "value")
                })
                .collect();
            assert_eq!(
                choices,
                ["automatic", "light", "dark"].map(|value| Some(String::from(value))),
                "the buttons of the Appearance group in {copy} on {page} do not send automatic, light and dark, in that order"
            );
        }
        assert_eq!(
            wide, narrow,
            "a menu entry does not carry its own class: the two copies of the menu on {page} differ"
        );
    }
}
