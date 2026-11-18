//! Every page of milestone 2 slices 5 and 6 in every state that shows a message, rendered from
//! its template with made-up details and written out as a phone shows it, in plain text: the
//! bar with its Menu button on a page that has one, then the heading and each label, help
//! line, box and button in order, and on the home page each balance line with its amount at
//! the right. The menu is also written out open and as a computer shows it. The renderings are
//! file snapshots beside this file (decisions 60 and 115), so a change of wording shows as a
//! changed snapshot; the contract's MSG.md is made from them.

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

/// The entries of the menu as the page carries them: once for a phone (inside the Menu
/// button's `details`) and once for a computer (the `nav` beside Nage's name).
#[derive(Default)]
struct MenuText {
    brand: String,
    button: String,
    narrow: Vec<String>,
    wide: Vec<String>,
}

/// Where in the bar the renderer is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Page,
    Narrow,
    Wide,
}

/// The page `html` as plain text, as `view` shows it: the browser tab's title; the bar of a
/// page that has one, Nage's name at the left and the Menu button at the right, on a phone
/// (after a tap, the menu's entries follow, one per line), or Nage's name and the entries in
/// one row on a computer; `# ` before the heading; a notice between `>>` and `<<`; each label
/// on its own line, a box drawn as `[ ____ ]` with what it holds (a password box never holds
/// anything), the currency list as `[ CAD v ]`, a tick box as `[x]` or `[ ]` before its
/// label; a help line indented; a problem under its box after `!`; a button as `( … )`; a
/// link as `-> …`, with `(this page)` after the menu's entry for the page in view; a
/// section's heading after `## ` with its total at the right; a group's heading with a
/// colon; an account's line, and an Other line, with its amount at the right, set in by its
/// level, the owner's name after the account's in the parentheses the page writes; the Net
/// worth line under a rule. Hidden fields are not shown.
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
        match name {
            "title" | "h1" | "h3" | "p" | "label" | "button" | "option" | "summary" | "strong" => {
                element = Some((name, attributes));
                text.clear();
                tick = None;
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
            "a" => {
                current = attributes.contains(r#"aria-current="page""#);
                text.clear();
            }
            "nav" if attributes.contains("menu-wide") => zone = Zone::Wide,
            "details" => zone = Zone::Narrow,
            "/nav" | "/details" => zone = Zone::Page,
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
            "/a" if zone != Zone::Page => {
                let mark = if current { " (this page)" } else { "" };
                entry = Some(format!("-> {}{mark}", plain(&text)));
                text.clear();
            }
            "/li" => {
                let attributes = item.take().unwrap_or_default();
                match zone {
                    Zone::Narrow => menu
                        .narrow
                        .push(entry.take().unwrap_or_else(|| plain(&text))),
                    Zone::Wide => menu.wide.push(entry.take().unwrap_or_else(|| plain(&text))),
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
                assert_eq!(
                    menu.narrow, menu.wide,
                    "the menu's entries for a phone and for a computer differ"
                );
                match view {
                    View::Phone => lines.push(row(0, &menu.brand, &menu.button)),
                    View::PhoneMenuOpen => {
                        lines.push(row(0, &menu.brand, &menu.button));
                        lines.extend(menu.narrow.iter().map(|entry| format!("  {entry}")));
                    }
                    View::Computer => {
                        lines.push(format!("{}    {}", menu.brand, menu.wide.join("   ")));
                    }
                }
                lines.push(String::new());
            }
            "/title" | "/h1" | "/h3" | "/p" | "/small" | "/label" | "/button" | "/option"
            | "/summary" | "/strong" => {
                let words = plain(&text);
                if let Some((open, attributes)) = element.take() {
                    match open {
                        "title" => lines.push(format!("Browser tab: {words}")),
                        "h1" => lines.push(format!("\n# {words}\n")),
                        "h3" => lines.push(format!("\n{words}:")),
                        "strong" => menu.brand = words,
                        "summary" => menu.button = format!("( {words} )"),
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
                        "button" if zone != Zone::Page => entry = Some(format!("( {words} )")),
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

/// The layout of a page that Sam is signed in on, with `current` marked in the menu and
/// `message` at the top.
fn sams(current: Option<Entry>, message: Option<Message>) -> Layout {
    Layout {
        menu: Some(Menu {
            display_name: String::from("Sam"),
            current,
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

/// The set-up page as text: empty with CAD selected and the tick box ticked; refused with a
/// problem in every box, each under its own; refused with one problem, everything typed kept
/// except the passwords; and the page of the 404 once Nage is set up.
///
/// Proves: decision 105 (what the set-up page shows and says, in each state), decision 73
/// (the title), decision 102, decision 116 (with the layout of a page without a signed-in
/// user, which holds no menu, the set-up page and its closed page show no bar).
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
/// page carries no menu in any state.
///
/// Proves: decision 106 (what the sign-in page shows and says), decision 111 (the wait),
/// decision 112 (the sign-in page says that a change overtaken by a reset was not made),
/// decision 117 (the one-time messages of the sign-in page, at the top of the page),
/// decision 116 (with the layout of a page without a signed-in user, which holds no menu,
/// the sign-in page shows no bar), decision 102.
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
/// each with its heading, its message and a button back to Nage, and no menu.
///
/// Proves: decision 110 (the 403 page says what was refused, why and what to do; the 413),
/// decision 116 (with the layout of a page without a signed-in user, which holds no menu,
/// the refusal pages show no bar), decision 102.
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
/// line: Home, marked as the page in view, Change password, who is signed in and the Sign out
/// button); on a computer (Nage's name and the same entries in one row, no Menu button). Then
/// the open menu on the change-password page, which marks Change password, and on the page
/// for an unknown address, which marks no entry. The page carries the entries once for a
/// phone and once for a computer, and the two lists are the same.
///
/// Proves: decision 116 (the bar with Nage's name and the Menu button; the menu lists Home,
/// Change password, who is signed in and Sign out; the page carries the same entries a second
/// time for a computer's screen, where the stylesheet, which no cargo test reads, sets them in
/// one row; the page in view is marked).
#[test]
fn the_menu_closed_open_and_on_a_computer() {
    let home = home_page(Some(starter_accounts()), None);
    let bar = |text: String| {
        let end = text.find("\n\n# ").unwrap_or(text.len());
        format!("{}\n", &text[..end])
    };
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
