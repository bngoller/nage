//! The chart of accounts (SPEC 6.2, W2, INV-8 to INV-12's Core halves, decisions 92, 93, 95 and
//! 96): the five kinds and the eight system keys, the account model, and the operations that
//! create, rename, move, re-own, archive and unarchive accounts, each taking the write
//! transaction, the acting user and the instant, refusing with a message a form can show, and
//! writing the updated pair on every change. The tree is small, so every operation loads it in
//! one query and walks it in Rust (depth, descendants, cycles). Setup alone writes system keys.

use std::fmt;
use std::str::FromStr;

use crate::fold::NameError;
use crate::storage::WriteTransaction;
use crate::timestamp::Timestamp;
use crate::users::UserId;

/// The most path segments a full name may have, the root included (SPEC 6.2, INV-9).
pub const MAX_DEPTH: usize = 6;

/// An account's id: `accounts.id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]
#[sqlx(transparent)]
pub struct AccountId(pub i64);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The five kinds of SPEC 6.2, stored as the singular words of schema 0002's CHECK; every
/// account inherits its root's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Asset,
    Liability,
    Equity,
    Income,
    Expense,
}

impl Kind {
    /// The five kinds in the order SPEC 6.2 names the roots.
    pub const ALL: [Kind; 5] = [
        Kind::Asset,
        Kind::Liability,
        Kind::Equity,
        Kind::Income,
        Kind::Expense,
    ];

    /// The stored text: `asset`, `liability`, `equity`, `income`, `expense`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Asset => "asset",
            Kind::Liability => "liability",
            Kind::Equity => "equity",
            Kind::Income => "income",
            Kind::Expense => "expense",
        }
    }

    /// The system key of this kind's root: `assets`, `liabilities`, `equity`, `income`,
    /// `expenses`.
    pub const fn root_key(self) -> SystemKey {
        match self {
            Kind::Asset => SystemKey::Assets,
            Kind::Liability => SystemKey::Liabilities,
            Kind::Equity => SystemKey::Equity,
            Kind::Income => SystemKey::Income,
            Kind::Expense => SystemKey::Expenses,
        }
    }

    /// The root's name at setup (SPEC 6.2): `Assets`, `Liabilities`, `Equity`, `Income`,
    /// `Expenses`.
    pub const fn root_name(self) -> &'static str {
        match self {
            Kind::Asset => "Assets",
            Kind::Liability => "Liabilities",
            Kind::Equity => "Equity",
            Kind::Income => "Income",
            Kind::Expense => "Expenses",
        }
    }

    /// The sign of SPEC 6.3 (`natural = stored × kind_sign`): +1 for Assets and Expenses, −1 for
    /// Liabilities, Income and Equity.
    pub const fn sign(self) -> i64 {
        match self {
            Kind::Asset | Kind::Expense => 1,
            Kind::Liability | Kind::Income | Kind::Equity => -1,
        }
    }

    /// Whether accounts of this kind may have an owner (SPEC 6.2: Assets and Liabilities only).
    pub const fn allows_owner(self) -> bool {
        matches!(self, Kind::Asset | Kind::Liability)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = UnknownKind;

    /// Exactly one of the five stored texts.
    fn from_str(text: &str) -> Result<Kind, UnknownKind> {
        Kind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| UnknownKind(text.to_owned()))
    }
}

/// A text that is not one of the five kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownKind(pub String);

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown account kind {:?}", self.0)
    }
}

impl std::error::Error for UnknownKind {}

/// The eight system keys of SPEC 6.2, by which code and `check` find the system accounts after
/// a rename: the five roots and `Equity:Opening Balances`, `Equity:Valuation Changes` and
/// `Expenses:Uncategorized`. Only setup writes them (decision 92).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemKey {
    Assets,
    Liabilities,
    Equity,
    Income,
    Expenses,
    OpeningBalances,
    ValuationChanges,
    Uncategorized,
}

impl SystemKey {
    /// The eight keys: the roots in SPEC 6.2's order, then the three other system accounts.
    pub const ALL: [SystemKey; 8] = [
        SystemKey::Assets,
        SystemKey::Liabilities,
        SystemKey::Equity,
        SystemKey::Income,
        SystemKey::Expenses,
        SystemKey::OpeningBalances,
        SystemKey::ValuationChanges,
        SystemKey::Uncategorized,
    ];

    /// The stored text: `assets`, `liabilities`, `equity`, `income`, `expenses`,
    /// `opening_balances`, `valuation_changes`, `uncategorized`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SystemKey::Assets => "assets",
            SystemKey::Liabilities => "liabilities",
            SystemKey::Equity => "equity",
            SystemKey::Income => "income",
            SystemKey::Expenses => "expenses",
            SystemKey::OpeningBalances => "opening_balances",
            SystemKey::ValuationChanges => "valuation_changes",
            SystemKey::Uncategorized => "uncategorized",
        }
    }

    /// The kind of the account the key names.
    pub const fn kind(self) -> Kind {
        match self {
            SystemKey::Assets => Kind::Asset,
            SystemKey::Liabilities => Kind::Liability,
            SystemKey::Equity | SystemKey::OpeningBalances | SystemKey::ValuationChanges => {
                Kind::Equity
            }
            SystemKey::Income => Kind::Income,
            SystemKey::Expenses | SystemKey::Uncategorized => Kind::Expense,
        }
    }

    /// The account's name at setup (SPEC 6.2); the three non-roots can be renamed afterwards.
    pub const fn name(self) -> &'static str {
        match self {
            SystemKey::OpeningBalances => "Opening Balances",
            SystemKey::ValuationChanges => "Valuation Changes",
            SystemKey::Uncategorized => "Uncategorized",
            root => root.kind().root_name(),
        }
    }

    /// The key of the root the account sits under: `None` for a root, the kind's root key for
    /// the three others.
    pub const fn parent_key(self) -> Option<SystemKey> {
        if self.is_root() {
            None
        } else {
            Some(self.kind().root_key())
        }
    }

    /// Whether the key names a root.
    pub const fn is_root(self) -> bool {
        matches!(
            self,
            SystemKey::Assets
                | SystemKey::Liabilities
                | SystemKey::Equity
                | SystemKey::Income
                | SystemKey::Expenses
        )
    }
}

impl fmt::Display for SystemKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SystemKey {
    type Err = UnknownSystemKey;

    /// Exactly one of the eight stored texts.
    fn from_str(text: &str) -> Result<SystemKey, UnknownSystemKey> {
        SystemKey::ALL
            .into_iter()
            .find(|key| key.as_str() == text)
            .ok_or_else(|| UnknownSystemKey(text.to_owned()))
    }
}

/// A text that is not one of the eight system keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownSystemKey(pub String);

impl fmt::Display for UnknownSystemKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown system key {:?}", self.0)
    }
}

impl std::error::Error for UnknownSystemKey {}

/// A row of `accounts` as the core reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    /// The name as stored: normalized, never folded.
    pub name: String,
    /// `None` for a root.
    pub parent_id: Option<AccountId>,
    pub kind: Kind,
    /// `Some` for the eight system accounts.
    pub system_key: Option<SystemKey>,
    /// `None` means joint (SPEC 5.1).
    pub owner_user_id: Option<UserId>,
    /// Always false in this slice (milestone 7 sets it).
    pub is_balance_only: bool,
    /// `Some` while archived.
    pub archived_at: Option<Timestamp>,
    pub created_by: UserId,
    pub created_at: Timestamp,
    pub updated_by: UserId,
    pub updated_at: Timestamp,
}

impl Account {
    /// Whether the account is one of the five roots.
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Whether the account is one of the eight system accounts.
    pub fn is_system(&self) -> bool {
        self.system_key.is_some()
    }

    /// Whether the account is archived.
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// What a new account is made of (W2): the name as typed, the parent, and the owner for an
/// Assets or Liabilities account (`None` means joint). The kind is the parent's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
    pub name: String,
    pub parent: AccountId,
    pub owner: Option<UserId>,
}

/// Creates an account under `new.parent` at `now` by `by` (SPEC 6.2, W2, decision 96), stopping
/// at its first refusal: the name is normalized and checked (`Name`); the parent must exist
/// (`PlaceNotListed`) and not be archived (`ParentArchived`); the depth stays within
/// [`MAX_DEPTH`] (`TooDeep`); an owner needs an Assets or Liabilities parent
/// (`OwnerNotAllowed`) and must exist (`OwnerNotListed`), a deactivated user included (decision
/// 96: ownership is a label); the name must be new among the parent's children by the fold,
/// archived children included (`NameTaken`). The kind is the parent's, the system key `None`,
/// the balance-only flag off, and the updated pair equals the created pair. Returns the row as
/// stored. The create form goes through [`create_from_form`], which gathers every refusal of
/// the form at once and judges the place and the owner by the form's own pick lists.
pub async fn create_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    new: &NewAccount,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let found = tree.get(new.parent);
    let refused = Refused {
        word: found.map_or(Word::Account, |parent| Word::of(parent.kind)),
        place: None,
        action: Action::Created,
    };
    let name = fold::checked_name(&new.name).map_err(|error| AccountError::Name {
        refused: refused.clone(),
        error,
    })?;
    let (Some(parent), Some(parent_depth)) = (found, tree.depth(new.parent)) else {
        return Err(AccountError::PlaceNotListed { refused });
    };
    if parent.is_archived() {
        return Err(AccountError::ParentArchived {
            refused,
            parent: tree.full(parent.id),
        });
    }
    if parent_depth >= MAX_DEPTH {
        return Err(AccountError::TooDeep {
            refused,
            depth: parent_depth.saturating_add(1),
            inside: false,
        });
    }
    if let Some(owner) = new.owner {
        check_owner(tx, parent.kind, owner, &refused).await?;
    }
    if let Some(sibling) = tree.sibling_named(Some(parent.id), &fold::fold(&name), None) {
        return Err(AccountError::NameTaken {
            refused,
            holder: tree.full(parent.id),
            name: sibling.name.clone(),
        });
    }
    let row = NewRow {
        name: &name,
        parent: Some(parent.id),
        kind: parent.kind,
        system_key: None,
        owner: new.owner,
    };
    insert_row(tx, by, now, row).await
}

/// Renames `account` (SPEC 6.2, W2): not a section (`Section`; the three built-in accounts may
/// be renamed, and so may an archived account); the name checked as for creation (`Name`) and
/// new among the siblings by the fold (`NameTaken`), the account itself excepted, so a change
/// of case alone is allowed; an unknown id is `NoSuchAccount`. Writes the updated pair.
pub async fn rename_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    name: &str,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    let refused = tree.refused(current, Action::Renamed);
    if current.is_root() {
        return Err(AccountError::Section { refused });
    }
    let name = fold::checked_name(name).map_err(|error| AccountError::Name {
        refused: refused.clone(),
        error,
    })?;
    let folded = fold::fold(&name);
    if let Some(sibling) = tree.sibling_named(current.parent_id, &folded, Some(account)) {
        return Err(AccountError::NameTaken {
            refused,
            holder: current
                .parent_id
                .map(|parent| tree.full(parent))
                .unwrap_or_default(),
            name: sibling.name.clone(),
        });
    }
    write_name(tx, by, now, account, &name).await?;
    reload(tx, account).await
}

/// Moves `account` under `new_parent` (SPEC 6.2, W2): not a section (`Section`) or a built-in
/// account (`BuiltIn`); the new parent must exist and not be a built-in account, which holds
/// nothing (`PlaceNotListed`), have the account's kind (`KindMismatch`), be neither the account
/// nor one of its descendants (`MoveUnderDescendant`) and not be archived (`ParentArchived`),
/// judged by the change form's own rule ([`move_place_refusal`]); the moved subtree's deepest leaf
/// must stay within [`MAX_DEPTH`] (`TooDeep`); the name must be new among the new siblings by
/// the fold (`NameTaken`); an unknown account is `NoSuchAccount`. Writes the updated pair.
pub async fn move_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    new_parent: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let moving = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    let refused = tree.refused(moving, Action::Moved);
    if moving.is_root() {
        return Err(AccountError::Section { refused });
    }
    if moving.is_system() {
        return Err(AccountError::BuiltIn { refused });
    }
    if let Some(error) = move_place_refusal(&tree, moving, new_parent, &refused) {
        return Err(error);
    }
    if let Some(sibling) =
        tree.sibling_named(Some(new_parent), &fold::fold(&moving.name), Some(account))
    {
        return Err(AccountError::NameTaken {
            refused,
            holder: tree.full(new_parent),
            name: sibling.name.clone(),
        });
    }
    write_parent(tx, by, now, account, new_parent).await?;
    reload(tx, account).await
}

/// Sets or clears the owner of `account` (SPEC 5.1, 6.2): not a section (`Section`); an Assets
/// or Liabilities account that is not a built-in one (`OwnerNotAllowed`, which every built-in
/// account meets, all three lying under Equity and Expenses); the owner must exist
/// (`OwnerNotListed`), a deactivated user included, since ownership is a label; an unknown
/// account is `NoSuchAccount`. Writes the updated pair.
pub async fn set_owner(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    owner: Option<UserId>,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    let refused = tree.refused(current, Action::Changed);
    if current.is_root() {
        return Err(AccountError::Section { refused });
    }
    if !current.kind.allows_owner() || current.is_system() {
        return Err(AccountError::OwnerNotAllowed { refused });
    }
    if let Some(owner) = owner {
        check_owner(tx, current.kind, owner, &refused).await?;
    }
    write_owner(tx, by, now, account, owner).await?;
    reload(tx, account).await
}

/// Archives `account` at `now` (SPEC 6.2, INV-12's Core half): not a section (`Section`) or a
/// built-in account (`BuiltIn`); not already archived (`AlreadyArchived`, which the pages treat
/// as nothing to do); every child archived (`ChildrenNotArchived`); the account's own live
/// postings summing to zero over all dates (`BalanceNotZero`, with that sum in natural sign);
/// an unknown account is `NoSuchAccount`. Writes `archived_at = now` and the updated pair.
pub async fn archive_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    let refused = tree.refused(current, Action::Archived);
    if current.is_root() {
        return Err(AccountError::Section { refused });
    }
    if current.is_system() {
        return Err(AccountError::BuiltIn { refused });
    }
    let place = tree.full(account);
    if current.is_archived() {
        return Err(AccountError::AlreadyArchived { account: place });
    }
    if let Some(child) = tree
        .ordered_children(account)
        .into_iter()
        .find(|child| !child.is_archived())
    {
        return Err(AccountError::ChildrenNotArchived {
            account: place,
            child: tree.full(child.id),
        });
    }
    let balance = balances::own_balance(tx, account).await?;
    if balance != Money::ZERO {
        let digits = book_digits(tx).await?;
        return Err(AccountError::BalanceNotZero {
            kind: current.kind,
            account: place,
            balance: balances::natural(balance, current.kind).unwrap_or(balance),
            digits,
        });
    }
    sqlx::query!(
        "UPDATE accounts SET archived_at = ?1, updated_by = ?2, updated_at = ?1 WHERE id = ?3",
        now,
        by,
        account
    )
    .execute(&mut **tx)
    .await?;
    reload(tx, account).await
}

/// Unarchives `account` (SPEC 6.2 as decision 96 reads it): archived (`NotArchived`, which the
/// pages treat as nothing to do) and its parent not archived (`ParentArchivedUnarchiveFirst`),
/// so that every child of an archived account stays archived (INV-12); an unknown account is
/// `NoSuchAccount`. Clears `archived_at` and writes the updated pair.
pub async fn unarchive_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if !current.is_archived() {
        return Err(AccountError::NotArchived {
            account: tree.full(account),
        });
    }
    if let Some(parent) = current
        .parent_id
        .and_then(|parent| tree.get(parent))
        .filter(|parent| parent.is_archived())
    {
        return Err(AccountError::ParentArchivedUnarchiveFirst {
            account: tree.full(account),
            parent: tree.full(parent.id),
        });
    }
    sqlx::query!(
        "UPDATE accounts SET archived_at = NULL, updated_by = ?1, updated_at = ?2 WHERE id = ?3",
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    reload(tx, account).await
}

/// The account `id`, or `None`.
pub async fn account(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
) -> Result<Option<Account>, AccountError> {
    Ok(sqlx::query_as!(
        Account,
        r#"SELECT id AS "id!: AccountId", name, parent_id AS "parent_id?: AccountId", kind AS "kind!: Kind", system_key AS "system_key?: SystemKey", owner_user_id AS "owner_user_id?: UserId", is_balance_only AS "is_balance_only!: bool", archived_at AS "archived_at?: Timestamp", created_by AS "created_by!: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by!: UserId", updated_at AS "updated_at!: Timestamp" FROM accounts WHERE id = ?1"#,
        id
    )
    .fetch_optional(conn)
    .await?)
}

/// Every account, by id.
pub async fn accounts(conn: &mut sqlx::SqliteConnection) -> Result<Vec<Account>, AccountError> {
    Ok(sqlx::query_as!(
        Account,
        r#"SELECT id AS "id!: AccountId", name, parent_id AS "parent_id?: AccountId", kind AS "kind!: Kind", system_key AS "system_key?: SystemKey", owner_user_id AS "owner_user_id?: UserId", is_balance_only AS "is_balance_only!: bool", archived_at AS "archived_at?: Timestamp", created_by AS "created_by!: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by!: UserId", updated_at AS "updated_at!: Timestamp" FROM accounts ORDER BY id"#
    )
    .fetch_all(conn)
    .await?)
}

/// The children of `parent`, by id (archived ones included).
pub async fn children(
    conn: &mut sqlx::SqliteConnection,
    parent: AccountId,
) -> Result<Vec<Account>, AccountError> {
    Ok(sqlx::query_as!(
        Account,
        r#"SELECT id AS "id!: AccountId", name, parent_id AS "parent_id?: AccountId", kind AS "kind!: Kind", system_key AS "system_key?: SystemKey", owner_user_id AS "owner_user_id?: UserId", is_balance_only AS "is_balance_only!: bool", archived_at AS "archived_at?: Timestamp", created_by AS "created_by!: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by!: UserId", updated_at AS "updated_at!: Timestamp" FROM accounts WHERE parent_id = ?1 ORDER BY id"#,
        parent
    )
    .fetch_all(conn)
    .await?)
}

/// The system account `key` names (SPEC 6.2), found by its key whatever its name;
/// `MissingSystemAccount` when it is absent (INV-11 broken).
pub async fn system_account(
    conn: &mut sqlx::SqliteConnection,
    key: SystemKey,
) -> Result<Account, AccountError> {
    sqlx::query_as!(
        Account,
        r#"SELECT id AS "id!: AccountId", name, parent_id AS "parent_id?: AccountId", kind AS "kind!: Kind", system_key AS "system_key?: SystemKey", owner_user_id AS "owner_user_id?: UserId", is_balance_only AS "is_balance_only!: bool", archived_at AS "archived_at?: Timestamp", created_by AS "created_by!: UserId", created_at AS "created_at!: Timestamp", updated_by AS "updated_by!: UserId", updated_at AS "updated_at!: Timestamp" FROM accounts WHERE system_key = ?1"#,
        key
    )
    .fetch_optional(conn)
    .await?
    .ok_or(AccountError::MissingSystemAccount(key))
}

/// The full name of `id` (SPEC 6.2): the names from the root down, joined with a colon, such
/// as `Expenses:Food:Groceries`; `NoSuchAccount` for an unknown id.
pub async fn full_name(
    conn: &mut sqlx::SqliteConnection,
    id: AccountId,
) -> Result<String, AccountError> {
    let tree = Tree::load(conn).await?;
    tree.full_name(id).ok_or(AccountError::NoSuchAccount)
}

/// The account whose full name is `full_name`, each segment matched by the fold against the
/// children of the previous one (so `expenses:food` finds `Expenses:Food`), archived accounts
/// included; `None` when a segment is empty or matches nothing.
pub async fn resolve(
    conn: &mut sqlx::SqliteConnection,
    full_name: &str,
) -> Result<Option<Account>, AccountError> {
    let tree = Tree::load(conn).await?;
    Ok(tree.resolve(full_name).cloned())
}

/// The word the pages use for an account (decision 129): "category" under Income and
/// Expenses, "account" under Assets, Liabilities and Equity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Account,
    Category,
}

impl Word {
    /// The word for an account of `kind`.
    pub const fn of(kind: Kind) -> Word {
        match kind {
            Kind::Income | Kind::Expense => Word::Category,
            Kind::Asset | Kind::Liability | Kind::Equity => Word::Account,
        }
    }

    /// `account` or `category`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Word::Account => "account",
            Word::Category => "category",
        }
    }

    /// The word with its article: `an account` or `a category`.
    pub const fn with_article(self) -> &'static str {
        match self {
            Word::Account => "an account",
            Word::Category => "a category",
        }
    }

    /// The plural: `accounts` or `categories`.
    pub const fn plural(self) -> &'static str {
        match self {
            Word::Account => "accounts",
            Word::Category => "categories",
        }
    }

    /// The word with a capital: `Account` or `Category`.
    pub const fn capitalized(self) -> &'static str {
        match self {
            Word::Account => "Account",
            Word::Category => "Category",
        }
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What an account refusal says was not done (decision 136): the verb of its first part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Created,
    Renamed,
    Moved,
    /// Its owner was to change.
    Changed,
    Archived,
    Unarchived,
}

impl Action {
    /// The past participle: `created`, `renamed`, `moved`, `changed`, `archived`,
    /// `unarchived`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Action::Created => "created",
            Action::Renamed => "renamed",
            Action::Moved => "moved",
            Action::Changed => "changed",
            Action::Archived => "archived",
            Action::Unarchived => "unarchived",
        }
    }
}

/// The first part of an account refusal (decision 102): what was refused. A new account has no
/// place yet and is named by its word ("The category was not created"); an existing one by its
/// full place ("Assets:Savings was not moved").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub word: Word,
    /// The full place, such as `Assets:Savings`; `None` for an account not yet created.
    pub place: Option<String>,
    pub action: Action,
}

impl Refused {
    /// The end of the remedy: `create it again` for a new account, `save again` otherwise.
    fn again(&self) -> &'static str {
        match self.action {
            Action::Created => "create it again",
            _ => "save again",
        }
    }

    /// `its name` for a new account, `its new name` for a rename.
    fn its_name(&self) -> &'static str {
        match self.action {
            Action::Created => "its name",
            _ => "its new name",
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.place {
            Some(place) => write!(f, "{place} was not {}", self.action.as_str()),
            None => write!(f, "The {} was not {}", self.word, self.action.as_str()),
        }
    }
}

/// Why an account operation was refused (SPEC 6.2, W2, decisions 96 and 136). Each message says
/// what was refused, why and what to do (decision 102), except a section's refusal, where
/// nothing can be done, and the clash's, whose page offers the two choices; it names accounts
/// by their full place and says "category" under Income and Expenses (decision 129), and can be
/// shown beside a form (W2: every refusal explains itself). `NoSuchAccount` is answered by the
/// page for an unknown address, which never prints the number (decision 136 amending 102);
/// `MissingSystemAccount` and `Database` are failures, not refusals.
#[derive(Debug)]
pub enum AccountError {
    /// The name is refused by the name rules.
    Name { refused: Refused, error: NameError },
    /// A sibling, archived or not, has this name ignoring case where the form ends: `holder` is
    /// the full place of the parent, `name` the sibling's name as stored.
    NameTaken {
        refused: Refused,
        holder: String,
        name: String,
    },
    /// No account has this id.
    NoSuchAccount,
    /// The place chosen is not one the form's pick list offers: a number no account has, or
    /// that is not a well-formed number, a place in another section, another section, a
    /// built-in account (the pause's P3 and P9: Nage deletes no account, so "no longer exists"
    /// is never true).
    PlaceNotListed { refused: Refused },
    /// The owner chosen is not one the form's pick list offers: a number no user has, or that
    /// is not a well-formed number, or a deactivated user who is not the account's saved owner
    /// (the pause's P3 and P9).
    OwnerNotListed { refused: Refused },
    /// The account, at the full place `account`, was archived after a full change form was
    /// shown, and the save changes its place or its owner (the pause's P4): only its name can
    /// be changed now. It belongs to no box of the form and shows at the top.
    ArchivedMeanwhile { account: String, kind: Kind },
    /// Every refusal of one form at once (the pause's P3): the create form's, from
    /// [`create_from_form`].
    Form(Box<FormRefusals>),
    /// The parent, at the full place `parent`, is archived: nothing live is created or moved
    /// under it.
    ParentArchived { refused: Refused, parent: String },
    /// A root (one of the five sections) is never renamed, moved, archived or given an owner.
    Section { refused: Refused },
    /// A built-in (system) account other than a root is never moved or archived.
    BuiltIn { refused: Refused },
    /// The new parent, at the full place `target`, is the account or one of its descendants.
    MoveUnderDescendant {
        word: Word,
        account: String,
        target: String,
    },
    /// The result would be `depth` segments deep, more than [`MAX_DEPTH`]; `inside` when the
    /// deepest account is one inside the moved account rather than the account itself.
    TooDeep {
        refused: Refused,
        depth: usize,
        inside: bool,
    },
    /// The new parent is in another section: `section` is the name of the section asked for.
    KindMismatch {
        word: Word,
        account: String,
        section: String,
    },
    /// Only Assets and Liabilities accounts that are not roots or system accounts have owners.
    OwnerNotAllowed { refused: Refused },
    /// A child, at the full place `child`, is not archived.
    ChildrenNotArchived { account: String, child: String },
    /// The account's own live postings over all dates sum to `balance` (natural sign), not
    /// zero; under Liabilities the sentence says what is owed on it.
    BalanceNotZero {
        kind: Kind,
        account: String,
        balance: Money,
        digits: MinorDigits,
    },
    /// The account is archived already.
    AlreadyArchived { account: String },
    /// The account is not archived.
    NotArchived { account: String },
    /// The parent, at the full place `parent`, is archived: unarchive it first.
    ParentArchivedUnarchiveFirst { account: String, parent: String },
    /// A system account is missing (INV-11 broken).
    MissingSystemAccount(SystemKey),
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name { refused, error } => {
                write!(f, "{refused} because {} {error}. ", refused.its_name())?;
                let remedy = match error {
                    NameError::Empty => "Type a name",
                    NameError::TooLong { .. } => "Shorten it",
                    NameError::Control => "Type it again on one line",
                    NameError::Colon => {
                        f.write_str("Nage uses the colon to write where a thing sits, as in Expenses:Groceries. ")?;
                        "Use another sign, such as a dash"
                    }
                };
                write!(f, "{remedy}, then {}.", refused.again())
            }
            Self::NameTaken {
                refused,
                holder,
                name,
            } => {
                write!(
                    f,
                    "{refused} because {holder} already holds one named {name}. Capitals and extra spaces do not count, and archived ones count too. "
                )?;
                f.write_str(match refused.action {
                    Action::Moved => "Choose another place, or change its name as well.",
                    _ => "Choose another name.",
                })
            }
            Self::NoSuchAccount => f.write_str(
                "Nage could not find this account, because no account has that number. Go to Accounts and choose it from the list.",
            ),
            Self::PlaceNotListed { refused } => write!(
                f,
                "{refused} because the place chosen for it is not one of the places in the list. Choose its place again."
            ),
            Self::OwnerNotListed { refused } => write!(
                f,
                "{refused} because the owner chosen is not one of the users in the list. Choose Joint or one of the users listed."
            ),
            Self::ArchivedMeanwhile { account, kind } => {
                write!(
                    f,
                    "{account} was archived while you were working, by another user or in another window, so only its name can be changed now. "
                )?;
                f.write_str(if kind.allows_owner() {
                    "Unarchive it first to move it or to change its owner."
                } else {
                    "Unarchive it first to move it."
                })
            }
            Self::Form(refusals) => write!(f, "{refusals}"),
            Self::ParentArchived { refused, parent } => write!(
                f,
                "{refused} because {parent} is archived, and nothing new goes inside an archived {}. Choose another place, or unarchive it first.",
                refused.word
            ),
            Self::Section { refused } => write!(
                f,
                "{refused} because it is one of Nage's five sections, which stay as they are."
            ),
            Self::BuiltIn { refused } => write!(
                f,
                "{refused} because it is built in: Nage itself uses it. You can still rename it."
            ),
            Self::MoveUnderDescendant {
                word,
                account,
                target,
            } => {
                if target == account {
                    write!(
                        f,
                        "{account} was not moved because {} cannot go inside itself. Choose another place.",
                        word.with_article()
                    )
                } else {
                    write!(
                        f,
                        "{account} was not moved because {target} is inside it, and {} cannot go inside itself. Choose another place.",
                        word.with_article()
                    )
                }
            }
            Self::TooDeep {
                refused,
                depth,
                inside,
            } => {
                let what = if *inside {
                    format!("the deepest {} inside it", refused.word)
                } else {
                    String::from("it")
                };
                write!(
                    f,
                    "{refused} because {what} would sit {depth} levels deep, and Nage allows {MAX_DEPTH}, the section included. Choose a place higher up."
                )
            }
            Self::KindMismatch {
                word,
                account,
                section,
            } => write!(
                f,
                "{account} was not moved because {} stays in its section. To have it under {section}, make a new one there and archive this one.",
                word.with_article()
            ),
            Self::OwnerNotAllowed { refused } => write!(
                f,
                "{refused} because only accounts under Assets and Liabilities have an owner. Leave it without one."
            ),
            Self::ChildrenNotArchived { account, child } => write!(
                f,
                "{account} was not archived because {child} inside it is not archived. Archive that one first."
            ),
            Self::BalanceNotZero {
                kind,
                account,
                balance,
                digits,
            } => {
                let figure = balance.display(*digits);
                let why = match kind {
                    Kind::Liability => format!("{figure} is owed on it"),
                    _ => format!("it holds {figure}"),
                };
                write!(
                    f,
                    "{account} was not archived because {why}, and only {} at zero can be archived. Bring it to zero first.",
                    Word::of(*kind).with_article()
                )
            }
            Self::AlreadyArchived { account } => write!(
                f,
                "{account} was not archived because it is archived already. Nothing needs to be done."
            ),
            Self::NotArchived { account } => write!(
                f,
                "{account} was not unarchived because it is not archived. Nothing needs to be done."
            ),
            Self::ParentArchivedUnarchiveFirst { account, parent } => write!(
                f,
                "{account} was not unarchived because {parent}, which holds it, is archived. Unarchive that one first."
            ),
            Self::MissingSystemAccount(key) => write!(f, "the system account {key} is missing"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for AccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name { error, .. } => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

/// Every refusal of one form at once (decisions 132 and 134, the pause's P3, the maintainer's
/// ruling "Every problem at once"): at most one per box, each shown under its box, in the order
/// name, inside, owner. The name box holds the first broken name rule, else the name taken
/// where the form ends, which is left out when the place itself is refused. A refusal that
/// belongs to no box of the form (P4's archived meanwhile) is `top`, shown in the message at
/// the top of the form.
#[derive(Debug, Default)]
pub struct FormRefusals {
    pub top: Option<AccountError>,
    pub name: Option<AccountError>,
    pub inside: Option<AccountError>,
    pub owner: Option<AccountError>,
}

impl FormRefusals {
    /// Whether no box was refused.
    pub fn is_empty(&self) -> bool {
        self.top.is_none() && self.name.is_none() && self.inside.is_none() && self.owner.is_none()
    }
}

impl fmt::Display for FormRefusals {
    /// The sentences in the order the form shows them, the top one first, one space apart.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sentences: Vec<String> = [&self.top, &self.name, &self.inside, &self.owner]
            .into_iter()
            .flatten()
            .map(ToString::to_string)
            .collect();
        f.write_str(&sentences.join(" "))
    }
}

/// One box of a form as a request sends it (the pause's P2, P3 and P6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent<T> {
    /// The request does not carry the box, or the page's form has no such box (the handler
    /// reads only the boxes its form has): on the change form an unchanged box, on the create
    /// form an empty one (directly under the section, Joint).
    Absent,
    /// The box's value, a number parsed as Nage prints it.
    Value(T),
    /// A value that is not a well-formed number: refused under its box as not in the list, and,
    /// as a value the form showed, equal to nothing saved.
    Malformed,
}

/// The boxes of an account form as a request sends them: the name, the place it goes inside
/// and the owner (`Value(None)` for Joint). The change form also sends back, in hidden fields,
/// the values it showed (decision 134).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentBoxes {
    pub name: Sent<String>,
    pub inside: Sent<AccountId>,
    pub owner: Sent<Option<UserId>>,
}

/// Creates an account from the create form of the section `under` (decision 132; the pause's
/// P3 and P6), by `by` at `now`, in `tx`: the name always; `inside` absent means directly under
/// the section; `owner` is read only under Assets and Liabilities, and absent means Joint. The
/// place and the owner are judged by the same function as the form's pick lists
/// ([`crate::chart::new_form`]), so the list and the check cannot differ: a number no account
/// has, a malformed one, a place in another section, another section or a built-in account is
/// `PlaceNotListed` under Inside; an archived place `ParentArchived`; a place at the sixth level
/// `TooDeep`; an owner no user is, a malformed one or a deactivated user `OwnerNotListed` under
/// Owner; an owner sent under Income or Expenses `OwnerNotAllowed`. Every refusal is gathered
/// before anything is written (decision 99) and returned at once as `AccountError::Form`;
/// otherwise it creates as [`create_account`] does.
pub async fn create_from_form(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    under: Kind,
    sent: &SentBoxes,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let section = tree
        .root_of(under)
        .ok_or(AccountError::MissingSystemAccount(under.root_key()))?;
    let refused = Refused {
        word: Word::of(under),
        place: None,
        action: Action::Created,
    };
    let mut refusals = FormRefusals::default();
    let typed = match &sent.name {
        Sent::Value(name) => name.as_str(),
        Sent::Absent | Sent::Malformed => "",
    };
    let name = match fold::checked_name(typed) {
        Ok(name) => Some(name),
        Err(error) => {
            refusals.name = Some(AccountError::Name {
                refused: refused.clone(),
                error,
            });
            None
        }
    };
    let place = match sent.inside {
        Sent::Absent => Some(section.id),
        Sent::Value(target) => match create_place_refusal(&tree, under, target, &refused) {
            None => Some(target),
            Some(error) => {
                refusals.inside = Some(error);
                None
            }
        },
        Sent::Malformed => {
            refusals.inside = Some(AccountError::PlaceNotListed {
                refused: refused.clone(),
            });
            None
        }
    };
    let owner = match sent.owner {
        Sent::Absent | Sent::Value(None) => None,
        Sent::Value(Some(_)) | Sent::Malformed if !under.allows_owner() => {
            refusals.owner = Some(AccountError::OwnerNotAllowed {
                refused: refused.clone(),
            });
            None
        }
        Sent::Value(Some(owner)) => {
            let users = users::users(tx).await?;
            if owner_offered(&users, owner, None) {
                Some(owner)
            } else {
                refusals.owner = Some(AccountError::OwnerNotListed {
                    refused: refused.clone(),
                });
                None
            }
        }
        Sent::Malformed => {
            refusals.owner = Some(AccountError::OwnerNotListed {
                refused: refused.clone(),
            });
            None
        }
    };
    if let (Some(name), Some(place)) = (&name, place)
        && let Some(sibling) = tree.sibling_named(Some(place), &fold::fold(name), None)
    {
        refusals.name = Some(AccountError::NameTaken {
            refused: refused.clone(),
            holder: tree.full(place),
            name: sibling.name.clone(),
        });
    }
    let (Some(name), Some(place)) = (name, place) else {
        return Err(AccountError::Form(Box::new(refusals)));
    };
    if !refusals.is_empty() {
        return Err(AccountError::Form(Box::new(refusals)));
    }
    let row = NewRow {
        name: &name,
        parent: Some(place),
        kind: under,
        system_key: None,
        owner,
    };
    insert_row(tx, by, now, row).await
}

/// Why the create form of the section of `kind` does not offer `target` as the place a new
/// account goes inside (decision 132, the pause's P3 and P5), or `None` when its list offers
/// it: the one judgement of both the form's list ([`crate::chart::new_form`]) and its check
/// ([`create_from_form`]). The section itself is offered; a number no account has, a place in
/// another section, another section and a built-in account are `PlaceNotListed`; an archived
/// place `ParentArchived`; a place at the sixth level `TooDeep`.
pub(crate) fn create_place_refusal(
    tree: &Tree,
    kind: Kind,
    target: AccountId,
    refused: &Refused,
) -> Option<AccountError> {
    let not_listed = || {
        Some(AccountError::PlaceNotListed {
            refused: refused.clone(),
        })
    };
    let Some(place) = tree.get(target) else {
        return not_listed();
    };
    if place.kind != kind {
        return not_listed();
    }
    if place.is_root() {
        return None;
    }
    if place.is_system() {
        return not_listed();
    }
    let Some(depth) = tree.depth(target) else {
        return not_listed();
    };
    if place.is_archived() {
        return Some(AccountError::ParentArchived {
            refused: refused.clone(),
            parent: tree.full(target),
        });
    }
    if depth >= MAX_DEPTH {
        return Some(AccountError::TooDeep {
            refused: refused.clone(),
            depth: depth.saturating_add(1),
            inside: false,
        });
    }
    None
}

/// Why `moving` cannot go inside `target` (decision 134, the pause's P3 and P5), or `None` when
/// it can: the one judgement of the change form's list ([`crate::chart::edit_form`]), of its
/// check ([`change_account`]) and of [`move_account`]. A number no account has and a built-in
/// account, which holds nothing, are `PlaceNotListed`; a place in another section, or another
/// section, `KindMismatch`; the account itself or a place inside it `MoveUnderDescendant`; an
/// archived place `ParentArchived`; a place where the account, with what is inside it, would
/// pass six levels `TooDeep`. The account's own section is a place it can go.
pub(crate) fn move_place_refusal(
    tree: &Tree,
    moving: &Account,
    target: AccountId,
    refused: &Refused,
) -> Option<AccountError> {
    let not_listed = || {
        Some(AccountError::PlaceNotListed {
            refused: refused.clone(),
        })
    };
    let Some(place) = tree.get(target) else {
        return not_listed();
    };
    if place.is_system() && !place.is_root() {
        return not_listed();
    }
    let account = refused.place.clone().unwrap_or_default();
    if place.kind != moving.kind {
        return Some(AccountError::KindMismatch {
            word: refused.word,
            account,
            section: tree
                .root_of(place.kind)
                .map(|root| root.name.clone())
                .unwrap_or_else(|| place.kind.root_name().to_owned()),
        });
    }
    if tree.is_descendant(target, moving.id) {
        return Some(AccountError::MoveUnderDescendant {
            word: refused.word,
            account,
            target: tree.full(target),
        });
    }
    if place.is_archived() {
        return Some(AccountError::ParentArchived {
            refused: refused.clone(),
            parent: tree.full(target),
        });
    }
    let Some(depth) = tree.depth(target) else {
        return not_listed();
    };
    let height = tree.height(moving.id);
    let deepest = depth.saturating_add(height);
    if deepest > MAX_DEPTH {
        return Some(AccountError::TooDeep {
            refused: refused.clone(),
            depth: deepest,
            inside: height > 1,
        });
    }
    None
}

/// Whether a pick list offers `user` as an owner of an account whose saved owner is
/// `saved_owner` (decision 132, the pause's P3 and P5): an active user, or the account's own
/// owner while deactivated. The one judgement of the lists ([`crate::chart`]) and of the
/// forms' checks.
pub(crate) fn owner_offered(users: &[User], user: UserId, saved_owner: Option<UserId>) -> bool {
    users
        .iter()
        .find(|candidate| candidate.id == user)
        .is_some_and(|candidate| candidate.is_enabled() || Some(candidate.id) == saved_owner)
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use std::collections::BTreeMap;

use crate::fold;

/// The whole chart, loaded in one query and walked in Rust: depth, descendants, cycles, full
/// names and sibling names. The account pages' models ([`crate::chart`]) walk the same tree.
pub(crate) struct Tree {
    by_id: BTreeMap<AccountId, Account>,
}

impl Tree {
    /// Every account, by id.
    pub(crate) async fn load(conn: &mut sqlx::SqliteConnection) -> Result<Tree, AccountError> {
        let all = accounts(conn).await?;
        Ok(Tree::from_accounts(all))
    }

    /// The tree of `all`, the accounts as read.
    pub(crate) fn from_accounts(all: impl IntoIterator<Item = Account>) -> Tree {
        Tree {
            by_id: all
                .into_iter()
                .map(|account| (account.id, account))
                .collect(),
        }
    }

    /// The root of `kind`, found by its system key whatever its name.
    pub(crate) fn root_of(&self, kind: Kind) -> Option<&Account> {
        let key = kind.root_key();
        self.by_id
            .values()
            .find(|account| account.system_key == Some(key))
    }

    /// The children of `parent`, archived ones included, by the fold of their names (decision
    /// 95), the id settling a tie: the order of the list page at every level.
    pub(crate) fn ordered_children(&self, parent: AccountId) -> Vec<&Account> {
        let mut children: Vec<&Account> = self.children_of(parent).collect();
        children.sort_by_cached_key(|account| (fold::fold(&account.name), account.id));
        children
    }

    /// Everything below `root`, archived accounts included, as the list page writes it out: each
    /// account followed by what is inside it, in [`Tree::ordered_children`]'s order, with its
    /// level (1 directly under `root`). A corrupt chain deeper than [`MAX_DEPTH`] stops there.
    pub(crate) fn walk(&self, root: AccountId) -> Vec<(&Account, u8)> {
        let mut out = Vec::new();
        self.walk_into(root, 1, &mut out);
        out
    }

    /// The step of [`Tree::walk`] at `level`.
    fn walk_into<'a>(&'a self, parent: AccountId, level: u8, out: &mut Vec<(&'a Account, u8)>) {
        if usize::from(level) >= MAX_DEPTH.saturating_add(1) {
            return;
        }
        for child in self.ordered_children(parent) {
            out.push((child, level));
            self.walk_into(child.id, level.saturating_add(1), out);
        }
    }

    /// The full name of `id`, or an empty text for an unknown id or a broken chain, which the
    /// operations never produce and `check` reports.
    pub(crate) fn full(&self, id: AccountId) -> String {
        self.full_name(id).unwrap_or_default()
    }

    /// The first part of a refusal of `action` on `account` (decision 136): its word and its
    /// full place.
    pub(crate) fn refused(&self, account: &Account, action: Action) -> Refused {
        Refused {
            word: Word::of(account.kind),
            place: Some(self.full(account.id)),
            action,
        }
    }

    /// The ids from `id` up to its root, `id` first; `None` for an unknown id, a broken chain
    /// or a chain of more than [`MAX_DEPTH`] steps (a cycle), which the operations never
    /// produce and `check` reports.
    pub(crate) fn chain(&self, id: AccountId) -> Option<Vec<AccountId>> {
        let mut chain = Vec::new();
        let mut cursor = id;
        loop {
            let account = self.by_id.get(&cursor)?;
            chain.push(cursor);
            match account.parent_id {
                None => return Some(chain),
                Some(_) if chain.len() > MAX_DEPTH => return None,
                Some(parent) => cursor = parent,
            }
        }
    }

    /// The full name of `id`: the names from the root down, joined with a colon.
    fn full_name(&self, id: AccountId) -> Option<String> {
        let chain = self.chain(id)?;
        let names: Vec<&str> = chain
            .iter()
            .rev()
            .filter_map(|id| self.by_id.get(id).map(|account| account.name.as_str()))
            .collect();
        Some(names.join(":"))
    }

    /// The children of `parent`, archived ones included.
    fn children_of(&self, parent: AccountId) -> impl Iterator<Item = &Account> {
        self.by_id
            .values()
            .filter(move |account| account.parent_id == Some(parent))
    }

    /// The account at `full_name`, each segment matched by the fold from the roots down.
    fn resolve(&self, full_name: &str) -> Option<&Account> {
        let mut segments = full_name.split(':');
        let first = fold::fold(segments.next()?);
        if first.is_empty() {
            return None;
        }
        let mut current = self
            .by_id
            .values()
            .find(|account| account.is_root() && fold::fold(&account.name) == first)?;
        for segment in segments {
            let wanted = fold::fold(segment);
            if wanted.is_empty() {
                return None;
            }
            current = self
                .children_of(current.id)
                .find(|child| fold::fold(&child.name) == wanted)?;
        }
        Some(current)
    }
}

/// The columns of a new row that the two creators write.
struct NewRow<'a> {
    name: &'a str,
    parent: Option<AccountId>,
    kind: Kind,
    system_key: Option<SystemKey>,
    owner: Option<UserId>,
}

/// Inserts `row` by `by` at `now`, the updated pair equal to the created pair, the balance-only
/// flag off, and returns it as stored.
async fn insert_row(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    row: NewRow<'_>,
) -> Result<Account, AccountError> {
    let folded = fold::fold(row.name);
    let id = sqlx::query!(
        "INSERT INTO accounts (name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, NULL, ?7, ?8, ?7, ?8)",
        row.name,
        folded,
        row.parent,
        row.kind,
        row.system_key,
        row.owner,
        by,
        now
    )
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    reload(tx, AccountId(id)).await
}

/// Writes a system account (SPEC 6.2) for setup: `key`'s name and kind, under `parent` (`None`
/// for a root), by `by` at `now`. Only setup calls this; no later operation writes a key.
pub(crate) async fn insert_system_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    key: SystemKey,
    parent: Option<AccountId>,
) -> Result<Account, AccountError> {
    insert_row(
        tx,
        by,
        now,
        NewRow {
            name: key.name(),
            parent,
            kind: key.kind(),
            system_key: Some(key),
            owner: None,
        },
    )
    .await
}

/// Writes a starter-chart account (SPEC 6.2) for setup: `name` under `parent` with `kind`, no
/// key and no owner, by `by` at `now`. The names come from `setup::STARTER_CHART` and satisfy
/// the name, depth and sibling rules by construction; only setup calls this.
pub(crate) async fn insert_starter_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    name: &str,
    parent: AccountId,
    kind: Kind,
) -> Result<Account, AccountError> {
    insert_row(
        tx,
        by,
        now,
        NewRow {
            name,
            parent: Some(parent),
            kind,
            system_key: None,
            owner: None,
        },
    )
    .await
}

/// The account `id` as stored after a write, which must exist.
async fn reload(tx: &mut WriteTransaction, id: AccountId) -> Result<Account, AccountError> {
    account(tx, id).await?.ok_or(AccountError::NoSuchAccount)
}

use crate::book::{self, BookError};
use crate::money::{MinorDigits, Money};
use crate::users::User;
use crate::{balances, users};

/// What is saved in the boxes of the change form (decision 134): the name, the account it sits
/// inside and its owner (`None` for joint). A fresh change form shows these and sends them back
/// as what it showed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountBoxes {
    pub name: String,
    pub parent: Option<AccountId>,
    pub owner: Option<UserId>,
}

/// One box of the change form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxName {
    Name,
    Inside,
    Owner,
}

impl BoxName {
    /// How the clash's sentence names the box: `its name`, `its place`, `its owner`.
    pub const fn as_str(self) -> &'static str {
        match self {
            BoxName::Name => "its name",
            BoxName::Inside => "its place",
            BoxName::Owner => "its owner",
        }
    }
}

/// Another save changed a box that this one changes to another value, after the form was
/// opened (decision 134, the pause's P2): nothing of this save is written. The clash page shows
/// what is saved in the clashing boxes, sends what is saved now in every box as what it showed,
/// and holds what the person typed in the boxes they changed and what is saved now in every
/// other box, so that Save mine writes only the person's own boxes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    /// The account's full place as saved now.
    pub place: String,
    /// The boxes this save changes that another save changed meanwhile to another value, in
    /// the form's order.
    pub boxes: Vec<BoxName>,
    /// The boxes the person changed from what the form showed, clashing or not, in the form's
    /// order: the clash page keeps what was typed in these and what is saved in the others.
    pub changed: Vec<BoxName>,
    /// What is saved now in every box.
    pub saved: AccountBoxes,
}

impl fmt::Display for Clash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.boxes.iter().map(|name| name.as_str()).collect();
        let (list, verb) = match names.as_slice() {
            [one] => ((*one).to_owned(), "was"),
            [first @ .., last] => (format!("{} and {last}", first.join(", ")), "were"),
            [] => (String::from("it"), "was"),
        };
        write!(
            f,
            "Your change to {} was not saved because {list} {verb} changed while you were working, by another user or in another window.",
            self.place
        )
    }
}

/// What a change made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    /// The form sent what it showed: nothing was written.
    Nothing,
    /// The changed boxes were saved; the account as stored now.
    Saved(Account),
}

/// Why a change was not saved.
#[derive(Debug)]
pub enum ChangeError {
    /// Another save changed the same box to another value meanwhile (decision 134): 409.
    Clash(Box<Clash>),
    /// Every refusal of the form at once (decision 136, the pause's P3): 422.
    Refused(Box<FormRefusals>),
    /// Not a refusal: `NoSuchAccount` (the page for an unknown address), a missing built-in
    /// account or a database failure (decision 102).
    Failed(AccountError),
}

impl fmt::Display for ChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clash(clash) => write!(f, "{clash}"),
            Self::Refused(refusals) => write!(f, "{refusals}"),
            Self::Failed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Clash(_) | Self::Refused(_) => None,
            Self::Failed(error) => Some(error),
        }
    }
}

/// Changes `account` from the change form (decision 134, the pause's P2, P3 and P4), by `by`
/// at `now`, in `tx`, judging in this order and writing nothing before both are settled
/// (decision 99):
///
/// - **Changed** is judged per box against what the form `shown`: the name after it is
///   normalized as the core stores it (a trailing space is no change; a change of capitals
///   alone is one); a box `wanted` does not carry (`Sent::Absent`) is unchanged; a shown value
///   that is absent or malformed equals nothing saved, so that box counts as changed by
///   another save.
/// - **First the clash**: a box the person changed, that another save changed since the form
///   was shown, to a value other than this save's, is a [`Clash`] naming those boxes, and
///   nothing is written. Two saves that changed a box to the same value do not clash, and that
///   box needs no write.
/// - **Then every refusal at once**, as [`FormRefusals`] (P3), the account named by its saved
///   full place: the name rules and the name taken where the form ends (the new place with the
///   new name, so a rename and a move in one save pass when the new place holds no such name);
///   the place judged by the same function as the change form's pick list
///   ([`crate::chart::edit_form`]): a number no account has, a malformed one or a built-in
///   account `PlaceNotListed`, another section `KindMismatch`, an archived place
///   `ParentArchived`, the account itself or a place inside it `MoveUnderDescendant`, too deep
///   `TooDeep`; the owner: a number no user has, a malformed one or a deactivated user who is
///   not the saved owner `OwnerNotListed`. A section is `Section`. An archived account takes a
///   new name alone: a change of its place or its owner from what the form showed is
///   `ArchivedMeanwhile` at the top (P4), and nothing is written.
/// - **Then the write**: only the changed boxes, with the updated pair (INV-26); a form that
///   changes nothing, or only boxes already saved as sent, writes nothing (`Changed::Nothing`).
///
/// An unknown account is `ChangeError::Failed(NoSuchAccount)`.
pub async fn change_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    shown: &SentBoxes,
    wanted: &SentBoxes,
) -> Result<Changed, ChangeError> {
    let tree = Tree::load(tx).await.map_err(ChangeError::Failed)?;
    let current = tree
        .get(account)
        .ok_or(ChangeError::Failed(AccountError::NoSuchAccount))?;
    let place = tree.full(account);
    let saved = AccountBoxes {
        name: current.name.clone(),
        parent: current.parent_id,
        owner: current.owner_user_id,
    };
    let shown_name = match &shown.name {
        Sent::Value(name) => Some(fold::normalize(name)),
        Sent::Absent | Sent::Malformed => None,
    };
    let wanted_name = match &wanted.name {
        Sent::Absent => Wanted::Unsent,
        Sent::Value(name) => Wanted::Value(fold::normalize(name)),
        Sent::Malformed => Wanted::Malformed,
    };
    let name_box = judge(shown_name.as_ref(), &wanted_name, &saved.name);
    let shown_inside = match shown.inside {
        Sent::Value(id) => Some(Some(id)),
        Sent::Absent | Sent::Malformed => None,
    };
    let wanted_inside = match wanted.inside {
        Sent::Absent => Wanted::Unsent,
        Sent::Value(id) => Wanted::Value(Some(id)),
        Sent::Malformed => Wanted::Malformed,
    };
    let inside_box = judge(shown_inside.as_ref(), &wanted_inside, &saved.parent);
    let shown_owner = match shown.owner {
        Sent::Value(owner) => Some(owner),
        Sent::Absent | Sent::Malformed => None,
    };
    let wanted_owner = match wanted.owner {
        Sent::Absent => Wanted::Unsent,
        Sent::Value(owner) => Wanted::Value(owner),
        Sent::Malformed => Wanted::Malformed,
    };
    let owner_box = judge(shown_owner.as_ref(), &wanted_owner, &saved.owner);
    let boxes = [
        (BoxName::Name, name_box),
        (BoxName::Inside, inside_box),
        (BoxName::Owner, owner_box),
    ];
    let clashing: Vec<BoxName> = boxes
        .iter()
        .filter(|(_, judged)| judged.clash)
        .map(|(name, _)| *name)
        .collect();
    if !clashing.is_empty() {
        return Err(ChangeError::Clash(Box::new(Clash {
            place,
            boxes: clashing,
            changed: boxes
                .iter()
                .filter(|(_, judged)| judged.changed)
                .map(|(name, _)| *name)
                .collect(),
            saved,
        })));
    }
    let refused = |action| Refused {
        word: Word::of(current.kind),
        place: Some(place.clone()),
        action,
    };
    let mut refusals = FormRefusals::default();
    if current.is_root() && (name_box.write || inside_box.write || owner_box.write) {
        refusals.top = Some(AccountError::Section {
            refused: refused(Action::Changed),
        });
        return Err(ChangeError::Refused(Box::new(refusals)));
    }
    let archived = current.is_archived();
    if archived && (inside_box.write || owner_box.write) {
        refusals.top = Some(AccountError::ArchivedMeanwhile {
            account: place.clone(),
            kind: current.kind,
        });
    }
    let mut new_name = None;
    if name_box.write {
        let typed = match &wanted.name {
            Sent::Value(name) => name.as_str(),
            Sent::Absent | Sent::Malformed => "",
        };
        match fold::checked_name(typed) {
            Ok(name) => new_name = Some(name),
            Err(error) => {
                refusals.name = Some(AccountError::Name {
                    refused: refused(Action::Renamed),
                    error,
                });
            }
        }
    }
    let mut new_parent = None;
    if inside_box.write && !archived {
        let moved = refused(Action::Moved);
        let refusal = if current.is_system() {
            Some(AccountError::BuiltIn { refused: moved })
        } else {
            match wanted.inside {
                Sent::Value(target) => {
                    let refusal = move_place_refusal(&tree, current, target, &moved);
                    if refusal.is_none() {
                        new_parent = Some(target);
                    }
                    refusal
                }
                Sent::Absent | Sent::Malformed => {
                    Some(AccountError::PlaceNotListed { refused: moved })
                }
            }
        };
        refusals.inside = refusal;
    }
    let mut new_owner = None;
    if owner_box.write && !archived {
        let changed = refused(Action::Changed);
        if !current.kind.allows_owner() || current.is_system() {
            refusals.owner = Some(AccountError::OwnerNotAllowed { refused: changed });
        } else {
            match wanted.owner {
                Sent::Value(None) => new_owner = Some(None),
                Sent::Value(Some(user)) => {
                    let users = users::users(tx)
                        .await
                        .map_err(|error| ChangeError::Failed(AccountError::Database(error)))?;
                    if owner_offered(&users, user, saved.owner) {
                        new_owner = Some(Some(user));
                    } else {
                        refusals.owner = Some(AccountError::OwnerNotListed { refused: changed });
                    }
                }
                Sent::Absent | Sent::Malformed => {
                    refusals.owner = Some(AccountError::OwnerNotListed { refused: changed });
                }
            }
        }
    }
    let renamed_or_moved = new_name.is_some() || new_parent.is_some();
    if renamed_or_moved && refusals.name.is_none() && refusals.inside.is_none() {
        let final_name = new_name.as_deref().unwrap_or(&saved.name);
        let final_parent = new_parent.or(saved.parent);
        if let Some(sibling) =
            tree.sibling_named(final_parent, &fold::fold(final_name), Some(account))
        {
            refusals.name = Some(AccountError::NameTaken {
                refused: refused(if new_name.is_some() {
                    Action::Renamed
                } else {
                    Action::Moved
                }),
                holder: final_parent
                    .map(|parent| tree.full(parent))
                    .unwrap_or_default(),
                name: sibling.name.clone(),
            });
        }
    }
    if !refusals.is_empty() {
        return Err(ChangeError::Refused(Box::new(refusals)));
    }
    if new_name.is_none() && new_parent.is_none() && new_owner.is_none() {
        return Ok(Changed::Nothing);
    }
    let write = async {
        if new_name.is_some() || new_parent.is_some() {
            let name = new_name.as_deref().unwrap_or(&saved.name);
            let parent = new_parent.or(saved.parent);
            write_name_and_parent(tx, by, now, account, name, parent).await?;
        }
        if let Some(owner) = new_owner {
            write_owner(tx, by, now, account, owner).await?;
        }
        reload(tx, account).await
    };
    write.await.map(Changed::Saved).map_err(ChangeError::Failed)
}

/// What a request sends in one box of the change form, after it was parsed and normalized.
enum Wanted<T> {
    /// The request does not carry the box: unchanged.
    Unsent,
    Value(T),
    /// Not a well-formed number: changed, and refused under its box.
    Malformed,
}

/// How one box of a change was judged (decision 134, the pause's P2).
#[derive(Clone, Copy)]
struct Judged {
    /// The person changed the box from what the form showed.
    changed: bool,
    /// Another save changed it since the form was shown, to another value than this save's.
    clash: bool,
    /// The box needs a write: changed, to another value than what is saved.
    write: bool,
}

/// Judges one box: `shown` is what the form showed (`None` for a missing or malformed shown
/// value, which equals nothing saved), `wanted` what the request sends, `saved` what is saved.
fn judge<T: PartialEq>(shown: Option<&T>, wanted: &Wanted<T>, saved: &T) -> Judged {
    let other_changed = shown != Some(saved);
    match wanted {
        Wanted::Unsent => Judged {
            changed: false,
            clash: false,
            write: false,
        },
        Wanted::Value(value) => {
            let changed = shown != Some(value);
            let differs = value != saved;
            Judged {
                changed,
                clash: changed && other_changed && differs,
                write: changed && differs,
            }
        }
        Wanted::Malformed => Judged {
            changed: true,
            clash: other_changed,
            write: true,
        },
    }
}

/// Writes `name` (normalized, checked) as the name of `account` with its fold and the updated
/// pair of `by` at `now`.
async fn write_name(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    name: &str,
) -> Result<(), AccountError> {
    let folded = fold::fold(name);
    sqlx::query!(
        "UPDATE accounts SET name = ?1, name_folded = ?2, updated_by = ?3, updated_at = ?4 WHERE id = ?5",
        name,
        folded,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes `name` (normalized, checked) with its fold and `parent` as the name and the parent of
/// `account`, with the updated pair of `by` at `now`, in one statement, so that a rename and a
/// move judged where the form ends never meet the sibling index halfway.
async fn write_name_and_parent(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    name: &str,
    parent: Option<AccountId>,
) -> Result<(), AccountError> {
    let folded = fold::fold(name);
    sqlx::query!(
        "UPDATE accounts SET name = ?1, name_folded = ?2, parent_id = ?3, updated_by = ?4, updated_at = ?5 WHERE id = ?6",
        name,
        folded,
        parent,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes `parent` as the parent of `account` with the updated pair of `by` at `now`.
async fn write_parent(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    parent: AccountId,
) -> Result<(), AccountError> {
    sqlx::query!(
        "UPDATE accounts SET parent_id = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        parent,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes `owner` (`None` for joint) as the owner of `account` with the updated pair of `by`
/// at `now`.
async fn write_owner(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    owner: Option<UserId>,
) -> Result<(), AccountError> {
    sqlx::query!(
        "UPDATE accounts SET owner_user_id = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        owner,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The book's minor digits, with which a refusal writes an amount (SPEC 6.10); a missing or
/// unreadable book row is a failure, not a refusal.
async fn book_digits(conn: &mut sqlx::SqliteConnection) -> Result<MinorDigits, AccountError> {
    match book::book(conn).await {
        Ok(Some(book)) => Ok(book.minor_digits),
        Ok(None) => Err(AccountError::Database(sqlx::Error::RowNotFound)),
        Err(BookError::Database(error)) => Err(AccountError::Database(error)),
        Err(error) => Err(AccountError::Database(sqlx::Error::Protocol(
            error.to_string(),
        ))),
    }
}

impl Tree {
    /// The account `id`, if any.
    pub(crate) fn get(&self, id: AccountId) -> Option<&Account> {
        self.by_id.get(&id)
    }

    /// The depth of `id`: 1 for a root.
    pub(crate) fn depth(&self, id: AccountId) -> Option<usize> {
        self.chain(id).map(|chain| chain.len())
    }

    /// Whether `id` is `of` or lies below it.
    pub(crate) fn is_descendant(&self, id: AccountId, of: AccountId) -> bool {
        self.chain(id).is_some_and(|chain| chain.contains(&of))
    }

    /// The height of the subtree at `id`: 1 for a leaf, 1 more than the tallest child's.
    fn height(&self, id: AccountId) -> usize {
        self.children_of(id)
            .map(|child| self.height(child.id))
            .max()
            .map_or(1, |tallest| tallest.saturating_add(1))
    }

    /// The sibling under `parent` (`None` for the roots), other than `except`, whose name has
    /// the fold `folded`, archived or not, if any.
    fn sibling_named(
        &self,
        parent: Option<AccountId>,
        folded: &str,
        except: Option<AccountId>,
    ) -> Option<&Account> {
        self.by_id.values().find(|account| {
            account.parent_id == parent
                && Some(account.id) != except
                && fold::fold(&account.name) == folded
        })
    }
}

/// An owner is allowed on an account of `kind` only when the kind allows one (Assets and
/// Liabilities: `OwnerNotAllowed`) and the user exists (`OwnerNotListed`); a deactivated user
/// may own (decision 96: ownership is a label). `refused` is the refusal's first part.
async fn check_owner(
    tx: &mut WriteTransaction,
    kind: Kind,
    owner: UserId,
    refused: &Refused,
) -> Result<(), AccountError> {
    if !kind.allows_owner() {
        return Err(AccountError::OwnerNotAllowed {
            refused: refused.clone(),
        });
    }
    if users::user(tx, owner).await?.is_none() {
        return Err(AccountError::OwnerNotListed {
            refused: refused.clone(),
        });
    }
    Ok(())
}
