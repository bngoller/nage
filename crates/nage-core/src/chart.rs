//! The account pages' models (SPEC 6.2, W2, decisions 120 and 128 to 135): the list of the
//! chart, one account's page and the forms' pick lists, each a typed model read in the caller's
//! read transaction, so that the accounts, the owners and the balances are of one moment; the
//! templates only render them. The balances are those of the home page (decision 96's roll-up
//! at the end of the day handed in, [`crate::balances::balances_as_of`]), never a second
//! calculation:
//!
//! - **The list** groups the five sections as the pages word them (decision 129): Assets and
//!   Liabilities (Accounts), Income and Expenses (Categories), Equity (Built in). Each section
//!   lists its accounts as a tree, by the fold of their names at every level (decision 95), the
//!   id settling a tie; an account inside another is one level deeper. Archived accounts are
//!   hidden with everything inside them unless the list is asked to show them, and are counted
//!   either way. Every account under Assets and Liabilities shows its balance at the end of the
//!   day in natural sign (SPEC 6.3), an account that holds others the roll-up with what is
//!   inside it, and those two sections' headings their totals; categories, built-in accounts
//!   and Equity show no figure. Under an account that holds others and has an amount of its
//!   own that is not zero, a last line Other shows that amount at the level of what is inside
//!   it, exactly as the home page's (decision 119, the pause's P8). While no account line with
//!   a figure shows (none under Assets and Liabilities, or all of them archived and hidden)
//!   there is no figure at all (decision 131). A section whose accounts are all archived and
//!   hidden says so apart from a section that holds nothing.
//! - **An owner** is named by display name with one of six colours, given by the order the
//!   users were created: the first user the first colour, the seventh the first again
//!   (decision 138). Nothing is stored.
//! - **An account's page** holds its name, its full place, the trail of the accounts that hold
//!   it, whether it is built in or archived, its owner and balance under Assets and
//!   Liabilities, and the accounts directly inside it, archived ones included and marked, with
//!   the Other line of its own amount (decision 133); an account under Assets or Liabilities
//!   that is neither built in nor archived also its opening balance or none yet, read from the
//!   opening-balance read and never from the roll-up (decision 141, A7).
//! - **The pick lists** (the pause's P5) offer, after the section itself, the places of one
//!   section in the order of the list page (the tree, by the fold at every level), each written
//!   in full, and only places where the thing can go: never an archived place, never a
//!   built-in account (nothing goes inside one); the create form also leaves out the places at
//!   the sixth level; the change form also leaves out the account itself, everything inside
//!   it, and every place where it, with what is inside it, would pass six levels. The owners
//!   are Joint, then every active user by the fold of the display name, and a deactivated user
//!   only on an account already theirs, marked. A place or an owner a returned form holds but
//!   the list no longer offers ([`Kept`]) stays in its list as one extra choice, marked
//!   archived or deactivated; the create form leaves a built-in account out even then; the
//!   change form keeps its place whatever the reason its list leaves it out (an archived place,
//!   the archived account that holds an archived one, a built-in account, a place too deep, the
//!   account itself or a place inside it), so that a browser never sends Directly under the
//!   section for a place nobody chose; a place that is not archived is kept unmarked, its
//!   problem under the box. The clash page's form
//!   ([`clash_form`]) keeps an archived account's Inside and Owner when the save changed them. The same function judges what a list offers and what a form
//!   sends ([`crate::accounts::create_from_form`], [`crate::accounts::change_account`]), so the
//!   two cannot differ (the pause's P3).

use std::fmt;

use crate::accounts::{
    AccountBoxes, AccountError, AccountId, BoxName, Clash, Kind, SystemKey, Word,
};
use crate::balances::BalanceError;
use crate::book::BookError;
use crate::date::Date;
use crate::money::{MinorDigits, Money};
use crate::users::UserId;

/// How many owner colours there are (decision 138).
pub const OWNER_COLOURS: u8 = 6;

/// An owner as a page names one: the display name and the colour of its tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerTag {
    pub display_name: String,
    /// 1 to [`OWNER_COLOURS`]: the user's place in the order users were created, the seventh
    /// taking the first colour again.
    pub colour: u8,
}

/// Who owns an account under Assets or Liabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Nobody: the account is joint (SPEC 5.1).
    Joint,
    /// One user.
    User(OwnerTag),
}

/// One line of the list or of an account's "Inside it": an account that leads to its page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: AccountId,
    /// The name as stored, never the full place.
    pub name: String,
    /// 1 for an account directly under its section (or directly inside the account whose page
    /// lists it), one more for each level below.
    pub level: u8,
    /// The owner's tag when the account has an owner.
    pub owner: Option<OwnerTag>,
    /// Whether it is one of the three built-in accounts.
    pub built_in: bool,
    pub archived: bool,
    /// The roll-up at the end of the day in natural sign, under Assets and Liabilities only,
    /// while the page shows figures. An archived account's may differ from zero through
    /// entries dated after the day.
    pub balance: Option<Money>,
}

/// The line Other (decision 119 as the pause's P8 carries it over): the own amount of the
/// account above it, which holds others; not an account and not a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherRow {
    /// The level of what is inside that account.
    pub level: u8,
    /// That account's own live postings up to the day, in natural sign; never zero.
    pub amount: Money,
}

/// A line of the list or of an account's "Inside it", in the order the page shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChartLine {
    /// An account's line.
    Account(Row),
    /// The last line under an account that holds others and has an amount of its own, under
    /// Assets and Liabilities while the page shows figures.
    Other(OtherRow),
}

/// One section of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartSection {
    pub kind: Kind,
    pub id: AccountId,
    /// The root's name as stored: `Assets`, `Liabilities`, `Income`, `Expenses`, `Equity`.
    pub name: String,
    /// The section's roll-up in natural sign: Assets and Liabilities only, while the page shows
    /// figures.
    pub total: Option<Money>,
    /// The accounts as a tree written out in order, each account's Other line after what is
    /// inside it.
    pub rows: Vec<ChartLine>,
    /// Whether the section holds accounts that are all archived and hidden: the page says
    /// "Every account here is archived." in place of "No accounts here yet." (the pause's P8).
    pub all_archived: bool,
}

/// The list at `/accounts` (decision 131).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// The day the balances are for: their end.
    pub as_of: Date,
    pub minor_digits: MinorDigits,
    /// Whether the figures show: some account line with a figure shows under Assets or
    /// Liabilities. Without one, the page has no balances line and no total.
    pub balances_shown: bool,
    /// Whether the archived accounts stand in their places (`?archived=shown`).
    pub archived_shown: bool,
    /// How many accounts are archived, shown or not.
    pub archived: usize,
    pub assets: ChartSection,
    pub liabilities: ChartSection,
    pub income: ChartSection,
    pub expenses: ChartSection,
    pub equity: ChartSection,
}

/// A step of an account's trail: an account that holds it, from the top-level one down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrailStep {
    pub id: AccountId,
    pub name: String,
}

/// One account's page at `/accounts/<id>` (decision 133).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountView {
    pub as_of: Date,
    pub minor_digits: MinorDigits,
    pub id: AccountId,
    pub name: String,
    /// The full place, such as `Assets:Savings:Holiday fund`.
    pub place: String,
    pub kind: Kind,
    pub word: Word,
    /// The accounts that hold it, the top-level one first; its section is not a step.
    pub trail: Vec<TrailStep>,
    /// The key of a built-in account.
    pub built_in: Option<SystemKey>,
    pub archived: bool,
    /// Under Assets and Liabilities: the owner, or joint.
    pub owner: Option<Owner>,
    /// Under Assets and Liabilities: the roll-up at the end of the day in natural sign.
    pub balance: Option<Money>,
    /// For an account under Assets or Liabilities that is neither built in nor archived: its
    /// live opening balance, or none yet (decision 141); `None` for every other account: a
    /// category, a built-in account and an archived account show neither the fact nor its
    /// action.
    pub opening_balance: Option<OpeningFact>,
    /// The accounts directly inside it, archived ones included and marked, each at level 1,
    /// then the Other line of its own amount at level 1 where it has one under Assets and
    /// Liabilities.
    pub inside: Vec<ChartLine>,
    /// Whether an account can be made inside it, so that its page offers "New ... inside": it
    /// is not archived, not built in and not at the sixth level (the pause's P5 and P13).
    pub can_hold: bool,
}

/// The fact Opening balance on an account's page (decision 141): the figure and its day, or
/// None yet, which makes Enter opening balance the page's main action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningFact {
    /// The account has no live opening balance.
    NoneYet,
    /// The live opening balance: its date and its amount in natural sign.
    Entered { date: Date, amount: Money },
}

/// The fact Opening balance of `account`, an account under Assets or Liabilities that is neither
/// built in nor archived, read in the caller's transaction from the opening-balance read
/// ([`crate::transactions::opening_balance`]), never from the roll-up (A7).
async fn opening_fact(
    conn: &mut sqlx::SqliteConnection,
    account: AccountId,
) -> Result<OpeningFact, ChartError> {
    Ok(
        match transactions::opening_balance(conn, account)
            .await
            .map_err(opening_read_error)?
        {
            Some(entry) => OpeningFact::Entered {
                date: entry.date,
                amount: entry.amount,
            },
            None => OpeningFact::NoneYet,
        },
    )
}

/// The failure of the opening-balance read as the pages' models name it: a stored amount
/// without a natural sign is a figure that does not fit (`Overflow`); the chart, the book row
/// and the database pass theirs on. The read refuses nothing else.
fn opening_read_error(error: transactions::OpeningBalanceError) -> ChartError {
    use transactions::OpeningBalanceError as Read;
    match error {
        Read::Corrupt => ChartError::Overflow,
        Read::Accounts(error) => ChartError::Accounts(error),
        Read::Book(error) => ChartError::Book(error),
        Read::Database(error) => ChartError::Database(error),
        _ => ChartError::Database(sqlx::Error::RowNotFound),
    }
}

/// What the opening-balance pages read of an account (decisions 140 and 141, the pause's P5):
/// the form, the confirmation and the clash pages read this and the form-shaped operations only,
/// never the roll-up of balances, so a total that does not fit can never lock them out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpeningBalanceAccount {
    pub id: AccountId,
    pub name: String,
    /// The full place, such as `Assets:Checking`.
    pub place: String,
    /// The accounts that hold it, the top-level one first; its section is not a step.
    pub trail: Vec<TrailStep>,
    /// Its section's kind, which words the amount box and the tick box.
    pub kind: Kind,
    /// The book's minor digits.
    pub minor_digits: MinorDigits,
    pub archived: bool,
    /// Whether it takes an opening balance: the core's own rule (SPEC 6.6, INV-13), an account
    /// under Assets or Liabilities that is neither a section nor built in; the web never
    /// rebuilds it.
    pub takes_opening_balance: bool,
    /// Its live opening balance with its entry and version, in natural sign; `None` when it has
    /// none.
    pub opening_balance: Option<crate::transactions::OpeningBalance>,
}

/// The account `id` as the opening-balance pages read it, in the caller's transaction, from the
/// accounts and the opening-balance read ([`crate::transactions::opening_balance`]) and never
/// from the roll-up (the pause's P5); `None` for an unknown id and for a section.
pub async fn opening_balance_account(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
) -> Result<Option<OpeningBalanceAccount>, ChartError> {
    let minor_digits = book_digits(conn).await?;
    let tree = Tree::load(conn).await?;
    let Some(account) = tree.get(id).filter(|account| !account.is_root()) else {
        return Ok(None);
    };
    let takes_opening_balance = transactions::account_takes_opening_balance(account);
    let opening_balance = if takes_opening_balance {
        transactions::opening_balance(conn, id)
            .await
            .map_err(opening_read_error)?
    } else {
        None
    };
    Ok(Some(OpeningBalanceAccount {
        id,
        name: account.name.clone(),
        place: tree.full(id),
        trail: trail(&tree, id),
        kind: account.kind,
        minor_digits,
        archived: account.is_archived(),
        takes_opening_balance,
        opening_balance,
    }))
}

/// A place a pick list offers: an account by its full place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub id: AccountId,
    pub full_name: String,
    /// Only on the one extra choice a returned form keeps (the pause's P5): the place was
    /// archived meanwhile, and the list marks it "(archived)".
    pub archived: bool,
}

/// What a returned form (422 or 409) holds in its lists (the pause's P5): the place and the
/// owner it sends, which stay in their lists as one extra choice each when the lists no longer
/// offer them, so that what was typed is kept and the next send does not move the account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    pub place: Option<AccountId>,
    pub owner: Option<UserId>,
}

/// An owner a pick list offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerChoice {
    pub id: UserId,
    pub display_name: String,
    /// Offered only on an account already theirs, or kept by a returned form, and marked
    /// "(deactivated)".
    pub deactivated: bool,
}

/// The create form's lists for one section (decision 132).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewForm {
    pub kind: Kind,
    pub word: Word,
    /// The section itself: "Directly under Assets".
    pub section: Place,
    /// The other places in the order of the list page, each in full: archived places,
    /// built-in accounts and places at the sixth level left out (the pause's P5).
    pub places: Vec<Place>,
    /// Under Assets and Liabilities: the active users, by the fold of the display name.
    pub owners: Option<Vec<OwnerChoice>>,
}

/// The change form of one account (decision 134).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditForm {
    pub id: AccountId,
    pub name: String,
    pub place: String,
    pub kind: Kind,
    pub word: Word,
    pub trail: Vec<TrailStep>,
    /// The key of a built-in account, whose form holds the name alone.
    pub built_in: Option<SystemKey>,
    /// Whether the account is archived: its form, like a built-in account's, is headed Change
    /// name and holds the name alone (the pause's P4).
    pub archived: bool,
    /// The section itself: "Directly under Expenses".
    pub section: Place,
    /// The other places of the same section in the order of the list page, the account
    /// itself, what is inside it, archived places, built-in accounts and every place where it
    /// would pass six levels left out, but for the one extra choice a returned form keeps
    /// ([`Kept`]), whatever the reason the list leaves it out, even one that went inside the
    /// account meanwhile;
    /// `None` for a built-in or an archived account.
    pub places: Option<Vec<Place>>,
    /// Under Assets and Liabilities, for an account neither built in nor archived: the active
    /// users, and the owner when deactivated.
    pub owners: Option<Vec<OwnerChoice>>,
    /// What is saved now, which a fresh form shows and sends back as what it showed.
    pub saved: AccountBoxes,
}

/// The list as of `today` (decision 131), read through `conn` in the caller's read transaction;
/// `archived_shown` puts the archived accounts in their places. `NotInitialized` without a
/// book row; `Overflow` when a roll-up or a natural sign does not fit in 64 bits.
pub async fn chart(
    conn: &mut sqlx::SqliteConnection,
    today: Date,
    archived_shown: bool,
) -> Result<Chart, ChartError> {
    let minor_digits = book_digits(conn).await?;
    let read = Read::new(conn, today).await?;
    let balances_shown = [Kind::Asset, Kind::Liability].into_iter().any(|kind| {
        read.tree.root_of(kind).is_some_and(|root| {
            read.tree
                .ordered_children(root.id)
                .iter()
                .any(|account| archived_shown || !account.is_archived())
        })
    });
    let archived = read
        .balances
        .values()
        .filter(|balance| balance.account.is_archived())
        .count();
    let section = |kind| read.section(kind, archived_shown, balances_shown);
    Ok(Chart {
        as_of: today,
        minor_digits,
        balances_shown,
        archived_shown,
        archived,
        assets: section(Kind::Asset)?,
        liabilities: section(Kind::Liability)?,
        income: section(Kind::Income)?,
        expenses: section(Kind::Expense)?,
        equity: section(Kind::Equity)?,
    })
}

/// The page of the account `id` as of `today` (decision 133); `None` for an unknown id and for
/// a root, which has no page of its own (the sections are headings).
pub async fn account_view(
    conn: &mut sqlx::SqliteConnection,
    today: Date,
    id: AccountId,
) -> Result<Option<AccountView>, ChartError> {
    let minor_digits = book_digits(conn).await?;
    let read = Read::new(conn, today).await?;
    let Some(account) = read.tree.get(id).filter(|account| !account.is_root()) else {
        return Ok(None);
    };
    let kind = account.kind;
    let figures = kind.allows_owner();
    let owner = if figures {
        Some(match account.owner_user_id {
            None => Owner::Joint,
            Some(user) => Owner::User(read.owner_tag(user)?),
        })
    } else {
        None
    };
    let children = read.tree.ordered_children(id);
    let mut inside = Vec::with_capacity(children.len());
    for child in &children {
        inside.push(ChartLine::Account(read.row(child, 1, figures)?));
    }
    let own = read.balance(id)?.own;
    if figures && !children.is_empty() && own != Money::ZERO {
        inside.push(ChartLine::Other(OtherRow {
            level: 1,
            amount: natural(own, kind)?,
        }));
    }
    let depth = read.tree.depth(id).unwrap_or(MAX_DEPTH);
    let opening_balance = if figures && !account.is_system() && !account.is_archived() {
        Some(opening_fact(conn, id).await?)
    } else {
        None
    };
    Ok(Some(AccountView {
        as_of: today,
        minor_digits,
        id,
        name: account.name.clone(),
        place: read.tree.full(id),
        kind,
        word: Word::of(kind),
        trail: trail(&read.tree, id),
        built_in: account.system_key,
        archived: account.is_archived(),
        owner,
        balance: if figures {
            Some(natural(read.balance(id)?.total, kind)?)
        } else {
            None
        },
        opening_balance,
        inside,
        can_hold: !account.is_archived() && !account.is_system() && depth < MAX_DEPTH,
    }))
}

/// The create form's lists for the section of `kind` (decision 132), with what a returned form
/// keeps (`Kept::default()` for a fresh form). Equity has no create form on the pages
/// (decision 131), and the caller never asks for it.
pub async fn new_form(
    conn: &mut sqlx::SqliteConnection,
    kind: Kind,
    kept: Kept,
) -> Result<NewForm, ChartError> {
    let tree = Tree::load(conn).await?;
    let users = users::users(conn).await?;
    let root = root_of(&tree, kind)?;
    let refused = Refused {
        word: Word::of(kind),
        place: None,
        action: Action::Created,
    };
    let places = tree
        .walk(root.id)
        .into_iter()
        .filter(|(account, _)| {
            accounts::create_place_refusal(&tree, kind, account.id, &refused).is_none()
                || (kept.place == Some(account.id) && !account.is_system())
        })
        .map(|(account, _)| place(&tree, account))
        .collect();
    Ok(NewForm {
        kind,
        word: Word::of(kind),
        section: place(&tree, root),
        places,
        owners: kind
            .allows_owner()
            .then(|| owner_choices(&users, None, kept.owner)),
    })
}

/// The change form of the account `id` (decision 134), with what a returned form keeps
/// (`Kept::default()` for a fresh form); `None` for an unknown id and for a root.
pub async fn edit_form(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
    kept: Kept,
) -> Result<Option<EditForm>, ChartError> {
    change_form(conn, id, kept, false).await
}

/// The change form the clash page of the account `id` holds (decision 134, the pause's P2 and
/// P4), with what the clashing save sent kept (`kept`): as [`edit_form`], except that when the
/// save changed the place or the owner (`clash.changed`), an archived account's form keeps its
/// Inside and Owner boxes, so that the boxes the person changed on a full form shown before the
/// archive hold what was typed; Save mine then meets the refusal of a move or an owner of an
/// archived account. A built-in account's form holds the name alone.
pub async fn clash_form(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
    kept: Kept,
    clash: &Clash,
) -> Result<Option<EditForm>, ChartError> {
    let full = clash.changed.iter().any(|name| *name != BoxName::Name);
    change_form(conn, id, kept, full).await
}

/// The change form of [`edit_form`]; with `full`, an archived account's form keeps its Inside
/// and Owner boxes ([`clash_form`]).
async fn change_form(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
    kept: Kept,
    full: bool,
) -> Result<Option<EditForm>, ChartError> {
    let tree = Tree::load(conn).await?;
    let users = users::users(conn).await?;
    let Some(account) = tree.get(id).filter(|account| !account.is_root()) else {
        return Ok(None);
    };
    let kind = account.kind;
    let root = root_of(&tree, kind)?;
    let name_only = account.is_system() || account.is_archived();
    let name_only = if full { account.is_system() } else { name_only };
    let refused = tree.refused(account, Action::Moved);
    let places = (!name_only).then(|| {
        tree.walk(root.id)
            .into_iter()
            .filter(|(place, _)| {
                // The kept place stays whatever the reason the list leaves it out (decision 148,
                // leftover N6): archived, the account itself or inside it, built in, too deep now.
                accounts::move_place_refusal(&tree, account, place.id, &refused).is_none()
                    || kept.place == Some(place.id)
            })
            .map(|(place_account, _)| place(&tree, place_account))
            .collect()
    });
    Ok(Some(EditForm {
        id,
        name: account.name.clone(),
        place: tree.full(id),
        kind,
        word: Word::of(kind),
        trail: trail(&tree, id),
        built_in: account.system_key,
        archived: account.is_archived(),
        section: place(&tree, root),
        places,
        owners: (kind.allows_owner() && !name_only)
            .then(|| owner_choices(&users, account.owner_user_id, kept.owner)),
        saved: AccountBoxes {
            name: account.name.clone(),
            parent: account.parent_id,
            owner: account.owner_user_id,
        },
    }))
}

/// What the list and an account's page read in the caller's transaction: the tree, each
/// account's balances as of the day, and the users in the order they were created.
struct Read {
    tree: Tree,
    balances: BTreeMap<AccountId, AccountBalance>,
    users: Vec<User>,
}

impl Read {
    /// Reads the balances as of `today` (decision 96's roll-up, the home page's) and the users.
    async fn new(conn: &mut sqlx::SqliteConnection, today: Date) -> Result<Read, ChartError> {
        let all = balances::balances_as_of(&mut *conn, today).await?;
        let users = users::users(&mut *conn).await?;
        let tree = Tree::from_accounts(all.accounts.iter().map(|balance| balance.account.clone()));
        let balances = all
            .accounts
            .into_iter()
            .map(|balance| (balance.account.id, balance))
            .collect();
        Ok(Read {
            tree,
            balances,
            users,
        })
    }

    /// The balances of `id`, which the read holds for every account.
    fn balance(&self, id: AccountId) -> Result<&AccountBalance, ChartError> {
        self.balances
            .get(&id)
            .ok_or(ChartError::Database(sqlx::Error::RowNotFound))
    }

    /// The tag of the owner `user`; a database failure if the read does not hold the user,
    /// which the foreign key on `accounts.owner_user_id` rules out.
    fn owner_tag(&self, user: UserId) -> Result<OwnerTag, ChartError> {
        let place = self
            .users
            .iter()
            .position(|candidate| candidate.id == user)
            .ok_or(ChartError::Database(sqlx::Error::RowNotFound))?;
        Ok(OwnerTag {
            display_name: self.users[place].display_name.clone(),
            colour: colour(place),
        })
    }

    /// The line of `account` at `level`, with its figure when `figures`.
    fn row(&self, account: &Account, level: u8, figures: bool) -> Result<Row, ChartError> {
        Ok(Row {
            id: account.id,
            name: account.name.clone(),
            level,
            owner: account
                .owner_user_id
                .map(|user| self.owner_tag(user))
                .transpose()?,
            built_in: account.is_system(),
            archived: account.is_archived(),
            balance: if figures {
                Some(natural(self.balance(account.id)?.total, account.kind)?)
            } else {
                None
            },
        })
    }

    /// The section of `kind` (decision 131): its rows as a tree, archived accounts left out
    /// with what is inside them unless `archived_shown`, the figures and the total under Assets
    /// and Liabilities while `balances_shown`.
    fn section(
        &self,
        kind: Kind,
        archived_shown: bool,
        balances_shown: bool,
    ) -> Result<ChartSection, ChartError> {
        let root = root_of(&self.tree, kind)?;
        let figures = balances_shown && kind.allows_owner();
        let mut rows = Vec::new();
        self.write_rows(root.id, 1, archived_shown, figures, &mut rows)?;
        let top = self.tree.ordered_children(root.id);
        Ok(ChartSection {
            kind,
            id: root.id,
            name: root.name.clone(),
            total: if figures {
                Some(natural(self.balance(root.id)?.total, kind)?)
            } else {
                None
            },
            rows,
            all_archived: !archived_shown
                && !top.is_empty()
                && top.iter().all(|account| account.is_archived()),
        })
    }

    /// Writes the lines of the accounts inside `parent` at `level` into `rows`, each followed by
    /// what is inside it and, when `figures`, its Other line.
    fn write_rows(
        &self,
        parent: AccountId,
        level: u8,
        archived_shown: bool,
        figures: bool,
        rows: &mut Vec<ChartLine>,
    ) -> Result<(), ChartError> {
        if usize::from(level) > MAX_DEPTH {
            return Ok(());
        }
        let shown: Vec<&Account> = self
            .tree
            .ordered_children(parent)
            .into_iter()
            .filter(|account| archived_shown || !account.is_archived())
            .collect();
        for account in shown {
            rows.push(ChartLine::Account(self.row(account, level, figures)?));
            let below = level.saturating_add(1);
            let before = rows.len();
            self.write_rows(account.id, below, archived_shown, figures, rows)?;
            let own = self.balance(account.id)?.own;
            if figures && rows.len() > before && own != Money::ZERO {
                rows.push(ChartLine::Other(OtherRow {
                    level: below,
                    amount: natural(own, account.kind)?,
                }));
            }
        }
        Ok(())
    }
}

/// The colour of the user at `place` in the order users were created (decision 138): 1 for the
/// first, [`OWNER_COLOURS`] for the sixth, 1 again for the seventh.
fn colour(place: usize) -> u8 {
    place
        .checked_rem(usize::from(OWNER_COLOURS))
        .and_then(|rest| u8::try_from(rest).ok())
        .and_then(|rest| rest.checked_add(1))
        .unwrap_or(1)
}

/// The owners a list offers (the pause's P5): every user [`accounts::owner_offered`] offers on
/// an account whose saved owner is `saved_owner`, and `kept` when a returned form holds it, each
/// marked when deactivated, by the fold of the display name, then of the username.
fn owner_choices(
    users: &[User],
    saved_owner: Option<UserId>,
    kept: Option<UserId>,
) -> Vec<OwnerChoice> {
    let mut offered: Vec<&User> = users
        .iter()
        .filter(|user| {
            accounts::owner_offered(users, user.id, saved_owner) || kept == Some(user.id)
        })
        .collect();
    offered.sort_by_cached_key(|user| (fold(&user.display_name), fold(&user.username), user.id));
    offered
        .into_iter()
        .map(|user| OwnerChoice {
            id: user.id,
            display_name: user.display_name.clone(),
            deactivated: !user.is_enabled(),
        })
        .collect()
}

/// The place `account` in a pick list: its full name, marked when it is archived (only a
/// choice a returned form keeps can be).
fn place(tree: &Tree, account: &Account) -> Place {
    Place {
        id: account.id,
        full_name: tree.full(account.id),
        archived: account.is_archived(),
    }
}

/// The accounts that hold `id`, the top-level one first; its section is not a step.
fn trail(tree: &Tree, id: AccountId) -> Vec<TrailStep> {
    let chain = tree.chain(id).unwrap_or_default();
    chain
        .iter()
        .skip(1)
        .rev()
        .skip(1)
        .filter_map(|holder| tree.get(*holder))
        .map(|holder| TrailStep {
            id: holder.id,
            name: holder.name.clone(),
        })
        .collect()
}

/// The root of `kind`; `MissingSystemAccount` when it is absent (INV-11 broken).
fn root_of(tree: &Tree, kind: Kind) -> Result<&Account, ChartError> {
    tree.root_of(kind)
        .ok_or(ChartError::Accounts(AccountError::MissingSystemAccount(
            kind.root_key(),
        )))
}

/// `stored` in the natural sign of `kind` (SPEC 6.3); `Overflow` when it does not fit.
fn natural(stored: Money, kind: Kind) -> Result<Money, ChartError> {
    balances::natural(stored, kind).ok_or(ChartError::Overflow)
}

/// The book's minor digits; `NotInitialized` without a book row.
async fn book_digits(conn: &mut sqlx::SqliteConnection) -> Result<MinorDigits, ChartError> {
    book::book(conn)
        .await?
        .map(|book| book.minor_digits)
        .ok_or(ChartError::NotInitialized)
}

use std::collections::BTreeMap;

use crate::accounts::{self, Account, Action, MAX_DEPTH, Refused, Tree};
use crate::balances::{self, AccountBalance};
use crate::book;
use crate::fold::fold;
use crate::transactions;
use crate::users::{self, User};

/// Why a model of the account pages could not be read. None of these is a refusal: a page
/// answers 500 for each (decision 102).
#[derive(Debug)]
pub enum ChartError {
    /// The book is not set up: there is no book row.
    NotInitialized,
    /// A roll-up or a natural sign does not fit in 64 bits.
    Overflow,
    /// The book row could not be read.
    Book(BookError),
    /// The chart could not be read, or lacks a root.
    Accounts(AccountError),
    /// Any other database failure.
    Database(sqlx::Error),
}

impl fmt::Display for ChartError {
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

impl std::error::Error for ChartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Book(error) => Some(error),
            Self::Accounts(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::NotInitialized | Self::Overflow => None,
        }
    }
}

impl From<BalanceError> for ChartError {
    fn from(error: BalanceError) -> Self {
        match error {
            BalanceError::Overflow => Self::Overflow,
            BalanceError::Accounts(error) => Self::Accounts(error),
            BalanceError::Database(error) => Self::Database(error),
        }
    }
}

impl From<BookError> for ChartError {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<AccountError> for ChartError {
    fn from(error: AccountError) -> Self {
        Self::Accounts(error)
    }
}

impl From<sqlx::Error> for ChartError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}
