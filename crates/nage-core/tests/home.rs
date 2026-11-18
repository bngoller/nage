//! The home page's model (SPEC 6.2, 6.3, 6.10, G7, W1, INV-4, INV-25, decisions 119 and 120):
//! the Assets and Liabilities balances at the end of a day, grouped, ordered, signed and with
//! the archived accounts hidden, as `nage_core::home::home_page` reads them. Every book is built
//! through the core's operations (decisions 98 and 104): the accounts by `create_account`, the
//! users by `create_user`, the amounts by the opening-balance operations and
//! `create_transaction`; no test writes a row with unchecked SQL but the one deactivation that
//! `test_support::deactivate_unchecked` plants until milestone 8 slice 1's operation exists
//! (decisions 98 and 115). The tests that build accounts or entries of their own run
//! `nage check` over the ledger, except the empty state's test, the overflow tests, since
//! SQLite's own sums can stop on such amounts (decision 99), and the property, which holds
//! each random ledger against decision 96's kind totals instead.

mod common;

use nage_core::accounts::{self, AccountId, Kind, NewAccount};
use nage_core::balances;
use nage_core::home::{
    self, GroupHeading, Home, HomeBalances, HomeError, Line, OtherLine, Section,
};
use nage_core::money::{MinorDigits, Money};
use nage_core::setup::{Setup, initialize};
use nage_core::storage::{Storage, WriteTransaction};
use nage_core::test_support::{self, open_migrated};
use nage_core::transactions;
use nage_core::users::{self, NewUser, UserId};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;

use common::{
    TODAY_TEXT, TOMORROW_TEXT, account_named, assert_sound, begin, block_on, check_report, commit,
    currency, date, later, money, now, open_balance, post, reader, seeded, standard_setup, today,
};

/// The home page's model as of `as_of` through the storage's read pool.
async fn home_as_of(storage: &Storage, as_of: &str) -> Home {
    let mut conn = reader(storage).await;
    home::home_page(&mut conn, date(as_of))
        .await
        .unwrap_or_else(|error| panic!("cannot read the home page as of {as_of}: {error}"))
}

/// The figures of `home`, which must have some.
#[track_caller]
fn figures(home: &Home) -> &HomeBalances {
    home.balances
        .as_ref()
        .unwrap_or_else(|| panic!("the home page is in its empty state: {home:?}"))
}

/// `section` written out one text per group heading and per line, in order: `[Joint]` or
/// `[<display name>]` for a heading, `<level> <name> <minor units>` for an account's line,
/// followed by ` (<owner>)` when the line names its owner, and `<level> <other> <minor units>`
/// for an Other line, which an account named Other cannot be taken for.
fn written(section: &Section) -> Vec<String> {
    let mut out = Vec::new();
    for group in &section.groups {
        match &group.heading {
            Some(GroupHeading::Joint) => out.push(String::from("[Joint]")),
            Some(GroupHeading::Owner(name)) => out.push(format!("[{name}]")),
            None => {}
        }
        for line in &group.lines {
            match line {
                Line::Account(line) => {
                    let owner = line
                        .owner
                        .as_ref()
                        .map(|owner| format!(" ({owner})"))
                        .unwrap_or_default();
                    out.push(format!(
                        "{} {} {}{owner}",
                        line.level,
                        line.name,
                        line.amount.minor()
                    ));
                }
                Line::Other(other) => {
                    out.push(format!("{} <other> {}", other.level, other.amount.minor()));
                }
            }
        }
    }
    out
}

/// The level of `line`, an account's or an Other line.
fn level(line: &Line) -> u8 {
    match line {
        Line::Account(line) => line.level,
        Line::Other(other) => other.level,
    }
}

/// The amount of `line`, an account's or an Other line.
fn amount(line: &Line) -> Money {
    match line {
        Line::Account(line) => line.amount,
        Line::Other(other) => other.amount,
    }
}

/// Creates the account `name` under `parent` for `owner` (`None` is joint) by `by` at
/// [`later`], panicking on a refusal.
async fn account(
    tx: &mut WriteTransaction,
    by: UserId,
    parent: AccountId,
    name: &str,
    owner: Option<UserId>,
) -> AccountId {
    accounts::create_account(
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
    .unwrap_or_else(|error| panic!("cannot create the account {name}: {error}"))
    .id
}

/// Creates the user `username` with the display name `display_name`, not an admin, by `admin`
/// at [`later`], panicking on a refusal.
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

/// Right after a set-up with the starter accounts, the home page's model names the day it was
/// asked for and the book's minor digits, and holds the two sections under their roots' names:
/// every starter account of Assets and of Liabilities has a line at zero, in the order of
/// their names and not in the order they were created in (Checking, Savings, Cash), each
/// directly under its root; the totals and the net worth are zero; and since nobody owns an
/// account, each section is one group without a heading. Equity, Income and Expenses have no
/// line anywhere.
///
/// Proves: decision 119 (Assets and Liabilities only, under their roots' names; an account
/// with nothing in it shows a zero amount; no group heading when no listed account has an
/// owner; the order by name), decision 120 (the model carries the day it was asked for),
/// SPEC W1 (the home page of a book set up with the starter accounts).
#[tokio::test]
async fn the_starter_accounts_are_listed_at_zero_without_group_headings() {
    let (_dir, storage, _initialized) = seeded(true).await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(home.as_of, today(), "the day the balances are for");
    assert_eq!(
        home.minor_digits,
        MinorDigits::TWO,
        "the book's minor digits"
    );
    let figures = figures(&home);
    assert_eq!(figures.assets.kind, Kind::Asset, "the first section's kind");
    assert_eq!(figures.assets.name, "Assets", "the first section's name");
    assert_eq!(
        figures.liabilities.kind,
        Kind::Liability,
        "the second section's kind"
    );
    assert_eq!(
        figures.liabilities.name, "Liabilities",
        "the second section's name"
    );
    assert_eq!(
        written(&figures.assets),
        ["1 Cash 0", "1 Checking 0", "1 Savings 0"],
        "the Assets section"
    );
    assert_eq!(
        written(&figures.liabilities),
        ["1 Credit Card 0"],
        "the Liabilities section"
    );
    assert_eq!(figures.assets.groups.len(), 1, "groups under Assets");
    assert_eq!(
        figures.liabilities.groups.len(),
        1,
        "groups under Liabilities"
    );
    assert_eq!(figures.assets.total, Money::ZERO, "the Assets total");
    assert_eq!(
        figures.liabilities.total,
        Money::ZERO,
        "the Liabilities total"
    );
    assert_eq!(figures.net_worth, Money::ZERO, "the net worth");
}

/// A book set up without the starter accounts holds only the roots and the three other system
/// accounts, none of them under Assets or Liabilities, so the model is the empty state: no
/// figure at all, with the day and the minor digits still named. An account that is archived
/// is not one to list, so a book whose only Assets account is archived is still in the empty
/// state. With one joint account under Liabilities and none under Assets, both sections are
/// there: Assets with its zero total and no group, Liabilities with its line and no group
/// heading, since the archived account's owner, Amy, owns no listed account.
///
/// Proves: decision 119 (the empty state when Assets and Liabilities hold no account to list,
/// as after a set-up that declined the starter accounts; an archived account is not listed,
/// and its owner brings no group heading, since only a listed account's owner does; both
/// sections are shown whenever one holds an account), SPEC W1 (the empty state), SPEC 6.2
/// (archived accounts are hidden from lists).
#[tokio::test]
async fn a_book_without_an_account_to_list_is_the_empty_state() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        home.balances, None,
        "the figures of a book without accounts"
    );
    assert_eq!(home.as_of, today(), "the day of the empty state");
    assert_eq!(
        home.minor_digits,
        MinorDigits::TWO,
        "the minor digits of the empty state"
    );

    let mut tx = begin(&storage, "an archived account").await;
    let assets = account_named(&mut tx, "Assets").await;
    let amy = user(&mut tx, admin, "amy", "Amy").await;
    let old = account(&mut tx, admin, assets, "Old account", Some(amy)).await;
    accounts::archive_account(&mut tx, admin, later(), old)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Old account: {error}"));
    commit(tx, "an archived account").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        home.balances, None,
        "the figures of a book whose only account is archived"
    );

    let mut tx = begin(&storage, "a card").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    account(&mut tx, admin, liabilities, "Card", None).await;
    commit(tx, "a card").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert!(
        figures.assets.groups.is_empty(),
        "the groups of a section without an account to list: {:?}",
        figures.assets.groups
    );
    assert_eq!(
        figures.assets.total,
        Money::ZERO,
        "the total of the empty section"
    );
    assert_eq!(
        written(&figures.liabilities),
        ["1 Card 0"],
        "the Liabilities section"
    );
    assert_eq!(figures.net_worth, Money::ZERO, "the net worth");
}

/// The top-level accounts of a section are grouped by their owners: the joint accounts first,
/// then one group per owner in the order of the owners' display names ignoring capitals
/// (`alex` before `Bea`, which a comparison of the characters as written would turn round),
/// two owners with the same display name in the order of their usernames ignoring capitals
/// (`chris` before `Kit`, which a comparison as written would turn round). A group is headed
/// with its owner's display name, never the username; a section has a group only for an owner
/// who has an account in it, an owner whose only account is a card (Dee) among them, and no
/// Joint group without a joint account. An owner who has been deactivated (Kit) still heads
/// the group of the accounts they own, in its place. The model takes no acting user, so it is
/// the same for everyone.
///
/// Proves: decision 119 (the joint accounts first, then one group per owner in each section,
/// an owner whose accounts are all under Liabilities included, in alphabetical order of the
/// owners' display names ignoring capitals, the username, folded as decision 95 folds,
/// settling a tie; the same for every user), decision 96 (a deactivated user still owns
/// accounts, ownership being a label), SPEC 5.1 and G7 (owners).
#[tokio::test]
async fn groups_are_the_joint_accounts_first_then_each_owner_by_display_name() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the owners' accounts").await;
    let assets = account_named(&mut tx, "Assets").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    // Created in an order that is neither the display names' nor the usernames'.
    let kit = user(&mut tx, admin, "Kit", "Chris").await;
    let amy = user(&mut tx, admin, "amy", "Bea").await;
    let zed = user(&mut tx, admin, "zed", "alex").await;
    let chris = user(&mut tx, admin, "chris", "Chris").await;
    let dee = user(&mut tx, admin, "dee", "Dee").await;
    account(&mut tx, admin, assets, "Kit's account", Some(kit)).await;
    account(&mut tx, admin, assets, "Amy's account", Some(amy)).await;
    account(&mut tx, admin, assets, "Shared", None).await;
    account(&mut tx, admin, assets, "Zed's account", Some(zed)).await;
    account(&mut tx, admin, assets, "Chris's account", Some(chris)).await;
    account(&mut tx, admin, assets, "Amy's other account", Some(amy)).await;
    account(&mut tx, admin, liabilities, "Amy's card", Some(amy)).await;
    account(&mut tx, admin, liabilities, "Dee's card", Some(dee)).await;
    // Deactivation is milestone 8 slice 1's operation; until then the tests plant it.
    test_support::deactivate_unchecked(&mut tx, kit, admin, later()).await;
    assert_sound(&mut tx, "the owners' accounts").await;
    commit(tx, "the owners' accounts").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.assets),
        [
            "[Joint]",
            "1 Shared 0",
            "[alex]",
            "1 Zed's account 0",
            "[Bea]",
            "1 Amy's account 0",
            "1 Amy's other account 0",
            "[Chris]",
            "1 Chris's account 0",
            "[Chris]",
            "1 Kit's account 0",
        ],
        "the Assets section"
    );
    assert_eq!(
        written(&figures.liabilities),
        ["[Bea]", "1 Amy's card 0", "[Dee]", "1 Dee's card 0"],
        "the Liabilities section"
    );
}

/// An account inside a parent stays under that parent whoever owns it, and its line names its
/// owner when it has one other than its group's: Alex's account inside the joint Savings is
/// listed under Savings in the Joint group and names Alex; inside Alex's own account, Alex's
/// sub-account names nobody, a joint sub-account names nobody, and Sam's sub-account names
/// Sam, as does what is inside it. A book in which only an account inside a parent has an
/// owner still shows headings, since a listed account has an owner: its one Assets group is
/// headed Joint, and so is the one group of the Liabilities section, whose card is joint, since
/// the headings are decided for the page and not for each section.
///
/// Proves: decision 119 (an account inside a parent stays under that parent whoever owns it,
/// tagged with its owner's display name when it has an owner other than its group's; a joint
/// account inside an owned parent gets no tag; no group heading only when no listed account
/// of the page has an owner, so a section of joint accounts beside a section with an owner is
/// headed Joint).
#[tokio::test]
async fn an_account_inside_a_parent_stays_there_and_names_its_other_owner() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a sub-account of another owner").await;
    let assets = account_named(&mut tx, "Assets").await;
    let alex = user(&mut tx, admin, "alex", "Alex").await;
    let savings = account(&mut tx, admin, assets, "Savings", None).await;
    account(&mut tx, admin, savings, "Holiday", Some(alex)).await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    account(&mut tx, admin, liabilities, "Card", None).await;
    commit(tx, "a sub-account of another owner").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        written(&figures(&home).assets),
        ["[Joint]", "1 Savings 0", "2 Holiday 0 (Alex)"],
        "the Assets section when only a sub-account has an owner"
    );
    assert_eq!(
        written(&figures(&home).liabilities),
        ["[Joint]", "1 Card 0"],
        "the Liabilities section, all joint, beside an Assets section with an owner"
    );

    let mut tx = begin(&storage, "an owner's account with sub-accounts").await;
    let sam = user(&mut tx, admin, "sam", "Sam").await;
    let pot = account(&mut tx, admin, assets, "Alex's pot", Some(alex)).await;
    account(&mut tx, admin, pot, "Bike", Some(alex)).await;
    account(&mut tx, admin, pot, "Common", None).await;
    let loan = account(&mut tx, admin, pot, "Loan from Sam", Some(sam)).await;
    account(&mut tx, admin, loan, "Second part", Some(sam)).await;
    assert_sound(&mut tx, "an owner's account with sub-accounts").await;
    commit(tx, "an owner's account with sub-accounts").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        written(&figures(&home).assets),
        [
            "[Joint]",
            "1 Savings 0",
            "2 Holiday 0 (Alex)",
            "[Alex]",
            "1 Alex's pot 0",
            "2 Bike 0",
            "2 Common 0",
            "2 Loan from Sam 0 (Sam)",
            "3 Second part 0 (Sam)",
        ],
        "the Assets section with an owner's account"
    );
}

/// Inside a group and inside every account the lines stand in the order of their names
/// ignoring capitals, whatever order the accounts were created in: `apple`, `Banana`,
/// `cherry` (a comparison of the characters as written would put `Banana` first), and under
/// `Banana` its own `anchovy`, `Bream` and `cod`; an account's sub-accounts follow it before
/// the next account at its level, down to the fifth level below the root.
///
/// Proves: decision 119 (inside a group and inside every account the accounts are in
/// alphabetical order of their names ignoring capitals, by the fold of decision 95, at every
/// level, the deepest an account can have included; each section lists its accounts as a
/// tree).
#[tokio::test]
async fn accounts_stand_in_the_order_of_their_names_at_every_level() {
    let (_dir, storage, initialized) = seeded(false).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "accounts out of order").await;
    let assets = account_named(&mut tx, "Assets").await;
    account(&mut tx, admin, assets, "cherry", None).await;
    let banana = account(&mut tx, admin, assets, "Banana", None).await;
    account(&mut tx, admin, assets, "apple", None).await;
    account(&mut tx, admin, banana, "cod", None).await;
    let bream = account(&mut tx, admin, banana, "Bream", None).await;
    account(&mut tx, admin, banana, "anchovy", None).await;
    let third = account(&mut tx, admin, bream, "third", None).await;
    let fourth = account(&mut tx, admin, third, "fourth", None).await;
    account(&mut tx, admin, fourth, "fifth", None).await;
    assert_sound(&mut tx, "accounts out of order").await;
    commit(tx, "accounts out of order").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        written(&figures(&home).assets),
        [
            "1 apple 0",
            "1 Banana 0",
            "2 anchovy 0",
            "2 Bream 0",
            "3 third 0",
            "4 fourth 0",
            "5 fifth 0",
            "2 cod 0",
            "1 cherry 0",
        ],
        "the Assets section"
    );
}

/// A parent's figure is its own entries and everything inside it, and the section's total is
/// its root's roll-up; the parent's own entries show on its Other line, last under it. An
/// archived account has no line, and neither has an account inside it, but its parent's figure
/// and the section's total still count what it holds as of the day: an account whose entries
/// sum to zero over all dates, and so may be archived, with one entry dated before today and
/// its reversal dated after today, holds the first entry's amount today, so the lines under
/// Savings (Holiday's 200.00 and its own 1,000.00) fall 5.00 short of its 1,205.00.
///
/// Proves: decision 119 (a parent's figure is its own live postings and everything inside it,
/// and its own amount stands on its Other line; archived accounts are left out of the list
/// while their parent's figure still counts them, which changes a figure only through entries
/// dated after today), decision 96 (the roll-up), SPEC 6.2 (archived accounts hidden from
/// lists).
#[tokio::test]
async fn figures_roll_up_and_an_archived_account_is_hidden_but_counted() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a parent with an archived account inside").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let holiday = account(&mut tx, admin, savings, "Holiday", None).await;
    let closed = account(&mut tx, admin, savings, "Closed", None).await;
    let inside = account(&mut tx, admin, closed, "Inside", None).await;
    open_balance(&mut tx, admin, now(), savings, "2026-11-01", 100_000).await;
    open_balance(&mut tx, admin, now(), holiday, "2026-11-01", 20_000).await;
    // 5.00 moves into Closed before today and back out after today: zero over all dates.
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-02",
        &[(closed, 500), (checking, -500)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        TOMORROW_TEXT,
        &[(closed, -500), (checking, 500)],
    )
    .await;
    for id in [inside, closed] {
        accounts::archive_account(&mut tx, admin, later(), id)
            .await
            .unwrap_or_else(|error| panic!("cannot archive account {id}: {error}"));
    }
    assert_sound(&mut tx, "a parent with an archived account inside").await;
    commit(tx, "a parent with an archived account inside").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.assets),
        [
            "1 Cash 0",
            "1 Checking -500",
            "1 Savings 120500",
            "2 Holiday 20000",
            "2 <other> 100000",
        ],
        "the Assets section: Savings holds its own 1,000.00, Holiday's 200.00 and the archived account's 5.00"
    );
    assert_eq!(
        figures.assets.total,
        money(120_000),
        "the Assets total: Savings' 1,205.00 less the 5.00 that left Checking"
    );
}

/// A listed account that has listed accounts inside it and entries of its own carries, as the
/// last line under it, an Other line with its own amount, one level below it: after its
/// sub-accounts and what is inside them, whatever their names, so after `Zebra` and after an
/// account that is itself named Other. Savings holds 300.00 of its own beside Bonds, Other and
/// Zebra; Zebra holds 7.00 of its own beside its sub-account Stripes, so its own Other line
/// stands after Stripes, and Savings' after it. The Other line is no account: it names no
/// owner, and the lines directly under each account add up to its figure.
///
/// Proves: decision 119 (a listed account with a listed account inside it and own live
/// postings up to the day that do not sum to zero carries, as the last line under it, after
/// its listed sub-accounts whatever their names, a line named Other with that own amount; it is
/// never sorted among the names), SPEC R2 (a parent's own postings as an Other line).
#[tokio::test]
async fn a_parent_with_entries_of_its_own_shows_them_last_on_an_other_line() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a parent with entries of its own").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let zebra = account(&mut tx, admin, savings, "Zebra", None).await;
    let other = account(&mut tx, admin, savings, "Other", None).await;
    let bonds = account(&mut tx, admin, savings, "Bonds", None).await;
    let stripes = account(&mut tx, admin, zebra, "Stripes", None).await;
    for (id, natural) in [
        (savings, 30_000),
        (zebra, 700),
        (other, 4_000),
        (bonds, 5_000),
        (stripes, 300),
    ] {
        open_balance(&mut tx, admin, now(), id, "2026-11-01", natural).await;
    }
    assert_sound(&mut tx, "a parent with entries of its own").await;
    commit(tx, "a parent with entries of its own").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.assets),
        [
            "1 Cash 0",
            "1 Checking 0",
            "1 Savings 40000",
            "2 Bonds 5000",
            "2 Other 4000",
            "2 Zebra 1000",
            "3 Stripes 300",
            "3 <other> 700",
            "2 <other> 30000",
        ],
        "the Assets section: Savings and Zebra each end with their own amount"
    );
    assert_eq!(
        figures.assets.groups[0].lines.last(),
        Some(&Line::Other(OtherLine {
            level: 2,
            amount: money(30_000),
        })),
        "the last line under Savings"
    );
    assert_eq!(figures.assets.total, money(40_000), "the Assets total");
}

/// No Other line where it would show nothing or nothing would stand above it: a parent whose
/// own entries sum to zero (300.00 in and out again, both before today), an account with
/// entries of its own and nothing inside it (Checking), a parent whose one sub-account is
/// archived (Old pot), and a parent whose own entries are all dated after the day (Savings,
/// whose 80.00 arrives tomorrow). Asked for the day after, Savings shows its Other line. A
/// section's root never has one: the roots carry no posting.
///
/// Proves: decision 119 (no Other line when the own amount up to the day is zero, none under
/// an account with no listed account inside it, an archived account not being listed, and
/// none for own entries dated after the day), decision 96 (the balances at the end of the
/// day).
#[tokio::test]
async fn no_other_line_without_an_own_amount_or_a_listed_account_inside() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "parents without an Other line").await;
    let assets = account_named(&mut tx, "Assets").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let zero_pot = account(&mut tx, admin, assets, "Zero pot", None).await;
    let inside = account(&mut tx, admin, zero_pot, "Inside", None).await;
    let old_pot = account(&mut tx, admin, assets, "Old pot", None).await;
    let closed = account(&mut tx, admin, old_pot, "Closed", None).await;
    let holiday = account(&mut tx, admin, savings, "Holiday", None).await;
    open_balance(&mut tx, admin, now(), checking, "2026-11-01", 2_000).await;
    open_balance(&mut tx, admin, now(), inside, "2026-11-01", 1_000).await;
    open_balance(&mut tx, admin, now(), old_pot, "2026-11-01", 5_000).await;
    open_balance(&mut tx, admin, now(), holiday, "2026-11-01", 1_000).await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-02",
        &[(zero_pot, 30_000), (salary, -30_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-03",
        &[(zero_pot, -30_000), (salary, 30_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        TOMORROW_TEXT,
        &[(savings, 8_000), (salary, -8_000)],
    )
    .await;
    accounts::archive_account(&mut tx, admin, later(), closed)
        .await
        .unwrap_or_else(|error| panic!("cannot archive Closed: {error}"));
    assert_sound(&mut tx, "parents without an Other line").await;
    commit(tx, "parents without an Other line").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        written(&figures(&home).assets),
        [
            "1 Cash 0",
            "1 Checking 2000",
            "1 Old pot 5000",
            "1 Savings 1000",
            "2 Holiday 1000",
            "1 Zero pot 1000",
            "2 Inside 1000",
        ],
        "the Assets section today: no Other line"
    );
    let tomorrow = home_as_of(&storage, TOMORROW_TEXT).await;
    assert_eq!(
        written(&figures(&tomorrow).assets),
        [
            "1 Cash 0",
            "1 Checking 2000",
            "1 Old pot 5000",
            "1 Savings 9000",
            "2 Holiday 1000",
            "2 <other> 8000",
            "1 Zero pot 1000",
            "2 Inside 1000",
        ],
        "the Assets section tomorrow: Savings' own 80.00 on its Other line"
    );
}

/// Under Liabilities the Other line is in natural sign like every other figure: a card that
/// owes 120.00 of its own beside a sub-card shows Other 120.00 under it, and a card overpaid by
/// 15.00 of its own beside a sub-card shows Other -15.00.
///
/// Proves: decision 119 (the Other line's amount in the natural sign of SPEC 6.3), SPEC 6.3.
#[tokio::test]
async fn the_other_line_under_liabilities_is_in_natural_sign() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "cards with entries of their own").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let second = account(&mut tx, admin, card, "Second card", None).await;
    let store = account(&mut tx, admin, liabilities, "Store card", None).await;
    let store_sub = account(&mut tx, admin, store, "Store sub", None).await;
    for (id, natural) in [
        (card, 12_000),
        (second, 2_000),
        (store, -1_500),
        (store_sub, 500),
    ] {
        open_balance(&mut tx, admin, now(), id, "2026-11-01", natural).await;
    }
    assert_sound(&mut tx, "cards with entries of their own").await;
    commit(tx, "cards with entries of their own").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.liabilities),
        [
            "1 Credit Card 14000",
            "2 Second card 2000",
            "2 <other> 12000",
            "1 Store card -1000",
            "2 Store sub 500",
            "2 <other> -1500",
        ],
        "the Liabilities section"
    );
    assert_eq!(
        figures.liabilities.total,
        money(13_000),
        "the Liabilities total"
    );
}

/// An Other line's own amount is refused like any other figure when its natural sign does not
/// fit in 64 bits. A card whose own entries hold the most negative amount, beside a sub-card
/// that holds one minor unit the other way: the card's roll-up, the Liabilities total, every
/// kind's total and the net worth fit, and only the card's own amount turned to natural sign
/// does not.
///
/// Proves: decision 120 (a balance whose roll-up, natural sign or net worth does not fit in 64
/// bits is refused rather than shown as a wrong figure, the Other line's own amount included),
/// decision 119 (the Other line's amount in natural sign), decision 96.
#[tokio::test]
async fn an_own_amount_whose_natural_sign_does_not_fit_is_refused() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "a card that owes the most negative amount").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    let second = account(&mut tx, admin, card, "Second card", None).await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(checking, i64::MAX), (card, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(groceries, 1), (card, -1)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(second, 1), (salary, -1)],
    )
    .await;
    let error = home::home_page(&mut tx, today()).await;
    assert!(
        matches!(error, Err(HomeError::Overflow)),
        "the home page over an own amount whose natural sign does not fit: {error:?}"
    );
}

/// Every figure is in natural sign. Under Assets an account shows what it holds, and an
/// overdrawn one a negative amount. Under Liabilities a card shows what is owed as a plain
/// positive amount and an overpaid card a negative one; the section's total is what is owed
/// in all, and the net worth is what is held minus what is owed.
///
/// Proves: decision 119 (every figure in the natural sign of SPEC 6.3; a card shows what is
/// owed as a plain positive amount and an overpaid card a negative one; the Net worth line is
/// the Assets total minus the Liabilities total), SPEC 6.3.
#[tokio::test]
async fn a_card_shows_what_is_owed_and_an_overpaid_card_a_negative_amount() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "two cards").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let store = account(&mut tx, admin, liabilities, "Store card", None).await;
    open_balance(&mut tx, admin, now(), checking, "2026-11-01", 248_015).await;
    open_balance(&mut tx, admin, now(), cash, "2026-11-01", -1_250).await;
    open_balance(&mut tx, admin, now(), card, "2026-11-01", 120_480).await;
    open_balance(&mut tx, admin, now(), store, "2026-11-01", -5_000).await;
    assert_sound(&mut tx, "two cards").await;
    commit(tx, "two cards").await;

    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.assets),
        ["1 Cash -1250", "1 Checking 248015", "1 Savings 0"],
        "the Assets section"
    );
    assert_eq!(
        written(&figures.liabilities),
        ["1 Credit Card 120480", "1 Store card -5000"],
        "the Liabilities section"
    );
    assert_eq!(figures.assets.total, money(246_765), "the Assets total");
    assert_eq!(
        figures.liabilities.total,
        money(115_480),
        "the Liabilities total"
    );
    assert_eq!(
        figures.net_worth,
        money(131_285),
        "the net worth: 2,467.65 held minus 1,154.80 owed"
    );
}

/// A removed opening balance counts nowhere: after its removal the account's line, the
/// section's total and the net worth are what they were before it was created, and a new
/// opening balance then counts alone.
///
/// Proves: INV-25 (a deleted transaction is excluded from every balance), decision 119 (a
/// removed opening balance counts nowhere on the home page), decision 100 (the removal).
#[tokio::test]
async fn a_removed_opening_balance_counts_nowhere() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let before = home_as_of(&storage, TODAY_TEXT).await;
    let mut tx = begin(&storage, "an opening balance and its removal").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let opening = open_balance(&mut tx, admin, now(), checking, "2026-11-01", 75_000).await;
    transactions::remove_opening_balance(&mut tx, admin, later(), opening.id, opening.version)
        .await
        .unwrap_or_else(|error| panic!("cannot remove the opening balance: {error}"));
    assert_sound(&mut tx, "an opening balance and its removal").await;
    commit(tx, "an opening balance and its removal").await;
    let after = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(after, before, "the home page after the removal");

    let mut tx = begin(&storage, "a new opening balance").await;
    open_balance(&mut tx, admin, later(), checking, "2026-11-02", 1_000).await;
    commit(tx, "a new opening balance").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    let figures = figures(&home);
    assert_eq!(
        written(&figures.assets),
        ["1 Cash 0", "1 Checking 1000", "1 Savings 0"],
        "the Assets section with the new opening balance"
    );
    assert_eq!(figures.net_worth, money(1_000), "the net worth");
}

/// The balances are those at the end of the day the caller hands in: an entry dated that day
/// counts, one dated the day after does not, and the same book asked for the day after counts
/// both and names that day.
///
/// Proves: decision 119 (the balances at the end of the day the page names), decision 96 (a
/// balance as of a day is the balance at the end of that day), decision 120 (today is handed
/// in by the caller).
#[tokio::test]
async fn the_balances_are_those_at_the_end_of_the_day_handed_in() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "entries of today and of tomorrow").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let cash = account_named(&mut tx, "Assets:Cash").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    post(
        &mut tx,
        admin,
        now(),
        TODAY_TEXT,
        &[(checking, 30_000), (salary, -30_000)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        TOMORROW_TEXT,
        &[(cash, 4_000), (salary, -4_000)],
    )
    .await;
    assert_sound(&mut tx, "entries of today and of tomorrow").await;
    commit(tx, "entries of today and of tomorrow").await;

    let today_home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(today_home.as_of, date(TODAY_TEXT), "the day named today");
    assert_eq!(
        written(&figures(&today_home).assets),
        ["1 Cash 0", "1 Checking 30000", "1 Savings 0"],
        "the Assets section today"
    );
    assert_eq!(
        figures(&today_home).net_worth,
        money(30_000),
        "the net worth today"
    );
    let tomorrow_home = home_as_of(&storage, TOMORROW_TEXT).await;
    assert_eq!(
        tomorrow_home.as_of,
        date(TOMORROW_TEXT),
        "the day named tomorrow"
    );
    assert_eq!(
        written(&figures(&tomorrow_home).assets),
        ["1 Cash 4000", "1 Checking 30000", "1 Savings 0"],
        "the Assets section tomorrow"
    );
}

/// The model carries the minor digits of the book as it was set up: a book in JPY, which has
/// no minor unit, reports none, and so does the empty state of a JPY book set up without the
/// starter accounts, though that page shows no figure.
///
/// Proves: decision 120 (the model takes the book's minor digits, in the empty state too),
/// SPEC 6.10.
#[tokio::test]
async fn the_model_carries_the_books_minor_digits() {
    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a book in JPY").await;
    let setup = Setup {
        currency: currency("JPY"),
        ..standard_setup(true)
    };
    initialize(&mut tx, now(), &setup)
        .await
        .unwrap_or_else(|error| panic!("cannot set the JPY book up: {error}"));
    commit(tx, "a book in JPY").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        home.minor_digits,
        MinorDigits::ZERO,
        "the minor digits of a book in JPY"
    );

    let (_dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "a book in JPY without accounts").await;
    let setup = Setup {
        currency: currency("JPY"),
        ..standard_setup(false)
    };
    initialize(&mut tx, now(), &setup)
        .await
        .unwrap_or_else(|error| panic!("cannot set the empty JPY book up: {error}"));
    commit(tx, "a book in JPY without accounts").await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert_eq!(
        home.balances, None,
        "the figures of a JPY book without accounts"
    );
    assert_eq!(
        home.minor_digits,
        MinorDigits::ZERO,
        "the minor digits of the empty state of a book in JPY"
    );
}

/// The household's book of the page snapshots, built through the operations: a user Alex
/// beside the admin Sam; joint Checking, Savings (with the joint Emergency fund and Alex's
/// Holiday inside, and 500.00 of its own) and an empty account with a long name; Alex's Cash
/// and Sam's Brokerage; a joint card that is owed on and Sam's store card, overpaid. Returns
/// the storage with its directory.
async fn household() -> (test_support::TestDir, Storage) {
    let (dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "the household").await;
    let mut setup = standard_setup(false);
    setup.admin.username = String::from("sam");
    setup.admin.display_name = String::from("Sam");
    let sam = initialize(&mut tx, now(), &setup)
        .await
        .unwrap_or_else(|error| panic!("cannot set the household's book up: {error}"))
        .admin
        .id;
    let alex = user(&mut tx, sam, "alex", "Alex").await;
    let assets = account_named(&mut tx, "Assets").await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let checking = account(&mut tx, sam, assets, "Checking", None).await;
    let savings = account(&mut tx, sam, assets, "Savings", None).await;
    let emergency = account(&mut tx, sam, savings, "Emergency fund", None).await;
    let holiday = account(&mut tx, sam, savings, "Holiday", Some(alex)).await;
    account(
        &mut tx,
        sam,
        assets,
        "Renovation fund for the kitchen and the upstairs bathroom",
        None,
    )
    .await;
    let cash = account(&mut tx, sam, assets, "Cash", Some(alex)).await;
    let brokerage = account(&mut tx, sam, assets, "Brokerage", Some(sam)).await;
    let card = account(&mut tx, sam, liabilities, "Credit Card", None).await;
    let store = account(&mut tx, sam, liabilities, "Store card", Some(sam)).await;
    for (id, natural) in [
        (checking, 248_015),
        (savings, 50_000),
        (emergency, 900_000),
        (holiday, 300_000),
        (cash, 8_218),
        (brokerage, 135_000),
        (card, 120_480),
        (store, -5_000),
    ] {
        open_balance(&mut tx, sam, now(), id, "2026-11-01", natural).await;
    }
    assert_sound(&mut tx, "the household").await;
    commit(tx, "the household").await;
    (dir, storage)
}

/// The home page's model of a household's book in use, whole, as a golden snapshot: the day,
/// the minor digits, the two sections with their totals, the groups with their headings, every
/// line with its level, its amount and the owner it names, the Other line, and the net worth.
/// The book holds what the page snapshot `home_2_household` of nage-web shows (joint accounts
/// and the accounts of two owners, an account of another owner inside a joint parent, an
/// account at zero, a card that is owed on and an overpaid one) and, beyond it, 500.00 that the
/// joint Savings holds of its own, which its Other line shows.
///
/// Proves: decision 120 (the home page's figures come from a typed model in nage-core, pinned
/// by a golden snapshot), decision 119 (the model groups, orders and signs as the row says),
/// SPEC 10.1 (report goldens snapshot the data model), decision 60.
#[tokio::test]
async fn the_model_of_a_household_is_the_golden() {
    let (_dir, storage) = household().await;
    let home = home_as_of(&storage, TODAY_TEXT).await;
    assert!(
        check_report(&storage).await.violations().is_empty(),
        "nage check finds something wrong with the household's book"
    );
    insta::assert_debug_snapshot!(home);
}

/// An amount that does not fit in 64 bits is refused, never shown wrong. Two opening balances
/// under Assets, the largest amount the book can hold and one minor unit more beside it,
/// overflow the root's roll-up; the largest amount paid into an account, with one minor unit
/// of credit on a card, overflows the net worth alone: each section's total fits, and so does
/// every kind's. A card that owes the most negative amount has a stored sum that fits and a
/// natural sign that does not, and so has the Liabilities total; and with a second card that
/// holds one minor unit the other way, the Liabilities total, every roll-up and the net worth
/// (zero) fit, and only the first card's own line turned to natural sign does not.
///
/// Proves: decision 120 (a balance whose roll-up, natural sign or net worth does not fit in 64
/// bits is refused rather than shown as a wrong figure), decision 96 (an overflow is refused,
/// never wrapped), SPEC 6.10.
#[tokio::test]
async fn an_amount_that_does_not_fit_in_64_bits_is_refused() {
    let (_dir, storage, initialized) = seeded(true).await;
    let admin = initialized.admin.id;
    let mut tx = begin(&storage, "the largest amount and one more").await;
    let checking = account_named(&mut tx, "Assets:Checking").await;
    let savings = account_named(&mut tx, "Assets:Savings").await;
    open_balance(&mut tx, admin, now(), checking, "2026-11-01", i64::MAX).await;
    open_balance(&mut tx, admin, now(), savings, "2026-11-01", 1).await;
    let error = home::home_page(&mut tx, today()).await;
    assert!(
        matches!(error, Err(HomeError::Overflow)),
        "the home page over a roll-up that does not fit: {error:?}"
    );
    drop(tx);

    let mut tx = begin(&storage, "the largest amount and a card in credit").await;
    let card = account_named(&mut tx, "Liabilities:Credit Card").await;
    let salary = account_named(&mut tx, "Income:Salary").await;
    let groceries = account_named(&mut tx, "Expenses:Groceries").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(checking, i64::MAX), (salary, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(card, 1), (groceries, -1)],
    )
    .await;
    let error = home::home_page(&mut tx, today()).await;
    assert!(
        matches!(error, Err(HomeError::Overflow)),
        "the home page over a net worth that does not fit: {error:?}"
    );
    drop(tx);

    let mut tx = begin(&storage, "a card that owes the most negative amount").await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(checking, i64::MAX), (card, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(groceries, 1), (card, -1)],
    )
    .await;
    let error = home::home_page(&mut tx, today()).await;
    assert!(
        matches!(error, Err(HomeError::Overflow)),
        "the home page over a card and a Liabilities total whose natural sign does not fit: {error:?}"
    );
    drop(tx);

    let mut tx = begin(
        &storage,
        "two cards, one of them owing the most negative amount",
    )
    .await;
    let liabilities = account_named(&mut tx, "Liabilities").await;
    let store = account(&mut tx, admin, liabilities, "Store card", None).await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(checking, i64::MAX), (card, -i64::MAX)],
    )
    .await;
    post(
        &mut tx,
        admin,
        now(),
        "2026-11-01",
        &[(store, 1), (card, -1)],
    )
    .await;
    let error = home::home_page(&mut tx, today()).await;
    assert!(
        matches!(error, Err(HomeError::Overflow)),
        "the home page over a card whose own line alone does not fit in natural sign: {error:?}"
    );
}

/// A database that holds no book has no home page: the read is refused as not set up, a
/// failure and not an empty state.
///
/// Proves: decision 120's model needs the book row it takes the minor digits from: over a
/// database without one the read ends in `NotInitialized`, never in an empty state.
#[tokio::test]
async fn a_database_without_a_book_has_no_home_page() {
    let (_dir, storage) = open_migrated().await;
    let mut conn = reader(&storage).await;
    let error = home::home_page(&mut conn, today()).await;
    assert!(
        matches!(error, Err(HomeError::NotInitialized)),
        "the home page of a database without a book: {error:?}"
    );
}

/// One random entry of the property: an amount moved between two of the book's accounts on a
/// day around today.
#[derive(Clone, Debug)]
struct Entry {
    from: Index,
    to: Index,
    minor: i64,
    /// Days after today; negative is before.
    day: i8,
}

/// A random entry: up to 10,000,000.00 on one of the seven days around today.
fn entry() -> impl Strategy<Value = Entry> {
    (
        any::<Index>(),
        any::<Index>(),
        1_i64..=1_000_000_000,
        -3_i8..=3,
    )
        .prop_map(|(from, to, minor, day)| Entry {
            from,
            to,
            minor,
            day,
        })
}

/// The day `offset` days after [`today`], written as a date.
fn day_text(offset: i8) -> String {
    let mut day = today();
    for _ in 0..offset.unsigned_abs() {
        day = if offset < 0 {
            day.yesterday()
        } else {
            day.tomorrow()
        }
        .unwrap_or_else(|| panic!("no day {offset} days from today"));
    }
    day.to_string()
}

proptest! {
    #![proptest_config(test_support::proptest_config(file!()))]

    /// Over random entries between the accounts of all five kinds of a book with the starter
    /// accounts, an owner's account and accounts inside others, under Assets two levels deep
    /// and under Liabilities one, dated around today, so that parents have entries of their
    /// own: each section's total is the kind's natural total of decision 96 and the sum of its
    /// top-level lines; in this book without archived accounts, every listed account that has
    /// lines under it has a figure equal to the sum of the lines directly under it, its Other
    /// line included; the net worth is the Assets total minus the Liabilities total; and it
    /// equals the stored sum of Equity, Income and Expenses as of the same day with its sign
    /// turned.
    ///
    /// Proves: decision 119 (the Net worth line is the difference of the two section totals
    /// and, by INV-4, the stored sum of Equity, Income and Expenses with its sign turned; each
    /// section's total is the sum of its top-level lines; without archived accounts, the lines
    /// under an account, its Other line among them, add up to its figure), INV-4, decision 96
    /// (the kind totals).
    #[test]
    fn the_net_worth_is_the_difference_of_the_totals_and_the_other_kinds_turned(
        entries in prop::collection::vec(entry(), 0..12),
    ) {
        block_on(async {
            let (_dir, storage, initialized) = seeded(true).await;
            let admin = initialized.admin.id;
            let mut tx = begin(&storage, "the random ledger").await;
            let alex = user(&mut tx, admin, "alex", "Alex").await;
            let assets = account_named(&mut tx, "Assets").await;
            let savings = account_named(&mut tx, "Assets:Savings").await;
            let card = account_named(&mut tx, "Liabilities:Credit Card").await;
            let holiday = account(&mut tx, admin, savings, "Holiday", None).await;
            let mut ids = vec![
                account(&mut tx, admin, assets, "Alex's cash", Some(alex)).await,
                holiday,
                account(&mut tx, admin, holiday, "Flights", None).await,
                account(&mut tx, admin, card, "Second card", None).await,
            ];
            for name in [
                "Assets:Checking",
                "Assets:Savings",
                "Assets:Cash",
                "Liabilities:Credit Card",
                "Equity:Opening Balances",
                "Income:Salary",
                "Expenses:Groceries",
                "Expenses:Uncategorized",
            ] {
                ids.push(account_named(&mut tx, name).await);
            }
            for entry in &entries {
                let (from, to) = (*entry.from.get(&ids), *entry.to.get(&ids));
                if from == to {
                    continue;
                }
                post(
                    &mut tx,
                    admin,
                    now(),
                    &day_text(entry.day),
                    &[(to, entry.minor), (from, -entry.minor)],
                )
                .await;
            }
            let home = home::home_page(&mut tx, today())
                .await
                .unwrap_or_else(|error| panic!("cannot read the home page: {error}"));
            let kinds = balances::balances_as_of(&mut tx, today())
                .await
                .unwrap_or_else(|error| panic!("cannot compute the balances: {error}"))
                .kinds;
            let kind = |kind: Kind| {
                kinds
                    .iter()
                    .find(|balance| balance.kind == kind)
                    .copied()
                    .unwrap_or_else(|| panic!("no total for {kind}"))
            };
            let figures = figures(&home);
            prop_assert_eq!(figures.assets.total, kind(Kind::Asset).natural, "the Assets total");
            prop_assert_eq!(
                figures.liabilities.total,
                kind(Kind::Liability).natural,
                "the Liabilities total"
            );
            for section in [&figures.assets, &figures.liabilities] {
                let top = Money::checked_sum(
                    section
                        .groups
                        .iter()
                        .flat_map(|group| &group.lines)
                        .filter(|line| level(line) == 1)
                        .map(amount),
                );
                prop_assert_eq!(top, Some(section.total), "the top-level lines of {}", &section.name);
                for group in &section.groups {
                    for (index, line) in group.lines.iter().enumerate() {
                        let Line::Account(parent) = line else {
                            continue;
                        };
                        let below = parent.level.saturating_add(1);
                        let under: Vec<Money> = group.lines[index + 1..]
                            .iter()
                            .take_while(|next| level(next) > parent.level)
                            .filter(|next| level(next) == below)
                            .map(amount)
                            .collect();
                        if under.is_empty() {
                            continue;
                        }
                        prop_assert_eq!(
                            Money::checked_sum(under),
                            Some(parent.amount),
                            "the lines under {}",
                            &parent.name
                        );
                    }
                }
            }
            prop_assert_eq!(
                figures.assets.total.checked_sub(figures.liabilities.total),
                Some(figures.net_worth),
                "the net worth against the two totals"
            );
            let others = Money::checked_sum(
                [Kind::Equity, Kind::Income, Kind::Expense].map(|other| kind(other).stored),
            );
            prop_assert_eq!(
                others.and_then(Money::checked_neg),
                Some(figures.net_worth),
                "the net worth against the stored sum of Equity, Income and Expenses"
            );
            Ok::<(), TestCaseError>(())
        })?;
    }
}
