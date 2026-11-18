//! The home page's model (SPEC 6.2, 6.3, 6.10, G7, W1, INV-25, decisions 119 and 120): the
//! Assets and Liabilities balances at the end of a day, as a typed report the page only
//! renders. One function reads, in the caller's read transaction, the book's minor digits, the
//! accounts, the owners' display names and the balances of decision 96, and groups, orders,
//! signs and hides:
//!
//! - **Two sections**, Assets and then Liabilities, each named as its root is and totalled by
//!   its root's roll-up; Equity, Income and Expenses are not shown.
//! - **Natural sign** (SPEC 6.3) on every figure: what an Assets account holds, what a
//!   Liabilities account owes, so a card shows what is owed as a plain positive amount and an
//!   overpaid card a negative one.
//! - **The roll-up** of decision 96: an account's figure is its own live postings dated on or
//!   before the day and everything inside it; a deleted transaction counts nowhere (INV-25).
//! - **Archived accounts are left out** of the lines, with everything inside them (INV-12:
//!   archived too), while their parent's figure and the section's total still count them.
//! - **The Other line** (as SPEC R2 shows a parent's own postings): a listed account with at
//!   least one listed account inside it, whose own live postings up to the day do not sum to
//!   zero, has as the last line under it, after its listed sub-accounts and what is inside
//!   them, a [`Line::Other`] with that own amount, one level below the account. None when the
//!   own amount is zero, none under an account with no listed account inside it, none under a
//!   section's root (INV-8: a root carries no posting). It is not an account: it names no
//!   owner and is never sorted among the names. So the lines directly under an account add up
//!   to its figure, except through a hidden archived account's entries dated after the day.
//! - **An account with nothing in it** has a line with the amount zero.
//! - **Groups** by the owner of the top-level account (the account directly under the root):
//!   the joint accounts first, then one group per owner, by the fold (decision 95) of the
//!   owner's display name and then of the username. An account inside a parent stays under
//!   that parent whoever owns it; its line names its owner when it has one and that owner is
//!   not its group's. When no listed account has an owner, no group has a heading.
//! - **Order** by the fold of the account's name, inside a group and inside every account.
//! - **Net worth**: the Assets total minus the Liabilities total, both in natural sign.
//! - **The empty state**: when Assets and Liabilities hold no account to list, the model
//!   holds no figure at all.
//!
//! An amount that does not fit in 64 bits is refused, never wrapped (decision 96).

use std::fmt;

use crate::accounts::{AccountError, Kind};
use crate::balances::BalanceError;
use crate::book::BookError;
use crate::date::Date;
use crate::money::{MinorDigits, Money};

/// What the home page shows (decision 119).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    /// The day the balances are for: the balances at the end of this day (decision 96).
    pub as_of: Date,
    /// The book's minor digits, with which every amount is displayed (SPEC 6.10).
    pub minor_digits: MinorDigits,
    /// The two sections and the net worth; `None` when Assets and Liabilities hold no account
    /// to list (only their roots, or only archived accounts): the empty state, which shows no
    /// figure.
    pub balances: Option<HomeBalances>,
}

/// The figures of the home page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeBalances {
    pub assets: Section,
    pub liabilities: Section,
    /// `assets.total` minus `liabilities.total`: what is held minus what is owed.
    pub net_worth: Money,
}

/// One section: Assets or Liabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// `Kind::Asset` or `Kind::Liability`.
    pub kind: Kind,
    /// The root's name as stored: `Assets` or `Liabilities`.
    pub name: String,
    /// The root's roll-up in natural sign, archived accounts included.
    pub total: Money,
    /// The joint accounts first, then one group per owner; no group is empty, so a section
    /// without an account to list has none.
    pub groups: Vec<Group>,
}

/// The top-level accounts of one owner, or the joint ones, with everything inside them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// `None` when no listed account of the page has an owner: then each section has at most
    /// one group and no heading shows.
    pub heading: Option<GroupHeading>,
    /// The accounts as a tree written out in order: each top-level account, followed by what
    /// is inside it, an account's Other line after its listed sub-accounts.
    pub lines: Vec<Line>,
}

/// One line of a group, in the order the page shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// An account's line.
    Account(AccountLine),
    /// The Other line of the account above it whose level is one less.
    Other(OtherLine),
}

/// The line named Other (decision 119): the own amount of an account that has listed accounts
/// inside it. It stands last under that account and is not an account itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherLine {
    /// One more than the level of the account whose own amount it shows (2 to 5).
    pub level: u8,
    /// That account's own live postings up to the day, in natural sign; never zero.
    pub amount: Money,
}

/// What a group is headed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupHeading {
    /// The accounts without an owner (SPEC 5.1).
    Joint,
    /// The accounts of one owner: the owner's display name.
    Owner(String),
}

/// One account's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountLine {
    /// The account's name as stored, never its full name.
    pub name: String,
    /// 1 for an account directly under its section's root, one more for each level below
    /// (at most 5, INV-9).
    pub level: u8,
    /// The account's roll-up in natural sign.
    pub amount: Money,
    /// The display name of the account's owner, when it has an owner and that owner is not the
    /// one its group is headed with; `None` otherwise, so a joint account inside an owned
    /// parent names nobody.
    pub owner: Option<String>,
}

/// The home page's model as of `today` (decisions 119 and 120), read through `conn`, which the
/// caller has put in one read transaction (SPEC 5.4), so the book, the accounts, the owners
/// and the balances are of one moment. `today` is the day in the configured zone, which the
/// caller hands in (decision 88): the core reads no clock. The module's list says what the
/// model holds. `NotInitialized` without a book row; `Overflow` when a roll-up, a natural sign
/// (an Other line's own amount included) or the net worth does not fit in 64 bits.
pub async fn home_page(conn: &mut sqlx::SqliteConnection, today: Date) -> Result<Home, HomeError> {
    let book = book::book(&mut *conn)
        .await?
        .ok_or(HomeError::NotInitialized)?;
    let all = balances::balances_as_of(&mut *conn, today).await?;
    let tree = Tree::new(&all.accounts);
    let assets = tree.root(Kind::Asset)?;
    let liabilities = tree.root(Kind::Liability)?;
    let empty = |root: &AccountBalance| tree.listed(root).is_empty();
    if empty(assets) && empty(liabilities) {
        return Ok(Home {
            as_of: today,
            minor_digits: book.minor_digits,
            balances: None,
        });
    }
    let mut owners = BTreeMap::new();
    for owner in tree.owners([assets, liabilities]) {
        let user = users::user(&mut *conn, owner)
            .await?
            .ok_or(HomeError::Database(sqlx::Error::RowNotFound))?;
        owners.insert(owner, user);
    }
    let assets = section(&tree, &owners, assets)?;
    let liabilities = section(&tree, &owners, liabilities)?;
    let net_worth = assets
        .total
        .checked_sub(liabilities.total)
        .ok_or(HomeError::Overflow)?;
    Ok(Home {
        as_of: today,
        minor_digits: book.minor_digits,
        balances: Some(HomeBalances {
            assets,
            liabilities,
            net_worth,
        }),
    })
}

/// Why the home page's model could not be read (decision 120). None of these is a refusal: no
/// user caused them, and a page answers 500 for each (decision 102).
#[derive(Debug)]
pub enum HomeError {
    /// The book is not set up: there is no book row.
    NotInitialized,
    /// A roll-up, the natural sign of a figure (an Other line's included) or the net worth
    /// does not fit in 64 bits.
    Overflow,
    /// The book row could not be read.
    Book(BookError),
    /// The chart of accounts could not be read, or lacks the Assets or the Liabilities root.
    Accounts(AccountError),
    /// Any other database failure.
    Database(sqlx::Error),
}

impl fmt::Display for HomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => f.write_str("the book is not set up"),
            Self::Overflow => f.write_str("a balance is too large"),
            Self::Book(error) => write!(f, "{error}"),
            Self::Accounts(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for HomeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Book(error) => Some(error),
            Self::Accounts(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::NotInitialized | Self::Overflow => None,
        }
    }
}

impl From<BalanceError> for HomeError {
    /// `Overflow` stays `Overflow`; the chart's and the database's failures keep their kind.
    fn from(error: BalanceError) -> Self {
        match error {
            BalanceError::Overflow => Self::Overflow,
            BalanceError::Accounts(error) => Self::Accounts(error),
            BalanceError::Database(error) => Self::Database(error),
        }
    }
}

impl From<BookError> for HomeError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<AccountError> for HomeError {
    fn from(error: AccountError) -> Self {
        Self::Accounts(error)
    }
}

impl From<sqlx::Error> for HomeError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use std::collections::{BTreeMap, BTreeSet};

use crate::accounts::AccountId;
use crate::balances::{self, AccountBalance};
use crate::book;
use crate::fold::fold;
use crate::users::{self, User, UserId};

/// The chart as the page walks it: every account's balances, and under each account the
/// accounts inside it that are listed (not archived), in the order of the fold of their names
/// (decision 95), the id settling a tie. An archived account is no one's listed child, so
/// whatever is inside it is not reached either.
struct Tree<'a> {
    accounts: &'a [AccountBalance],
    children: BTreeMap<AccountId, Vec<&'a AccountBalance>>,
}

impl<'a> Tree<'a> {
    /// The tree of `accounts`, one entry per account as `balances_as_of` reads them.
    fn new(accounts: &'a [AccountBalance]) -> Self {
        let mut children: BTreeMap<AccountId, Vec<&'a AccountBalance>> = BTreeMap::new();
        for balance in accounts {
            if let Some(parent) = balance.account.parent_id
                && !balance.account.is_archived()
            {
                children.entry(parent).or_default().push(balance);
            }
        }
        for listed in children.values_mut() {
            listed.sort_by_cached_key(|balance| (fold(&balance.account.name), balance.account.id));
        }
        Self { accounts, children }
    }

    /// The root of `kind`, found by its system key whatever its name; `MissingSystemAccount`
    /// when it is absent (INV-11 broken).
    fn root(&self, kind: Kind) -> Result<&'a AccountBalance, HomeError> {
        let key = kind.root_key();
        self.accounts
            .iter()
            .find(|balance| balance.account.system_key == Some(key))
            .ok_or(HomeError::Accounts(AccountError::MissingSystemAccount(key)))
    }

    /// The listed accounts directly inside `parent`, in order.
    fn listed(&self, parent: &AccountBalance) -> &[&'a AccountBalance] {
        self.children
            .get(&parent.account.id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The owners of the listed accounts under `roots`, at every level.
    fn owners(&self, roots: [&'a AccountBalance; 2]) -> BTreeSet<UserId> {
        let mut owners = BTreeSet::new();
        let mut pending = roots.to_vec();
        while let Some(balance) = pending.pop() {
            for child in self.listed(balance) {
                owners.extend(child.account.owner_user_id);
                pending.push(child);
            }
        }
        owners
    }
}

/// The section under `root`: its name, its total, and its groups. With no owner among the
/// page's listed accounts (`owners` empty) the top-level accounts are one group without a
/// heading, or none when there is no top-level account; otherwise the joint ones form the
/// group headed Joint, first, and each owner's form a group headed with the owner's display
/// name, in the order of the fold of the display name and then of the username.
fn section(
    tree: &Tree<'_>,
    owners: &BTreeMap<UserId, User>,
    root: &AccountBalance,
) -> Result<Section, HomeError> {
    let kind = root.account.kind;
    let top = tree.listed(root);
    let mut groups = Vec::new();
    if owners.is_empty() {
        if !top.is_empty() {
            groups.push(group(tree, owners, None, None, top)?);
        }
    } else {
        let joint: Vec<&AccountBalance> = top
            .iter()
            .copied()
            .filter(|balance| balance.account.owner_user_id.is_none())
            .collect();
        if !joint.is_empty() {
            groups.push(group(
                tree,
                owners,
                Some(GroupHeading::Joint),
                None,
                &joint,
            )?);
        }
        let mut owned = Vec::new();
        for id in top
            .iter()
            .filter_map(|balance| balance.account.owner_user_id)
            .collect::<BTreeSet<UserId>>()
        {
            owned.push(user_of(owners, id)?);
        }
        owned.sort_by_cached_key(|user| (fold(&user.display_name), fold(&user.username), user.id));
        for user in owned {
            let accounts: Vec<&AccountBalance> = top
                .iter()
                .copied()
                .filter(|balance| balance.account.owner_user_id == Some(user.id))
                .collect();
            groups.push(group(
                tree,
                owners,
                Some(GroupHeading::Owner(user.display_name.clone())),
                Some(user.id),
                &accounts,
            )?);
        }
    }
    Ok(Section {
        kind,
        name: root.account.name.clone(),
        total: natural(root.total, kind)?,
        groups,
    })
}

/// The group headed `heading` whose owner is `owner` (`None` for the joint group and for the
/// one group of a page without owners): the lines of `top`, each with what is inside it.
fn group(
    tree: &Tree<'_>,
    owners: &BTreeMap<UserId, User>,
    heading: Option<GroupHeading>,
    owner: Option<UserId>,
    top: &[&AccountBalance],
) -> Result<Group, HomeError> {
    let mut lines = Vec::new();
    for balance in top {
        write(tree, owners, owner, balance, 1, &mut lines)?;
    }
    Ok(Group { heading, lines })
}

/// Writes the line of `balance` at `level` into `lines`, then the lines of the listed accounts
/// inside it, then its Other line when it has a listed account inside it and an own amount
/// that is not zero. The line names the account's owner when it has one other than
/// `group_owner`, the owner of its group.
fn write(
    tree: &Tree<'_>,
    owners: &BTreeMap<UserId, User>,
    group_owner: Option<UserId>,
    balance: &AccountBalance,
    level: u8,
    lines: &mut Vec<Line>,
) -> Result<(), HomeError> {
    let kind = balance.account.kind;
    let owner = match balance.account.owner_user_id {
        Some(id) if Some(id) != group_owner => Some(user_of(owners, id)?.display_name.clone()),
        _ => None,
    };
    lines.push(Line::Account(AccountLine {
        name: balance.account.name.clone(),
        level,
        amount: natural(balance.total, kind)?,
        owner,
    }));
    let inside = tree.listed(balance);
    let below = level.saturating_add(1);
    for child in inside {
        write(tree, owners, group_owner, child, below, lines)?;
    }
    if !inside.is_empty() && balance.own != Money::ZERO {
        lines.push(Line::Other(OtherLine {
            level: below,
            amount: natural(balance.own, kind)?,
        }));
    }
    Ok(())
}

/// The owner `id` as read for the page; a database failure if it was not read, which the
/// foreign key on `accounts.owner_user_id` rules out.
fn user_of(owners: &BTreeMap<UserId, User>, id: UserId) -> Result<&User, HomeError> {
    owners
        .get(&id)
        .ok_or(HomeError::Database(sqlx::Error::RowNotFound))
}

/// `stored` in the natural sign of `kind` (SPEC 6.3); `Overflow` when it does not fit.
fn natural(stored: Money, kind: Kind) -> Result<Money, HomeError> {
    balances::natural(stored, kind).ok_or(HomeError::Overflow)
}
