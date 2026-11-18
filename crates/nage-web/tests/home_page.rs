//! The home page at `/` (SPEC 6.2, 6.3, 6.10, W1, decisions 119 and 120), through the real
//! router with a clock the test sets and a zone the test names. Balances are set through the
//! core's opening-balance operation and, where a date after today is needed, through
//! `create_transaction`; no row is written with unchecked SQL.

mod common;

use axum::http::StatusCode;
use nage_core::date::Date;
use nage_core::home;
use nage_core::transactions::{self, NewPosting, NewTransaction};

use common::app::{ADMIN, App, OTHER_PASSWORD, Options, PASSWORD, START};

/// The sentence of the empty state.
const EMPTY_STATE: &str =
    "This book has no accounts under Assets or Liabilities yet, so there are no balances to show.";

/// The sentence that names the day `day` as the day of the balances.
fn day_line(day: &str) -> String {
    format!("Balances at the end of {day}.")
}

/// The core's model for the application's today, read in one read transaction: what the page
/// must render.
async fn model(app: &App) -> home::Home {
    let mut read = app.read().await;
    home::home_page(&mut read, app.today())
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page's model: {error}"))
}

/// The figures of the core's model as the page writes them out (`Reply::figures`): every
/// amount displayed with the book's minor digits, an owner's tag in parentheses, and an Other
/// line under the name `Other`.
fn figures_of(model: &home::Home) -> Vec<String> {
    let Some(balances) = &model.balances else {
        return Vec::new();
    };
    let shown = |amount: nage_core::money::Money| amount.display(model.minor_digits).to_string();
    let mut out = Vec::new();
    for section in [&balances.assets, &balances.liabilities] {
        out.push(format!("## {} {}", section.name, shown(section.total)));
        for group in &section.groups {
            match &group.heading {
                Some(home::GroupHeading::Joint) => out.push(String::from("[Joint]")),
                Some(home::GroupHeading::Owner(name)) => out.push(format!("[{name}]")),
                None => {}
            }
            for line in &group.lines {
                match line {
                    home::Line::Account(line) => {
                        let owner = line
                            .owner
                            .as_ref()
                            .map(|owner| format!(" ({owner})"))
                            .unwrap_or_default();
                        out.push(format!(
                            "{} {}{owner} {}",
                            line.level,
                            line.name,
                            shown(line.amount)
                        ));
                    }
                    home::Line::Other(other) => {
                        out.push(format!("{} Other {}", other.level, shown(other.amount)));
                    }
                }
            }
        }
    }
    out.push(format!("Net worth {}", shown(balances.net_worth)));
    out
}

/// Right after a set-up with the starter accounts, `GET /` with a session answers 200 with the
/// home page in place of milestone 1's placeholder: the title `Nage`, the heading Home, the
/// sentence that names the day of the balances (the clock's date in UTC, the zone handed
/// in), the Assets and Liabilities sections with every starter account at 0.00 in the order
/// of their names, the Net worth line at 0.00, and no group heading. Equity, Income and
/// Expenses accounts are not shown, no line carries a link, and the page is exactly what
/// the core's model holds.
///
/// Proves: decision 119 (`/` is the home page; the day named; Assets and Liabilities only;
/// zero lines; no group heading without an owner; no link on a line), decision 120 (today in
/// the configured zone; the template only renders the core's model), SPEC W1 (the home page
/// after a set-up with the starter accounts), SPEC 6.10 (amounts as it displays them in a
/// book with two minor digits).
#[tokio::test]
async fn the_home_page_lists_the_starter_accounts_at_zero() {
    let app = App::with_book().await;
    let expected = figures_of(&model(&app).await);
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(page.status, StatusCode::OK, "GET / status:\n{}", page.body);
    assert!(
        page.body.contains("<title>Nage</title>"),
        "the title of the home page:\n{}",
        page.body
    );
    assert!(
        page.body.contains("<h1>Home</h1>"),
        "the heading of the home page:\n{}",
        page.body
    );
    assert!(
        page.shows(&day_line("2027-01-15")),
        "the home page does not name the day of its balances:\n{}",
        page.body
    );
    assert_eq!(
        page.figures(),
        [
            "## Assets 0.00",
            "1 Cash 0.00",
            "1 Checking 0.00",
            "1 Savings 0.00",
            "## Liabilities 0.00",
            "1 Credit Card 0.00",
            "Net worth 0.00",
        ],
        "the figures of the home page"
    );
    assert_eq!(
        page.figures(),
        expected,
        "the page against the core's model"
    );
    for hidden in [
        "Groceries",
        "Salary",
        "Opening Balances",
        "Equity",
        "Income",
        "Expenses",
    ] {
        assert!(
            !page.shows(hidden),
            "the home page shows {hidden}:\n{}",
            page.body
        );
    }
    let main = page.body.split("<main>").nth(1).unwrap_or_default();
    assert!(
        !main.contains("<a "),
        "a line of the home page carries a link:\n{main}"
    );
    assert!(
        !page.shows(EMPTY_STATE),
        "the home page of a book with accounts shows the empty state:\n{}",
        page.body
    );
}

/// With balances set through the opening-balance operation, the home page shows them as the
/// row says: the joint accounts first and then each owner's under the owner's display name;
/// an account of another owner inside a joint parent under that parent, with the owner's
/// name in parentheses on its line; a parent's figure with what is inside it, and the 500.00
/// the parent holds of its own on its Other line, last under it; an account with nothing in
/// it at 0.00; a card that is owed on as a plain positive amount and an overpaid card with a
/// minus; each section's total beside its heading and the Net worth line after Liabilities;
/// every amount with thousands commas and two decimals and no currency symbol. The page is
/// exactly what the core's model holds.
///
/// Proves: decision 119 (groups, the owner named on a line, the roll-up, the Other line,
/// natural signs with plain card amounts, the totals and the Net worth line), SPEC 6.10 (the
/// display of an amount, no currency symbol), SPEC 6.3 (natural sign), decision 120 (the
/// template only renders the core's model), decision 122 (balances set through the
/// opening-balance operations).
#[tokio::test]
async fn the_home_page_shows_the_balances_of_a_household() {
    let app = App::start().await;
    let sam = app.set_up_as(false).await.admin.id;
    let alex = app.add_named_user("alex", "Alex").await.id;
    let checking = app.add_account("Assets", "Checking", None).await;
    let savings = app.add_account("Assets", "Savings", None).await;
    let emergency = app
        .add_account("Assets:Savings", "Emergency fund", None)
        .await;
    let holiday = app
        .add_account("Assets:Savings", "Holiday", Some(alex))
        .await;
    app.add_account("Assets", "Renovation fund", None).await;
    let cash = app.add_account("Assets", "Cash", Some(alex)).await;
    let brokerage = app.add_account("Assets", "Brokerage", Some(sam)).await;
    let card = app.add_account("Liabilities", "Credit Card", None).await;
    let store = app
        .add_account("Liabilities", "Store card", Some(sam))
        .await;
    for (account, natural) in [
        (checking, 248_015),
        (savings, 50_000),
        (emergency, 900_000),
        (holiday, 300_000),
        (cash, 8_218),
        (brokerage, 135_000),
        (card, 120_480),
        (store, -5_000),
    ] {
        app.open_balance(account, "2027-01-01", natural).await;
    }
    let expected = figures_of(&model(&app).await);

    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(page.status, StatusCode::OK, "GET / status:\n{}", page.body);
    assert_eq!(
        page.figures(),
        [
            "## Assets 16,412.33",
            "[Joint]",
            "1 Checking 2,480.15",
            "1 Renovation fund 0.00",
            "1 Savings 12,500.00",
            "2 Emergency fund 9,000.00",
            "2 Holiday (Alex) 3,000.00",
            "2 Other 500.00",
            "[Alex]",
            "1 Cash 82.18",
            "[Sam]",
            "1 Brokerage 1,350.00",
            "## Liabilities 1,154.80",
            "[Joint]",
            "1 Credit Card 1,204.80",
            "[Sam]",
            "1 Store card -50.00",
            "Net worth 15,257.53",
        ],
        "the figures of the home page"
    );
    assert_eq!(
        page.figures(),
        expected,
        "the page against the core's model"
    );
    let main = page.body.split("<main>").nth(1).unwrap_or_default();
    for symbol in ["$", "CAD", "owed"] {
        assert!(
            !main.contains(symbol),
            "the home page writes {symbol:?} beside an amount:\n{main}"
        );
    }
}

/// `text` as askama writes it into a page: `&`, `<`, `>`, `"` and `'` as character references.
fn escaped(text: &str) -> String {
    text.replace('&', "&#38;")
        .replace('<', "&#60;")
        .replace('>', "&#62;")
        .replace('"', "&#34;")
        .replace('\'', "&#39;")
}

/// Names that hold markup are written as text, never as markup, in the five places the home
/// page writes a name typed by someone: a user whose display name is `<b>Al</b> & "Bo"` signs
/// in and owns a top-level account and an account named `<i>Rainy</i> & day` inside a joint
/// parent. The page holds no `<b>` and no `<i>` element; the escaped display name stands in
/// both copies of the menu, as the heading of its group and as the owner's tag on the line
/// inside the joint parent, and the escaped account name on its line; and the page's figures,
/// read back as text, are still exactly the core's model.
///
/// Proves: decision 119 (the home page writes account names and owners' display names as
/// text), decision 116 (the menu names the signed-in user as text), SPEC 5.3 (no markup from a
/// name reaches the page), decision 120 (the template only renders the core's model).
#[tokio::test]
async fn names_that_hold_markup_are_written_as_text() {
    const DISPLAY_NAME: &str = r#"<b>Al</b> & "Bo""#;
    const ACCOUNT_NAME: &str = "<i>Rainy</i> & day";
    let app = App::start().await;
    app.set_up_as(false).await;
    let bo = app.add_named_user("bo", DISPLAY_NAME).await.id;
    app.add_account("Assets", "Savings", None).await;
    app.add_account("Assets:Savings", ACCOUNT_NAME, Some(bo))
        .await;
    app.add_account("Assets", "Wallet", Some(bo)).await;
    let expected = figures_of(&model(&app).await);

    let mut client = app.signed_in("bo", OTHER_PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(page.status, StatusCode::OK, "GET / status:\n{}", page.body);
    for tag in ["<b>", "</b>", "<i>", "</i>"] {
        assert!(
            !page.body.contains(tag),
            "the home page holds the element {tag} from a name:\n{}",
            page.body
        );
    }
    let name = escaped(DISPLAY_NAME);
    for (needle, count, place) in [
        (format!("Signed in as {name}"), 2, "the menu, twice"),
        (
            format!("<h3>{name}</h3>"),
            1,
            "the heading of the owner's group",
        ),
        (
            format!(r#"<small class="owner">({name})</small>"#),
            1,
            "the owner's tag",
        ),
        (
            format!(r#"<span class="name">{} <small"#, escaped(ACCOUNT_NAME)),
            1,
            "the line of the account",
        ),
    ] {
        assert_eq!(
            page.body.matches(&needle).count(),
            count,
            "{place} ({needle}) on the home page:\n{}",
            page.body
        );
    }
    assert_eq!(
        page.figures(),
        [
            "## Assets 0.00",
            "[Joint]",
            "1 Savings 0.00",
            r#"2 <i>Rainy</i> & day (<b>Al</b> & "Bo") 0.00"#,
            r#"[<b>Al</b> & "Bo"]"#,
            "1 Wallet 0.00",
            "## Liabilities 0.00",
            "Net worth 0.00",
        ],
        "the figures of the home page, read back as text"
    );
    assert_eq!(
        page.figures(),
        expected,
        "the page against the core's model"
    );
}

/// The home page writes every amount with the book's own minor digits: in a book set up in
/// JPY, which has none, an opening balance of 123,456 yen reads `123,456` and no figure holds
/// a decimal point; in a book set up in BHD, which has three, the same number of minor units
/// reads `123.456` and every zero `0.000`. Then Savings gets 7,000 minor units of its own and
/// a sub-account Holiday with 500 of its own and Flights inside it, so the page has an Other
/// line at the second level and one at the third, which read `7,000` and `500` in JPY and
/// `7.000` and `0.500` in BHD. Each page is exactly what the core's model holds.
///
/// Proves: decision 119 (every figure written as SPEC 6.10 displays an amount, with the book's
/// minor digits, the Other line's included, each Other line at its own level), SPEC 6.10,
/// decision 120 (the template only renders the core's model).
#[tokio::test]
async fn the_home_page_writes_amounts_with_the_books_minor_digits() {
    for (code, figures) in [
        (
            "JPY",
            [
                "## Assets 123,456",
                "1 Cash 0",
                "1 Checking 123,456",
                "1 Savings 0",
                "## Liabilities 0",
                "1 Credit Card 0",
                "Net worth 123,456",
            ],
        ),
        (
            "BHD",
            [
                "## Assets 123.456",
                "1 Cash 0.000",
                "1 Checking 123.456",
                "1 Savings 0.000",
                "## Liabilities 0.000",
                "1 Credit Card 0.000",
                "Net worth 123.456",
            ],
        ),
    ] {
        let app = App::start().await;
        app.set_up_in(code, true).await;
        let checking = app.account("Assets:Checking").await;
        app.open_balance(checking, "2027-01-01", 123_456).await;
        let expected = figures_of(&model(&app).await);
        let mut client = app.signed_in(ADMIN, PASSWORD).await;
        let page = client.get("/").await;
        assert_eq!(
            page.status,
            StatusCode::OK,
            "GET / of a book in {code}:\n{}",
            page.body
        );
        assert_eq!(
            page.figures(),
            figures,
            "the figures of the home page of a book in {code}"
        );
        assert_eq!(
            page.figures(),
            expected,
            "the page of a book in {code} against the core's model"
        );

        let savings = app.account("Assets:Savings").await;
        let holiday = app.add_account("Assets:Savings", "Holiday", None).await;
        app.add_account("Assets:Savings:Holiday", "Flights", None)
            .await;
        app.open_balance(savings, "2027-01-01", 7_000).await;
        app.open_balance(holiday, "2027-01-01", 500).await;
        let expected = figures_of(&model(&app).await);
        let others = if code == "JPY" {
            ["3 Other 500", "2 Other 7,000"]
        } else {
            ["3 Other 0.500", "2 Other 7.000"]
        };
        for other in others {
            assert!(
                expected.iter().any(|shown| shown == other),
                "the model of a book in {code} lacks the line {other}: {expected:?}"
            );
        }
        let page = client.get("/").await;
        assert_eq!(
            page.figures(),
            expected,
            "the page of a book in {code} with Other lines against the core's model"
        );
    }
}

/// After a set-up that declined the starter accounts, the home page answers 200 with the
/// empty state in words and no figure: no section, no line, no Net worth, and no sentence
/// naming a day. The menu is there all the same.
///
/// Proves: decision 119 (the empty state in words and no figure when Assets and Liabilities
/// hold no account to list), SPEC W1's acceptance (the home page shows an empty state), SPEC
/// 6.2.
#[tokio::test]
async fn the_home_page_of_a_book_without_accounts_is_the_empty_state() {
    let app = App::start().await;
    app.set_up_as(false).await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(page.status, StatusCode::OK, "GET / status:\n{}", page.body);
    assert!(
        page.shows(EMPTY_STATE),
        "the home page does not show the empty state:\n{}",
        page.body
    );
    assert_eq!(
        page.figures(),
        Vec::<String>::new(),
        "the figures of the empty state"
    );
    for absent in ["Net worth", "Balances at the end of", "0.00"] {
        assert!(
            !page.shows(absent),
            "the empty state shows {absent:?}:\n{}",
            page.body
        );
    }
    assert!(
        page.body.contains(r#"<header class="bar">"#),
        "the empty state has no menu:\n{}",
        page.body
    );
}

/// The home page's day turns at midnight in the configured zone, not at midnight UTC. With
/// Toronto as the zone and the clock one second before midnight there (04:59:59 UTC on the
/// 15th), the page names the 14th and counts an entry dated the 14th but not one dated the
/// 15th; one second later it names the 15th and counts both. With UTC as the zone, the same
/// first instant is already the 15th.
///
/// Proves: decision 120 (today per request, as the date in the configured zone at the
/// clock's instant; the home page's day turns at midnight in that zone), decision 119 (the
/// balances at the end of the day the page names), decision 96, decision 122 (the day's turn
/// proven with a set clock and a named zone).
#[tokio::test]
async fn the_home_pages_day_turns_at_midnight_in_the_configured_zone() {
    // 2027-01-15T04:59:59Z: START is 2027-01-15T08:00:00Z.
    let before_midnight = -3 * 3_600 - 1;
    let toronto = App::start_with(Options {
        zone: Some("America/Toronto"),
        ..Options::default()
    })
    .await;
    toronto.clock.set(before_midnight);
    let admin = toronto.set_up().await.admin.id;
    assert_eq!(
        toronto.today().to_string(),
        "2027-01-14",
        "the tests' own reckoning of today in Toronto"
    );
    let checking = toronto.account("Assets:Checking").await;
    let salary = toronto.account("Income:Salary").await;
    toronto.open_balance(checking, "2027-01-14", 10_000).await;
    let fifteenth: Date = "2027-01-15"
        .parse()
        .unwrap_or_else(|error| panic!("2027-01-15 is no date: {error}"));
    let mut tx = toronto.write().await;
    transactions::create_transaction(
        &mut tx,
        admin,
        toronto.clock.now(),
        &NewTransaction {
            date: fifteenth,
            postings: vec![
                NewPosting {
                    account: checking,
                    amount: nage_core::money::Money::from_minor(2_500),
                },
                NewPosting {
                    account: salary,
                    amount: nage_core::money::Money::from_minor(-2_500),
                },
            ],
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the entry of the 15th: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the entry of the 15th: {error}"));

    let mut client = toronto.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(page.status, StatusCode::OK, "GET / before midnight");
    assert!(
        page.shows(&day_line("2027-01-14")),
        "one second before midnight in Toronto the page does not name the 14th:\n{}",
        page.body
    );
    assert_eq!(
        page.figures().first().map(String::as_str),
        Some("## Assets 100.00"),
        "the Assets total one second before midnight in Toronto"
    );
    toronto.clock.advance(1);
    let page = client.get("/").await;
    assert!(
        page.shows(&day_line("2027-01-15")),
        "at midnight in Toronto the page does not name the 15th:\n{}",
        page.body
    );
    assert_eq!(
        page.figures().first().map(String::as_str),
        Some("## Assets 125.00"),
        "the Assets total at midnight in Toronto"
    );

    let utc = App::start().await;
    utc.clock.set(before_midnight);
    utc.set_up().await;
    let mut client = utc.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert!(
        page.shows(&day_line("2027-01-15")),
        "at 04:59:59 UTC on the 15th the page of a server in UTC does not name the 15th:\n{}",
        page.body
    );
}

/// An instant whose date in the configured zone lies after 2199-12-31 has no today: the home
/// page answers a bare 500, a failure and not a refusal, while a page that needs no day
/// still answers. At noon UTC on 2199-12-31 it is already 2200 fourteen hours east of UTC.
///
/// Proves: decision 120 (an instant whose date falls outside decision 88's range answers
/// 500), decision 50 (a failure answers a bare 500).
#[tokio::test]
async fn a_day_outside_the_range_answers_500() {
    let app = App::start_with(Options {
        zone: Some("Pacific/Kiritimati"),
        ..Options::default()
    })
    .await;
    // 2199-12-31T12:00:00Z.
    app.clock.set(7_258_075_200 - START);
    app.set_up().await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(
        page.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET / on a day outside the range:\n{}",
        page.body
    );
    assert_eq!(page.body, "", "the body of the 500");
    let other = client.get("/account/password").await;
    assert_eq!(
        other.status,
        StatusCode::OK,
        "GET /account/password at the same instant"
    );
}

/// A balance that does not fit in 64 bits answers a bare 500 rather than a wrong figure: two
/// opening balances under Assets, the largest amount the book can hold and one minor unit
/// beside it, overflow the roll-up of Assets.
///
/// Proves: decision 120 (a balance whose roll-up does not fit in 64 bits answers 500 rather
/// than a wrong figure), decision 96, decision 50.
#[tokio::test]
async fn a_balance_that_does_not_fit_answers_500() {
    let app = App::with_book().await;
    let checking = app.account("Assets:Checking").await;
    let savings = app.account("Assets:Savings").await;
    app.open_balance(checking, "2027-01-01", i64::MAX).await;
    app.open_balance(savings, "2027-01-01", 1).await;
    let mut client = app.signed_in(ADMIN, PASSWORD).await;
    let page = client.get("/").await;
    assert_eq!(
        page.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET / over a roll-up that does not fit:\n{}",
        page.body
    );
    assert_eq!(page.body, "", "the body of the 500");
}
