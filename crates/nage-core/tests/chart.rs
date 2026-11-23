//! The account pages' models (SPEC 6.2, W2, decisions 120 and 128 to 138): the list of the chart,
//! one account's page and the forms' pick lists, read in one read transaction over books built
//! through the core's operations, with golden snapshots of their Debug renderings; and the home
//! page's empty state of a book whose accounts are all archived. Every scenario is also a subject
//! of nage check. The one row no operation writes yet, a deactivation, is planted by
//! `test_support::deactivate_unchecked` until milestone 8 slice 1 (decisions 98 and 115).

mod common;

use nage_core::accounts::{
    AccountBoxes, AccountId, Kind, NewAccount, SystemKey, Word, archive_account, create_account,
    move_account, set_owner,
};
use nage_core::chart::{
    self, AccountView, ChartLine, Kept, OWNER_COLOURS, OpeningBalanceAccount, OpeningFact, Owner,
    OwnerTag, Place, TrailStep,
};
use nage_core::home;
use nage_core::money::{MinorDigits, Money};
use nage_core::storage::{Storage, WriteTransaction};
use nage_core::test_support;
use nage_core::transactions;
use nage_core::users::{self, NewUser, UserId};

use common::{
    account_named, begin, check_report, commit, date, labels, later, open_balance, post, reader,
    seeded, today,
};

/// Creates `name` under `parent` for `owner` (`None` is joint) by `by` at [`later`], panicking on
/// a refusal.
async fn create(
    tx: &mut WriteTransaction,
    by: UserId,
    name: &str,
    parent: AccountId,
    owner: Option<UserId>,
) -> AccountId {
    create_account(
        tx,
        by,
        later(),
        &NewAccount {
            name: name.to_owned(),
            parent,
            owner,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create {name}: {error}"))
    .id
}

/// Archives `account` by `by` at [`later`], panicking on a refusal.
async fn archive(tx: &mut WriteTransaction, by: UserId, account: AccountId) {
    archive_account(tx, by, later(), account)
        .await
        .unwrap_or_else(|error| panic!("cannot archive account {account}: {error}"));
}

/// Creates the user `username` shown as `display_name`, not an admin, by `admin` at [`later`].
async fn user(
    tx: &mut WriteTransaction,
    admin: UserId,
    username: &str,
    display_name: &str,
) -> UserId {
    users::create_user(
        tx,
        Some(admin),
        later(),
        &NewUser {
            username: username.to_owned(),
            display_name: display_name.to_owned(),
            password_hash: String::from("hash"),
            is_admin: false,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot create the user {username}: {error}"))
    .id
}

/// `lines` written out one text per line: `<level> <name>`, then ` <minor units>` where the line
/// has a figure, ` (<owner>/<colour>)` where it names an owner, ` built in` and ` archived`; an
/// Other line as `<level> <other> <minor units>`.
fn written(lines: &[ChartLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            ChartLine::Account(row) => {
                let mut text = format!("{} {}", row.level, row.name);
                if let Some(balance) = row.balance {
                    text = format!("{text} {}", balance.minor());
                }
                if let Some(owner) = &row.owner {
                    text = format!("{text} ({}/{})", owner.display_name, owner.colour);
                }
                if row.built_in {
                    text.push_str(" built in");
                }
                if row.archived {
                    text.push_str(" archived");
                }
                text
            }
            ChartLine::Other(other) => {
                format!("{} <other> {}", other.level, other.amount.minor())
            }
        })
        .collect()
}

/// The full names of `places`, each followed by ` (archived)` when it is marked so.
fn named(places: &[Place]) -> Vec<String> {
    places
        .iter()
        .map(|place| {
            if place.archived {
                format!("{} (archived)", place.full_name)
            } else {
                place.full_name.clone()
            }
        })
        .collect()
}

/// Asserts that nage check finds nothing wrong in the committed book of `storage`.
async fn assert_checked(storage: &Storage, label: &str) {
    let report = check_report(storage).await;
    assert_eq!(labels(&report), Vec::<&str>::new(), "check over {label}");
}

/// The made-up household of these tests (decision 139), committed: the admin (shown as Admin),
/// Alex and Sam; Checking owned by Alex with 2,480.15 opened and 30.00 spent on Groceries;
/// Savings with 100.00 of its own, holding Emergency fund and Holiday fund (Alex's, 500.00 from
/// Salary); 1,204.80 owed on the Credit Card; Health holding dentist and Optician.
struct Household {
    admin: UserId,
    alex: UserId,
    sam: UserId,
    assets: AccountId,
    income: AccountId,
    expenses: AccountId,
    checking: AccountId,
    savings: AccountId,
    card: AccountId,
    salary: AccountId,
    groceries: AccountId,
    health: AccountId,
    gifts: AccountId,
    opening: AccountId,
    uncategorized: AccountId,
    emergency: AccountId,
    holiday: AccountId,
}

/// Builds and commits the [`Household`] on the standard setup of `storage`, `admin` acting.
async fn household(storage: &Storage, admin: UserId) -> Household {
    let mut tx = begin(storage, "the household").await;
    let alex = user(&mut tx, admin, "alex", "Alex").await;
    let sam = user(&mut tx, admin, "sam", "Sam").await;
    let assets = account_named(&mut tx, "Assets").await;
    let income = account_named(&mut tx, "Income").await;
    let expenses = account_named(&mut tx, "Expenses").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let health = account_named(&mut tx, "Expenses:Health").await;
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    let opening = account_named(&mut tx, "Equity:Opening Balances").await;
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    set_owner(&mut tx, admin, later(), checking, Some(alex))
        .await
        .unwrap_or_else(|error| panic!("cannot give Checking to Alex: {error}"));
    let emergency = create(&mut tx, admin, "Emergency fund", savings, None).await;
    let holiday = create(&mut tx, admin, "Holiday fund", savings, Some(alex)).await;
    create(&mut tx, admin, "dentist", health, None).await;
    create(&mut tx, admin, "Optician", health, None).await;
    open_balance(&mut tx, admin, later(), checking, "2026-01-01", 248_015).await;
    open_balance(&mut tx, admin, later(), savings, "2026-01-01", 10_000).await;
    open_balance(&mut tx, admin, later(), card, "2026-01-01", 120_480).await;
    post(
        &mut tx,
        admin,
        later(),
        "2026-02-01",
        &[(holiday, 50_000), (salary, -50_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        later(),
        "2026-03-01",
        &[(groceries, 3_000), (checking, -3_000)],
    )
    .await;
    commit(tx, "the household").await;
    Household {
        admin,
        alex,
        sam,
        assets,
        income,
        expenses,
        checking,
        savings,
        card,
        salary,
        groceries,
        health,
        gifts,
        opening,
        uncategorized,
        emergency,
        holiday,
    }
}

/// The Expenses rows of the household's list, archived ones hidden.
const HOUSEHOLD_EXPENSES: [&str; 16] = [
    "1 Dining",
    "1 Entertainment",
    "1 Fees",
    "1 Gifts",
    "1 Groceries",
    "1 Health",
    "2 dentist",
    "2 Optician",
    "1 Housing",
    "1 Other",
    "1 Shopping",
    "1 Subscriptions",
    "1 Transport",
    "1 Travel",
    "1 Uncategorized built in",
    "1 Utilities",
];

/// The list groups the five sections, writes each as a tree ordered by the fold of the names at
/// every level, shows balances in natural sign under Assets and Liabilities only, an owner's
/// tag, the built-in mark, the Other line of an account's own amount, and the totals the home
/// page shows; categories, built-in accounts and Equity show no figure; a card holding another
/// shows its own amount owed on its Other line in natural sign.
///
/// Proves: Decision 131 with 96, 119, P8 and SPEC 6.3: five sections as trees by the fold, balances in natural sign under Assets and Liabilities only, the Other line of an own amount, the totals as the home page's.
#[tokio::test]
async fn the_chart_groups_orders_and_figures_the_accounts() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut conn = reader(&storage).await;
    let list = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert_eq!(list.as_of, today());
    assert_eq!(list.minor_digits, MinorDigits::TWO);
    assert!(list.balances_shown, "the figures show");
    assert!(!list.archived_shown);
    assert_eq!(list.archived, 0);
    assert_eq!(
        (list.assets.kind, list.assets.id, list.assets.name.as_str()),
        (Kind::Asset, h.assets, "Assets")
    );
    assert_eq!(
        written(&list.assets.rows),
        [
            "1 Cash 0",
            "1 Checking 245015 (Alex/2)",
            "1 Savings 60000",
            "2 Emergency fund 0",
            "2 Holiday fund 50000 (Alex/2)",
            "2 <other> 10000",
        ]
    );
    assert_eq!(list.assets.total, Some(Money::from_minor(305_015)));
    assert_eq!(written(&list.liabilities.rows), ["1 Credit Card 120480"]);
    assert_eq!(list.liabilities.total, Some(Money::from_minor(120_480)));
    assert_eq!(list.income.kind, Kind::Income);
    assert_eq!(written(&list.income.rows), ["1 Other", "1 Salary"]);
    assert_eq!(list.income.total, None, "categories show no figure");
    assert_eq!(written(&list.expenses.rows), HOUSEHOLD_EXPENSES);
    assert_eq!(list.expenses.total, None);
    assert_eq!(list.equity.kind, Kind::Equity);
    assert_eq!(
        written(&list.equity.rows),
        [
            "1 Opening Balances built in",
            "1 Valuation Changes built in"
        ]
    );
    assert_eq!(list.equity.total, None, "Equity shows no figure");
    for section in [
        &list.assets,
        &list.liabilities,
        &list.income,
        &list.expenses,
        &list.equity,
    ] {
        assert!(!section.all_archived, "{}", section.name);
    }
    let home = home::home_page(&mut conn, today())
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page: {error}"));
    let figures = home
        .balances
        .unwrap_or_else(|| panic!("the home page shows no figure"));
    assert_eq!(list.assets.total, Some(figures.assets.total));
    assert_eq!(list.liabilities.total, Some(figures.liabilities.total));
    drop(conn);
    let mut tx = begin(&storage, "a card holding another").await;
    create(&mut tx, h.admin, "Points", h.card, None).await;
    commit(tx, "a card holding another").await;
    let mut conn = reader(&storage).await;
    let list = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list again: {error}"));
    assert_eq!(
        written(&list.liabilities.rows),
        ["1 Credit Card 120480", "2 Points 0", "2 <other> 120480"],
        "the card's own amount owed in natural sign"
    );
    drop(conn);
    assert_checked(&storage, "the household").await;
    storage.close().await;
}

/// Archived accounts are hidden with what is inside them and counted; asked for, they stand in
/// their places, marked, with their balance at the end of the day, which an entry dated after it
/// can make differ from zero; a section whose accounts are all archived and hidden says so, apart
/// from a section that holds nothing.
///
/// Proves: Decision 131 with P8 and SPEC 6.2: archived accounts are hidden with what is inside them, counted, shown in their places when asked, and a section whose accounts are all archived says so.
#[tokio::test]
async fn the_chart_hides_and_counts_the_archived_accounts() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "the archived accounts").await;
    let old = create(&mut tx, h.admin, "Old savings", h.assets, None).await;
    let jar = create(&mut tx, h.admin, "Jar", old, None).await;
    post(
        &mut tx,
        h.admin,
        later(),
        "2026-11-09",
        &[(jar, 500), (h.salary, -500)],
    )
    .await;
    post(
        &mut tx,
        h.admin,
        later(),
        "2026-12-01",
        &[(jar, -500), (h.salary, 500)],
    )
    .await;
    archive(&mut tx, h.admin, jar).await;
    archive(&mut tx, h.admin, old).await;
    archive(&mut tx, h.admin, h.gifts).await;
    commit(tx, "the archived accounts").await;
    let mut conn = reader(&storage).await;
    let hidden = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert_eq!(hidden.archived, 3, "Jar, Old savings and Gifts");
    assert!(!hidden.archived_shown);
    assert_eq!(
        written(&hidden.assets.rows),
        [
            "1 Cash 0",
            "1 Checking 245015 (Alex/2)",
            "1 Savings 60000",
            "2 Emergency fund 0",
            "2 Holiday fund 50000 (Alex/2)",
            "2 <other> 10000",
        ]
    );
    assert_eq!(
        hidden.assets.total,
        Some(Money::from_minor(305_515)),
        "the total counts the archived accounts"
    );
    assert!(!written(&hidden.expenses.rows).contains(&String::from("1 Gifts")));
    assert!(!hidden.assets.all_archived && !hidden.expenses.all_archived);
    let shown = chart::chart(&mut conn, today(), true)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list with the archived: {error}"));
    assert!(shown.archived_shown);
    assert_eq!(shown.archived, 3);
    assert_eq!(
        written(&shown.assets.rows),
        [
            "1 Cash 0",
            "1 Checking 245015 (Alex/2)",
            "1 Old savings 500 archived",
            "2 Jar 500 archived",
            "1 Savings 60000",
            "2 Emergency fund 0",
            "2 Holiday fund 50000 (Alex/2)",
            "2 <other> 10000",
        ]
    );
    assert!(written(&shown.expenses.rows).contains(&String::from("1 Gifts archived")));
    drop(conn);
    assert_checked(&storage, "the archived accounts").await;
    storage.close().await;

    let (_dir, storage, initialized) = seeded(false).await;
    let mut tx = begin(&storage, "a book of archived income").await;
    let income = account_named(&mut tx, "Income").await;
    let old_job = create(&mut tx, initialized.admin.id, "Old job", income, None).await;
    archive(&mut tx, initialized.admin.id, old_job).await;
    commit(tx, "a book of archived income").await;
    let mut conn = reader(&storage).await;
    let hidden = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert!(hidden.income.all_archived, "every category is archived");
    assert!(hidden.income.rows.is_empty());
    assert!(
        !hidden.assets.all_archived && hidden.assets.rows.is_empty(),
        "Assets holds nothing"
    );
    assert_eq!(written(&hidden.expenses.rows), ["1 Uncategorized built in"]);
    assert!(!hidden.expenses.all_archived);
    assert_eq!(hidden.archived, 1);
    let shown = chart::chart(&mut conn, today(), true)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list with the archived: {error}"));
    assert_eq!(written(&shown.income.rows), ["1 Old job archived"]);
    assert!(!shown.income.all_archived);
    drop(conn);
    assert_checked(&storage, "a book of archived income").await;
    storage.close().await;
}

/// While no account line with a figure shows, because Assets and Liabilities hold no account or
/// only archived ones that are hidden, the list has no balances line and no total; shown, an
/// archived account brings the figures back, and so does a live account under Liabilities alone.
///
/// Proves: Decision 131 with P8: while no account line with a figure shows, none under Assets and Liabilities or all archived and hidden, the list has no balances line and no total.
#[tokio::test]
async fn the_chart_of_a_book_without_accounts_shows_no_figure() {
    let (_dir, storage, initialized) = seeded(false).await;
    let mut conn = reader(&storage).await;
    let empty = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert!(!empty.balances_shown);
    assert_eq!((empty.assets.total, empty.liabilities.total), (None, None));
    assert!(empty.assets.rows.is_empty() && empty.liabilities.rows.is_empty());
    assert!(!empty.assets.all_archived && !empty.liabilities.all_archived);
    assert_eq!(empty.archived, 0);
    drop(conn);
    let mut tx = begin(&storage, "an archived jar").await;
    let assets = account_named(&mut tx, "Assets").await;
    let jar = create(&mut tx, initialized.admin.id, "Jar", assets, None).await;
    archive(&mut tx, initialized.admin.id, jar).await;
    commit(tx, "an archived jar").await;
    let mut conn = reader(&storage).await;
    let hidden = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert!(!hidden.balances_shown, "no figure shows");
    assert_eq!(
        (hidden.assets.total, hidden.liabilities.total),
        (None, None)
    );
    assert!(hidden.assets.all_archived);
    assert!(hidden.assets.rows.is_empty());
    assert!(!hidden.liabilities.all_archived);
    let shown = chart::chart(&mut conn, today(), true)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list with the archived: {error}"));
    assert!(shown.balances_shown);
    assert_eq!(written(&shown.assets.rows), ["1 Jar 0 archived"]);
    assert_eq!(shown.assets.total, Some(Money::ZERO));
    assert_eq!(shown.liabilities.total, Some(Money::ZERO));
    drop(conn);
    let mut tx = begin(&storage, "a card alone").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    create(
        &mut tx,
        initialized.admin.id,
        "Credit Card",
        liabilities,
        None,
    )
    .await;
    commit(tx, "a card alone").await;
    let mut conn = reader(&storage).await;
    let card_alone = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert!(
        card_alone.balances_shown,
        "a live account under Liabilities alone brings the figures"
    );
    assert_eq!(written(&card_alone.liabilities.rows), ["1 Credit Card 0"]);
    assert_eq!(
        (card_alone.assets.total, card_alone.liabilities.total),
        (Some(Money::ZERO), Some(Money::ZERO))
    );
    drop(conn);
    assert_checked(&storage, "an archived jar").await;
    storage.close().await;
}

/// An owner's colour is the user's place in the order users were created: the first user the
/// first of six colours, the sixth the sixth, the seventh the first again; a deactivation does
/// not move it, since nothing is stored.
///
/// Proves: Decision 138: the first user gets the first of six colours and the seventh the first again; nothing is stored.
#[tokio::test]
async fn owner_colours_follow_the_order_users_were_created() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "seven owners").await;
    let assets = account_named(&mut tx, "Assets").await;
    let mut owners = vec![admin];
    for number in 2..=7 {
        owners.push(
            user(
                &mut tx,
                admin,
                &format!("user{number}"),
                &format!("User {number}"),
            )
            .await,
        );
    }
    let mut accounts = Vec::new();
    for (number, owner) in (1..=7).zip(&owners) {
        accounts.push(
            create(
                &mut tx,
                admin,
                &format!("Account {number}"),
                assets,
                Some(*owner),
            )
            .await,
        );
    }
    test_support::deactivate_unchecked(&mut tx, owners[2], admin, later()).await;
    commit(tx, "seven owners").await;
    let mut conn = reader(&storage).await;
    let list = chart::chart(&mut conn, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    assert_eq!(OWNER_COLOURS, 6);
    assert_eq!(
        written(&list.assets.rows),
        [
            "1 Account 1 0 (Admin/1)",
            "1 Account 2 0 (User 2/2)",
            "1 Account 3 0 (User 3/3)",
            "1 Account 4 0 (User 4/4)",
            "1 Account 5 0 (User 5/5)",
            "1 Account 6 0 (User 6/6)",
            "1 Account 7 0 (User 7/1)",
        ]
    );
    let view = chart::account_view(&mut conn, today(), accounts[6])
        .await
        .unwrap_or_else(|error| panic!("cannot read Account 7: {error}"))
        .unwrap_or_else(|| panic!("Account 7 has no page"));
    assert_eq!(
        view.owner,
        Some(Owner::User(OwnerTag {
            display_name: String::from("User 7"),
            colour: 1,
        }))
    );
    drop(conn);
    assert_checked(&storage, "seven owners").await;
    storage.close().await;
}

/// An account's page holds its name, full place, the trail of the accounts that hold it, its
/// owner and balance under Assets and Liabilities, the accounts directly inside it with their
/// balances, archived ones marked, and the Other line of its own amount, in natural sign under
/// Liabilities too; a category's page has no owner and no figure.
///
/// Proves: Decision 133 with P8: the trail of holders, the full place, owner and balance under Assets and Liabilities, the accounts inside, archived ones marked, and the Other line of its own amount.
#[tokio::test]
async fn an_accounts_page_holds_its_trail_owner_balance_and_inside() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "an old jar").await;
    let old_jar = create(&mut tx, h.admin, "Old jar", h.savings, None).await;
    archive(&mut tx, h.admin, old_jar).await;
    create(&mut tx, h.admin, "Points", h.card, None).await;
    commit(tx, "an old jar").await;
    let mut conn = reader(&storage).await;
    let savings = chart::account_view(&mut conn, today(), h.savings)
        .await
        .unwrap_or_else(|error| panic!("cannot read Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no page"));
    assert_eq!(
        written(&savings.inside),
        [
            "1 Emergency fund 0",
            "1 Holiday fund 50000 (Alex/2)",
            "1 Old jar 0 archived",
            "1 <other> 10000",
        ]
    );
    assert_eq!(
        AccountView {
            inside: Vec::new(),
            ..savings
        },
        AccountView {
            as_of: today(),
            minor_digits: MinorDigits::TWO,
            id: h.savings,
            name: String::from("Savings"),
            place: String::from("Assets:Savings"),
            kind: Kind::Asset,
            word: Word::Account,
            trail: Vec::new(),
            built_in: None,
            archived: false,
            owner: Some(Owner::Joint),
            balance: Some(Money::from_minor(60_000)),
            opening_balance: savings.opening_balance,
            inside: Vec::new(),
            can_hold: true,
        }
    );
    let holiday = chart::account_view(&mut conn, today(), h.holiday)
        .await
        .unwrap_or_else(|error| panic!("cannot read Holiday fund: {error}"))
        .unwrap_or_else(|| panic!("Holiday fund has no page"));
    assert_eq!(
        holiday.trail,
        [TrailStep {
            id: h.savings,
            name: String::from("Savings"),
        }]
    );
    assert_eq!(holiday.place, "Assets:Savings:Holiday fund");
    assert_eq!(
        holiday.owner,
        Some(Owner::User(OwnerTag {
            display_name: String::from("Alex"),
            colour: 2,
        }))
    );
    assert_eq!(holiday.balance, Some(Money::from_minor(50_000)));
    assert!(holiday.inside.is_empty() && holiday.can_hold);
    let card = chart::account_view(&mut conn, today(), h.card)
        .await
        .unwrap_or_else(|error| panic!("cannot read the card: {error}"))
        .unwrap_or_else(|| panic!("the card has no page"));
    assert_eq!(card.balance, Some(Money::from_minor(120_480)));
    assert_eq!(card.owner, Some(Owner::Joint));
    assert_eq!(
        written(&card.inside),
        ["1 Points 0", "1 <other> 120480"],
        "the card's own amount owed in natural sign"
    );
    let health = chart::account_view(&mut conn, today(), h.health)
        .await
        .unwrap_or_else(|error| panic!("cannot read Health: {error}"))
        .unwrap_or_else(|| panic!("Health has no page"));
    assert_eq!((health.kind, health.word), (Kind::Expense, Word::Category));
    assert_eq!((health.owner, health.balance), (None, None));
    assert_eq!(written(&health.inside), ["1 dentist", "1 Optician"]);
    let checking = chart::account_view(&mut conn, today(), h.checking)
        .await
        .unwrap_or_else(|error| panic!("cannot read Checking: {error}"))
        .unwrap_or_else(|| panic!("Checking has no page"));
    assert_eq!(checking.balance, Some(Money::from_minor(245_015)));
    drop(conn);
    assert_checked(&storage, "an old jar").await;
    storage.close().await;
}

/// A built-in account's page carries its key and cannot hold another; an archived account's
/// page is marked and cannot hold another; a section and an unknown number have no page.
///
/// Proves: Decisions 133 and 135 with INV-11: a built-in account carries its key, an archived one its mark, and a root or an unknown number has no page.
#[tokio::test]
async fn a_built_in_or_archived_account_page_and_no_page_for_a_root() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "archived gifts").await;
    archive(&mut tx, h.admin, h.gifts).await;
    commit(tx, "archived gifts").await;
    let mut conn = reader(&storage).await;
    let opening = chart::account_view(&mut conn, today(), h.opening)
        .await
        .unwrap_or_else(|error| panic!("cannot read Opening Balances: {error}"))
        .unwrap_or_else(|| panic!("Opening Balances has no page"));
    assert_eq!(opening.built_in, Some(SystemKey::OpeningBalances));
    assert_eq!(opening.place, "Equity:Opening Balances");
    assert_eq!((opening.kind, opening.word), (Kind::Equity, Word::Account));
    assert_eq!((opening.owner, opening.balance), (None, None));
    assert!(!opening.can_hold && !opening.archived);
    let uncategorized = chart::account_view(&mut conn, today(), h.uncategorized)
        .await
        .unwrap_or_else(|error| panic!("cannot read Uncategorized: {error}"))
        .unwrap_or_else(|| panic!("Uncategorized has no page"));
    assert_eq!(uncategorized.built_in, Some(SystemKey::Uncategorized));
    assert_eq!(uncategorized.word, Word::Category);
    let gifts = chart::account_view(&mut conn, today(), h.gifts)
        .await
        .unwrap_or_else(|error| panic!("cannot read Gifts: {error}"))
        .unwrap_or_else(|| panic!("Gifts has no page"));
    assert!(gifts.archived && !gifts.can_hold);
    assert_eq!(gifts.built_in, None);
    for (id, label) in [
        (h.assets, "Assets"),
        (h.expenses, "Expenses"),
        (AccountId(999), "an unknown number"),
    ] {
        let page = chart::account_view(&mut conn, today(), id)
            .await
            .unwrap_or_else(|error| panic!("cannot look {label} up: {error}"));
        assert_eq!(page, None, "{label} has no page");
    }
    drop(conn);
    assert_checked(&storage, "archived gifts").await;
    storage.close().await;
}

/// An account at the sixth level, an archived account and a built-in account cannot hold
/// another, so their pages offer no New ... inside; one at the fifth level can.
///
/// Proves: Decision 133 with P5 and P13: an account at the sixth level, an archived account and a built-in account cannot hold another, so their pages offer no New inside.
#[tokio::test]
async fn an_account_at_the_sixth_level_can_hold_nothing() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "six levels").await;
    let mut parent = account_named(&mut tx, "Expenses").await;
    let mut chain = Vec::new();
    for name in ["Home", "Repairs", "Roof", "Gutters", "Leaves"] {
        parent = create(&mut tx, admin, name, parent, None).await;
        chain.push(parent);
    }
    let gifts = account_named(&mut tx, "Expenses:Gifts").await;
    archive(&mut tx, admin, gifts).await;
    let uncategorized = account_named(&mut tx, "Expenses:Uncategorized").await;
    commit(tx, "six levels").await;
    let mut conn = reader(&storage).await;
    let leaves = chart::account_view(&mut conn, today(), chain[4])
        .await
        .unwrap_or_else(|error| panic!("cannot read Leaves: {error}"))
        .unwrap_or_else(|| panic!("Leaves has no page"));
    assert!(!leaves.can_hold, "the sixth level holds nothing");
    assert_eq!(leaves.place, "Expenses:Home:Repairs:Roof:Gutters:Leaves");
    let trail: Vec<&str> = leaves.trail.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(trail, ["Home", "Repairs", "Roof", "Gutters"]);
    let gutters = chart::account_view(&mut conn, today(), chain[3])
        .await
        .unwrap_or_else(|error| panic!("cannot read Gutters: {error}"))
        .unwrap_or_else(|| panic!("Gutters has no page"));
    assert!(gutters.can_hold, "the fifth level holds another");
    for (id, label) in [
        (gifts, "an archived category"),
        (uncategorized, "a built-in category"),
    ] {
        let view = chart::account_view(&mut conn, today(), id)
            .await
            .unwrap_or_else(|error| panic!("cannot read {label}: {error}"))
            .unwrap_or_else(|| panic!("{label} has no page"));
        assert!(!view.can_hold, "{label} holds nothing");
    }
    drop(conn);
    assert_checked(&storage, "six levels").await;
    storage.close().await;
}

/// The create form's lists: the places of one section in the order of the list page, each in
/// full, without archived places, built-in accounts or places at the sixth level; the owners
/// under Assets and Liabilities only, every active user by the fold of the display name, then of
/// the username.
///
/// Proves: Decision 132 with P5: places of one section in the list page's order, in full, archived places, built-in accounts and sixth-level places left out; Joint then active users by the fold.
#[tokio::test]
async fn the_pick_lists_offer_the_places_and_the_owners() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "the lists").await;
    let mut parent = h.expenses;
    for name in ["Home", "Repairs", "Roof", "Gutters", "Leaves"] {
        parent = create(&mut tx, h.admin, name, parent, None).await;
    }
    archive(&mut tx, h.admin, h.gifts).await;
    user(&mut tx, h.admin, "bea", "bea").await;
    let robin_zoe = user(&mut tx, h.admin, "zoe", "Robin").await;
    let robin_rob = user(&mut tx, h.admin, "rob", "Robin").await;
    let zed = user(&mut tx, h.admin, "zed", "Zed").await;
    test_support::deactivate_unchecked(&mut tx, zed, h.admin, later()).await;
    commit(tx, "the lists").await;
    let mut conn = reader(&storage).await;
    let expenses = chart::new_form(&mut conn, Kind::Expense, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the Expenses form: {error}"));
    assert_eq!(
        (expenses.kind, expenses.word),
        (Kind::Expense, Word::Category)
    );
    assert_eq!(
        expenses.section,
        Place {
            id: h.expenses,
            full_name: String::from("Expenses"),
            archived: false,
        }
    );
    assert_eq!(
        named(&expenses.places),
        [
            "Expenses:Dining",
            "Expenses:Entertainment",
            "Expenses:Fees",
            "Expenses:Groceries",
            "Expenses:Health",
            "Expenses:Health:dentist",
            "Expenses:Health:Optician",
            "Expenses:Home",
            "Expenses:Home:Repairs",
            "Expenses:Home:Repairs:Roof",
            "Expenses:Home:Repairs:Roof:Gutters",
            "Expenses:Housing",
            "Expenses:Other",
            "Expenses:Shopping",
            "Expenses:Subscriptions",
            "Expenses:Transport",
            "Expenses:Travel",
            "Expenses:Utilities",
        ]
    );
    assert_eq!(expenses.owners, None, "a category has no owner");
    let assets = chart::new_form(&mut conn, Kind::Asset, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the Assets form: {error}"));
    assert_eq!(assets.word, Word::Account);
    assert_eq!(
        named(&assets.places),
        [
            "Assets:Cash",
            "Assets:Checking",
            "Assets:Savings",
            "Assets:Savings:Emergency fund",
            "Assets:Savings:Holiday fund",
        ]
    );
    let owners = assets
        .owners
        .unwrap_or_else(|| panic!("the Assets form has no owners"));
    let names: Vec<(&str, bool)> = owners
        .iter()
        .map(|owner| (owner.display_name.as_str(), owner.deactivated))
        .collect();
    assert_eq!(
        names,
        [
            ("Admin", false),
            ("Alex", false),
            ("bea", false),
            ("Robin", false),
            ("Robin", false),
            ("Sam", false)
        ]
    );
    assert_eq!(owners[1].id, h.alex);
    assert_eq!(
        (owners[3].id, owners[4].id),
        (robin_rob, robin_zoe),
        "the same display name by the username"
    );
    assert_eq!(owners[5].id, h.sam);
    let income = chart::new_form(&mut conn, Kind::Income, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the Income form: {error}"));
    assert_eq!(named(&income.places), ["Income:Other", "Income:Salary"]);
    assert_eq!(income.section.id, h.income);
    drop(conn);
    assert_checked(&storage, "the lists").await;
    storage.close().await;
}

/// The change form offers no place that is the account, inside it, archived, built in or too
/// deep for what it holds; a deactivated owner only on the account already theirs, marked; the
/// form of a built-in or an archived account holds the name alone; it shows what is saved.
///
/// Proves: Decision 134 with P4 and P5: no place that is the account, inside it, archived, built in or too deep for what it holds; a deactivated owner only on its own account; an archived account's form the name alone.
#[tokio::test]
async fn the_change_form_leaves_out_the_account_and_what_is_inside_it() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "the change forms").await;
    let mut parent = h.checking;
    for name in ["Level three", "Level four", "Level five"] {
        parent = create(&mut tx, h.admin, name, parent, None).await;
    }
    let old = create(&mut tx, h.admin, "Old savings", h.assets, None).await;
    archive(&mut tx, h.admin, old).await;
    let zed = user(&mut tx, h.admin, "zed", "Zed").await;
    set_owner(&mut tx, h.admin, later(), h.savings, Some(zed))
        .await
        .unwrap_or_else(|error| panic!("cannot give Savings to Zed: {error}"));
    test_support::deactivate_unchecked(&mut tx, zed, h.admin, later()).await;
    commit(tx, "the change forms").await;
    let mut conn = reader(&storage).await;
    let savings = chart::edit_form(&mut conn, h.savings, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no form"));
    assert_eq!(
        named(savings.places.as_deref().unwrap_or_default()),
        [
            "Assets:Cash",
            "Assets:Checking",
            "Assets:Checking:Level three",
            "Assets:Checking:Level three:Level four",
        ]
    );
    assert_eq!(savings.section.full_name, "Assets");
    assert_eq!(
        savings.saved,
        AccountBoxes {
            name: String::from("Savings"),
            parent: Some(h.assets),
            owner: Some(zed),
        }
    );
    let owners: Vec<(&str, bool)> = savings
        .owners
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|owner| (owner.display_name.as_str(), owner.deactivated))
        .collect();
    assert_eq!(
        owners,
        [
            ("Admin", false),
            ("Alex", false),
            ("Sam", false),
            ("Zed", true)
        ]
    );
    assert!(!savings.archived && savings.built_in.is_none());
    let checking = chart::edit_form(&mut conn, h.checking, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Checking: {error}"))
        .unwrap_or_else(|| panic!("Checking has no form"));
    let owners: Vec<&str> = checking
        .owners
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|owner| owner.display_name.as_str())
        .collect();
    assert_eq!(owners, ["Admin", "Alex", "Sam"], "Zed only on Savings");
    let emergency = chart::edit_form(&mut conn, h.emergency, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Emergency fund: {error}"))
        .unwrap_or_else(|| panic!("Emergency fund has no form"));
    assert_eq!(
        named(emergency.places.as_deref().unwrap_or_default()),
        [
            "Assets:Cash",
            "Assets:Checking",
            "Assets:Checking:Level three",
            "Assets:Checking:Level three:Level four",
            "Assets:Checking:Level three:Level four:Level five",
            "Assets:Savings",
            "Assets:Savings:Holiday fund",
        ]
    );
    assert_eq!(
        emergency.trail,
        [TrailStep {
            id: h.savings,
            name: String::from("Savings"),
        }]
    );
    let groceries = chart::edit_form(&mut conn, h.groceries, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Groceries: {error}"))
        .unwrap_or_else(|| panic!("Groceries has no form"));
    assert_eq!(groceries.word, Word::Category);
    assert_eq!(groceries.owners, None, "a category has no owner");
    let places = named(groceries.places.as_deref().unwrap_or_default());
    assert!(
        !places.contains(&String::from("Expenses:Groceries"))
            && !places.contains(&String::from("Expenses:Uncategorized"))
            && places.contains(&String::from("Expenses:Health:dentist")),
        "{places:?}"
    );
    for (id, label) in [
        (old, "an archived account"),
        (h.opening, "a built-in account"),
    ] {
        let form = chart::edit_form(&mut conn, id, Kept::default())
            .await
            .unwrap_or_else(|error| panic!("cannot read the form of {label}: {error}"))
            .unwrap_or_else(|| panic!("{label} has no form"));
        assert_eq!(
            (form.places, form.owners),
            (None, None),
            "{label}: the name alone"
        );
    }
    let opening = chart::edit_form(&mut conn, h.opening, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Opening Balances: {error}"))
        .unwrap_or_else(|| panic!("Opening Balances has no form"));
    assert_eq!(opening.built_in, Some(SystemKey::OpeningBalances));
    let old_form = chart::edit_form(&mut conn, old, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Old savings: {error}"))
        .unwrap_or_else(|| panic!("Old savings has no form"));
    assert!(old_form.archived);
    for (id, label) in [
        (h.assets, "a section"),
        (AccountId(999), "an unknown number"),
    ] {
        let form = chart::edit_form(&mut conn, id, Kept::default())
            .await
            .unwrap_or_else(|error| panic!("cannot look {label} up: {error}"));
        assert_eq!(form, None, "{label} has no form");
    }
    drop(conn);
    assert_checked(&storage, "the change forms").await;
    storage.close().await;
}

/// A place archived meanwhile and a deactivated owner that a returned form holds stay in their
/// lists as one extra choice each, in their order, marked; a kept choice the lists offer anyway
/// is not doubled, and one of another section or a built-in account is not added; the change
/// form keeps the account itself, or a place that went inside it meanwhile, as one extra choice,
/// so that the next send is refused again instead of moving the account where nobody chose.
///
/// Proves: Decisions 132 and 134 with P5: a place archived meanwhile or a deactivated owner a returned form holds stays in its list as one extra choice, marked archived or deactivated.
#[tokio::test]
async fn a_returned_form_keeps_a_choice_the_lists_no_longer_offer() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "kept choices").await;
    let old = create(&mut tx, h.admin, "Old savings", h.assets, None).await;
    archive(&mut tx, h.admin, old).await;
    let zed = user(&mut tx, h.admin, "zed", "Zed").await;
    test_support::deactivate_unchecked(&mut tx, zed, h.admin, later()).await;
    commit(tx, "kept choices").await;
    let mut conn = reader(&storage).await;
    let kept = Kept {
        place: Some(old),
        owner: Some(zed),
    };
    let form = chart::new_form(&mut conn, Kind::Asset, kept)
        .await
        .unwrap_or_else(|error| panic!("cannot read the returned form: {error}"));
    assert_eq!(
        named(&form.places),
        [
            "Assets:Cash",
            "Assets:Checking",
            "Assets:Old savings (archived)",
            "Assets:Savings",
            "Assets:Savings:Emergency fund",
            "Assets:Savings:Holiday fund",
        ]
    );
    let owners: Vec<(&str, bool)> = form
        .owners
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|owner| (owner.display_name.as_str(), owner.deactivated))
        .collect();
    assert_eq!(
        owners,
        [
            ("Admin", false),
            ("Alex", false),
            ("Sam", false),
            ("Zed", true)
        ]
    );
    let edit = chart::edit_form(&mut conn, h.checking, kept)
        .await
        .unwrap_or_else(|error| panic!("cannot read the returned change form: {error}"))
        .unwrap_or_else(|| panic!("Checking has no form"));
    assert!(
        named(edit.places.as_deref().unwrap_or_default())
            .contains(&String::from("Assets:Old savings (archived)"))
    );
    assert!(
        edit.owners
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|owner| owner.id == zed && owner.deactivated)
    );
    let plain = chart::new_form(&mut conn, Kind::Asset, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the fresh form: {error}"));
    for (kept, label) in [
        (
            Kept {
                place: Some(h.checking),
                owner: Some(h.alex),
            },
            "choices the lists offer",
        ),
        (
            Kept {
                place: Some(h.groceries),
                owner: None,
            },
            "a place of another section",
        ),
    ] {
        let form = chart::new_form(&mut conn, Kind::Asset, kept)
            .await
            .unwrap_or_else(|error| panic!("cannot read the form with {label}: {error}"));
        assert_eq!(form, plain, "{label}");
    }
    let plain_expenses = chart::new_form(&mut conn, Kind::Expense, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the fresh Expenses form: {error}"));
    let kept_built_in = chart::new_form(
        &mut conn,
        Kind::Expense,
        Kept {
            place: Some(h.uncategorized),
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the form keeping Uncategorized: {error}"));
    assert_eq!(
        kept_built_in, plain_expenses,
        "a built-in account kept is not added"
    );
    let plain_savings = chart::edit_form(&mut conn, h.savings, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no form"));
    let plain_places = named(plain_savings.places.as_deref().unwrap_or_default());
    assert!(
        !plain_places.contains(&String::from("Assets:Savings:Emergency fund")),
        "{plain_places:?}"
    );
    for (place, full_name, label) in [
        (h.savings, "Assets:Savings", "the account itself"),
        (
            h.emergency,
            "Assets:Savings:Emergency fund",
            "a place that went inside it",
        ),
    ] {
        let form = chart::edit_form(
            &mut conn,
            h.savings,
            Kept {
                place: Some(place),
                owner: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("cannot read the form keeping {label}: {error}"))
        .unwrap_or_else(|| panic!("Savings has no form"));
        let places = named(form.places.as_deref().unwrap_or_default());
        assert_eq!(
            places.iter().filter(|kept| *kept == full_name).count(),
            1,
            "{label} kept as one extra choice: {places:?}"
        );
        assert_eq!(places.len(), plain_places.len() + 1, "{label}: {places:?}");
    }
    drop(conn);
    assert_checked(&storage, "kept choices").await;
    storage.close().await;
}

/// The models of the list, an account's page, a create form and a change form of the household,
/// read in one read transaction, as golden snapshots of their Debug renderings.
///
/// Proves: Decision 120's pattern for 131 to 134: golden snapshots of the list, an account page and the pick lists, read in one read transaction.
#[tokio::test]
async fn the_models_of_the_account_pages_golden() {
    let (_dir, storage, initialized) = seeded(true).await;
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = storage
        .begin_read()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a read transaction: {error}"));
    let list = chart::chart(&mut tx, today(), false)
        .await
        .unwrap_or_else(|error| panic!("cannot read the list: {error}"));
    let page = chart::account_view(&mut tx, today(), h.savings)
        .await
        .unwrap_or_else(|error| panic!("cannot read Savings: {error}"));
    let new = chart::new_form(&mut tx, Kind::Asset, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the Assets form: {error}"));
    let edit = chart::edit_form(&mut tx, h.holiday, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Holiday fund: {error}"));
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot end the read transaction: {error}"));
    insta::assert_debug_snapshot!("account_pages_list", list);
    insta::assert_debug_snapshot!("account_pages_savings", page);
    insta::assert_debug_snapshot!("account_pages_new_account", new);
    insta::assert_debug_snapshot!("account_pages_change_holiday_fund", edit);
    assert_checked(&storage, "the household").await;
    storage.close().await;
}

/// The home page's empty state says whether Assets and Liabilities hold only archived accounts:
/// not for a book without any, nor for one whose only archived account is a category; yes once
/// every account under Assets and Liabilities is archived; never while figures show.
///
/// Proves: Decision 137 amending 119: the home model's empty state says whether Assets and Liabilities hold only archived accounts.
#[tokio::test]
async fn home_tells_an_all_archived_book_from_an_empty_one() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "an archived category").await;
    let income = account_named(&mut tx, "Income").await;
    let old_job = create(&mut tx, admin, "Old job", income, None).await;
    archive(&mut tx, admin, old_job).await;
    commit(tx, "an archived category").await;
    let mut conn = reader(&storage).await;
    let empty = home::home_page(&mut conn, today())
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page: {error}"));
    assert_eq!(empty.balances, None);
    assert!(!empty.all_archived, "a book without accounts");
    drop(conn);
    let mut tx = begin(&storage, "archived accounts").await;
    let assets = account_named(&mut tx, "Assets").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let jar = create(&mut tx, admin, "Jar", assets, None).await;
    let loan = create(&mut tx, admin, "Old loan", liabilities, None).await;
    archive(&mut tx, admin, jar).await;
    archive(&mut tx, admin, loan).await;
    commit(tx, "archived accounts").await;
    let mut conn = reader(&storage).await;
    let archived = home::home_page(&mut conn, today())
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page: {error}"));
    assert_eq!(archived.balances, None);
    assert!(archived.all_archived, "every account is archived");
    drop(conn);
    let mut tx = begin(&storage, "a live account").await;
    create(&mut tx, admin, "Wallet", liabilities, None).await;
    commit(tx, "a live account").await;
    let mut conn = reader(&storage).await;
    let live = home::home_page(&mut conn, today())
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page: {error}"));
    assert!(live.balances.is_some());
    assert!(!live.all_archived);
    drop(conn);
    assert_checked(&storage, "the archived and live accounts").await;
    storage.close().await;
}

/// The account `id` as the opening-balance pages read it.
async fn read(conn: &mut sqlx::SqliteConnection, id: AccountId) -> Option<OpeningBalanceAccount> {
    chart::opening_balance_account(conn, id)
        .await
        .unwrap_or_else(|error| panic!("cannot read account {id}: {error}"))
}

/// The page of `id` as of today, which must exist.
async fn view(conn: &mut sqlx::SqliteConnection, id: AccountId, label: &str) -> AccountView {
    chart::account_view(conn, today(), id)
        .await
        .unwrap_or_else(|error| panic!("cannot read {label}: {error}"))
        .unwrap_or_else(|| panic!("{label} has no page"))
}

/// An account's page under Assets or Liabilities that is neither built in nor archived names its
/// live opening balance in natural sign with its day (a card's amount owed, an overdrawn
/// account's below zero), or none yet, also after a removal and on an account inside another;
/// a category, a built-in account and an archived account name none.
///
/// Proves: Decision 141 with A7: an Assets or Liabilities account's view carries its live opening balance in natural sign with its day, or none yet after a removal, and a category, a built-in account and an archived account carry none.
#[tokio::test]
async fn an_accounts_page_names_its_opening_balance() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut conn = reader(&storage).await;
    let fresh = account_named(&mut conn, "Assets:Checking").await;
    assert_eq!(
        view(&mut conn, fresh, "Checking before any entry")
            .await
            .opening_balance,
        Some(OpeningFact::NoneYet),
        "an account without one yet"
    );
    drop(conn);
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "more opening balances").await;
    let wallet = create(&mut tx, h.admin, "Wallet", h.assets, None).await;
    open_balance(&mut tx, h.admin, later(), wallet, "2026-02-01", -1_500).await;
    let old = create(&mut tx, h.admin, "Old jar", h.assets, None).await;
    archive(&mut tx, h.admin, old).await;
    let savings_entry = transactions::opening_balance(&mut tx, h.savings)
        .await
        .unwrap_or_else(|error| panic!("cannot read the opening balance of Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no opening balance"));
    transactions::remove_opening_balance(
        &mut tx,
        h.admin,
        later(),
        savings_entry.id,
        savings_entry.version,
    )
    .await
    .unwrap_or_else(|error| panic!("cannot remove the opening balance of Savings: {error}"));
    commit(tx, "more opening balances").await;
    let mut conn = reader(&storage).await;
    let entered = |on: &str, natural: i64| {
        Some(OpeningFact::Entered {
            date: date(on),
            amount: Money::from_minor(natural),
        })
    };
    for (id, label, expected) in [
        (h.checking, "Checking", entered("2026-01-01", 248_015)),
        (h.card, "the card", entered("2026-01-01", 120_480)),
        (wallet, "an overdrawn wallet", entered("2026-02-01", -1_500)),
        (
            h.savings,
            "Savings after a removal",
            Some(OpeningFact::NoneYet),
        ),
        (h.emergency, "Emergency fund", Some(OpeningFact::NoneYet)),
        (h.groceries, "a category", None),
        (h.opening, "a built-in account", None),
        (old, "an archived account", None),
    ] {
        assert_eq!(
            view(&mut conn, id, label).await.opening_balance,
            expected,
            "{label}"
        );
    }
    drop(conn);
    assert_checked(&storage, "the opening balances").await;
    storage.close().await;
}

/// The read of the opening-balance pages gives an account that takes one its name, full place,
/// trail, section, the book's digits, the archived mark and its live opening balance with entry
/// and version, or none; a category and a built-in account are read but take none; an archived
/// account is marked; a section and an unknown number have no read. The read never touches the
/// roll-up: over a book whose totals no longer fit in 64 bits (built through the core's
/// operations: the largest amount the book can hold paid into Cash from income, beside the
/// household's own balances), where an account's page cannot be read, it still answers.
///
/// Proves: Decisions 140 and 141 with the pause's P5: the read gives an account that takes one its full place, trail, kind, the book's digits and its live opening balance with entry and version, or none; a category and a built-in account take none; an archived account is marked; a section and an unknown number have no read; the roll-up is never read.
#[tokio::test]
async fn the_opening_balance_pages_read_the_account_and_its_opening_balance() {
    let (_dir, storage, initialized) = seeded(true).await;
    let mut conn = reader(&storage).await;
    let fresh = account_named(&mut conn, "Assets:Cash").await;
    assert_eq!(
        read(&mut conn, fresh).await,
        Some(OpeningBalanceAccount {
            id: fresh,
            name: String::from("Cash"),
            place: String::from("Assets:Cash"),
            trail: Vec::new(),
            kind: Kind::Asset,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: true,
            opening_balance: None,
        }),
        "an account without one yet"
    );
    drop(conn);
    let h = household(&storage, initialized.admin.id).await;
    let mut tx = begin(&storage, "an archived jar").await;
    let old = create(&mut tx, h.admin, "Old jar", h.savings, None).await;
    archive(&mut tx, h.admin, old).await;
    commit(tx, "an archived jar").await;
    let mut conn = reader(&storage).await;
    let checking_entry = transactions::opening_balance(&mut conn, h.checking)
        .await
        .unwrap_or_else(|error| panic!("cannot read the opening balance of Checking: {error}"));
    assert!(checking_entry.is_some());
    let card_entry = transactions::opening_balance(&mut conn, h.card)
        .await
        .unwrap_or_else(|error| panic!("cannot read the opening balance of the card: {error}"));
    let savings = TrailStep {
        id: h.savings,
        name: String::from("Savings"),
    };
    let expected = [
        OpeningBalanceAccount {
            id: h.checking,
            name: String::from("Checking"),
            place: String::from("Assets:Checking"),
            trail: Vec::new(),
            kind: Kind::Asset,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: true,
            opening_balance: checking_entry,
        },
        OpeningBalanceAccount {
            id: h.card,
            name: String::from("Credit Card"),
            place: String::from("Liabilities:Credit Card"),
            trail: Vec::new(),
            kind: Kind::Liability,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: true,
            opening_balance: card_entry,
        },
        OpeningBalanceAccount {
            id: h.holiday,
            name: String::from("Holiday fund"),
            place: String::from("Assets:Savings:Holiday fund"),
            trail: vec![savings.clone()],
            kind: Kind::Asset,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: true,
            opening_balance: None,
        },
        OpeningBalanceAccount {
            id: old,
            name: String::from("Old jar"),
            place: String::from("Assets:Savings:Old jar"),
            trail: vec![savings],
            kind: Kind::Asset,
            minor_digits: MinorDigits::TWO,
            archived: true,
            takes_opening_balance: true,
            opening_balance: None,
        },
        OpeningBalanceAccount {
            id: h.groceries,
            name: String::from("Groceries"),
            place: String::from("Expenses:Groceries"),
            trail: Vec::new(),
            kind: Kind::Expense,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: false,
            opening_balance: None,
        },
        OpeningBalanceAccount {
            id: h.opening,
            name: String::from("Opening Balances"),
            place: String::from("Equity:Opening Balances"),
            trail: Vec::new(),
            kind: Kind::Equity,
            minor_digits: MinorDigits::TWO,
            archived: false,
            takes_opening_balance: false,
            opening_balance: None,
        },
    ];
    for account in &expected {
        assert_eq!(
            read(&mut conn, account.id).await.as_ref(),
            Some(account),
            "{}",
            account.place
        );
    }
    for (id, label) in [
        (h.assets, "a section"),
        (h.expenses, "another section"),
        (AccountId(999), "an unknown number"),
    ] {
        assert_eq!(read(&mut conn, id).await, None, "{label} has no read");
    }
    drop(conn);
    assert_checked(&storage, "the household").await;

    let mut tx = begin(&storage, "a total that does not fit").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    post(
        &mut tx,
        h.admin,
        later(),
        "2026-03-02",
        &[(cash, i64::MAX), (h.salary, -i64::MAX)],
    )
    .await;
    commit(tx, "a total that does not fit").await;
    let mut conn = reader(&storage).await;
    assert!(
        chart::account_view(&mut conn, today(), h.checking)
            .await
            .is_err(),
        "the roll-up of Assets no longer fits"
    );
    assert_eq!(
        read(&mut conn, h.checking).await.as_ref(),
        Some(&expected[0]),
        "the read of the opening-balance pages answers without the roll-up"
    );
    drop(conn);
    storage.close().await;
}

/// The accounts of the starter chart the test of leftover N6 uses, and the admin.
struct Places {
    admin: UserId,
    checking: AccountId,
    savings: AccountId,
    groceries: AccountId,
    uncategorized: AccountId,
}

/// A refused change whose kept place another save has since moved one level too deep for the
/// account and what it holds, and one whose kept place is a built-in account, each come back
/// with that place in the change form's list, once, so that the next send is refused again
/// instead of moving the account where nobody chose.
///
/// Proves: Decision 148 with 134, leftover N6 and the pause's P7: a refused change whose kept place another save moved one level too deep, and one whose kept place is built in, each come back with that place in the change form's list; red against the scaffold's planted N6, the assertion's message carrying the stub.
#[tokio::test]
async fn a_returned_change_form_keeps_a_place_too_deep_now_or_built_in() {
    const STUB: &str = "core: a returned change form keeps a place too deep now or built in";
    let (_dir, storage, initialized) = seeded(true).await;
    let mut tx = begin(&storage, "a place moved deeper").await;
    let h = Places {
        admin: initialized.admin.id,
        checking: account_named(&mut tx, "Assets:Checking").await,
        savings: account_named(&mut tx, "Assets:Savings").await,
        groceries: account_named(&mut tx, "Expenses:Groceries").await,
        uncategorized: account_named(&mut tx, "Expenses:Uncategorized").await,
    };
    create(&mut tx, h.admin, "Emergency fund", h.savings, None).await;
    let level_three = create(&mut tx, h.admin, "Level three", h.checking, None).await;
    let level_four = create(&mut tx, h.admin, "Level four", level_three, None).await;
    let wallet = create(&mut tx, h.admin, "Wallet", h.checking, None).await;
    commit(tx, "a place moved deeper").await;

    let mut conn = reader(&storage).await;
    let before = chart::edit_form(&mut conn, h.savings, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no form"));
    assert!(
        named(before.places.as_deref().unwrap_or_default())
            .contains(&String::from("Assets:Checking:Wallet")),
        "Savings, holding two levels, fits inside Wallet before the move"
    );
    drop(conn);
    let mut tx = begin(&storage, "the other save").await;
    move_account(&mut tx, h.admin, later(), wallet, level_four)
        .await
        .unwrap_or_else(|error| panic!("cannot move Wallet: {error}"));
    commit(tx, "the other save").await;

    let mut conn = reader(&storage).await;
    let plain = chart::edit_form(&mut conn, h.savings, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Savings: {error}"))
        .unwrap_or_else(|| panic!("Savings has no form"));
    let plain_places = named(plain.places.as_deref().unwrap_or_default());
    let too_deep = "Assets:Checking:Level three:Level four:Wallet";
    assert!(
        !plain_places.contains(&String::from(too_deep)),
        "Wallet is too deep for Savings now: {plain_places:?}"
    );
    let kept_deep = chart::edit_form(
        &mut conn,
        h.savings,
        Kept {
            place: Some(wallet),
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the form keeping Wallet: {error}"))
    .unwrap_or_else(|| panic!("Savings has no form"));
    let places = named(kept_deep.places.as_deref().unwrap_or_default());
    assert_eq!(
        places.iter().filter(|place| *place == too_deep).count(),
        1,
        "{STUB}: the place too deep now is kept as one extra choice: {places:?}"
    );
    assert_eq!(
        places.len(),
        plain_places.len() + 1,
        "{STUB}: one extra choice: {places:?}"
    );

    let plain = chart::edit_form(&mut conn, h.groceries, Kept::default())
        .await
        .unwrap_or_else(|error| panic!("cannot read the form of Groceries: {error}"))
        .unwrap_or_else(|| panic!("Groceries has no form"));
    let plain_places = named(plain.places.as_deref().unwrap_or_default());
    assert!(!plain_places.contains(&String::from("Expenses:Uncategorized")));
    let kept_built_in = chart::edit_form(
        &mut conn,
        h.groceries,
        Kept {
            place: Some(h.uncategorized),
            owner: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("cannot read the form keeping Uncategorized: {error}"))
    .unwrap_or_else(|| panic!("Groceries has no form"));
    let places = named(kept_built_in.places.as_deref().unwrap_or_default());
    assert_eq!(
        places
            .iter()
            .filter(|place| *place == "Expenses:Uncategorized")
            .count(),
        1,
        "{STUB}: the built-in place is kept as one extra choice: {places:?}"
    );
    assert_eq!(
        places.len(),
        plain_places.len() + 1,
        "{STUB}: one extra choice: {places:?}"
    );
    drop(conn);
    assert_checked(&storage, "the moved place").await;
    storage.close().await;
}
