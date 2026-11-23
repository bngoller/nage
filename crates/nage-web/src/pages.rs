//! HTML pages on the askama base layout (SPEC 11, decision 50): the busy page, the pages of
//! milestone 2 slice 5 (decisions 105, 106, 110 and 112), whose handlers live in `setup`,
//! `login` and `account`, the pages of slice 6 (decisions 116 to 120): the home page and the
//! page for an unknown address, whose handlers live in `home` and `not_found`, and the account
//! pages of slice 7 (decisions 128 to 135): the list, the choice page, the create and change
//! forms, an account's page, the clash page and the confirmations (`AccountsPage`,
//! `NewWhatPage`, `AccountNewPage`, `AccountEditPage`, `AccountPage`, `AccountClashPage`,
//! `AccountConfirmPage`), whose handlers live in `accounts`. The layout
//! carries the bar with its menu on a page that knows who is signed in, and a one-time message
//! at the top of the page that shows one. Every page keeps the title `Nage` (decision 73).

use askama::Template;
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use nage_core::accounts::{
    AccountBoxes, AccountId, BoxName, Clash, Kind, Sent, SentBoxes, SystemKey, Word,
};
use nage_core::chart::{
    AccountView, Chart, ChartLine, EditForm, NewForm, OpeningFact, Owner, TrailStep,
};
use nage_core::currency::Currency;
use nage_core::date::Date;
use nage_core::home::{GroupHeading, Home, Line};
use nage_core::money::MinorDigits;
use nage_core::transactions::{OpeningBalance, OpeningClash, TransactionId};
use nage_core::users::UserId;

use crate::assets;
use crate::theme::{self, Theme};

#[cfg(test)]
mod tests;

/// What the base layout holds: the content-hashed asset URLs it links (decision 46) with the
/// two icons of the mark, the palette the browser's choice names (the appearance switch), the
/// menu of a page that knows who is signed in (decision 116) and the one-time message of a
/// page that shows one (decision 117).
pub(crate) struct Layout {
    pub(crate) htmx_js: &'static str,
    pub(crate) app_css: &'static str,
    /// The browser-tab icon (SVG).
    pub(crate) icon_svg: &'static str,
    /// The home-screen icon (PNG, 180 by 180).
    pub(crate) icon_png: &'static str,
    /// The appearance in force for this browser: `data-theme` on the page's `html` element
    /// for Light or Dark, none for Automatic, and the menu's choice marked as pressed.
    pub(crate) theme: Theme,
    /// The bar with the mark, Nage's name and the menu. `Some` on the home page, the
    /// change-password page, the page for an unknown address and every account page; `None` on
    /// the sign-in page,
    /// the set-up page and its closed page, the refusal pages and the busy page, which carry the
    /// mark and the name alone. None of them knows a signed-in user but one: the 422 page that
    /// refuses an appearance the menu does not offer (`theme::refused`) is answered only to a
    /// signed-in person, since `POST /theme` stands behind the session rule, and is still a
    /// refusal page without the menu (decisions 116, 124 and 125).
    pub(crate) menu: Option<Menu>,
    /// The trail at the top of a page of a place (decision 130): where the page stands, every
    /// step but the last a link back. Empty on the pages that have none; it stands before the
    /// message.
    pub(crate) trail: Vec<Crumb>,
    /// The one-time message, or the refusal of a form or an action, shown above the page's
    /// heading.
    pub(crate) message: Option<Message>,
}

impl Layout {
    /// The URLs of the embedded files and the appearance of the request being answered
    /// (`theme::current`), with no menu, no trail and no message.
    pub(crate) fn embedded() -> Self {
        Self {
            htmx_js: assets::linked("htmx.min.js"),
            app_css: assets::linked("app.css"),
            icon_svg: assets::linked("mark.svg"),
            icon_png: assets::linked("home-screen.png"),
            theme: theme::current(),
            menu: None,
            trail: Vec::new(),
            message: None,
        }
    }
}

/// A step of the trail (decision 130): its words, and the address it leads back to; the last
/// step, the page itself, leads nowhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Crumb {
    pub(crate) name: String,
    pub(crate) href: Option<String>,
}

/// The menu (decision 116, amended by 137): Home, Accounts, Change password, the appearance
/// switch, who is signed in, and the Sign out button, which posts to `/logout`. It lists only
/// pages that exist; a later page adds its entry in its own slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Menu {
    /// The display name of the signed-in user.
    pub(crate) display_name: String,
    /// The entry of the page in view, which the menu marks; `None` on a page that has no
    /// entry, such as the page for an unknown address.
    pub(crate) current: Option<Entry>,
    /// The page the appearance form returns to, in its hidden field `back`: the address of the
    /// page the form stands on, on the pages of slices 5 and 6 its path with its query as the
    /// request had them ([`here`]), which the template escapes like any text; on every account
    /// page its own address as Nage writes it, never the request's text, so that no number from
    /// a path or a query is printed (decision 128); `/` on the page for an unknown address, which
    /// never holds its address (decision 118). `POST /theme` follows it only when it is a path
    /// on this site (`session::safe_next`).
    pub(crate) back: String,
}

/// The address of the page a request asks for, as the menu's appearance form returns to it:
/// the request's path with its query, exactly as the request had them.
pub(crate) fn here(uri: &Uri) -> String {
    uri.path_and_query()
        .map_or_else(|| uri.path().to_owned(), |path| path.as_str().to_owned())
}

/// The pages the menu links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// `/`.
    Home,
    /// `/accounts` and every account page (decision 137).
    Accounts,
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
/// (decision 110), and the 422 of an appearance the menu does not offer (`theme`), each with a
/// button back to Nage.
#[derive(Template)]
#[template(path = "refusal.html")]
pub(crate) struct RefusalPage {
    pub(crate) layout: Layout,
    pub(crate) heading: &'static str,
    pub(crate) message: String,
}

/// The list of the chart at `/accounts` (decision 131): the one button, the day of the
/// balances, the five sections in three groups, and the way to show or hide the archived
/// accounts.
#[derive(Template)]
#[template(path = "accounts.html")]
pub(crate) struct AccountsPage {
    pub(crate) layout: Layout,
    pub(crate) chart: Chart,
}

impl AccountsPage {
    /// The list under `layout`, its menu marking Accounts; the list has no trail.
    pub(crate) fn new(layout: Layout, chart: Chart) -> Self {
        Self {
            layout: place_layout(layout, Vec::new()),
            chart,
        }
    }
}

/// `layout` as a page of the place Accounts wears it (decision 130): its menu marking
/// Accounts as the entry in view, and `trail` at the top.
pub(crate) fn place_layout(mut layout: Layout, trail: Vec<Crumb>) -> Layout {
    if let Some(menu) = layout.menu.as_mut() {
        menu.current = Some(Entry::Accounts);
    }
    layout.trail = trail;
    layout
}

/// A step of the trail named `name`, leading to `href`, or the page itself for `None`.
pub(crate) fn crumb(name: &str, href: Option<String>) -> Crumb {
    Crumb {
        name: name.to_owned(),
        href,
    }
}

/// The trail of an account's page: Accounts, then each account that holds it, each a link,
/// then its own name, a link too when `last` names a further step (Change, Archive,
/// Unarchive), which ends the trail.
pub(crate) fn account_trail(
    steps: &[TrailStep],
    id: AccountId,
    name: &str,
    last: Option<&str>,
) -> Vec<Crumb> {
    let mut trail = vec![crumb("Accounts", Some(String::from("/accounts")))];
    for step in steps {
        trail.push(crumb(&step.name, Some(format!("/accounts/{}", step.id))));
    }
    match last {
        Some(last) => {
            trail.push(crumb(name, Some(format!("/accounts/{id}"))));
            trail.push(crumb(last, None));
        }
        None => trail.push(crumb(name, None)),
    }
    trail
}

/// The page "What do you want to add?" at `/accounts/new` (decision 132): four choices, each
/// leading to the create form of its section.
#[derive(Template)]
#[template(path = "account_new_what.html")]
pub(crate) struct NewWhatPage {
    pub(crate) layout: Layout,
}

impl NewWhatPage {
    /// The page under `layout`, with the trail Accounts, New.
    pub(crate) fn new(layout: Layout) -> Self {
        let trail = vec![
            crumb("Accounts", Some(String::from("/accounts"))),
            crumb("New", None),
        ];
        Self {
            layout: place_layout(layout, trail),
        }
    }
}

/// What an account form shows in its boxes: what was typed, or what is saved; `inside` and
/// `owner` as the lists send them (an account's number, and an empty owner for Joint).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountTyped {
    pub(crate) name: String,
    pub(crate) inside: String,
    pub(crate) owner: String,
}

/// The problems of a refused account form, each under its own box, all at once (decision 132,
/// the pause's P3): the core's [`nage_core::accounts::FormRefusals`] in the core's own words.
/// The line above the form is the layout's message, which also carries a refusal that belongs
/// to no box (the pause's P4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountProblems {
    pub(crate) name: Option<String>,
    pub(crate) inside: Option<String>,
    pub(crate) owner: Option<String>,
}

/// The create form of one section (decision 132): Name, Inside, and Owner under Assets and
/// Liabilities.
#[derive(Template)]
#[template(path = "account_new.html")]
pub(crate) struct AccountNewPage {
    pub(crate) layout: Layout,
    pub(crate) form: NewForm,
    pub(crate) typed: AccountTyped,
    pub(crate) problems: AccountProblems,
    /// The place the form was opened inside, from "New account inside Savings" (the pause's
    /// P10): the way back leads to its page, and the form's action keeps `inside` in its query,
    /// so that a returned form keeps its trail.
    pub(crate) from: Option<TrailStep>,
}

impl AccountNewPage {
    /// The form under `layout`. Opened inside a place, `path` is that place's whole path from
    /// the top-level account down to the place itself, and the trail is Accounts, each step of
    /// it a link, then New account (or New category); otherwise Accounts, New account (the
    /// pause's P10).
    pub(crate) fn new(
        layout: Layout,
        form: NewForm,
        typed: AccountTyped,
        problems: AccountProblems,
        path: Vec<TrailStep>,
    ) -> Self {
        let last = format!("New {}", form.word);
        let mut trail = vec![crumb("Accounts", Some(String::from("/accounts")))];
        for step in &path {
            trail.push(crumb(&step.name, Some(format!("/accounts/{}", step.id))));
        }
        trail.push(crumb(&last, None));
        Self {
            layout: place_layout(layout, trail),
            form,
            typed,
            problems,
            from: path.last().cloned(),
        }
    }
}

/// One account's page (decision 133): what it is, its owner and balance under Assets and
/// Liabilities, what is inside it with the Other line of its own amount, and its actions: an
/// archived account's are Unarchive, the main one, and Change name (the pause's P4); a refusal
/// of Archive or Unarchive stands at the top as the layout's message.
#[derive(Template)]
#[template(path = "account.html")]
pub(crate) struct AccountPage {
    pub(crate) layout: Layout,
    pub(crate) view: AccountView,
}

impl AccountPage {
    /// The page under `layout`, with the trail from Accounts through the accounts that hold
    /// it to its name.
    pub(crate) fn new(layout: Layout, view: AccountView) -> Self {
        let trail = account_trail(&view.trail, view.id, &view.name, None);
        Self {
            layout: place_layout(layout, trail),
            view,
        }
    }

    /// The sentence that says what Nage uses a built-in account for (decision 133).
    pub(crate) fn about(&self) -> Option<&'static str> {
        self.view.built_in.map(built_in_about)
    }

    /// The fact Opening balance's figure, as SPEC 6.10 writes it with the book's digits (a
    /// minus before an amount below zero), and its day; `None` when there is none yet or the
    /// page shows no such fact (decision 141).
    pub(crate) fn opening_entered(&self) -> Option<(String, Date)> {
        match self.view.opening_balance {
            Some(OpeningFact::Entered { date, amount }) => {
                Some((amount.display(self.view.minor_digits).to_string(), date))
            }
            Some(OpeningFact::NoneYet) | None => None,
        }
    }

    /// Whether Enter opening balance is the page's yellow button, first among its actions: an
    /// account that can have one and has none yet (decision 141).
    pub(crate) fn enters_opening_balance(&self) -> bool {
        self.view.opening_balance == Some(OpeningFact::NoneYet)
    }

    /// Whether Change opening balance stands, outlined, among the actions (decision 141).
    pub(crate) fn changes_opening_balance(&self) -> bool {
        matches!(self.view.opening_balance, Some(OpeningFact::Entered { .. }))
    }
}

/// What Nage uses a built-in account for, as its page says it (decision 133).
pub(crate) fn built_in_about(key: SystemKey) -> &'static str {
    match key {
        SystemKey::OpeningBalances => {
            "Nage uses this account itself: every opening balance is entered against it. It always stays here and cannot be archived. You may rename it."
        }
        SystemKey::ValuationChanges => {
            "Nage uses this account itself: when you update the balance of an account you track by its balance alone, such as a pension, the difference is entered against it. It always stays here and cannot be archived. You may rename it."
        }
        SystemKey::Uncategorized => {
            "Nage uses this category itself: an entry whose category has not been chosen yet waits here. It always stays here and cannot be archived. You may rename it."
        }
        SystemKey::Assets
        | SystemKey::Liabilities
        | SystemKey::Equity
        | SystemKey::Income
        | SystemKey::Expenses => "This is one of Nage's five sections, which stay as they are.",
    }
}

/// The change form (decision 134): Name, Inside and Owner, or the name alone for a built-in or
/// an archived account (the pause's P4). Its hidden fields send back what the form showed: a
/// fresh form's what is saved; a returned form's (422) the shown values the request sent,
/// parsed, never refreshed from what is saved (the pause's P2).
#[derive(Template)]
#[template(path = "account_edit.html")]
pub(crate) struct AccountEditPage {
    pub(crate) layout: Layout,
    pub(crate) form: EditForm,
    pub(crate) typed: AccountTyped,
    pub(crate) problems: AccountProblems,
    pub(crate) shown: SentBoxes,
}

impl AccountEditPage {
    /// The form under `layout`, with the trail from Accounts through the account to Change,
    /// sending back `shown` as what it showed.
    pub(crate) fn new(
        layout: Layout,
        form: EditForm,
        typed: AccountTyped,
        problems: AccountProblems,
        shown: SentBoxes,
    ) -> Self {
        let trail = account_trail(&form.trail, form.id, &form.name, Some("Change"));
        Self {
            layout: place_layout(layout, trail),
            form,
            typed,
            problems,
            shown,
        }
    }

    /// The help line under Inside.
    pub(crate) fn inside_help(&self) -> String {
        inside_help(self.form.kind, self.form.word)
    }

    /// The heading: Change name for a built-in or an archived account, else Change account or
    /// Change category.
    pub(crate) fn heading(&self) -> String {
        change_heading(&self.form)
    }

    /// The hidden field `shown_name`, `shown_inside` or `shown_owner`, by `field`.
    pub(crate) fn shown(&self, field: &str) -> Option<String> {
        shown_field(&self.shown, field)
    }
}

/// The heading of a change form (decision 134, the pause's P4).
pub(crate) fn change_heading(form: &EditForm) -> String {
    if form.built_in.is_some() || form.archived {
        String::from("Change name")
    } else {
        format!("Change {}", form.word)
    }
}

/// What a fresh change form sends back as what it showed: what is saved now.
pub(crate) fn shown_from_saved(saved: &AccountBoxes) -> SentBoxes {
    SentBoxes {
        name: Sent::Value(saved.name.clone()),
        inside: saved.parent.map_or(Sent::Absent, Sent::Value),
        owner: Sent::Value(saved.owner),
    }
}

/// The value of the hidden field `field` (`shown_name`, `shown_inside` or `shown_owner`) for
/// `shown`, or `None` to leave the field out: a box whose shown value was absent or malformed
/// is sent as absent again, so that it still equals nothing saved (the pause's P2). Joint is
/// the empty text; a number is written as Nage prints it.
pub(crate) fn shown_field(shown: &SentBoxes, field: &str) -> Option<String> {
    match field {
        "shown_name" => match &shown.name {
            Sent::Value(name) => Some(name.clone()),
            Sent::Absent | Sent::Malformed => None,
        },
        "shown_inside" => match shown.inside {
            Sent::Value(id) => Some(id.to_string()),
            Sent::Absent | Sent::Malformed => None,
        },
        "shown_owner" => match shown.owner {
            Sent::Value(Some(id)) => Some(id.to_string()),
            Sent::Value(None) => Some(String::new()),
            Sent::Absent | Sent::Malformed => None,
        },
        _ => None,
    }
}

/// The help line under a change form's Inside box (decision 134): only places of the section,
/// and how to have the thing in the other section of its group.
pub(crate) fn inside_help(kind: Kind, word: Word) -> String {
    let section = kind.root_name();
    let other = match kind {
        Kind::Asset => Some(Kind::Liability),
        Kind::Liability => Some(Kind::Asset),
        Kind::Income => Some(Kind::Expense),
        Kind::Expense => Some(Kind::Income),
        Kind::Equity => None,
    };
    let first = format!(
        "Only places under {section}: {} stays in its section.",
        word.with_article()
    );
    match other {
        Some(other) => format!(
            "{first} To move it to {}, make a new one there and archive this one.",
            other.root_name()
        ),
        None => first,
    }
}

/// The page of a change that clashed with another save (decision 134, 409; the pause's P2):
/// the refused box at the top, what is saved now in the clashing boxes only, the form with
/// what the person typed in the boxes they changed and what is saved now in every other box,
/// and hidden values that are what is saved now, so that Save mine writes only the person's
/// own boxes; Save mine and Keep what is saved.
#[derive(Template)]
#[template(path = "account_clash.html")]
pub(crate) struct AccountClashPage {
    pub(crate) layout: Layout,
    pub(crate) form: EditForm,
    pub(crate) typed: AccountTyped,
    /// No problem shows on this page; the shared boxes read it.
    pub(crate) problems: AccountProblems,
    /// What is saved now in each clashing box: its label and its value in words.
    pub(crate) saved: Vec<(&'static str, String)>,
    /// What is saved now, sent back as what the form showed.
    pub(crate) shown: SentBoxes,
}

impl AccountClashPage {
    /// The page under `layout` for `clash`, `typed` the form as the person sent it, with the
    /// trail from Accounts through the account to Change.
    pub(crate) fn new(layout: Layout, form: EditForm, clash: Clash, typed: AccountTyped) -> Self {
        let trail = account_trail(&form.trail, form.id, &form.name, Some("Change"));
        let saved = saved_lines(&form, &clash);
        let typed = clash_boxes(&clash, typed);
        let shown = shown_from_saved(&clash.saved);
        Self {
            layout: place_layout(layout, trail),
            form,
            typed,
            problems: AccountProblems::default(),
            saved,
            shown,
        }
    }

    /// The help line under Inside.
    pub(crate) fn inside_help(&self) -> String {
        inside_help(self.form.kind, self.form.word)
    }

    /// The heading, as the change form's; Change account when the form of an archived account
    /// keeps its Inside and Owner boxes (`nage_core::chart::clash_form`).
    pub(crate) fn heading(&self) -> String {
        if self.form.archived && self.form.places.is_some() {
            format!("Change {}", self.form.word)
        } else {
            change_heading(&self.form)
        }
    }

    /// The hidden field `shown_name`, `shown_inside` or `shown_owner`, by `field`.
    pub(crate) fn shown(&self, field: &str) -> Option<String> {
        shown_field(&self.shown, field)
    }
}

/// The boxes of the clash page (the pause's P2): what the person typed in each box of
/// `clash.changed`, and what is saved now (`clash.saved`) in every other box.
pub(crate) fn clash_boxes(clash: &Clash, typed: AccountTyped) -> AccountTyped {
    let changed = |name: BoxName| clash.changed.contains(&name);
    let saved = &clash.saved;
    AccountTyped {
        name: if changed(BoxName::Name) {
            typed.name
        } else {
            saved.name.clone()
        },
        inside: if changed(BoxName::Inside) {
            typed.inside
        } else {
            saved
                .parent
                .map(|parent| parent.to_string())
                .unwrap_or_default()
        },
        owner: if changed(BoxName::Owner) {
            typed.owner
        } else {
            saved
                .owner
                .map(|owner| owner.to_string())
                .unwrap_or_default()
        },
    }
}

/// The confirmation of Archive or Unarchive (decision 135).
#[derive(Template)]
#[template(path = "account_confirm.html")]
pub(crate) struct AccountConfirmPage {
    pub(crate) layout: Layout,
    pub(crate) view: AccountView,
    /// Archive when true, Unarchive when false.
    pub(crate) archive: bool,
}

impl AccountConfirmPage {
    /// The page under `layout`, with the trail from Accounts through the account to Archive
    /// or Unarchive.
    pub(crate) fn new(layout: Layout, view: AccountView, archive: bool) -> Self {
        let last = if archive { "Archive" } else { "Unarchive" };
        let trail = account_trail(&view.trail, view.id, &view.name, Some(last));
        Self {
            layout: place_layout(layout, trail),
            view,
            archive,
        }
    }
}

/// What the clash page's block "Saved now" shows (decision 134): for each clashing box, its
/// label (Name, Inside, Owner) and what is saved in it now, in words: the name, the full place
/// of the account it sits inside or "Directly under" its section, the owner's display name or
/// Joint.
pub(crate) fn saved_lines(form: &EditForm, clash: &Clash) -> Vec<(&'static str, String)> {
    let saved = &clash.saved;
    clash
        .boxes
        .iter()
        .map(|name| match name {
            BoxName::Name => ("Name", saved.name.clone()),
            BoxName::Inside => ("Inside", saved_place(form, clash)),
            BoxName::Owner => ("Owner", saved_owner(form, saved.owner)),
        })
        .collect()
}

/// Where the account sits now, in words: "Directly under" its section, or the full place of
/// the account that holds it, as the form's list writes it, else as the account's own full
/// place says it (everything before its last colon).
fn saved_place(form: &EditForm, clash: &Clash) -> String {
    let parent = clash.saved.parent;
    if parent == Some(form.section.id) {
        return format!("Directly under {}", form.section.full_name);
    }
    form.places
        .iter()
        .flatten()
        .find(|place| Some(place.id) == parent)
        .map(|place| place.full_name.clone())
        .or_else(|| {
            clash
                .place
                .rsplit_once(':')
                .map(|(holder, _)| holder.to_owned())
        })
        .unwrap_or_default()
}

/// Who owns the account now, in words: Joint, or the owner's display name as the form's list
/// writes it.
fn saved_owner(form: &EditForm, owner: Option<UserId>) -> String {
    match owner {
        None => String::from("Joint"),
        Some(id) => form
            .owners
            .iter()
            .flatten()
            .find(|choice| choice.id == id)
            .map(|choice| choice.display_name.clone())
            .unwrap_or_default(),
    }
}

/// The account an opening-balance page is about (decision 141): what its pages name, taken from
/// the core's read for these pages (`nage_core::chart::opening_balance_account`, the pause's P5),
/// never from the roll-up of balances (A7); the web rebuilds no rule of the core from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpeningAccount {
    pub(crate) id: AccountId,
    pub(crate) name: String,
    /// The full place, such as `Assets:Checking`.
    pub(crate) place: String,
    /// Assets or Liabilities: it words the amount box and the tick box.
    pub(crate) kind: Kind,
    /// The accounts that hold it, the top-level one first.
    pub(crate) trail: Vec<TrailStep>,
    pub(crate) minor_digits: MinorDigits,
}

/// What the opening-balance form shows in its boxes (decision 141): what was typed, kept on a
/// refused form and on a clash page, or what is saved, an amount below zero without its minus
/// and with the tick box ticked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpeningTyped {
    pub(crate) date: String,
    pub(crate) amount: String,
    pub(crate) below_zero: bool,
}

/// The problems of a refused opening-balance form, each under its box, both at once (decision
/// 145): the core's `OpeningBoxRefusal` in the core's own words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpeningProblems {
    pub(crate) date: Option<String>,
    pub(crate) amount: Option<String>,
}

/// The label of the amount box (decision 141): Balance under Assets, Amount owed under
/// Liabilities.
pub(crate) fn amount_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Liability => "Amount owed",
        _ => "Balance",
    }
}

/// The help line of the amount box (decision 141).
pub(crate) fn amount_help(kind: Kind) -> &'static str {
    match kind {
        Kind::Liability => "What the statement says you owed at the end of that day.",
        _ => "What the account held at the end of that day.",
    }
}

/// The words of the tick box under the amount box (decision 141, the pause's P6), which makes
/// the amount one below zero; no text calls every account under Liabilities a card, since a
/// loan stands there too.
pub(crate) fn tick_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Liability => "The account was in credit (the amount was owed to you)",
        _ => "The account was overdrawn (it held less than nothing)",
    }
}

/// The trail's last step on the form and its clash pages (decision 140).
pub(crate) const OPENING_BALANCE_STEP: &str = "Opening balance";

/// The trail's last step on the Remove confirmation (decision 143).
pub(crate) const REMOVE_OPENING_BALANCE_STEP: &str = "Remove opening balance";

/// The opening-balance form at `/accounts/<number>/opening-balance` (decisions 140 and 141): the
/// trail, the heading Opening balance (Change opening balance when one exists), the account's
/// place, the warning of SPEC 6.6 and its example, Date with its help line, the amount box
/// labelled by section with its help line and the tick box, the yellow Save opening balance,
/// the way back and, on the change form, Remove opening balance. A refused form (422) keeps
/// what was typed, the tick as sent, every problem under its box.
#[derive(Template)]
#[template(path = "opening_balance.html")]
pub(crate) struct OpeningBalancePage {
    pub(crate) layout: Layout,
    pub(crate) account: OpeningAccount,
    /// The entry the change form names and the version it shows, sent back in hidden fields;
    /// `None` on the form that enters one.
    pub(crate) entry: Option<(TransactionId, i64)>,
    pub(crate) typed: OpeningTyped,
    pub(crate) problems: OpeningProblems,
    /// Today, the server's day in the configured zone (A8): the help line and the date box's
    /// hint.
    pub(crate) today: Date,
}

impl OpeningBalancePage {
    /// The form under `layout`, with the trail from Accounts through the account to Opening
    /// balance.
    pub(crate) fn new(
        layout: Layout,
        account: OpeningAccount,
        entry: Option<(TransactionId, i64)>,
        typed: OpeningTyped,
        problems: OpeningProblems,
        today: Date,
    ) -> Self {
        let trail = account_trail(
            &account.trail,
            account.id,
            &account.name,
            Some(OPENING_BALANCE_STEP),
        );
        Self {
            layout: place_layout(layout, trail),
            account,
            entry,
            typed,
            problems,
            today,
        }
    }

    /// The heading: Change opening balance when the form changes one, else Opening balance.
    pub(crate) fn heading(&self) -> &'static str {
        if self.entry.is_some() {
            "Change opening balance"
        } else {
            "Opening balance"
        }
    }

    /// The help lines show on the form.
    pub(crate) fn helps(&self) -> bool {
        true
    }

    /// The amount box's label.
    pub(crate) fn amount_label(&self) -> &'static str {
        amount_label(self.account.kind)
    }

    /// The amount box's help line.
    pub(crate) fn amount_help(&self) -> &'static str {
        amount_help(self.account.kind)
    }

    /// The tick box's words.
    pub(crate) fn tick_label(&self) -> &'static str {
        tick_label(self.account.kind)
    }
}

/// The page of a save that met another save or removal (decision 144, 409): the sentence at
/// the top, a block Saved now (the date and the amount saved now, or No opening balance), a
/// block Your change with both boxes and the tick as sent, the yellow button (Save mine, or
/// Save as a new opening balance after a removal) and an outlined way to the account's page
/// (Keep what is saved, or Keep it removed). Nothing is merged.
#[derive(Template)]
#[template(path = "opening_balance_clash.html")]
pub(crate) struct OpeningBalanceClashPage {
    pub(crate) layout: Layout,
    pub(crate) account: OpeningAccount,
    pub(crate) clash: OpeningClash,
    pub(crate) typed: OpeningTyped,
    /// No problem shows on this page; the shared boxes read it.
    pub(crate) problems: OpeningProblems,
    pub(crate) today: Date,
}

impl OpeningBalanceClashPage {
    /// The page under `layout` for `clash`, `typed` the boxes as the person sent them, with
    /// the trail from Accounts through the account to Opening balance; the clash's sentence is
    /// the layout's message.
    pub(crate) fn new(
        layout: Layout,
        account: OpeningAccount,
        clash: OpeningClash,
        typed: OpeningTyped,
        today: Date,
    ) -> Self {
        let trail = account_trail(
            &account.trail,
            account.id,
            &account.name,
            Some(OPENING_BALANCE_STEP),
        );
        Self {
            layout: place_layout(layout, trail),
            account,
            clash,
            typed,
            problems: OpeningProblems::default(),
            today,
        }
    }

    /// The heading says what the person set out to do (decision 140, the pause's P9): Opening
    /// balance on the entered-meanwhile page, whose save entered one; Change opening balance on
    /// the changed-meanwhile and removed-meanwhile pages, whose save changed one.
    pub(crate) fn heading(&self) -> &'static str {
        match self.clash {
            OpeningClash::Entered { .. } => "Opening balance",
            _ => "Change opening balance",
        }
    }

    /// The entry and the version Save mine names, which are the ones saved now; `None` after a
    /// removal, whose yellow button enters a new one.
    pub(crate) fn entry(&self) -> Option<(TransactionId, i64)> {
        self.clash.saved().map(|saved| (saved.id, saved.version))
    }

    /// The yellow button's words.
    pub(crate) fn save_words(&self) -> &'static str {
        match self.clash {
            OpeningClash::Removed { .. } => "Save as a new opening balance",
            _ => "Save mine",
        }
    }

    /// The outlined button's words.
    pub(crate) fn keep_words(&self) -> &'static str {
        match self.clash {
            OpeningClash::Removed { .. } => "Keep it removed",
            _ => "Keep what is saved",
        }
    }

    /// No help line shows on this page.
    pub(crate) fn helps(&self) -> bool {
        false
    }

    /// The amount box's label.
    pub(crate) fn amount_label(&self) -> &'static str {
        amount_label(self.account.kind)
    }

    /// The amount box's help line.
    pub(crate) fn amount_help(&self) -> &'static str {
        amount_help(self.account.kind)
    }

    /// The tick box's words.
    pub(crate) fn tick_label(&self) -> &'static str {
        tick_label(self.account.kind)
    }
}

/// The confirmation of Remove at `/accounts/<number>/opening-balance/remove` (decision 143):
/// the trail, Remove the opening balance?, the line You typed 0 when a zero saved on the form
/// led here, the figure in full, the sentence that it cannot be brought back yet, the yellow
/// Remove opening balance, which names the entry and the version shown, and Keep it. After a
/// removal that met a change, the same page with the figure as it is now and the sentence at the
/// top (409).
#[derive(Template)]
#[template(path = "opening_balance_remove.html")]
pub(crate) struct OpeningBalanceRemovePage {
    pub(crate) layout: Layout,
    pub(crate) account: OpeningAccount,
    /// The entry as saved now, which the page shows and Remove names.
    pub(crate) saved: OpeningBalance,
    /// Whether the page answers a zero typed on the form (decision 143).
    pub(crate) typed_zero: bool,
}

impl OpeningBalanceRemovePage {
    /// The page under `layout`, with the trail from Accounts through the account to Remove
    /// opening balance.
    pub(crate) fn new(
        layout: Layout,
        account: OpeningAccount,
        saved: OpeningBalance,
        typed_zero: bool,
    ) -> Self {
        let trail = account_trail(
            &account.trail,
            account.id,
            &account.name,
            Some(REMOVE_OPENING_BALANCE_STEP),
        );
        Self {
            layout: place_layout(layout, trail),
            account,
            saved,
            typed_zero,
        }
    }
}

/// The heading of the Origin check's 403 page, which the refusal of an appearance the menu
/// does not offer shares.
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
