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

/// Creates an account under `new.parent` at `now` by `by` (SPEC 6.2, W2, decision 96): the
/// parent must exist (`NoSuchParent`) and not be archived (`ParentArchived`); the name is
/// normalized and checked (`Name`) and must be new among the parent's children by the fold,
/// archived children included (`NameTaken`); the depth stays within [`MAX_DEPTH`] (`TooDeep`);
/// an owner needs an Assets or Liabilities parent (`OwnerNotAllowed`) and must exist
/// (`NoSuchUser`). The kind is the parent's, the system key `None`, the balance-only flag off,
/// and the updated pair equals the created pair. Returns the row as stored.
pub async fn create_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    new: &NewAccount,
) -> Result<Account, AccountError> {
    let name = fold::checked_name(&new.name, Colon::Refused).map_err(AccountError::Name)?;
    let tree = Tree::load(tx).await?;
    let parent = tree.get(new.parent).ok_or(AccountError::NoSuchParent)?;
    if parent.is_archived() {
        return Err(AccountError::ParentArchived);
    }
    let parent_depth = tree.depth(parent.id).ok_or(AccountError::NoSuchParent)?;
    if parent_depth >= MAX_DEPTH {
        return Err(AccountError::TooDeep);
    }
    if let Some(owner) = new.owner {
        check_owner(tx, parent.kind, owner).await?;
    }
    if tree.sibling_taken(Some(parent.id), &fold::fold(&name), None) {
        return Err(AccountError::NameTaken);
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

/// Renames `account` (SPEC 6.2, W2): not a root (`RootRenamed`; the three other system accounts
/// may be renamed); the name checked as for creation and new among the siblings by the fold,
/// the account itself excepted, so a change of case alone is allowed. Writes the updated pair.
pub async fn rename_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    name: &str,
) -> Result<Account, AccountError> {
    let name = fold::checked_name(name, Colon::Refused).map_err(AccountError::Name)?;
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if current.is_root() {
        return Err(AccountError::RootRenamed);
    }
    let folded = fold::fold(&name);
    if tree.sibling_taken(current.parent_id, &folded, Some(account)) {
        return Err(AccountError::NameTaken);
    }
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
    reload(tx, account).await
}

/// Moves `account` under `new_parent` (SPEC 6.2, W2): not a root (`RootMoved`) or a system
/// account (`SystemAccountMoved`); the new parent must exist (`NoSuchParent`), not be archived
/// (`ParentArchived`), have the account's kind (`KindMismatch`) and be neither the account nor
/// one of its descendants (`MoveUnderDescendant`); the moved subtree's deepest leaf must stay
/// within [`MAX_DEPTH`] (`TooDeep`); the name must be new among the new siblings by the fold
/// (`NameTaken`). Writes the updated pair.
pub async fn move_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    new_parent: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let moving = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if moving.is_root() {
        return Err(AccountError::RootMoved);
    }
    if moving.is_system() {
        return Err(AccountError::SystemAccountMoved);
    }
    let parent = tree.get(new_parent).ok_or(AccountError::NoSuchParent)?;
    if tree.is_descendant(parent.id, account) {
        return Err(AccountError::MoveUnderDescendant);
    }
    if parent.kind != moving.kind {
        return Err(AccountError::KindMismatch);
    }
    if parent.is_archived() {
        return Err(AccountError::ParentArchived);
    }
    let parent_depth = tree.depth(parent.id).ok_or(AccountError::NoSuchParent)?;
    if parent_depth.saturating_add(tree.height(account)) > MAX_DEPTH {
        return Err(AccountError::TooDeep);
    }
    if tree.sibling_taken(Some(parent.id), &fold::fold(&moving.name), Some(account)) {
        return Err(AccountError::NameTaken);
    }
    sqlx::query!(
        "UPDATE accounts SET parent_id = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        new_parent,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    reload(tx, account).await
}

/// Sets or clears the owner of `account` (SPEC 5.1, 6.2): an Assets or Liabilities account that
/// is not a root or system account (`OwnerNotAllowed`); the owner must exist (`NoSuchUser`), a
/// deactivated user included, since ownership is a label. Writes the updated pair.
pub async fn set_owner(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
    owner: Option<UserId>,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if !current.kind.allows_owner() || current.is_root() || current.is_system() {
        return Err(AccountError::OwnerNotAllowed);
    }
    if let Some(owner) = owner {
        check_owner(tx, current.kind, owner).await?;
    }
    sqlx::query!(
        "UPDATE accounts SET owner_user_id = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
        owner,
        by,
        now,
        account
    )
    .execute(&mut **tx)
    .await?;
    reload(tx, account).await
}

/// Archives `account` at `now` (SPEC 6.2, INV-12's Core half): not a system account
/// (`SystemAccountArchived`, roots included); not already archived (`AlreadyArchived`); every
/// child archived (`ChildrenNotArchived`); the account's own live postings summing to zero over
/// all dates (`BalanceNotZero`). Writes `archived_at = now` and the updated pair.
pub async fn archive_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if current.is_system() {
        return Err(AccountError::SystemAccountArchived);
    }
    if current.is_archived() {
        return Err(AccountError::AlreadyArchived);
    }
    if tree.children_of(account).any(|child| !child.is_archived()) {
        return Err(AccountError::ChildrenNotArchived);
    }
    let balance = balances::own_balance(tx, account).await?;
    if balance != Money::ZERO {
        return Err(AccountError::BalanceNotZero);
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

/// Unarchives `account` (SPEC 6.2 as decision 96 reads it): archived (`NotArchived`) and its
/// parent not archived (`ParentArchivedUnarchiveFirst`), so that every child of an archived
/// account stays archived (INV-12). Clears `archived_at` and writes the updated pair.
pub async fn unarchive_account(
    tx: &mut WriteTransaction,
    by: UserId,
    now: Timestamp,
    account: AccountId,
) -> Result<Account, AccountError> {
    let tree = Tree::load(tx).await?;
    let current = tree.get(account).ok_or(AccountError::NoSuchAccount)?;
    if !current.is_archived() {
        return Err(AccountError::NotArchived);
    }
    let parent_archived = current
        .parent_id
        .and_then(|parent| tree.get(parent))
        .is_some_and(Account::is_archived);
    if parent_archived {
        return Err(AccountError::ParentArchivedUnarchiveFirst);
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

/// Why an account operation was refused (SPEC 6.2, W2, decision 96); each message can be shown
/// beside a form (W2: every refusal explains itself).
#[derive(Debug)]
pub enum AccountError {
    /// The name is refused by the name rules.
    Name(NameError),
    /// A sibling, archived or not, has this name ignoring case.
    NameTaken,
    /// No account has this id.
    NoSuchAccount,
    /// No account has the parent's id.
    NoSuchParent,
    /// No user has the owner's id.
    NoSuchUser,
    /// The parent is archived: nothing live is created or moved under it.
    ParentArchived,
    /// Roots are never renamed.
    RootRenamed,
    /// Roots are never moved.
    RootMoved,
    /// System accounts are never re-parented.
    SystemAccountMoved,
    /// System accounts are never archived.
    SystemAccountArchived,
    /// The new parent is the account or one of its descendants.
    MoveUnderDescendant,
    /// The result would be deeper than [`MAX_DEPTH`] segments.
    TooDeep,
    /// The new parent has another kind.
    KindMismatch,
    /// Only Assets and Liabilities accounts that are not roots or system accounts have owners.
    OwnerNotAllowed,
    /// A child is not archived.
    ChildrenNotArchived,
    /// The account's live postings do not sum to zero.
    BalanceNotZero,
    /// The account is archived already.
    AlreadyArchived,
    /// The account is not archived.
    NotArchived,
    /// The parent is archived: unarchive it first.
    ParentArchivedUnarchiveFirst,
    /// A system account is missing (INV-11 broken).
    MissingSystemAccount(SystemKey),
    /// Any database failure.
    Database(sqlx::Error),
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "the name {error}"),
            Self::NameTaken => f.write_str("an account with this name already exists here"),
            Self::NoSuchAccount => f.write_str("no such account"),
            Self::NoSuchParent => f.write_str("the parent account does not exist"),
            Self::NoSuchUser => f.write_str("no such user"),
            Self::ParentArchived => f.write_str("the parent account is archived"),
            Self::RootRenamed => f.write_str("a root account cannot be renamed"),
            Self::RootMoved => f.write_str("a root account cannot be moved"),
            Self::SystemAccountMoved => f.write_str("a system account cannot be moved"),
            Self::SystemAccountArchived => f.write_str("a system account cannot be archived"),
            Self::MoveUnderDescendant => {
                f.write_str("an account cannot be moved under itself or one of its descendants")
            }
            Self::TooDeep => f.write_str("the chart allows at most six levels, the root included"),
            Self::KindMismatch => f.write_str("an account can only be moved within its kind"),
            Self::OwnerNotAllowed => {
                f.write_str("only accounts under Assets or Liabilities have owners")
            }
            Self::ChildrenNotArchived => f.write_str("archive the child accounts first"),
            Self::BalanceNotZero => f.write_str("the account's balance is not zero"),
            Self::AlreadyArchived => f.write_str("the account is already archived"),
            Self::NotArchived => f.write_str("the account is not archived"),
            Self::ParentArchivedUnarchiveFirst => f.write_str("unarchive the parent account first"),
            Self::MissingSystemAccount(key) => write!(f, "the system account {key} is missing"),
            Self::Database(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for AccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AccountError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

use std::collections::BTreeMap;

use crate::fold;

/// The whole chart, loaded in one query and walked in Rust: depth, descendants, cycles, full
/// names and sibling names.
struct Tree {
    by_id: BTreeMap<AccountId, Account>,
}

impl Tree {
    /// Every account, by id.
    async fn load(conn: &mut sqlx::SqliteConnection) -> Result<Tree, AccountError> {
        let all = accounts(conn).await?;
        Ok(Tree {
            by_id: all
                .into_iter()
                .map(|account| (account.id, account))
                .collect(),
        })
    }

    /// The ids from `id` up to its root, `id` first; `None` for an unknown id, a broken chain
    /// or a chain of more than [`MAX_DEPTH`] steps (a cycle), which the operations never
    /// produce and `check` reports.
    fn chain(&self, id: AccountId) -> Option<Vec<AccountId>> {
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

use crate::fold::Colon;
use crate::money::Money;
use crate::{balances, users};

impl Tree {
    /// The account `id`, if any.
    fn get(&self, id: AccountId) -> Option<&Account> {
        self.by_id.get(&id)
    }

    /// The depth of `id`: 1 for a root.
    fn depth(&self, id: AccountId) -> Option<usize> {
        self.chain(id).map(|chain| chain.len())
    }

    /// Whether `id` is `of` or lies below it.
    fn is_descendant(&self, id: AccountId, of: AccountId) -> bool {
        self.chain(id).is_some_and(|chain| chain.contains(&of))
    }

    /// The height of the subtree at `id`: 1 for a leaf, 1 more than the tallest child's.
    fn height(&self, id: AccountId) -> usize {
        self.children_of(id)
            .map(|child| self.height(child.id))
            .max()
            .map_or(1, |tallest| tallest.saturating_add(1))
    }

    /// Whether a sibling under `parent` (`None` for the roots), other than `except`, has a
    /// name whose fold is `folded`.
    fn sibling_taken(
        &self,
        parent: Option<AccountId>,
        folded: &str,
        except: Option<AccountId>,
    ) -> bool {
        self.by_id.values().any(|account| {
            account.parent_id == parent
                && Some(account.id) != except
                && fold::fold(&account.name) == folded
        })
    }
}

/// An owner is allowed on an account of `kind` only when the kind allows one (Assets and
/// Liabilities) and the user exists; a deactivated user may own (ownership is a label).
async fn check_owner(
    tx: &mut WriteTransaction,
    kind: Kind,
    owner: UserId,
) -> Result<(), AccountError> {
    if !kind.allows_owner() {
        return Err(AccountError::OwnerNotAllowed);
    }
    if users::user(tx, owner).await?.is_none() {
        return Err(AccountError::NoSuchUser);
    }
    Ok(())
}
