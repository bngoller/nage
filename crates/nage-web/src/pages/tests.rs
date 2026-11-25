//! Every page of milestone 2 slices 5, 6, 6a, 7 and 7a in every state, rendered
//! from its template with made-up details and written out in plain text in the HTML's order,
//! which is the computer's order and the order a keyboard meets (decision 130, the pause's P10;
//! a phone's design moves a section's New line and a confirmation's main button in the style
//! file, which the browser suite proves): Nage's name, with the Menu button on a page that has
//! one, then the heading and each label, help line, box and button in order, and on the home
//! page each balance line with its amount at the right, the width a phone's. The menu is also
//! written out open and as a computer shows it, in the rail.
//! The renderings are file snapshots beside this file (decisions 60 and 115), so a change of
//! wording shows as a changed snapshot; the contract's MSG.md is made from them. The structure
//! the look styles (the boxes' field wrappers, `aria-invalid` and the menu entries' classes)
//! is read from the HTML itself.

use askama::Template;
use nage_core::accounts::{
    AccountBoxes, AccountError, AccountId, Action, BoxName, Clash, Kind, Refused, SystemKey, Word,
};
use nage_core::book::BookError;
use nage_core::chart::{
    AccountView, Chart, ChartLine, ChartSection, EditForm, NewForm, OpeningFact, OtherRow, Owner,
    OwnerChoice, OwnerTag, Place, Row, TrailStep,
};
use nage_core::currency::CURRENCIES;
use nage_core::date::{Date, DateError};
use nage_core::fold::{self, NameError};
use nage_core::home::{
    AccountLine, Group, GroupHeading, Home, HomeBalances, Line, OtherLine, Section,
};
use nage_core::money::ParseMoneyError;
use nage_core::money::{MinorDigits, Money};
use nage_core::password::PasswordError;
use nage_core::transactions::{
    OpeningBalance, OpeningBalanceError, OpeningBoxRefusal, OpeningClash, TransactionId,
};
use nage_core::users::{UserError, UserId};

use super::{
    AccountClashPage, AccountConfirmPage, AccountEditPage, AccountNewPage, AccountPage,
    AccountProblems, AccountTyped, AccountsPage, CROSS_SITE_HEADING, Entry, HomePage, Layout,
    LoginPage, Menu, Message, NewWhatPage, NotFoundPage, OpeningAccount, OpeningBalanceClashPage,
    OpeningBalancePage, OpeningBalanceRemovePage, OpeningProblems, OpeningTyped, PasswordPage,
    PasswordProblems, RefusalPage, SetupClosedPage, SetupPage, SetupProblems, SetupView,
    TOO_LARGE_HEADING, shown_from_saved,
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
    /// The trail at the top of a page of a place (decision 130).
    Trail,
}

/// Whether the start tag's `attributes` carry `class` among the words of their class.
fn has_class(attributes: &str, class: &str) -> bool {
    attribute(attributes, "class")
        .is_some_and(|classes| classes.split_whitespace().any(|word| word == class))
}

/// The level a line's `level-N` class names, 1 when it names none.
fn level_of(attributes: &str) -> usize {
    attributes
        .split("level-")
        .nth(1)
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(1)
}

/// Writes the heading of a section of the list (decision 131) that `pending` holds, with the
/// total `amount` at its right, once its line is complete.
fn flush_heading(
    lines: &mut Vec<String>,
    pending: &mut Option<String>,
    amount: &mut Option<String>,
) {
    if let Some(name) = pending.take() {
        let total = amount.take().unwrap_or_default();
        lines.push(format!("\n{}", row(0, &format!("### {name}"), &total)));
    }
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
///
/// The pages of a place (decision 130) add: the trail as `Trail: ` and its steps joined by
/// ` > `; a pill as `[…]` after what it marks; a link drawn as a button as `( … )`, and a link
/// of its own line (a section's New line, a way back) as `-> …`; a section of the list after
/// `### ` with its total at the right; a row of a list as `-> ` and its name, set in by its
/// level, with its pills and its amount at the right, and the Other line without the arrow; a
/// fact as `Label: value`; a list of choices as `[ chosen v ]` with every choice on the line
/// under it after `choices: `, the first one chosen when none is marked, as a browser shows it;
/// each choice of the page "What do you want to add?" as its link with its second line under it.
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
    let mut crumbs: Vec<String> = Vec::new();
    let mut tag_open = false;
    let mut link: Option<bool> = None;
    let mut heading: Option<String> = None;
    let mut rows_list = false;
    let mut choose_list = false;
    let mut label = String::new();
    let mut select_name = String::new();
    let mut choices: Vec<String> = Vec::new();
    let mut in_fact = false;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let end = rest[start..]
            .find('>')
            .map_or(rest.len() - 1, |index| index + start);
        let tag = &rest[start + 1..end];
        rest = &rest[end + 1..];
        let (name, attributes) = tag.split_once(' ').unwrap_or((tag, ""));
        let in_menu = matches!(zone, Zone::Narrow | Zone::Wide);
        // A paragraph's spans (the account's place, the confirmation's figure line) and a fact's
        // (the opening balance's figure and its day) are its words; the Net worth line alone
        // reads its spans as a name and an amount.
        let in_place =
            matches!(element, Some(("p", open)) if !open.contains("net-worth")) || in_fact;
        match name {
            "nav" if attributes.contains(r#"class="trail""#) => {
                zone = Zone::Trail;
                crumbs.clear();
            }
            "li" if zone == Zone::Trail => text.clear(),
            "/li" if zone == Zone::Trail => {
                crumbs.push(plain(&text));
                text.clear();
            }
            "/nav" if zone == Zone::Trail => {
                lines.push(format!("Trail: {}\n", crumbs.join(" > ")));
                zone = Zone::Page;
                text.clear();
            }
            _ if zone == Zone::Trail => {}
            "small" if has_class(attributes, "tag") => {
                text.push_str(" [");
                tag_open = true;
            }
            "/small" if tag_open => {
                text.push(']');
                tag_open = false;
            }
            "span" | "/span" if in_place => {}
            "a" if zone == Zone::Page && has_class(attributes, "button") => {
                flush_heading(&mut lines, &mut heading, &mut figure.amount);
                link = Some(true);
                text.clear();
            }
            "a" if zone == Zone::Page
                && (has_class(attributes, "add") || has_class(attributes, "back")) =>
            {
                flush_heading(&mut lines, &mut heading, &mut figure.amount);
                link = Some(false);
                text.clear();
            }
            "/a" if link.is_some() => {
                let words = plain(&text);
                if link.take() == Some(true) {
                    lines.push(format!("\n( {words} )\n"));
                } else {
                    lines.push(format!("-> {words}"));
                }
                text.clear();
            }
            "/div" if heading.is_some() => {
                flush_heading(&mut lines, &mut heading, &mut figure.amount);
            }
            "ul" => {
                rows_list = has_class(attributes, "rows");
                choose_list = has_class(attributes, "choose");
                if choose_list {
                    lines.push(String::new());
                }
            }
            "/ul" => {
                if rows_list {
                    lines.push(String::new());
                }
                rows_list = false;
                choose_list = false;
            }
            "li" if rows_list || choose_list => {
                item = Some(attributes);
                figure = Figure::default();
                span = "";
                text.clear();
            }
            "span" if rows_list => {
                if has_class(attributes, "amount") {
                    figure.name = plain(&text);
                    span = attributes;
                    text.clear();
                }
            }
            "/span" if rows_list => {
                if span.contains("amount") {
                    figure.amount = Some(plain(&text));
                    span = "";
                    text.clear();
                }
            }
            "/li" if rows_list => {
                let level = level_of(item.take().unwrap_or_default());
                let name = if figure.name.is_empty() {
                    plain(&text)
                } else {
                    figure.name.clone()
                };
                let amount = figure.amount.clone().unwrap_or_default();
                lines.push(row(level * 2, &name, &amount));
                text.clear();
            }
            "/strong" if choose_list => {
                figure.name = plain(&text);
                text.clear();
            }
            "span" if choose_list => text.clear(),
            "/span" if choose_list => {
                figure.owner = Some(plain(&text));
                text.clear();
            }
            "/li" if choose_list => {
                item = None;
                lines.push(format!(
                    "{}\n   {}",
                    figure.name,
                    figure.owner.clone().unwrap_or_default()
                ));
                text.clear();
            }
            "dt" | "dd" => {
                in_fact = name == "dd";
                text.clear();
            }
            "/dt" => {
                label = plain(&text);
                text.clear();
            }
            "/dd" => {
                in_fact = false;
                lines.push(format!("{label}: {}", plain(&text)));
                text.clear();
            }
            "h3" => {
                element = Some((name, attributes));
                figure = Figure::default();
                text.clear();
                tick = None;
            }
            "title" | "h1" | "p" | "label" | "button" | "option" | "summary" | "legend" => {
                element = Some((name, attributes));
                text.clear();
                tick = None;
            }
            "header"
                if attributes.contains(r#"class="bar""#) || attributes.contains("masthead") =>
            {
                zone = Zone::Bar;
                menu.lone = attributes.contains("masthead");
            }
            "header" => {}
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
                choices.clear();
                select_name = attribute(attributes, "name").unwrap_or_default();
            }
            "/select" if select_name == "currency" => {
                lines.push(format!(
                    "[ {selected} v ]  (a list of {options} currency codes)"
                ));
            }
            "/select" => {
                let shown = if selected.is_empty() {
                    choices.first().cloned().unwrap_or_default()
                } else {
                    selected.clone()
                };
                lines.push(format!("[ {shown} v ]"));
                lines.push(format!("  choices: {}", choices.join(" | ")));
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
                    Zone::Bar | Zone::Trail => {}
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
                let title = if figure.name.is_empty() {
                    plain(&text)
                } else {
                    figure.name.clone()
                };
                lines.push(format!("\n{}", row(0, &format!("## {title}"), &amount)));
                text.clear();
            }
            "/header" if zone == Zone::Bar => {
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
                        "h3" if attributes.contains(r#"id="k-"#) => heading = Some(words),
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
                            choices.push(words.clone());
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
    let mut page = lines
        .join("\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
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
                Some(Entry::Accounts) => "/accounts",
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
            all_archived: false,
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
    insta::assert_snapshot!(
        "home_8_long_name_large_amount",
        rendered(&home_page(Some(a_long_name_at_the_largest_amount()), None))
    );
}

/// The starter accounts beside an account of a 64-character name at the largest amount a form
/// takes (decision 142): its name wraps on its line, the figure keeps its place at the right.
fn a_long_name_at_the_largest_amount() -> HomeBalances {
    let largest = 99_999_999_999_999;
    HomeBalances {
        assets: Section {
            kind: Kind::Asset,
            name: String::from("Assets"),
            total: Money::from_minor(largest),
            groups: vec![Group {
                heading: None,
                lines: vec![
                    line("Cash", 1, 0, None),
                    line("Checking", 1, 0, None),
                    line(
                        "Holiday fund for the long summer trip to the coast and the lakes",
                        1,
                        largest,
                        None,
                    ),
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
        net_worth: Money::from_minor(largest),
    }
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
        ("accounts", r#"<a href="/accounts""#, ">Accounts</a>"),
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

// The account pages (decisions 128 to 138): a made-up household's chart, built as the core's
// models, rendered by the pages' own constructors.

/// The day the account pages' balances are for: 2026-11-16.
fn the_day() -> Date {
    Date::new(2026, 11, 16).unwrap_or_else(|error| panic!("2026-11-16 is no date: {error}"))
}

/// The layout of an account page that Sam is signed in on, with `message` at the top. The menu
/// marks no entry: the page's constructor marks Accounts.
fn accounts_layout(message: Option<Message>) -> Layout {
    sams(None, message)
}

/// Alex's tag, the second user's colour.
fn alex() -> OwnerTag {
    OwnerTag {
        display_name: String::from("Alex"),
        colour: 2,
    }
}

/// Sam's tag, the first user's colour.
fn sam() -> OwnerTag {
    OwnerTag {
        display_name: String::from("Sam"),
        colour: 1,
    }
}

/// A row of a list: the account `id` called `name` at `level`, joint, with `balance` minor
/// units when it shows a figure.
fn row_of(id: i64, name: &str, level: u8, balance: Option<i64>) -> Row {
    Row {
        id: AccountId(id),
        name: name.to_owned(),
        level,
        owner: None,
        built_in: false,
        archived: false,
        balance: balance.map(Money::from_minor),
    }
}

/// [`row_of`] as a line of a list.
fn row_line(id: i64, name: &str, level: u8, balance: Option<i64>) -> ChartLine {
    ChartLine::Account(row_of(id, name, level, balance))
}

/// [`row_line`] with the owner `owner`.
fn owned_line(id: i64, name: &str, balance: Option<i64>, owner: OwnerTag) -> ChartLine {
    ChartLine::Account(Row {
        owner: Some(owner),
        ..row_of(id, name, 1, balance)
    })
}

/// [`row_line`] of an archived account.
fn archived_line(id: i64, name: &str, level: u8, balance: Option<i64>) -> ChartLine {
    ChartLine::Account(Row {
        archived: true,
        ..row_of(id, name, level, balance)
    })
}

/// The line of a built-in account directly under its section.
fn built_in_line(id: i64, name: &str) -> ChartLine {
    ChartLine::Account(Row {
        built_in: true,
        ..row_of(id, name, 1, None)
    })
}

/// The Other line at `level` with `minor` minor units.
fn other_line(level: u8, minor: i64) -> ChartLine {
    ChartLine::Other(OtherRow {
        level,
        amount: Money::from_minor(minor),
    })
}

/// The section of `kind` with the id `id`, its `total` and its `rows`.
fn chart_section(
    kind: Kind,
    id: i64,
    total: Option<i64>,
    rows: Vec<ChartLine>,
    all_archived: bool,
) -> ChartSection {
    ChartSection {
        kind,
        id: AccountId(id),
        name: kind.root_name().to_owned(),
        total: total.map(Money::from_minor),
        rows,
        all_archived,
    }
}

/// The two built-in accounts of Equity.
fn equity_section() -> ChartSection {
    chart_section(
        Kind::Equity,
        3,
        None,
        vec![
            built_in_line(6, "Opening Balances"),
            built_in_line(7, "Valuation Changes"),
        ],
        false,
    )
}

/// A list as of [`the_day`] in a book with two minor digits.
fn chart_of(
    balances_shown: bool,
    archived_shown: bool,
    archived: usize,
    sections: [ChartSection; 4],
) -> Chart {
    let [assets, liabilities, income, expenses] = sections;
    Chart {
        as_of: the_day(),
        minor_digits: MinorDigits::TWO,
        balances_shown,
        archived_shown,
        archived,
        assets,
        liabilities,
        income,
        expenses,
        equity: equity_section(),
    }
}

/// The list right after a set-up with the starter accounts: every account at zero.
fn starter_chart() -> Chart {
    let expenses = [
        (31, "Dining"),
        (32, "Entertainment"),
        (33, "Fees"),
        (34, "Gifts"),
        (21, "Groceries"),
        (35, "Health"),
        (36, "Housing"),
        (37, "Other"),
        (38, "Shopping"),
        (39, "Subscriptions"),
        (40, "Transport"),
        (41, "Travel"),
    ];
    let mut expense_rows: Vec<ChartLine> = expenses
        .into_iter()
        .map(|(id, name)| row_line(id, name, 1, None))
        .collect();
    expense_rows.insert(12, built_in_line(8, "Uncategorized"));
    expense_rows.push(row_line(42, "Utilities", 1, None));
    chart_of(
        true,
        false,
        0,
        [
            chart_section(
                Kind::Asset,
                1,
                Some(0),
                vec![
                    row_line(10, "Cash", 1, Some(0)),
                    row_line(11, "Checking", 1, Some(0)),
                    row_line(12, "Savings", 1, Some(0)),
                ],
                false,
            ),
            chart_section(
                Kind::Liability,
                2,
                Some(0),
                vec![row_line(15, "Credit Card", 1, Some(0))],
                false,
            ),
            chart_section(
                Kind::Income,
                4,
                None,
                vec![
                    row_line(30, "Other", 1, None),
                    row_line(26, "Salary", 1, None),
                ],
                false,
            ),
            chart_section(Kind::Expense, 5, None, expense_rows, false),
        ],
    )
}

/// A household's list: owned and joint accounts, Savings holding two accounts and 250.00 of
/// its own, a card that is owed on, categories inside Food, and two archived accounts, which
/// stand in their places when `shown`.
fn household_chart(shown: bool) -> Chart {
    let mut assets = vec![
        owned_line(10, "Cash", Some(4_000), alex()),
        owned_line(11, "Checking", Some(248_015), sam()),
        row_line(12, "Savings", 1, Some(935_000)),
        row_line(13, "Emergency fund", 2, Some(150_000)),
        row_line(14, "Holiday fund", 2, Some(760_000)),
        other_line(2, 25_000),
    ];
    let mut expenses = vec![
        row_line(20, "Food", 1, None),
        row_line(21, "Groceries", 2, None),
        row_line(22, "Restaurants", 2, None),
        row_line(23, "Pet food", 1, None),
        row_line(24, "Rent", 1, None),
        built_in_line(8, "Uncategorized"),
    ];
    if shown {
        assets.insert(2, archived_line(17, "Old savings", 1, Some(0)));
        expenses.insert(3, archived_line(27, "Old gym", 1, None));
    }
    chart_of(
        true,
        shown,
        2,
        [
            chart_section(Kind::Asset, 1, Some(1_187_015), assets, false),
            chart_section(
                Kind::Liability,
                2,
                Some(120_480),
                vec![
                    owned_line(15, "Credit Card", Some(120_480), sam()),
                    row_line(16, "Travel card", 1, Some(0)),
                ],
                false,
            ),
            chart_section(
                Kind::Income,
                4,
                None,
                vec![
                    row_line(25, "Interest", 1, None),
                    row_line(26, "Salary", 1, None),
                ],
                false,
            ),
            chart_section(Kind::Expense, 5, None, expenses, false),
        ],
    )
}

/// The list of a book set up without the starter accounts: nothing under Assets, Liabilities
/// and Income, no figure.
fn declined_chart() -> Chart {
    chart_of(
        false,
        false,
        0,
        [
            chart_section(Kind::Asset, 1, None, Vec::new(), false),
            chart_section(Kind::Liability, 2, None, Vec::new(), false),
            chart_section(Kind::Income, 4, None, Vec::new(), false),
            chart_section(
                Kind::Expense,
                5,
                None,
                vec![built_in_line(8, "Uncategorized")],
                false,
            ),
        ],
    )
}

/// The list of a book whose accounts under Assets and Liabilities are all archived and hidden.
fn all_archived_chart() -> Chart {
    chart_of(
        false,
        false,
        2,
        [
            chart_section(Kind::Asset, 1, None, Vec::new(), true),
            chart_section(Kind::Liability, 2, None, Vec::new(), true),
            chart_section(
                Kind::Income,
                4,
                None,
                vec![row_line(26, "Salary", 1, None)],
                false,
            ),
            chart_section(
                Kind::Expense,
                5,
                None,
                vec![
                    row_line(21, "Groceries", 1, None),
                    built_in_line(8, "Uncategorized"),
                ],
                false,
            ),
        ],
    )
}

/// The list page over `chart`.
fn accounts_page(chart: Chart) -> AccountsPage {
    AccountsPage::new(accounts_layout(None), chart)
}

/// A step of a trail.
fn step(id: i64, name: &str) -> TrailStep {
    TrailStep {
        id: AccountId(id),
        name: name.to_owned(),
    }
}

/// The page of the account `id` called `name` at `place` under the section of `kind`: joint
/// and at zero under Assets and Liabilities, nothing inside it, neither built in nor archived.
fn view(id: i64, name: &str, place: &str, kind: Kind) -> AccountView {
    AccountView {
        as_of: the_day(),
        minor_digits: MinorDigits::TWO,
        id: AccountId(id),
        name: name.to_owned(),
        place: place.to_owned(),
        kind,
        word: Word::of(kind),
        trail: Vec::new(),
        built_in: None,
        archived: false,
        owner: kind.allows_owner().then_some(Owner::Joint),
        balance: kind.allows_owner().then_some(Money::ZERO),
        opening_balance: None,
        inside: Vec::new(),
        can_hold: true,
    }
}

/// The fact of an opening balance of `minor` minor units on 2026-09-30.
fn opening_fact(minor: i64) -> Option<OpeningFact> {
    Some(OpeningFact::Entered {
        date: day("2026-09-30"),
        amount: Money::from_minor(minor),
    })
}

/// Savings: joint, 9,350.00 with two accounts inside it and 250.00 of its own, its opening
/// balance 250.00 on 2026-09-30.
fn savings() -> AccountView {
    AccountView {
        balance: Some(Money::from_minor(935_000)),
        opening_balance: opening_fact(25_000),
        inside: vec![
            row_line(13, "Emergency fund", 1, Some(150_000)),
            row_line(14, "Holiday fund", 1, Some(760_000)),
            other_line(1, 25_000),
        ],
        ..view(12, "Savings", "Assets:Savings", Kind::Asset)
    }
}

/// Savings with an archived Jar inside it beside the two others.
fn savings_with_an_archived_jar() -> AccountView {
    AccountView {
        balance: Some(Money::from_minor(935_000)),
        opening_balance: opening_fact(25_000),
        inside: vec![
            row_line(13, "Emergency fund", 1, Some(150_000)),
            row_line(14, "Holiday fund", 1, Some(760_000)),
            archived_line(18, "Jar", 1, Some(0)),
            other_line(1, 25_000),
        ],
        ..view(12, "Savings", "Assets:Savings", Kind::Asset)
    }
}

/// Holiday fund inside Savings, Alex's, at zero.
fn holiday_fund() -> AccountView {
    AccountView {
        trail: vec![step(12, "Savings")],
        owner: Some(Owner::User(alex())),
        opening_balance: Some(OpeningFact::NoneYet),
        ..view(
            14,
            "Holiday fund",
            "Assets:Savings:Holiday fund",
            Kind::Asset,
        )
    }
}

/// Checking, Sam's, holding 2,480.15, all of it its opening balance on 2026-09-30.
fn checking() -> AccountView {
    AccountView {
        owner: Some(Owner::User(sam())),
        balance: Some(Money::from_minor(248_015)),
        opening_balance: opening_fact(248_015),
        ..view(11, "Checking", "Assets:Checking", Kind::Asset)
    }
}

/// The category Groceries inside Food.
fn groceries() -> AccountView {
    AccountView {
        trail: vec![step(20, "Food")],
        ..view(21, "Groceries", "Expenses:Food:Groceries", Kind::Expense)
    }
}

/// Old savings at zero, archived or not.
fn old_savings(archived: bool) -> AccountView {
    AccountView {
        archived,
        can_hold: !archived,
        opening_balance: (!archived).then_some(OpeningFact::NoneYet),
        ..view(17, "Old savings", "Assets:Old savings", Kind::Asset)
    }
}

/// Jar, archived inside the archived Old savings.
fn jar() -> AccountView {
    AccountView {
        trail: vec![step(17, "Old savings")],
        archived: true,
        can_hold: false,
        ..view(18, "Jar", "Assets:Old savings:Jar", Kind::Asset)
    }
}

/// The archived category Old gym.
fn old_gym() -> AccountView {
    AccountView {
        archived: true,
        can_hold: false,
        ..view(27, "Old gym", "Expenses:Old gym", Kind::Expense)
    }
}

/// The built-in account `key`.
fn built_in(key: SystemKey) -> AccountView {
    let (id, place) = match key {
        SystemKey::OpeningBalances => (6, "Equity:Opening Balances"),
        SystemKey::ValuationChanges => (7, "Equity:Valuation Changes"),
        SystemKey::Uncategorized => (8, "Expenses:Uncategorized"),
        other => panic!("{other} is a section, which has no page"),
    };
    AccountView {
        built_in: Some(key),
        can_hold: false,
        ..view(id, key.name(), place, key.kind())
    }
}

/// The built-in Opening Balances.
fn opening_balances() -> AccountView {
    built_in(SystemKey::OpeningBalances)
}

/// The page of `view` with `message` at the top.
fn account_page(view: AccountView, message: Option<Message>) -> AccountPage {
    AccountPage::new(accounts_layout(message), view)
}

/// The confirmation of Archive (`archive`) or Unarchive of `view`.
fn confirm_page(view: AccountView, archive: bool) -> AccountConfirmPage {
    AccountConfirmPage::new(accounts_layout(None), view, archive)
}

/// A place a pick list offers.
fn place(id: i64, full_name: &str) -> Place {
    Place {
        id: AccountId(id),
        full_name: full_name.to_owned(),
        archived: false,
    }
}

/// The section of `kind` as the first choice of a pick list.
fn section_place(kind: Kind) -> Place {
    let id = match kind {
        Kind::Asset => 1,
        Kind::Liability => 2,
        Kind::Equity => 3,
        Kind::Income => 4,
        Kind::Expense => 5,
    };
    place(id, kind.root_name())
}

/// An owner a pick list offers.
fn owner_choice(id: i64, name: &str, deactivated: bool) -> OwnerChoice {
    OwnerChoice {
        id: UserId(id),
        display_name: name.to_owned(),
        deactivated,
    }
}

/// The owners of the household: Alex and Sam, by name.
fn household_owners() -> Vec<OwnerChoice> {
    vec![
        owner_choice(2, "Alex", false),
        owner_choice(1, "Sam", false),
    ]
}

/// The create form under Assets.
fn assets_form() -> NewForm {
    NewForm {
        kind: Kind::Asset,
        word: Word::Account,
        section: section_place(Kind::Asset),
        places: vec![
            place(10, "Assets:Cash"),
            place(11, "Assets:Checking"),
            place(12, "Assets:Savings"),
            place(13, "Assets:Savings:Emergency fund"),
            place(14, "Assets:Savings:Holiday fund"),
        ],
        owners: Some(household_owners()),
    }
}

/// The create form under Expenses.
fn expenses_form() -> NewForm {
    NewForm {
        kind: Kind::Expense,
        word: Word::Category,
        section: section_place(Kind::Expense),
        places: vec![
            place(20, "Expenses:Food"),
            place(21, "Expenses:Food:Groceries"),
            place(22, "Expenses:Food:Restaurants"),
            place(23, "Expenses:Pet food"),
            place(24, "Expenses:Rent"),
        ],
        owners: None,
    }
}

/// The create form `form` holding `typed`, with `notice` at the top and `problems` under the
/// boxes, opened from the list.
fn new_page(
    form: NewForm,
    typed: AccountTyped,
    notice: Option<Message>,
    problems: AccountProblems,
) -> AccountNewPage {
    AccountNewPage::new(accounts_layout(notice), form, typed, problems, Vec::new())
}

/// The create form under Assets opened from "New account inside Savings".
fn new_page_inside_savings() -> AccountNewPage {
    AccountNewPage::new(
        accounts_layout(None),
        assets_form(),
        AccountTyped {
            inside: String::from("12"),
            ..AccountTyped::default()
        },
        AccountProblems::default(),
        vec![step(12, "Savings")],
    )
}

/// The first part of a refusal of a new thing of `word`.
fn created(word: Word) -> Refused {
    Refused {
        word,
        place: None,
        action: Action::Created,
    }
}

/// The first part of a refusal of `action` on the account at `place`.
fn refused_on(place: &str, action: Action) -> Refused {
    Refused {
        word: Word::Account,
        place: Some(place.to_owned()),
        action,
    }
}

/// Why the core refuses the account name `typed`.
fn name_error(typed: &str) -> NameError {
    fold::checked_name(typed)
        .err()
        .unwrap_or_else(|| panic!("the core takes the name {typed:?}"))
}

/// The change form of the account `id` called `name` at `place` under the section of `kind`,
/// directly under the section, joint, offering no other place.
fn edit_form(id: i64, name: &str, place: &str, kind: Kind) -> EditForm {
    let section = section_place(kind);
    EditForm {
        id: AccountId(id),
        name: name.to_owned(),
        place: place.to_owned(),
        kind,
        word: Word::of(kind),
        trail: Vec::new(),
        built_in: None,
        archived: false,
        places: Some(Vec::new()),
        owners: kind.allows_owner().then(household_owners),
        saved: AccountBoxes {
            name: name.to_owned(),
            parent: Some(section.id),
            owner: None,
        },
        section,
    }
}

/// What a fresh change form of `form` holds: what is saved.
fn typed_from(form: &EditForm) -> AccountTyped {
    AccountTyped {
        name: form.saved.name.clone(),
        inside: form
            .saved
            .parent
            .map(|parent| parent.to_string())
            .unwrap_or_default(),
        owner: form
            .saved
            .owner
            .map(|owner| owner.to_string())
            .unwrap_or_default(),
    }
}

/// The change form of Savings, which can go directly under Assets, inside Cash or inside
/// Checking.
fn savings_form() -> EditForm {
    EditForm {
        places: Some(vec![place(10, "Assets:Cash"), place(11, "Assets:Checking")]),
        ..edit_form(12, "Savings", "Assets:Savings", Kind::Asset)
    }
}

/// The change form of Savings after another save renamed it Savings (joint).
fn savings_form_saved_now() -> EditForm {
    let mut form = EditForm {
        place: String::from("Assets:Savings (joint)"),
        name: String::from("Savings (joint)"),
        ..savings_form()
    };
    form.saved.name = String::from("Savings (joint)");
    form
}

/// The change form of the category Groceries inside Food.
fn groceries_form() -> EditForm {
    let mut form = EditForm {
        trail: vec![step(20, "Food")],
        places: Some(vec![
            place(20, "Expenses:Food"),
            place(22, "Expenses:Food:Restaurants"),
            place(23, "Expenses:Pet food"),
            place(24, "Expenses:Rent"),
        ]),
        ..edit_form(21, "Groceries", "Expenses:Food:Groceries", Kind::Expense)
    };
    form.saved.parent = Some(AccountId(20));
    form
}

/// The change form of the built-in Opening Balances: the name alone.
fn opening_balances_form() -> EditForm {
    EditForm {
        built_in: Some(SystemKey::OpeningBalances),
        places: None,
        owners: None,
        ..edit_form(
            6,
            "Opening Balances",
            "Equity:Opening Balances",
            Kind::Equity,
        )
    }
}

/// The change form of Old savings: the name alone while it is archived.
fn old_savings_form(archived: bool) -> EditForm {
    let form = edit_form(17, "Old savings", "Assets:Old savings", Kind::Asset);
    if archived {
        EditForm {
            archived: true,
            places: None,
            owners: None,
            ..form
        }
    } else {
        form
    }
}

/// The change form `form` holding `typed`, with `notice` at the top and `problems` under the
/// boxes, sending back what is saved as what it showed.
fn edit_page(
    form: EditForm,
    typed: AccountTyped,
    notice: Option<Message>,
    problems: AccountProblems,
) -> AccountEditPage {
    let shown = shown_from_saved(&form.saved);
    AccountEditPage::new(accounts_layout(notice), form, typed, problems, shown)
}

/// The change form of Checking refused in two boxes: a name with a colon, and a place, Old
/// savings, archived meanwhile, which the list keeps marked, as it keeps Robin, deactivated
/// meanwhile, as the owner chosen.
fn edit_page_refused_in_two_boxes() -> AccountEditPage {
    let old_savings = Place {
        archived: true,
        ..place(17, "Assets:Old savings")
    };
    let form = EditForm {
        places: Some(vec![
            place(10, "Assets:Cash"),
            old_savings,
            place(12, "Assets:Savings"),
            place(13, "Assets:Savings:Emergency fund"),
            place(14, "Assets:Savings:Holiday fund"),
        ]),
        owners: Some(vec![
            owner_choice(2, "Alex", false),
            owner_choice(3, "Robin", true),
            owner_choice(1, "Sam", false),
        ]),
        ..edit_form(11, "Checking", "Assets:Checking", Kind::Asset)
    };
    let typed = AccountTyped {
        name: String::from("Checking: joint"),
        inside: String::from("17"),
        owner: String::from("3"),
    };
    let problems = AccountProblems {
        name: Some(
            AccountError::Name {
                refused: refused_on("Assets:Checking", Action::Renamed),
                error: name_error(&typed.name),
            }
            .to_string(),
        ),
        inside: Some(
            AccountError::ParentArchived {
                refused: refused_on("Assets:Checking", Action::Moved),
                parent: String::from("Assets:Old savings"),
            }
            .to_string(),
        ),
        owner: None,
    };
    edit_page(
        form,
        typed,
        alert(Refusal::AccountNotSaved {
            word: Word::Account,
        }),
        problems,
    )
}

/// The clash page of `form` for `clash`, the person having sent `typed`.
fn clash_page(form: EditForm, clash: Clash, typed: AccountTyped) -> AccountClashPage {
    AccountClashPage::new(
        accounts_layout(alert(clash.to_string())),
        form,
        clash,
        typed,
    )
}

/// The clash page of Savings: the person renamed it Savings (house) while another save renamed
/// it Savings (joint).
fn clash_on_the_name() -> AccountClashPage {
    let form = savings_form_saved_now();
    let clash = Clash {
        place: form.place.clone(),
        boxes: vec![BoxName::Name],
        changed: vec![BoxName::Name],
        saved: form.saved.clone(),
    };
    let typed = AccountTyped {
        name: String::from("Savings (house)"),
        inside: String::from("1"),
        owner: String::new(),
    };
    clash_page(form, clash, typed)
}

/// The clash page of Cash: the person moved it inside Checking and gave it to Sam while
/// another save put it inside Savings and gave it to Alex.
fn clash_page_on_place_and_owner() -> AccountClashPage {
    let mut form = EditForm {
        trail: vec![step(12, "Savings")],
        places: Some(vec![
            place(11, "Assets:Checking"),
            place(12, "Assets:Savings"),
            place(13, "Assets:Savings:Emergency fund"),
            place(14, "Assets:Savings:Holiday fund"),
        ]),
        ..edit_form(10, "Cash", "Assets:Savings:Cash", Kind::Asset)
    };
    form.saved.parent = Some(AccountId(12));
    form.saved.owner = Some(UserId(2));
    let clash = Clash {
        place: form.place.clone(),
        boxes: vec![BoxName::Inside, BoxName::Owner],
        changed: vec![BoxName::Inside, BoxName::Owner],
        saved: form.saved.clone(),
    };
    let typed = AccountTyped {
        name: String::from("Cash"),
        inside: String::from("11"),
        owner: String::from("1"),
    };
    clash_page(form, clash, typed)
}

/// The list as text: right after a set-up with the starter accounts; a household's, with the
/// owners' pills, a figure on every account under Assets and Liabilities, the sections' totals
/// and the Other line of Savings; the same with the archived accounts shown in their places;
/// a book with no accounts under Assets, Liabilities and Income; a book whose accounts there
/// are all archived. Then the home page of such a book, whose link leads to this list. The
/// list marks Accounts in both copies of the menu, and only Accounts.
///
/// Proves: Decisions 129 to 131 with P8: the list's text with the starter accounts, a household with an Other line, archived shown, no accounts and all archived, the menu marking Accounts.
#[test]
fn the_accounts_list_in_each_state() {
    insta::assert_snapshot!(
        "accounts_1_starter_accounts",
        rendered(&accounts_page(starter_chart()))
    );
    insta::assert_snapshot!(
        "accounts_2_household",
        rendered(&accounts_page(household_chart(false)))
    );
    insta::assert_snapshot!(
        "accounts_3_archived_shown",
        rendered(&accounts_page(household_chart(true)))
    );
    insta::assert_snapshot!(
        "accounts_4_no_accounts",
        rendered(&accounts_page(declined_chart()))
    );
    insta::assert_snapshot!(
        "accounts_5_all_archived",
        rendered(&accounts_page(all_archived_chart()))
    );
    let mut all_archived = home_page(None, None);
    all_archived.home.all_archived = true;
    insta::assert_snapshot!("home_7_all_archived", rendered(&all_archived));

    let html = html_of(&accounts_page(starter_chart()));
    for (copy, open, close) in [
        ("the wide menu", r#"<nav class="menu-wide""#, "</nav>"),
        (
            "the narrow menu",
            r#"<details class="menu-narrow""#,
            "</details>",
        ),
    ] {
        let marked: Vec<Option<String>> = menu_entries(&html, open, close)
            .into_iter()
            .filter(|(_, inner)| inner.contains(r#"aria-current="page""#))
            .map(|(class, _)| class)
            .collect();
        assert_eq!(
            marked,
            [Some(String::from("accounts"))],
            "the entries {copy} marks as the page in view on the list"
        );
    }
}

/// The create pages as text: "What do you want to add?" with its four choices; the form
/// under Assets with Owner and the one under Expenses without it; the form under Expenses
/// refused for a colon in the name, and the one under Assets refused for a name Assets holds
/// already, each keeping what was typed, with the line at the top and the problem under its
/// box; and the form opened from "New account inside Savings", Savings chosen, its trail
/// through Savings and its way back to Savings.
///
/// Proves: Decision 132 with P10: What do you want to add, the forms under Assets and Expenses, one opened inside Savings with its trail and way back, and the refused forms with their problems under the boxes.
#[test]
fn the_create_pages_in_each_state() {
    insta::assert_snapshot!(
        "account_new_1_what",
        rendered(&NewWhatPage::new(accounts_layout(None)))
    );
    insta::assert_snapshot!(
        "account_new_2_form_assets",
        rendered(&new_page(
            assets_form(),
            AccountTyped::default(),
            None,
            AccountProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "account_new_3_form_expenses",
        rendered(&new_page(
            expenses_form(),
            AccountTyped::default(),
            None,
            AccountProblems::default()
        ))
    );
    let typed = AccountTyped {
        name: String::from("Bills: Hydro"),
        ..AccountTyped::default()
    };
    let problems = AccountProblems {
        name: Some(
            AccountError::Name {
                refused: created(Word::Category),
                error: name_error(&typed.name),
            }
            .to_string(),
        ),
        ..AccountProblems::default()
    };
    let notice = alert(Refusal::AccountNotCreated {
        word: Word::Category,
    });
    insta::assert_snapshot!(
        "account_new_4_refused",
        rendered(&new_page(expenses_form(), typed, notice, problems))
    );
    let typed = AccountTyped {
        name: String::from("checking"),
        inside: String::new(),
        owner: String::from("1"),
    };
    let problems = AccountProblems {
        name: Some(
            AccountError::NameTaken {
                refused: created(Word::Account),
                holder: String::from("Assets"),
                name: String::from("Checking"),
            }
            .to_string(),
        ),
        ..AccountProblems::default()
    };
    let notice = alert(Refusal::AccountNotCreated {
        word: Word::Account,
    });
    insta::assert_snapshot!(
        "account_new_5_name_taken",
        rendered(&new_page(assets_form(), typed, notice, problems))
    );
    insta::assert_snapshot!("account_new_6_inside", rendered(&new_page_inside_savings()));
}

/// An account's page as text: Savings with what is inside it and its Other line; Holiday fund,
/// Alex's, right after it was created; the category Groceries, with no owner and no figure;
/// the three built-in accounts, each with its sentence and Change name alone; Old savings
/// archived, with Unarchive and Change name, and the same with the one-time line of its
/// archive, and unarchived again with its own; Checking refused an archive for its balance,
/// Savings for an account inside it that is not archived, and Jar an unarchive for the archived
/// account that holds it, each at the top; Savings changed, with its line; Savings with an
/// archived account inside it, marked; and the archived category Old gym. In the HTML, Old
/// savings' Archive and, archived, its Unarchive lead to their confirmations; every account
/// page has at most one main button, and every one but the list a trail whose first step,
/// Accounts, is a link.
///
/// Proves: Decisions 133, 135 and 137 with P4, P8 and P9: an account's and a category's page, the three built-in ones, archived ones with Change name, an archived one inside, refused at the top, each message.
#[test]
fn an_accounts_page_in_each_state() {
    insta::assert_snapshot!("account_1_page", rendered(&account_page(savings(), None)));
    insta::assert_snapshot!(
        "account_2_created",
        rendered(&account_page(
            holiday_fund(),
            status(Notice::AccountCreated {
                word: Word::Account
            })
        ))
    );
    insta::assert_snapshot!(
        "account_3_category",
        rendered(&account_page(groceries(), None))
    );
    insta::assert_snapshot!(
        "account_4_built_in",
        rendered(&account_page(opening_balances(), None))
    );
    insta::assert_snapshot!(
        "account_5_archived",
        rendered(&account_page(old_savings(true), None))
    );
    let refusal = AccountError::BalanceNotZero {
        kind: Kind::Asset,
        account: String::from("Assets:Checking"),
        balance: Money::from_minor(248_015),
        digits: MinorDigits::TWO,
    };
    insta::assert_snapshot!(
        "account_6_archive_refused_balance",
        rendered(&account_page(checking(), alert(refusal)))
    );
    let refusal = AccountError::ChildrenNotArchived {
        account: String::from("Assets:Savings"),
        child: String::from("Assets:Savings:Emergency fund"),
    };
    insta::assert_snapshot!(
        "account_7_archive_refused_inside",
        rendered(&account_page(savings(), alert(refusal)))
    );
    let refusal = AccountError::ParentArchivedUnarchiveFirst {
        account: String::from("Assets:Old savings:Jar"),
        parent: String::from("Assets:Old savings"),
    };
    insta::assert_snapshot!(
        "account_8_unarchive_refused",
        rendered(&account_page(jar(), alert(refusal)))
    );
    insta::assert_snapshot!(
        "account_9_changed",
        rendered(&account_page(
            savings(),
            status(Notice::AccountChanged {
                word: Word::Account
            })
        ))
    );
    insta::assert_snapshot!(
        "account_10_archived_now",
        rendered(&account_page(
            old_savings(true),
            status(Notice::AccountArchived {
                word: Word::Account
            })
        ))
    );
    insta::assert_snapshot!(
        "account_11_unarchived_now",
        rendered(&account_page(
            old_savings(false),
            status(Notice::AccountUnarchived {
                word: Word::Account
            })
        ))
    );
    insta::assert_snapshot!(
        "account_12_inside_archived",
        rendered(&account_page(savings_with_an_archived_jar(), None))
    );
    insta::assert_snapshot!(
        "account_13_valuation_changes",
        rendered(&account_page(built_in(SystemKey::ValuationChanges), None))
    );
    insta::assert_snapshot!(
        "account_14_uncategorized",
        rendered(&account_page(built_in(SystemKey::Uncategorized), None))
    );
    insta::assert_snapshot!(
        "account_15_archived_category",
        rendered(&account_page(old_gym(), None))
    );

    for (archived, expected) in [
        (
            false,
            r#"<a class="button outline" href="/accounts/{id}/archive">Archive</a>"#,
        ),
        (
            true,
            r#"<a class="button" href="/accounts/{id}/unarchive">Unarchive</a>"#,
        ),
    ] {
        let view = old_savings(archived);
        let expected = expected.replace("{id}", &view.id.to_string());
        let html = html_of(&account_page(view, None));
        assert!(
            html.contains(&expected),
            "the account's page does not lead to its confirmation, archived {archived}:\n{html}"
        );
    }

    for (page, html) in every_account_page() {
        assert!(
            main_buttons(&html) <= 1,
            "{page} has more than one main button:\n{html}"
        );
        if !page.starts_with("the list") {
            assert!(
                html.contains(r#"<nav class="trail" aria-label="Trail"><ol><li><a href="/accounts">Accounts</a></li>"#),
                "the trail of {page} does not begin with a link to Accounts:\n{html}"
            );
        }
    }
}

/// How many main (yellow) buttons the page's main content holds: a link styled as a button
/// without `outline`, and a button without `outline`.
fn main_buttons(html: &str) -> usize {
    let main = html.find("<main").map_or(html, |at| &html[at..]);
    main.split('<')
        .skip(1)
        .filter(|chunk| {
            let tag = chunk.split('>').next().unwrap_or_default();
            (tag.starts_with("a ") && tag.contains(r#"class="button""#))
                || (tag.starts_with("button") && !tag.contains("outline"))
        })
        .count()
}

/// The change form as text: Savings with Name, Inside and Owner and the help line under
/// Inside; the category Groceries with Name and Inside; the built-in Opening Balances and the
/// archived Old savings, each headed Change name with the name alone; Savings refused for a
/// name too long, what was typed kept; Checking refused in two boxes at once, the place and the
/// owner the lists no longer offer kept and marked; and the clash page on the name alone and
/// on the place and the owner, with what is saved in the clashing boxes only and the person's
/// own boxes as typed.
///
/// Proves: Decision 134 with P2 to P5: the change form of an account, a category, a built-in and an archived account, refused in one and in two boxes with kept choices marked, and the clash page on one and two boxes.
#[test]
fn the_change_form_in_each_state() {
    insta::assert_snapshot!(
        "account_edit_1_form",
        rendered(&edit_page(
            savings_form(),
            typed_from(&savings_form()),
            None,
            AccountProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "account_edit_2_category",
        rendered(&edit_page(
            groceries_form(),
            typed_from(&groceries_form()),
            None,
            AccountProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "account_edit_3_built_in",
        rendered(&edit_page(
            opening_balances_form(),
            typed_from(&opening_balances_form()),
            None,
            AccountProblems::default()
        ))
    );
    let typed = AccountTyped {
        name: String::from(
            "Savings for the roof, the car, the garden and the summer holidays, 2027",
        ),
        ..typed_from(&savings_form())
    };
    let problems = AccountProblems {
        name: Some(
            AccountError::Name {
                refused: refused_on("Assets:Savings", Action::Renamed),
                error: name_error(&typed.name),
            }
            .to_string(),
        ),
        ..AccountProblems::default()
    };
    let notice = alert(Refusal::AccountNotSaved {
        word: Word::Account,
    });
    insta::assert_snapshot!(
        "account_edit_4_refused",
        rendered(&edit_page(savings_form(), typed, notice, problems))
    );
    insta::assert_snapshot!("account_edit_5_clash", rendered(&clash_on_the_name()));
    insta::assert_snapshot!(
        "account_edit_6_refused_two_boxes",
        rendered(&edit_page_refused_in_two_boxes())
    );
    insta::assert_snapshot!(
        "account_edit_7_clash_two_boxes",
        rendered(&clash_page_on_place_and_owner())
    );
    insta::assert_snapshot!(
        "account_edit_8_archived",
        rendered(&edit_page(
            old_savings_form(true),
            typed_from(&old_savings_form(true)),
            None,
            AccountProblems::default()
        ))
    );
}

/// The confirmations as text: Archive of Old savings and Unarchive of the archived Old
/// savings, each with its trail ending in Archive or Unarchive, its question, its place, its
/// sentence and its two buttons; in the HTML the main button comes first, before the way back.
///
/// Proves: Decision 135 with P10: the confirmation pages of Archive and Unarchive with their trail, sentence and two buttons, the main button first in the HTML.
#[test]
fn the_confirmations_of_archive_and_unarchive() {
    insta::assert_snapshot!(
        "account_archive_1_confirm",
        rendered(&confirm_page(old_savings(false), true))
    );
    insta::assert_snapshot!(
        "account_unarchive_1_confirm",
        rendered(&confirm_page(old_savings(true), false))
    );
    for (page, html, main, back) in [
        (
            "the confirmation of Archive",
            html_of(&confirm_page(old_savings(false), true)),
            r#"<button type="submit">Archive</button>"#,
            ">Keep it</a>",
        ),
        (
            "the confirmation of Unarchive",
            html_of(&confirm_page(old_savings(true), false)),
            r#"<button type="submit">Unarchive</button>"#,
            ">Keep it archived</a>",
        ),
    ] {
        let main_at = html.find(main);
        let back_at = html.find(back);
        assert!(
            main_at.is_some() && back_at.is_some() && main_at < back_at,
            "the main button does not come first on {page}:\n{html}"
        );
        assert_eq!(
            html.matches(r#"<form class="bare" method="post""#).count(),
            1,
            "the forms that act on {page}"
        );
    }
}

/// The boxes of the account forms, by name.
const ACCOUNT_BOXES: [&str; 3] = ["name", "inside", "owner"];

/// The create form under Assets and the change form of Savings, with a problem under no box,
/// under each box alone, under Name and Inside at once and under every box: `aria-invalid`
/// marks exactly the boxes with a problem, and each box stands in its field wrapper with its
/// label, the box, the help line under Inside on the change form and its problem line. The
/// name-only form of a built-in account marks its one box; the clash page marks none.
///
/// Proves: Decisions 123 and 132 with P3: on the account forms each box stands in its field wrapper and aria-invalid marks exactly the boxes with a problem, two at once included.
#[test]
fn the_account_forms_mark_exactly_the_boxes_with_a_problem() {
    let problem = || Some(String::from("This box has a problem."));
    let cases = [
        ("no problem", AccountProblems::default(), Vec::new()),
        (
            "a problem under the name alone",
            AccountProblems {
                name: problem(),
                ..AccountProblems::default()
            },
            vec!["name"],
        ),
        (
            "a problem under Inside alone",
            AccountProblems {
                inside: problem(),
                ..AccountProblems::default()
            },
            vec!["inside"],
        ),
        (
            "a problem under Owner alone",
            AccountProblems {
                owner: problem(),
                ..AccountProblems::default()
            },
            vec!["owner"],
        ),
        (
            "problems under the name and Inside",
            AccountProblems {
                name: problem(),
                inside: problem(),
                owner: None,
            },
            vec!["name", "inside"],
        ),
        (
            "a problem under every box",
            AccountProblems {
                name: problem(),
                inside: problem(),
                owner: problem(),
            },
            ACCOUNT_BOXES.to_vec(),
        ),
    ];
    for (state, problems, expected) in cases {
        let html = html_of(&new_page(
            assets_form(),
            AccountTyped::default(),
            alert(Refusal::AccountNotCreated {
                word: Word::Account,
            }),
            problems.clone(),
        ));
        assert_eq!(
            marked_invalid(&html, &ACCOUNT_BOXES),
            expected,
            "aria-invalid is not on exactly the boxes with a problem: the create form with {state}"
        );
        assert_fields(
            &html,
            &format!("the create form with {state}"),
            &[
                ("name", false, problems.name.is_some()),
                ("inside", false, problems.inside.is_some()),
                ("owner", false, problems.owner.is_some()),
            ],
        );
        let html = html_of(&edit_page(
            savings_form(),
            typed_from(&savings_form()),
            alert(Refusal::AccountNotSaved {
                word: Word::Account,
            }),
            problems.clone(),
        ));
        assert_eq!(
            marked_invalid(&html, &ACCOUNT_BOXES),
            expected,
            "aria-invalid is not on exactly the boxes with a problem: the change form with {state}"
        );
        assert_fields(
            &html,
            &format!("the change form with {state}"),
            &[
                ("name", false, problems.name.is_some()),
                ("inside", true, problems.inside.is_some()),
                ("owner", false, problems.owner.is_some()),
            ],
        );
    }
    let html = html_of(&edit_page(
        opening_balances_form(),
        typed_from(&opening_balances_form()),
        None,
        AccountProblems {
            name: problem(),
            ..AccountProblems::default()
        },
    ));
    assert_eq!(
        marked_invalid(&html, &["name"]),
        ["name"],
        "aria-invalid is not on exactly the boxes with a problem: the name-only form"
    );
    assert!(
        !html.contains(r#"name="inside""#) && !html.contains(r#"name="owner""#),
        "the name-only form holds another box:\n{html}"
    );
    assert_fields(&html, "the name-only form", &[("name", false, true)]);
    let html = html_of(&clash_page_on_place_and_owner());
    assert_eq!(
        marked_invalid(&html, &ACCOUNT_BOXES),
        Vec::<&str>::new(),
        "aria-invalid is not on exactly the boxes with a problem: the clash page"
    );
    assert_fields(
        &html,
        "the clash page",
        &[
            ("name", false, false),
            ("inside", true, false),
            ("owner", false, false),
        ],
    );
}

/// Every account page in its states, by name, as HTML.
fn every_account_page() -> Vec<(&'static str, String)> {
    vec![
        ("the list", html_of(&accounts_page(household_chart(true)))),
        (
            "the list with no accounts",
            html_of(&accounts_page(declined_chart())),
        ),
        (
            "the page What do you want to add?",
            html_of(&NewWhatPage::new(accounts_layout(None))),
        ),
        (
            "the create form",
            html_of(&new_page(
                assets_form(),
                AccountTyped::default(),
                None,
                AccountProblems::default(),
            )),
        ),
        (
            "the create form opened inside Savings",
            html_of(&new_page_inside_savings()),
        ),
        (
            "an account's page",
            html_of(&account_page(
                savings(),
                status(Notice::AccountChanged {
                    word: Word::Account,
                }),
            )),
        ),
        (
            "a category's page",
            html_of(&account_page(groceries(), None)),
        ),
        (
            "a built-in account's page",
            html_of(&account_page(opening_balances(), None)),
        ),
        (
            "an archived account's page",
            html_of(&account_page(old_savings(true), None)),
        ),
        (
            "the change form",
            html_of(&edit_page(
                savings_form(),
                typed_from(&savings_form()),
                None,
                AccountProblems::default(),
            )),
        ),
        (
            "the change form refused in two boxes",
            html_of(&edit_page_refused_in_two_boxes()),
        ),
        (
            "the name-only form",
            html_of(&edit_page(
                old_savings_form(true),
                typed_from(&old_savings_form(true)),
                None,
                AccountProblems::default(),
            )),
        ),
        ("the clash page", html_of(&clash_page_on_place_and_owner())),
        (
            "the confirmation of Archive",
            html_of(&confirm_page(old_savings(false), true)),
        ),
        (
            "the confirmation of Unarchive",
            html_of(&confirm_page(old_savings(true), false)),
        ),
    ]
}

/// Every account page in every state above carries no htmx attribute, no inline script, no
/// inline style and no event handler: every script is an empty element with a source, and no
/// start tag has an attribute that begins `hx-`, `style` or `on`.
///
/// Proves: Decision 116 and web rule 3 with P12: no account page in any state carries an htmx attribute, an inline script or an inline style.
#[test]
fn the_account_pages_carry_no_htmx_attribute_and_no_inline_code() {
    for (page, html) in every_account_page() {
        assert!(
            !html.contains("<style"),
            "{page} carries a style element:\n{html}"
        );
        for (at, _) in html.match_indices("<script") {
            let rest = &html[at..];
            let tag_end = rest.find('>').map_or(rest.len(), |index| index + 1);
            assert!(
                rest[..tag_end].contains(" src=") && rest[tag_end..].starts_with("</script>"),
                "{page} carries an inline script:\n{html}"
            );
        }
        for chunk in html.split('<').skip(1) {
            if chunk.starts_with('/') || chunk.starts_with('!') {
                continue;
            }
            let tag = chunk.split('>').next().unwrap_or_default();
            assert!(
                !tag.contains(" hx-") && !tag.contains(" data-hx-"),
                "{page} carries an htmx attribute: <{tag}>"
            );
            assert!(
                !tag.contains(" style="),
                "{page} carries an inline style: <{tag}>"
            );
            for (at, _) in tag.match_indices(" on") {
                let rest = &tag[at + 3..];
                let letters = rest.chars().take_while(char::is_ascii_alphabetic).count();
                assert!(
                    letters == 0 || !rest[letters..].starts_with('='),
                    "{page} carries an event handler: <{tag}>"
                );
            }
        }
    }
}

/// A name that holds markup and quotes is written as text on the account pages: in the heading
/// and the trail of an account's page, in the trail, the Inside list and the hidden shown name
/// of a change form, in the question of the confirmation of Archive, and in a line of the list;
/// the page as text shows the name as it was typed.
///
/// Proves: Web rule 5 with P12: a name holding markup and quotes is escaped in the heading, the trail, the Inside list, the hidden shown name and Archive name.
#[test]
fn a_name_with_markup_is_written_as_text_on_the_account_pages() {
    let name = r#"<b>"Tom's" & co</b>"#;
    let escaped = "&#60;b&#62;&#34;Tom&#39;s&#34; &#38; co&#60;/b&#62;";
    let marked = AccountView {
        name: name.to_owned(),
        place: format!("Assets:{name}:{name}"),
        trail: vec![step(12, name)],
        ..view(13, "x", "x", Kind::Asset)
    };
    let page = account_page(marked.clone(), None);
    let html = html_of(&page);
    for (what, expected) in [
        ("the heading", format!("<h1>{escaped}</h1>")),
        (
            "the trail's link",
            format!(r#"<a href="/accounts/12">{escaped}</a>"#),
        ),
        (
            "the trail's last step",
            format!(r#"<span aria-current="page">{escaped}</span>"#),
        ),
    ] {
        assert!(
            html.contains(&expected),
            "{what} of an account's page does not write the name as text:\n{html}"
        );
    }
    let text = rendered(&page);
    assert!(
        text.contains(&format!("Trail: Accounts > {name} > {name}"))
            && text.contains(&format!("# {name}")),
        "the account's page as text does not show the name as typed:\n{text}"
    );

    let mut form = EditForm {
        trail: vec![step(12, name)],
        places: Some(vec![place(12, &format!("Assets:{name}"))]),
        ..edit_form(13, name, &format!("Assets:{name}:{name}"), Kind::Asset)
    };
    form.saved.parent = Some(AccountId(12));
    let page = edit_page(
        form.clone(),
        typed_from(&form),
        None,
        AccountProblems::default(),
    );
    let html = html_of(&page);
    for (what, expected) in [
        (
            "the trail's link",
            format!(r#"<a href="/accounts/13">{escaped}</a>"#),
        ),
        (
            "the Inside list",
            format!(r#"<option value="12" selected>Assets:{escaped}</option>"#),
        ),
        (
            "the hidden shown name",
            format!(r#"<input type="hidden" name="shown_name" value="{escaped}">"#),
        ),
        ("the name box", format!(r#"name="name" value="{escaped}""#)),
    ] {
        assert!(
            html.contains(&expected),
            "{what} of the change form does not write the name as text:\n{html}"
        );
    }
    let text = rendered(&page);
    assert!(
        text.contains(&format!("[ Assets:{name} v ]")),
        "the change form as text does not show the name as typed:\n{text}"
    );

    let html = html_of(&confirm_page(marked, true));
    assert!(
        html.contains(&format!("Archive {escaped}?</h1>")),
        "the question of the confirmation does not write the name as text:\n{html}"
    );

    let mut chart = household_chart(false);
    if let Some(ChartLine::Account(row)) = chart.assets.rows.first_mut() {
        row.name = name.to_owned();
    }
    let html = html_of(&accounts_page(chart));
    assert!(
        html.contains(&format!(r#"<span class="name">{escaped}</span>"#))
            && !html.contains("<b>")
            && !html.contains("</b>"),
        "a line of the list does not write the name as text:\n{html}"
    );
    for (page, html) in [
        (
            "an account's page",
            html_of(&account_page(
                AccountView {
                    name: name.to_owned(),
                    ..view(13, "x", "x", Kind::Asset)
                },
                None,
            )),
        ),
        ("the change form", html_of(&page)),
    ] {
        assert!(
            !html.contains("<b>") && !html.contains("</b>"),
            "{page} holds the name's markup as markup:\n{html}"
        );
    }
}

/// The date `text`, written as Nage writes dates.
fn day(text: &str) -> Date {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is no date: {error}"))
}

/// Today on the opening-balance pages: 2026-11-16.
fn opening_today() -> Date {
    day("2026-11-16")
}

/// The account `id` called `name` at `place` under the section of `kind`, as the
/// opening-balance pages read it, directly under its section, in a book with two minor digits.
fn opening_account(id: i64, name: &str, place: &str, kind: Kind) -> OpeningAccount {
    OpeningAccount {
        id: AccountId(id),
        name: name.to_owned(),
        place: place.to_owned(),
        kind,
        trail: Vec::new(),
        minor_digits: MinorDigits::TWO,
    }
}

/// Checking under Assets.
fn checking_account() -> OpeningAccount {
    opening_account(11, "Checking", "Assets:Checking", Kind::Asset)
}

/// Savings under Assets.
fn savings_account() -> OpeningAccount {
    opening_account(12, "Savings", "Assets:Savings", Kind::Asset)
}

/// Cash under Assets.
fn cash_account() -> OpeningAccount {
    opening_account(10, "Cash", "Assets:Cash", Kind::Asset)
}

/// The Travel card under Liabilities.
fn card_account() -> OpeningAccount {
    opening_account(
        31,
        "Travel card",
        "Liabilities:Travel card",
        Kind::Liability,
    )
}

/// The Travel card under Liabilities, as the empty form names it.
fn travel_card_account() -> OpeningAccount {
    card_account()
}

/// The opening balance `id` of the account `account`: `minor` minor units in natural sign on
/// `on`, at `version`.
fn opening_entry(id: i64, account: i64, on: &str, minor: i64, version: i64) -> OpeningBalance {
    OpeningBalance {
        id: TransactionId(id),
        account: AccountId(account),
        date: day(on),
        amount: Money::from_minor(minor),
        version,
    }
}

/// Checking's opening balance: 2,480.15 on 2026-09-30.
fn checking_opening_balance() -> OpeningBalance {
    opening_entry(40, 11, "2026-09-30", 248_015, 1)
}

/// Cash's opening balance: 40.00 on 2026-09-30.
fn cash_opening_balance() -> OpeningBalance {
    opening_entry(41, 10, "2026-09-30", 4_000, 1)
}

/// The Travel card's opening balance: 1,204.80 owed on 2026-09-30.
fn card_opening_balance() -> OpeningBalance {
    opening_entry(42, 31, "2026-09-30", 120_480, 1)
}

/// The entry and version the change form of Checking names.
fn checking_entry() -> (TransactionId, i64) {
    (TransactionId(40), 1)
}

/// The entry and version the change form of an overdrawn Checking names.
fn overdrawn_entry() -> (TransactionId, i64) {
    (TransactionId(43), 2)
}

/// The boxes holding `date` and `amount`, the tick box ticked when `below_zero`.
fn opening_typed(date: &str, amount: &str, below_zero: bool) -> OpeningTyped {
    OpeningTyped {
        date: date.to_owned(),
        amount: amount.to_owned(),
        below_zero,
    }
}

/// The opening-balance form of `account` naming `entry`, holding `typed` with `problems` under
/// its boxes and `message` at the top, today 2026-11-16.
fn opening_page(
    account: OpeningAccount,
    message: Option<Message>,
    entry: Option<(TransactionId, i64)>,
    typed: OpeningTyped,
    problems: OpeningProblems,
) -> OpeningBalancePage {
    OpeningBalancePage::new(
        accounts_layout(message),
        account,
        entry,
        typed,
        problems,
        opening_today(),
    )
}

/// The clash page of `account` for `clash`, holding `typed`, its sentence at the top.
fn opening_clash_page(
    account: OpeningAccount,
    clash: OpeningClash,
    typed: OpeningTyped,
) -> OpeningBalanceClashPage {
    OpeningBalanceClashPage::new(
        accounts_layout(alert(&clash)),
        account,
        clash,
        typed,
        opening_today(),
    )
}

/// The Remove confirmation of `account` for `saved`, after a typed zero when `typed_zero`,
/// with `message` at the top.
fn remove_page(
    account: OpeningAccount,
    saved: OpeningBalance,
    typed_zero: bool,
    message: Option<Message>,
) -> OpeningBalanceRemovePage {
    OpeningBalanceRemovePage::new(accounts_layout(message), account, saved, typed_zero)
}

/// The problem `refusal` under its box, in the core's words.
fn problem(refusal: OpeningBoxRefusal) -> Option<String> {
    Some(refusal.to_string())
}

/// The opening-balance form as text in each state: empty under Assets and under Liabilities;
/// refused in both boxes, after today, over the limit and for two signs, what was typed and the
/// tick kept; the change form with Remove; and the change form of an account saved below zero,
/// its amount without the minus and its box ticked.
///
/// Proves: Decisions 140, 141 and 145 with the pause's P9: the form under Assets and under Liabilities, refused in both boxes, after today, over the limit and for two signs with the tick kept, the change form with Remove, and the change form of an account saved below zero, ticked and without its minus, as page-text snapshots.
#[test]
fn the_opening_balance_form_in_each_state() {
    let refused = || alert(Refusal::OpeningBalanceNotSaved);
    insta::assert_snapshot!(
        "opening_balance_1_form_assets",
        rendered(&opening_page(
            checking_account(),
            None,
            None,
            OpeningTyped::default(),
            OpeningProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_2_form_liabilities",
        rendered(&opening_page(
            travel_card_account(),
            None,
            None,
            OpeningTyped::default(),
            OpeningProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_3_refused_every_box",
        rendered(&opening_page(
            checking_account(),
            refused(),
            None,
            opening_typed("", "1.234,56", false),
            OpeningProblems {
                date: problem(OpeningBoxRefusal::Date(DateError::Empty)),
                amount: problem(OpeningBoxRefusal::Amount {
                    error: ParseMoneyError::MisplacedComma,
                    digits: MinorDigits::TWO,
                }),
            }
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_4_refused_after_today",
        rendered(&opening_page(
            checking_account(),
            refused(),
            None,
            opening_typed("2026-11-17", "2,480.15", false),
            OpeningProblems {
                date: problem(OpeningBoxRefusal::DateAfterToday {
                    today: opening_today()
                }),
                amount: None,
            }
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_5_refused_too_large",
        rendered(&opening_page(
            savings_account(),
            refused(),
            None,
            opening_typed("2026-09-30", "50,000,000,000,000,000.00", false),
            OpeningProblems {
                date: None,
                amount: problem(OpeningBoxRefusal::TooLarge {
                    digits: MinorDigits::TWO
                }),
            }
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_7_change",
        rendered(&opening_page(
            checking_account(),
            None,
            Some(checking_entry()),
            opening_typed("2026-09-30", "2,480.15", false),
            OpeningProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_11_change_overdrawn",
        rendered(&opening_page(
            checking_account(),
            None,
            Some(overdrawn_entry()),
            opening_typed("2026-09-30", "35.20", true),
            OpeningProblems::default()
        ))
    );
    insta::assert_snapshot!(
        "opening_balance_12_refused_two_signs",
        rendered(&opening_page(
            checking_account(),
            refused(),
            None,
            opening_typed("2026-09-30", "-35.20", true),
            OpeningProblems {
                date: None,
                amount: problem(OpeningBoxRefusal::TwoSigns),
            }
        ))
    );
}

/// The three clash pages of an opening balance as text, each headed by what the person set out
/// to do: changed meanwhile and removed meanwhile (Change opening balance), entered meanwhile
/// (Opening balance), and a card's changed-meanwhile page with Amount owed; each with its
/// sentence at the top, Saved now, Your change as sent and its two buttons. Save mine names the
/// entry and the version saved now; after a removal the form names none.
///
/// Proves: Decision 144 with the pause's P9: the three clash pages, changed, removed and entered meanwhile, each headed by what the person set out to do, and a card's changed-meanwhile page with Amount owed, with the sentence, Saved now, Your change and their two buttons, as page-text snapshots.
#[test]
fn the_clash_pages_of_an_opening_balance() {
    let changed = OpeningClash::Changed {
        account: String::from("Assets:Checking"),
        saved: opening_entry(40, 11, "2026-09-30", 250_000, 2),
        digits: MinorDigits::TWO,
    };
    let page = opening_clash_page(
        checking_account(),
        changed,
        opening_typed("2026-09-29", "2,480.15", false),
    );
    let html = html_of(&page);
    assert!(
        html.contains(r#"<input type="hidden" name="entry" value="40"><input type="hidden" name="version" value="2">"#),
        "Save mine does not name the entry and the version saved now:\n{html}"
    );
    insta::assert_snapshot!("opening_balance_8_clash", rendered(&page));

    let removed = OpeningClash::Removed {
        account: String::from("Assets:Checking"),
    };
    let page = opening_clash_page(
        checking_account(),
        removed,
        opening_typed("2026-09-30", "2,500.00", false),
    );
    let html = html_of(&page);
    assert!(
        !html.contains(r#"name="entry""#) && !html.contains(r#"name="version""#),
        "Save as a new opening balance names an entry:\n{html}"
    );
    insta::assert_snapshot!("opening_balance_9_removed_meanwhile", rendered(&page));

    let entered = OpeningClash::Entered {
        account: String::from("Assets:Checking"),
        saved: checking_opening_balance(),
        digits: MinorDigits::TWO,
    };
    insta::assert_snapshot!(
        "opening_balance_10_created_meanwhile",
        rendered(&opening_clash_page(
            checking_account(),
            entered,
            opening_typed("2026-09-30", "2,450.00", false)
        ))
    );

    let card = OpeningClash::Changed {
        account: String::from("Liabilities:Travel card"),
        saved: opening_entry(42, 31, "2026-09-30", 125_000, 2),
        digits: MinorDigits::TWO,
    };
    insta::assert_snapshot!(
        "opening_balance_13_clash_card",
        rendered(&opening_clash_page(
            card_account(),
            card,
            opening_typed("2026-09-30", "1,204.80", false)
        ))
    );
}

/// The Remove confirmation as text: from Remove; after a typed zero, with its line; after a
/// removal that met a change, with the figure as it is now and the sentence at the top; and a
/// card's. Its Remove names the entry and the version shown, and stands before Keep it in the
/// HTML, the computer's order.
///
/// Proves: Decision 143 with the pause's P9: the confirmation from Remove, the one a typed zero leads to with its line, the one after a change met with the figure as it is now, and a card's, the main button first in the HTML.
#[test]
fn the_remove_confirmation_in_each_state() {
    let page = remove_page(checking_account(), checking_opening_balance(), false, None);
    let html = html_of(&page);
    let remove = html.find("Remove opening balance</button>");
    let keep = html.find(">Keep it</a>");
    assert!(
        remove.is_some() && keep.is_some() && remove < keep,
        "Remove does not stand before Keep it:\n{html}"
    );
    assert!(
        html.contains(r#"<input type="hidden" name="entry" value="40">"#)
            && html.contains(r#"<input type="hidden" name="version" value="1">"#),
        "Remove does not name the entry and the version shown:\n{html}"
    );
    insta::assert_snapshot!("opening_balance_remove_1_confirm", rendered(&page));
    insta::assert_snapshot!(
        "opening_balance_6_zero",
        rendered(&remove_page(
            cash_account(),
            cash_opening_balance(),
            true,
            None
        ))
    );
    let changed = opening_entry(40, 11, "2026-09-30", 250_000, 2);
    let clash = OpeningClash::RemoveChanged {
        account: String::from("Assets:Checking"),
        saved: changed,
        digits: MinorDigits::TWO,
    };
    let page = remove_page(checking_account(), changed, false, alert(clash));
    assert!(
        html_of(&page).contains(r#"<input type="hidden" name="version" value="2">"#),
        "Remove after a change met does not name the version saved now"
    );
    insta::assert_snapshot!("opening_balance_remove_2_clash", rendered(&page));
    insta::assert_snapshot!(
        "opening_balance_remove_3_card",
        rendered(&remove_page(
            card_account(),
            card_opening_balance(),
            false,
            None
        ))
    );
}

/// Checking with no opening balance any more, at zero.
fn checking_without_opening_balance() -> AccountView {
    AccountView {
        balance: Some(Money::ZERO),
        opening_balance: Some(OpeningFact::NoneYet),
        ..checking()
    }
}

/// The Travel card, joint, owing 1,204.80, all of it its opening balance.
fn travel_card() -> AccountView {
    AccountView {
        balance: Some(Money::from_minor(120_480)),
        opening_balance: opening_fact(120_480),
        ..view(
            31,
            "Travel card",
            "Liabilities:Travel card",
            Kind::Liability,
        )
    }
}

/// Checking overdrawn by 35.20 from its opening balance.
fn overdrawn_checking() -> AccountView {
    AccountView {
        balance: Some(Money::from_minor(-3_520)),
        opening_balance: opening_fact(-3_520),
        ..checking()
    }
}

/// Checking archived at zero: no fact and no action of the opening balance.
fn archived_checking() -> AccountView {
    AccountView {
        balance: Some(Money::ZERO),
        archived: true,
        can_hold: false,
        opening_balance: None,
        ..checking()
    }
}

/// An account's page with its opening balance as text: after a save and after a removal with
/// their lines; a card's with what was owed; an overdrawn account's fact with its minus; and an
/// archived account's page with the archived-meanwhile sentence at its top and neither the fact
/// nor its action. Enter opening balance is the yellow button, first among the actions, only
/// while there is none; Change opening balance stands outlined.
///
/// Proves: Decisions 140, 141 and 146 with the pause's P9: an account's page after a save and after a removal with their lines, a card's page with what was owed, an overdrawn account's fact with its minus, and an archived account's page with the archived-meanwhile sentence at its top; the fact, and the yellow Enter opening balance first only while there is none.
#[test]
fn an_accounts_page_with_its_opening_balance() {
    let saved = account_page(checking(), status(Notice::OpeningBalanceSaved));
    let html = html_of(&saved);
    assert!(
        html.contains(r#"<a class="button outline" href="/accounts/11/opening-balance">Change opening balance</a>"#)
            && !html.contains("Enter opening balance"),
        "the page with an opening balance does not offer Change opening balance outlined:\n{html}"
    );
    insta::assert_snapshot!("account_16_opening_balance_saved", rendered(&saved));

    let removed = account_page(
        checking_without_opening_balance(),
        status(Notice::OpeningBalanceRemoved),
    );
    let html = html_of(&removed);
    let actions = html
        .split(r#"<div class="actions">"#)
        .nth(1)
        .unwrap_or_default();
    assert!(
        actions.trim_start().starts_with(
            r#"<a class="button" href="/accounts/11/opening-balance">Enter opening balance</a>"#
        ) && !html.contains("Change opening balance"),
        "Enter opening balance is not the yellow button first among the actions:\n{html}"
    );
    insta::assert_snapshot!("account_17_opening_balance_removed", rendered(&removed));
    insta::assert_snapshot!(
        "account_18_card_with_opening_balance",
        rendered(&account_page(travel_card(), None))
    );
    insta::assert_snapshot!(
        "account_19_overdrawn",
        rendered(&account_page(overdrawn_checking(), None))
    );
    let archived_meanwhile = OpeningBalanceError::ArchivedMeanwhile {
        account: String::from("Assets:Checking"),
    };
    let archived = account_page(archived_checking(), alert(archived_meanwhile));
    let html = html_of(&archived);
    assert!(
        !html.contains("opening-balance") && !html.contains("Opening balance"),
        "an archived account's page shows the opening balance or its action:\n{html}"
    );
    insta::assert_snapshot!("account_20_archived_meanwhile", rendered(&archived));
}

/// Every opening-balance page in every state, as `(what, HTML)`, for an account called `name`.
fn every_opening_balance_page(name: &str) -> Vec<(String, String)> {
    let account = || OpeningAccount {
        name: name.to_owned(),
        place: format!("Assets:{name}"),
        ..checking_account()
    };
    let saved = || opening_entry(40, 11, "2026-09-30", 248_015, 1);
    let typed = || opening_typed("2026-09-30", "2,480.15", false);
    let both = OpeningProblems {
        date: problem(OpeningBoxRefusal::Date(DateError::Empty)),
        amount: problem(OpeningBoxRefusal::Zero),
    };
    let place = format!("Assets:{name}");
    vec![
        (
            String::from("the enter form"),
            html_of(&opening_page(
                account(),
                None,
                None,
                OpeningTyped::default(),
                OpeningProblems::default(),
            )),
        ),
        (
            String::from("the refused form"),
            html_of(&opening_page(
                account(),
                alert(Refusal::OpeningBalanceNotSaved),
                None,
                opening_typed(name, name, true),
                both,
            )),
        ),
        (
            String::from("the change form"),
            html_of(&opening_page(
                account(),
                None,
                Some(checking_entry()),
                typed(),
                OpeningProblems::default(),
            )),
        ),
        (
            String::from("the changed-meanwhile page"),
            html_of(&opening_clash_page(
                account(),
                OpeningClash::Changed {
                    account: place.clone(),
                    saved: saved(),
                    digits: MinorDigits::TWO,
                },
                typed(),
            )),
        ),
        (
            String::from("the removed-meanwhile page"),
            html_of(&opening_clash_page(
                account(),
                OpeningClash::Removed {
                    account: place.clone(),
                },
                typed(),
            )),
        ),
        (
            String::from("the entered-meanwhile page"),
            html_of(&opening_clash_page(
                account(),
                OpeningClash::Entered {
                    account: place.clone(),
                    saved: saved(),
                    digits: MinorDigits::TWO,
                },
                typed(),
            )),
        ),
        (
            String::from("the confirmation"),
            html_of(&remove_page(account(), saved(), false, None)),
        ),
        (
            String::from("the confirmation after a typed zero"),
            html_of(&remove_page(account(), saved(), true, None)),
        ),
        (
            String::from("the confirmation after a change met"),
            html_of(&remove_page(
                account(),
                saved(),
                false,
                alert(OpeningClash::RemoveChanged {
                    account: place,
                    saved: saved(),
                    digits: MinorDigits::TWO,
                }),
            )),
        ),
    ]
}

/// The start tag of the box called `name` in `html`.
fn box_tag<'a>(html: &'a str, name: &str) -> &'a str {
    let needle = format!(r#"name="{name}""#);
    let at = html
        .find(&needle)
        .unwrap_or_else(|| panic!("no box called {name}:\n{html}"));
    let start = html[..at]
        .rfind('<')
        .unwrap_or_else(|| panic!("the box {name} has no start tag:\n{html}"));
    let end = html[at..].find('>').map_or(html.len(), |index| index + at);
    &html[start..=end]
}

/// The form that enters or changes an opening balance and the three clash pages carry
/// `novalidate`, so that no browser refuses a date itself, while the date box keeps 1900-01-01
/// and today as hints for its picker.
///
/// Proves: A8 with the pause's P4: the form that enters or changes one and the three clash pages carry novalidate, so no browser refuses a date itself, the date box keeping 1900-01-01 and today as hints.
#[test]
fn the_opening_balance_forms_carry_novalidate() {
    for (what, html) in every_opening_balance_page("Checking") {
        if what.starts_with("the confirmation") {
            continue;
        }
        let form = html
            .find(r#"action="/accounts/11/opening-balance""#)
            .and_then(|at| {
                let start = html[..at].rfind('<')?;
                let end = html[at..].find('>')? + at;
                Some(&html[start..=end])
            })
            .unwrap_or_default();
        assert!(
            form.starts_with("<form") && form.contains(" novalidate"),
            "{what} does not carry novalidate: {form}"
        );
        let date = box_tag(&html, "date");
        assert!(
            date.contains(r#"type="date""#)
                && date.contains(r#"min="1900-01-01""#)
                && date.contains(r#"max="2026-11-16""#),
            "the date box of {what} does not keep its hints: {date}"
        );
    }
}

/// Each box of the form stands in its field wrapper, `aria-invalid` marks exactly the boxes
/// with a problem, one or both, the tick box is ticked as sent, and the date box keeps its
/// hints.
///
/// Proves: Decisions 123 and 145: each box in its field wrapper, aria-invalid on exactly the boxes with a problem, one or both, the tick box ticked as sent, the date box's hints 1900-01-01 and today.
#[test]
fn the_opening_balance_form_marks_exactly_the_boxes_with_a_problem() {
    let date_problem = || problem(OpeningBoxRefusal::Date(DateError::Empty));
    let amount_problem = || problem(OpeningBoxRefusal::TwoSigns);
    for (date, amount, below_zero) in [
        (None, None, false),
        (date_problem(), None, true),
        (None, amount_problem(), false),
        (date_problem(), amount_problem(), true),
    ] {
        let marked = (date.is_some(), amount.is_some());
        let html = html_of(&opening_page(
            checking_account(),
            None,
            None,
            opening_typed("", "-1", below_zero),
            OpeningProblems { date, amount },
        ));
        assert!(
            html.contains("<div class=\"field\">\n<label for=\"date\">Date</label>\n<input type=\"date\" id=\"date\" name=\"date\"")
                && html.contains("<div class=\"field\">\n<label for=\"amount\">Balance</label>\n<input type=\"text\" id=\"amount\" name=\"amount\""),
            "a box of the form stands outside its field wrapper:\n{html}"
        );
        let date_box = box_tag(&html, "date");
        let amount_box = box_tag(&html, "amount");
        assert_eq!(
            (
                date_box.contains(r#"aria-invalid="true""#),
                amount_box.contains(r#"aria-invalid="true""#)
            ),
            marked,
            "the boxes marked with a problem: {date_box} {amount_box}"
        );
        assert!(
            date_box.contains(r#"min="1900-01-01""#) && date_box.contains(r#"max="2026-11-16""#),
            "the date box's hints: {date_box}"
        );
        let tick = box_tag(&html, "below_zero");
        assert_eq!(
            tick.contains(" checked"),
            below_zero,
            "the tick box as sent: {tick}"
        );
        assert!(
            amount_box.contains(r#"inputmode="decimal""#),
            "the amount box does not ask for the number keypad: {amount_box}"
        );
    }
}

/// No opening-balance page in any state carries an htmx attribute, an inline script or an
/// inline style, and an account name holding markup and quotes is written as text everywhere.
///
/// Proves: Decision 116 with A2: no opening-balance page in any state carries an htmx attribute, an inline script or an inline style, and an account name holding markup and quotes is written as text.
#[test]
fn the_opening_balance_pages_carry_no_htmx_attribute_and_no_inline_code() {
    let name = r#"<b>Joint</b> "main" & 'old'"#;
    let escaped = "&#60;b&#62;Joint&#60;/b&#62; &#34;main&#34; &#38; &#39;old&#39;";
    for (what, html) in every_opening_balance_page(name) {
        assert!(
            !html.contains(" hx-") && !html.contains(" data-hx-"),
            "{what} carries an htmx attribute:\n{html}"
        );
        for script in html.match_indices("<script").map(|(at, _)| {
            let end = html[at..].find('>').map_or(html.len(), |index| index + at);
            &html[at..=end.min(html.len() - 1)]
        }) {
            assert!(
                script.contains(" src="),
                "{what} carries an inline script: {script}"
            );
        }
        assert!(
            !html.contains("<style") && !html.contains(" style="),
            "{what} carries an inline style:\n{html}"
        );
        assert!(
            !html.contains("<b>") && html.contains(escaped),
            "{what} does not write the account's name as text:\n{html}"
        );
    }
    let page = account_page(
        AccountView {
            name: name.to_owned(),
            place: format!("Assets:{name}"),
            ..checking()
        },
        None,
    );
    let html = html_of(&page);
    assert!(
        !html.contains("<b>") && html.contains(escaped) && !html.contains(" hx-"),
        "the account's page with its opening balance does not write the name as text:\n{html}"
    );
}
