//! The schema of migration 0002 (decision 92; the Schema halves of INV-2, INV-6, INV-9, INV-10
//! and INV-26): what the `users`, `sessions`, `accounts`, `transactions` and `postings` tables
//! accept and refuse, the kind triggers, the unique indexes, the restrict rule of every foreign
//! key, the ids that are never reused and the shape of the whole schema. Every row is written
//! with unchecked SQL assembled from test constants through `Storage::begin_write`, as decision
//! 57 allows for schema tests, inside transactions that are rolled back, so no test leaves a row
//! behind.

mod common;

use nage_core::date::{Date, DateError};
use nage_core::storage::{Storage, WriteTransaction};
use nage_core::test_support::open_migrated;
use sqlx::{AssertSqlSafe, Row as _};

use common::{primary_code, sqlite_code};

/// `SQLITE_CONSTRAINT`, the primary result code of every constraint failure.
const CONSTRAINT: i32 = 19;
/// `SQLITE_CONSTRAINT_CHECK`.
const CHECK: i32 = 275;
/// `SQLITE_CONSTRAINT_FOREIGNKEY`: an insert or update naming a row that does not exist.
const FOREIGN_KEY: i32 = 787;
/// `SQLITE_CONSTRAINT_NOTNULL`.
const NOT_NULL: i32 = 1299;
/// `SQLITE_CONSTRAINT_TRIGGER`: a `RAISE(ABORT)` in a trigger, which is also how SQLite
/// implements a refused `ON DELETE RESTRICT`.
const TRIGGER: i32 = 1811;
/// `SQLITE_CONSTRAINT_UNIQUE`.
const UNIQUE: i32 = 2067;
/// `SQLITE_CONSTRAINT_DATATYPE`: a STRICT table refusing a value its column cannot hold.
const DATATYPE: i32 = 3091;

/// A valid RFC 3339 UTC whole-second timestamp, as an SQL literal.
const NOW: &str = "'2026-11-08T12:00:00Z'";
/// A timestamp whose shape the GLOB of decision 56 accepts and whose round trip through
/// `strftime` refuses: there is no 30th of February.
const BAD_TIME: &str = "'2026-02-30T00:00:00Z'";
/// A timestamp whose shape the GLOB of decision 56 accepts and whose round trip through
/// `strftime` is NULL, not another instant: there is no 13th month. Only the CHECK's `IS`
/// refuses it.
const NO_TIME: &str = "'2026-13-01T00:00:00Z'";
/// A valid timestamp later than `NOW`, for the stamps an edit, a deactivation, an archive, a
/// deletion or a session's use writes after the row's creation.
const LATER: &str = "'2026-11-09T08:30:00Z'";
/// The message of the two triggers that compare a child's kind with its parent's.
const KIND_MESSAGE: &str = "accounts.kind must equal the parent's kind";
/// The message of the trigger that refuses a kind change.
const FROZEN_MESSAGE: &str = "accounts.kind never changes";
/// The message SQLite gives a refused foreign key, on a delete and on an insert alike.
const FOREIGN_KEY_MESSAGE: &str = "FOREIGN KEY constraint failed";
/// A 64-character session token hash, as an SQL literal: the seeded session's.
const HASH_A: &str = "'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'";
/// Another 64-character hash.
const HASH_B: &str = "'fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210'";

/// The SQL literals of one `users` row, one per column.
#[derive(Clone, Copy)]
struct UserRow {
    id: &'static str,
    username: &'static str,
    username_folded: &'static str,
    display_name: &'static str,
    password_hash: &'static str,
    is_admin: &'static str,
    created_by: &'static str,
    created_at: &'static str,
    updated_by: &'static str,
    updated_at: &'static str,
    disabled_by: &'static str,
    disabled_at: &'static str,
}

impl UserRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO users (id, username, username_folded, display_name, password_hash, is_admin, created_by, created_at, updated_by, updated_at, disabled_by, disabled_at) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            self.id,
            self.username,
            self.username_folded,
            self.display_name,
            self.password_hash,
            self.is_admin,
            self.created_by,
            self.created_at,
            self.updated_by,
            self.updated_at,
            self.disabled_by,
            self.disabled_at,
        )
    }
}

/// The first admin: no creator, never edited.
const ADMIN: UserRow = UserRow {
    id: "1",
    username: "'admin'",
    username_folded: "'admin'",
    display_name: "'Admin'",
    password_hash: "'hash'",
    is_admin: "1",
    created_by: "NULL",
    created_at: NOW,
    updated_by: "NULL",
    updated_at: "NULL",
    disabled_by: "NULL",
    disabled_at: "NULL",
};

/// A member the admin created.
const MEMBER: UserRow = UserRow {
    id: "2",
    username: "'member'",
    username_folded: "'member'",
    display_name: "'Member'",
    is_admin: "0",
    created_by: "1",
    ..ADMIN
};

/// A newcomer, the base of the users table's refused rows, never inserted as it stands.
const NEWCOMER: UserRow = UserRow {
    id: "5",
    username: "'newcomer'",
    username_folded: "'newcomer'",
    display_name: "'Newcomer'",
    ..MEMBER
};

/// The SQL literals of one `sessions` row, one per column.
#[derive(Clone, Copy)]
struct SessionRow {
    id: &'static str,
    token_hash: &'static str,
    user_id: &'static str,
    created_at: &'static str,
    last_seen_at: &'static str,
    expires_at: &'static str,
}

impl SessionRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_seen_at, expires_at) VALUES ({}, {}, {}, {}, {}, {})",
            self.id,
            self.token_hash,
            self.user_id,
            self.created_at,
            self.last_seen_at,
            self.expires_at,
        )
    }
}

/// The admin's seeded session.
const SESSION: SessionRow = SessionRow {
    id: "1",
    token_hash: HASH_A,
    user_id: "1",
    created_at: NOW,
    last_seen_at: NOW,
    expires_at: "'2026-12-08T12:00:00Z'",
};

/// A second session, the base of the sessions table's refused rows.
const SESSION_B: SessionRow = SessionRow {
    id: "2",
    token_hash: HASH_B,
    user_id: "2",
    ..SESSION
};

/// The SQL literals of one `accounts` row, one per column.
#[derive(Clone, Copy)]
struct AccountRow {
    id: &'static str,
    name: &'static str,
    name_folded: &'static str,
    parent_id: &'static str,
    kind: &'static str,
    system_key: &'static str,
    owner_user_id: &'static str,
    is_balance_only: &'static str,
    archived_at: &'static str,
    created_by: &'static str,
    created_at: &'static str,
    updated_by: &'static str,
    updated_at: &'static str,
}

impl AccountRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO accounts (id, name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            self.id,
            self.name,
            self.name_folded,
            self.parent_id,
            self.kind,
            self.system_key,
            self.owner_user_id,
            self.is_balance_only,
            self.archived_at,
            self.created_by,
            self.created_at,
            self.updated_by,
            self.updated_at,
        )
    }
}

/// The Assets root.
const ASSETS: AccountRow = AccountRow {
    id: "1",
    name: "'Assets'",
    name_folded: "'assets'",
    parent_id: "NULL",
    kind: "'asset'",
    system_key: "'assets'",
    owner_user_id: "NULL",
    is_balance_only: "0",
    archived_at: "NULL",
    created_by: "1",
    created_at: NOW,
    updated_by: "1",
    updated_at: NOW,
};

/// `Checking`, an asset under the Assets root.
const CHECKING: AccountRow = AccountRow {
    id: "6",
    name: "'Checking'",
    name_folded: "'checking'",
    parent_id: "1",
    system_key: "NULL",
    ..ASSETS
};

/// `Savings`, a second asset under the Assets root, inserted by the accounts, kind-trigger and
/// id tests.
const SAVINGS: AccountRow = AccountRow {
    id: "8",
    name: "'Savings'",
    name_folded: "'savings'",
    ..CHECKING
};

/// `Probe`, an asset under the Assets root that no test inserts as it stands: the base of the
/// accounts table's refused rows, so that only the constraint a case targets can fire.
const PROBE: AccountRow = AccountRow {
    id: "21",
    name: "'Probe'",
    name_folded: "'probe'",
    ..CHECKING
};

/// The SQL literals of one `transactions` row, one per column.
#[derive(Clone, Copy)]
struct TransactionRow {
    id: &'static str,
    date: &'static str,
    payee: &'static str,
    memo: &'static str,
    origin: &'static str,
    version: &'static str,
    created_by: &'static str,
    created_at: &'static str,
    updated_by: &'static str,
    updated_at: &'static str,
    deleted_by: &'static str,
    deleted_at: &'static str,
}

impl TransactionRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO transactions (id, date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            self.id,
            self.date,
            self.payee,
            self.memo,
            self.origin,
            self.version,
            self.created_by,
            self.created_at,
            self.updated_by,
            self.updated_at,
            self.deleted_by,
            self.deleted_at,
        )
    }
}

/// The seeded transaction, `Market`, which the two seeded postings balance.
const MARKET: TransactionRow = TransactionRow {
    id: "1",
    date: "'2026-11-08'",
    payee: "'Market'",
    memo: "''",
    origin: "'manual'",
    version: "1",
    created_by: "1",
    created_at: NOW,
    updated_by: "1",
    updated_at: NOW,
    deleted_by: "NULL",
    deleted_at: "NULL",
};

/// A second transaction, the base of the transactions table's refused rows.
const LUNCH: TransactionRow = TransactionRow {
    id: "2",
    date: "'2026-11-09'",
    payee: "'Lunch'",
    ..MARKET
};

/// The SQL literals of one `postings` row, one per column.
#[derive(Clone, Copy)]
struct PostingRow {
    id: &'static str,
    transaction_id: &'static str,
    account_id: &'static str,
    amount: &'static str,
}

impl PostingRow {
    /// The INSERT statement for this row.
    fn insert(self) -> String {
        format!(
            "INSERT INTO postings (id, transaction_id, account_id, amount) VALUES ({}, {}, {}, {})",
            self.id, self.transaction_id, self.account_id, self.amount
        )
    }
}

/// The seeded posting that leaves `Checking`.
const PAID: PostingRow = PostingRow {
    id: "1",
    transaction_id: "1",
    account_id: "6",
    amount: "-4500",
};

/// The seeded posting that reaches `Groceries`.
const BOUGHT: PostingRow = PostingRow {
    id: "2",
    account_id: "7",
    amount: "4500",
    ..PAID
};

/// A further posting on `Checking`, the base of the postings table's refused rows.
const SPENT: PostingRow = PostingRow {
    id: "3",
    amount: "-1000",
    ..PAID
};

/// Begins a write transaction, naming `label` on failure.
async fn begin(storage: &Storage, label: &str) -> WriteTransaction {
    storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction for {label}: {error}"))
}

/// Executes one unchecked statement inside `tx`.
async fn execute(tx: &mut WriteTransaction, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe(sql.to_owned()))
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Executes `sql`, which the schema must accept; `label` names the case.
async fn accept(tx: &mut WriteTransaction, label: &str, sql: &str) {
    execute(tx, sql)
        .await
        .unwrap_or_else(|error| panic!("the schema refused {label}: {error}"));
}

/// Executes `sql`, which the schema must refuse with the extended result code `expected` (its
/// primary code `SQLITE_CONSTRAINT`), and returns SQLite's message; `label` names the case. A
/// refused statement is rolled back by itself and the transaction stays open.
async fn refuse(tx: &mut WriteTransaction, label: &str, sql: &str, expected: i32) -> String {
    let error = match execute(tx, sql).await {
        Ok(()) => panic!("the schema accepted {label}"),
        Err(error) => error,
    };
    let code = sqlite_code(&error)
        .unwrap_or_else(|| panic!("the schema refused {label} without a SQLite code: {error}"));
    assert_eq!(
        primary_code(code),
        CONSTRAINT,
        "primary result code for {label} (extended {code}): {error}"
    );
    assert_eq!(code, expected, "extended result code for {label}: {error}");
    error
        .as_database_error()
        .map(|database| database.message().to_owned())
        .unwrap_or_default()
}

/// Asserts that SQLite's `message` for `label` holds `needle`.
fn assert_message(label: &str, message: &str, needle: &str) {
    assert!(
        message.contains(needle),
        "the message for {label} does not hold {needle:?}: {message}"
    );
}

/// Reads the first column of every row `sql` returns, as text.
async fn texts(tx: &mut WriteTransaction, sql: &str) -> Vec<String> {
    sqlx::query(AssertSqlSafe(sql.to_owned()))
        .fetch_all(&mut **tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read `{sql}`: {error}"))
        .iter()
        .map(|row| {
            row.try_get_unchecked::<String, _>(0)
                .unwrap_or_else(|error| panic!("`{sql}` did not answer text: {error}"))
        })
        .collect()
}

/// Reads the one integer `sql` returns.
async fn integer(tx: &mut WriteTransaction, sql: &str) -> i64 {
    sqlx::query(AssertSqlSafe(sql.to_owned()))
        .fetch_one(&mut **tx)
        .await
        .unwrap_or_else(|error| panic!("cannot read `{sql}`: {error}"))
        .try_get_unchecked::<i64, _>(0)
        .unwrap_or_else(|error| panic!("`{sql}` did not answer an integer: {error}"))
}

/// Begins a write transaction and inserts the seed rows: the admin and a member, the five
/// roots with their system keys, `Checking` (6) under Assets and `Groceries` (7) under
/// Expenses, the transaction `Market` (1) with its two postings, and the admin's session.
async fn seeded(storage: &Storage, label: &str) -> WriteTransaction {
    let mut tx = begin(storage, label).await;
    accept(&mut tx, "the seed admin", &ADMIN.insert()).await;
    accept(&mut tx, "the seed member", &MEMBER.insert()).await;
    let roots = [
        ASSETS,
        AccountRow {
            id: "2",
            name: "'Liabilities'",
            name_folded: "'liabilities'",
            kind: "'liability'",
            system_key: "'liabilities'",
            ..ASSETS
        },
        AccountRow {
            id: "3",
            name: "'Equity'",
            name_folded: "'equity'",
            kind: "'equity'",
            system_key: "'equity'",
            ..ASSETS
        },
        AccountRow {
            id: "4",
            name: "'Income'",
            name_folded: "'income'",
            kind: "'income'",
            system_key: "'income'",
            ..ASSETS
        },
        AccountRow {
            id: "5",
            name: "'Expenses'",
            name_folded: "'expenses'",
            kind: "'expense'",
            system_key: "'expenses'",
            ..ASSETS
        },
        CHECKING,
        AccountRow {
            id: "7",
            name: "'Groceries'",
            name_folded: "'groceries'",
            parent_id: "5",
            kind: "'expense'",
            ..CHECKING
        },
    ];
    for row in roots {
        accept(&mut tx, "a seed account", &row.insert()).await;
    }
    accept(&mut tx, "the seed transaction", &MARKET.insert()).await;
    accept(
        &mut tx,
        "the seed posting that leaves Checking",
        &PAID.insert(),
    )
    .await;
    accept(
        &mut tx,
        "the seed posting that reaches Groceries",
        &BOUGHT.insert(),
    )
    .await;
    accept(&mut tx, "the seed session", &SESSION.insert()).await;
    tx
}

/// Rolls `tx` back, naming `label` on failure.
async fn roll_back(tx: WriteTransaction, label: &str) {
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back after {label}: {error}"));
}

/// Asserts that `table`'s columns, as `name type notnull pk` in declaration order, are
/// `expected`, and that none has a default.
async fn assert_columns(tx: &mut WriteTransaction, table: &str, expected: &[&str]) {
    let found = texts(
        tx,
        &format!(
            "SELECT name || ' ' || type || ' ' || \"notnull\" || ' ' || pk FROM pragma_table_info('{table}') ORDER BY cid"
        ),
    )
    .await;
    assert_eq!(
        found, expected,
        "the columns (name type notnull pk) of {table}"
    );
    let defaults = integer(
        tx,
        &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE dflt_value IS NOT NULL"),
    )
    .await;
    assert_eq!(defaults, 0, "the columns of {table} with a default");
}

/// Asserts that `table`'s foreign keys, as `from -> table.to on_delete` in the order of their
/// column names, are `expected`.
async fn assert_foreign_keys(tx: &mut WriteTransaction, table: &str, expected: &[&str]) {
    let found = texts(
        tx,
        &format!(
            "SELECT \"from\" || ' -> ' || \"table\" || '.' || \"to\" || ' ' || on_delete FROM pragma_foreign_key_list('{table}') ORDER BY \"from\""
        ),
    )
    .await;
    assert_eq!(
        found, expected,
        "the foreign keys (from -> table.to on_delete) of {table}"
    );
}

/// Asserts that `table`'s indexes, as `name unique origin partial` in name order, are
/// `expected`.
async fn assert_indexes(tx: &mut WriteTransaction, table: &str, expected: &[&str]) {
    let found = texts(
        tx,
        &format!(
            "SELECT name || ' ' || \"unique\" || ' ' || origin || ' ' || partial FROM pragma_index_list('{table}') ORDER BY name"
        ),
    )
    .await;
    assert_eq!(
        found, expected,
        "the indexes (name unique origin partial) of {table}"
    );
}

/// Asserts that the index `index` covers exactly the columns `expected`, in order.
async fn assert_index_columns(tx: &mut WriteTransaction, index: &str, expected: &[&str]) {
    let found = texts(
        tx,
        &format!("SELECT name FROM pragma_index_info('{index}') ORDER BY seqno"),
    )
    .await;
    assert_eq!(found, expected, "the columns of the index {index}");
}

/// Migration 0002 leaves exactly the recorded schema (decision 92): `sqlite_schema` holds the
/// six tables, the eight named indexes and the three triggers and nothing else beside SQLite's
/// own `sqlite_*` entries and sqlx's bookkeeping table; `sqlite_sequence` exists, the mark of
/// the two AUTOINCREMENT tables; each of the five new tables has exactly the recorded columns
/// with their type, NOT NULL and primary key and no default; each foreign key names its target
/// column with `ON DELETE RESTRICT`; each index has its recorded columns, uniqueness and
/// partiality, and the three partial indexes keep their WHERE clauses (pinned as text, since
/// no pragma shows them).
///
/// Proves: decision 92, INV-26 (schema half).
#[tokio::test]
async fn migration_0002_has_the_recorded_shape() {
    let (_dir, storage) = open_migrated().await;
    let label = "the shape of the schema";
    let mut tx = begin(&storage, label).await;

    let objects = texts(
        &mut tx,
        "SELECT type || ' ' || name || ' ' || tbl_name FROM sqlite_schema \
         WHERE name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY type, name",
    )
    .await;
    assert_eq!(
        objects,
        [
            "index accounts_parent_name_folded accounts",
            "index accounts_root_name_folded accounts",
            "index accounts_system_key accounts",
            "index postings_account_transaction postings",
            "index sessions_token_hash sessions",
            "index transactions_date_id transactions",
            "index transactions_live_date_id transactions",
            "index users_username_folded users",
            "table accounts accounts",
            "table book book",
            "table postings postings",
            "table sessions sessions",
            "table transactions transactions",
            "table users users",
            "trigger accounts_kind_matches_parent_on_insert accounts",
            "trigger accounts_kind_matches_parent_on_move accounts",
            "trigger accounts_kind_never_changes accounts",
        ],
        "the schema objects (type, name, table) of the migrated database"
    );
    let sequences = integer(
        &mut tx,
        "SELECT count(*) FROM sqlite_schema WHERE name = 'sqlite_sequence'",
    )
    .await;
    assert_eq!(sequences, 1, "sqlite_sequence, which AUTOINCREMENT brings");

    assert_columns(
        &mut tx,
        "users",
        &[
            "id INTEGER 0 1",
            "username TEXT 1 0",
            "username_folded TEXT 1 0",
            "display_name TEXT 1 0",
            "password_hash TEXT 1 0",
            "is_admin INTEGER 1 0",
            "created_by INTEGER 0 0",
            "created_at TEXT 1 0",
            "updated_by INTEGER 0 0",
            "updated_at TEXT 0 0",
            "disabled_by INTEGER 0 0",
            "disabled_at TEXT 0 0",
        ],
    )
    .await;
    assert_columns(
        &mut tx,
        "sessions",
        &[
            "id INTEGER 0 1",
            "token_hash TEXT 1 0",
            "user_id INTEGER 1 0",
            "created_at TEXT 1 0",
            "last_seen_at TEXT 1 0",
            "expires_at TEXT 1 0",
        ],
    )
    .await;
    assert_columns(
        &mut tx,
        "accounts",
        &[
            "id INTEGER 0 1",
            "name TEXT 1 0",
            "name_folded TEXT 1 0",
            "parent_id INTEGER 0 0",
            "kind TEXT 1 0",
            "system_key TEXT 0 0",
            "owner_user_id INTEGER 0 0",
            "is_balance_only INTEGER 1 0",
            "archived_at TEXT 0 0",
            "created_by INTEGER 1 0",
            "created_at TEXT 1 0",
            "updated_by INTEGER 1 0",
            "updated_at TEXT 1 0",
        ],
    )
    .await;
    assert_columns(
        &mut tx,
        "transactions",
        &[
            "id INTEGER 0 1",
            "date TEXT 1 0",
            "payee TEXT 1 0",
            "memo TEXT 1 0",
            "origin TEXT 1 0",
            "version INTEGER 1 0",
            "created_by INTEGER 1 0",
            "created_at TEXT 1 0",
            "updated_by INTEGER 1 0",
            "updated_at TEXT 1 0",
            "deleted_by INTEGER 0 0",
            "deleted_at TEXT 0 0",
        ],
    )
    .await;
    assert_columns(
        &mut tx,
        "postings",
        &[
            "id INTEGER 0 1",
            "transaction_id INTEGER 1 0",
            "account_id INTEGER 1 0",
            "amount INTEGER 1 0",
        ],
    )
    .await;

    assert_foreign_keys(
        &mut tx,
        "users",
        &[
            "created_by -> users.id RESTRICT",
            "disabled_by -> users.id RESTRICT",
            "updated_by -> users.id RESTRICT",
        ],
    )
    .await;
    assert_foreign_keys(&mut tx, "sessions", &["user_id -> users.id RESTRICT"]).await;
    assert_foreign_keys(
        &mut tx,
        "accounts",
        &[
            "created_by -> users.id RESTRICT",
            "owner_user_id -> users.id RESTRICT",
            "parent_id -> accounts.id RESTRICT",
            "updated_by -> users.id RESTRICT",
        ],
    )
    .await;
    assert_foreign_keys(
        &mut tx,
        "transactions",
        &[
            "created_by -> users.id RESTRICT",
            "deleted_by -> users.id RESTRICT",
            "updated_by -> users.id RESTRICT",
        ],
    )
    .await;
    assert_foreign_keys(
        &mut tx,
        "postings",
        &[
            "account_id -> accounts.id RESTRICT",
            "transaction_id -> transactions.id RESTRICT",
        ],
    )
    .await;

    assert_indexes(&mut tx, "users", &["users_username_folded 1 c 0"]).await;
    assert_indexes(&mut tx, "sessions", &["sessions_token_hash 1 c 0"]).await;
    assert_indexes(
        &mut tx,
        "accounts",
        &[
            "accounts_parent_name_folded 1 c 0",
            "accounts_root_name_folded 1 c 1",
            "accounts_system_key 1 c 1",
        ],
    )
    .await;
    assert_indexes(
        &mut tx,
        "transactions",
        &[
            "transactions_date_id 0 c 0",
            "transactions_live_date_id 0 c 1",
        ],
    )
    .await;
    assert_indexes(&mut tx, "postings", &["postings_account_transaction 0 c 0"]).await;
    assert_index_columns(&mut tx, "users_username_folded", &["username_folded"]).await;
    assert_index_columns(&mut tx, "sessions_token_hash", &["token_hash"]).await;
    assert_index_columns(
        &mut tx,
        "accounts_parent_name_folded",
        &["parent_id", "name_folded"],
    )
    .await;
    assert_index_columns(&mut tx, "accounts_root_name_folded", &["name_folded"]).await;
    assert_index_columns(&mut tx, "accounts_system_key", &["system_key"]).await;
    assert_index_columns(&mut tx, "transactions_date_id", &["date", "id"]).await;
    assert_index_columns(&mut tx, "transactions_live_date_id", &["date", "id"]).await;
    assert_index_columns(
        &mut tx,
        "postings_account_transaction",
        &["account_id", "transaction_id"],
    )
    .await;

    let partial = texts(
        &mut tx,
        "SELECT name || ': ' || sql FROM sqlite_schema WHERE type = 'index' AND sql LIKE '% WHERE %' ORDER BY name",
    )
    .await;
    assert_eq!(
        partial,
        [
            "accounts_root_name_folded: CREATE UNIQUE INDEX accounts_root_name_folded ON accounts (name_folded) WHERE parent_id IS NULL",
            "accounts_system_key: CREATE UNIQUE INDEX accounts_system_key ON accounts (system_key) WHERE system_key IS NOT NULL",
            "transactions_live_date_id: CREATE INDEX transactions_live_date_id ON transactions (date, id) WHERE deleted_at IS NULL",
        ],
        "the partial indexes' statements, whose WHERE clauses no pragma shows"
    );

    roll_back(tx, label).await;
    storage.close().await;
}

/// `users` accepts the first admin with no creator, a member the admin created, an edited
/// member (the updated pair set) and a deactivated member (the disabled pair set too), and
/// refuses with the named code: a second user whose folded name is taken, however the username
/// is spelled (UNIQUE, naming `users.username_folded`); `is_admin` outside 0 and 1 (CHECK) or
/// not an integer (DATATYPE); a `created_at`, `updated_at` or `disabled_at` that is not an RFC
/// 3339 UTC whole-second timestamp of a real instant (CHECK); one stamp of a pair without the
/// other (CHECK); a creator, editor or deactivator that does not exist (FOREIGN KEY); NULL in
/// a NOT NULL column (NOT NULL).
///
/// Proves: decision 92, INV-26 (schema half), SPEC 5.1.
#[tokio::test]
async fn users_accept_valid_rows_and_refuse_the_rest() {
    let (_dir, storage) = open_migrated().await;
    let label = "the users table";
    let mut tx = begin(&storage, label).await;

    let accepted = [
        ("the first admin, with no creator", ADMIN),
        ("a member the admin created", MEMBER),
        (
            "an edited member",
            UserRow {
                id: "3",
                username: "'editor'",
                username_folded: "'editor'",
                display_name: "'Editor'",
                updated_by: "1",
                updated_at: LATER,
                ..MEMBER
            },
        ),
        (
            "a deactivated member",
            UserRow {
                id: "4",
                username: "'retired'",
                username_folded: "'retired'",
                display_name: "'Retired'",
                updated_by: "1",
                updated_at: LATER,
                disabled_by: "1",
                disabled_at: LATER,
                ..MEMBER
            },
        ),
    ];
    for (case, row) in accepted {
        accept(&mut tx, case, &row.insert()).await;
    }

    let refused = [
        (
            "a second admin, spelled ADMIN",
            UserRow {
                username: "'ADMIN'",
                username_folded: "'admin'",
                ..NEWCOMER
            },
            UNIQUE,
        ),
        (
            "is_admin = 2",
            UserRow {
                is_admin: "2",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "is_admin = 'yes'",
            UserRow {
                is_admin: "'yes'",
                ..NEWCOMER
            },
            DATATYPE,
        ),
        (
            "is_admin = -1",
            UserRow {
                is_admin: "-1",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "created_at in the 13th month",
            UserRow {
                created_at: NO_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "created_at on the 30th of February",
            UserRow {
                created_at: BAD_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "created_at = 'yesterday'",
            UserRow {
                created_at: "'yesterday'",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "created_at = 'now'",
            UserRow {
                created_at: "'now'",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "updated_at without updated_by",
            UserRow {
                updated_at: NOW,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "updated_by without updated_at",
            UserRow {
                updated_by: "1",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "updated_at in the 13th month",
            UserRow {
                updated_by: "1",
                updated_at: NO_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "updated_at on the 30th of February",
            UserRow {
                updated_by: "1",
                updated_at: BAD_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "disabled_at without disabled_by",
            UserRow {
                disabled_at: NOW,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "disabled_by without disabled_at",
            UserRow {
                disabled_by: "1",
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "disabled_at in the 13th month",
            UserRow {
                disabled_by: "1",
                disabled_at: NO_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "disabled_at on the 30th of February",
            UserRow {
                disabled_by: "1",
                disabled_at: BAD_TIME,
                ..NEWCOMER
            },
            CHECK,
        ),
        (
            "created_by = 99",
            UserRow {
                created_by: "99",
                ..NEWCOMER
            },
            FOREIGN_KEY,
        ),
        (
            "updated_by = 99",
            UserRow {
                updated_by: "99",
                updated_at: NOW,
                ..NEWCOMER
            },
            FOREIGN_KEY,
        ),
        (
            "disabled_by = 99",
            UserRow {
                disabled_by: "99",
                disabled_at: NOW,
                ..NEWCOMER
            },
            FOREIGN_KEY,
        ),
        (
            "username = NULL",
            UserRow {
                username: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
        (
            "username_folded = NULL",
            UserRow {
                username_folded: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
        (
            "display_name = NULL",
            UserRow {
                display_name: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
        (
            "password_hash = NULL",
            UserRow {
                password_hash: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
        (
            "is_admin = NULL",
            UserRow {
                is_admin: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
        (
            "created_at = NULL",
            UserRow {
                created_at: "NULL",
                ..NEWCOMER
            },
            NOT_NULL,
        ),
    ];
    for (case, row, expected) in refused {
        let message = refuse(&mut tx, case, &row.insert(), expected).await;
        if expected == UNIQUE {
            assert_message(case, &message, "users.username_folded");
        }
    }

    let users = integer(&mut tx, "SELECT count(*) FROM users").await;
    assert_eq!(users, 4, "users after the accepted rows and the refusals");
    roll_back(tx, label).await;
    storage.close().await;
}

/// `sessions` accepts a second session for the member, seen after its creation, and refuses with
/// the named code: the seeded token hash again (UNIQUE, naming `sessions.token_hash`); a hash of
/// 63 or 65 characters, with an upper-case letter or with a character outside the hexadecimal
/// digits (CHECK); a user that does not exist (FOREIGN KEY); a timestamp of no real instant in
/// any of the three time columns (CHECK); NULL in a NOT NULL column.
///
/// Proves: decision 92, SPEC 5.3 (the stored hash).
#[tokio::test]
async fn sessions_accept_valid_rows_and_refuse_the_rest() {
    let (_dir, storage) = open_migrated().await;
    let label = "the sessions table";
    let mut tx = seeded(&storage, label).await;

    accept(
        &mut tx,
        "a second session, for the member, seen later",
        &SessionRow {
            last_seen_at: LATER,
            ..SESSION_B
        }
        .insert(),
    )
    .await;

    let refused = [
        (
            "the seeded token hash again",
            SessionRow {
                id: "3",
                token_hash: HASH_A,
                ..SESSION_B
            },
            UNIQUE,
        ),
        (
            "a hash of 63 characters",
            SessionRow {
                id: "3",
                token_hash: "'fedcba9876543210fedcba9876543210fedcba9876543210fedcba987654321'",
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "a hash of 65 characters",
            SessionRow {
                id: "3",
                token_hash: "'fedcba9876543210fedcba9876543210fedcba9876543210fedcba98765432100'",
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "a hash with an upper-case letter",
            SessionRow {
                id: "3",
                token_hash: "'FEDCBA9876543210fedcba9876543210fedcba9876543210fedcba9876543210'",
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "a hash ending in a g",
            SessionRow {
                id: "3",
                token_hash: "'fedcba9876543210fedcba9876543210fedcba9876543210fedcba987654321g'",
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "user_id = 99",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                user_id: "99",
                ..SESSION_B
            },
            FOREIGN_KEY,
        ),
        (
            "created_at in the 13th month",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                created_at: NO_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "created_at on the 30th of February",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                created_at: BAD_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "last_seen_at in the 13th month",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                last_seen_at: NO_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "last_seen_at on the 30th of February",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                last_seen_at: BAD_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "expires_at in the 13th month",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                expires_at: NO_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "expires_at on the 30th of February",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                expires_at: BAD_TIME,
                ..SESSION_B
            },
            CHECK,
        ),
        (
            "token_hash = NULL",
            SessionRow {
                id: "3",
                token_hash: "NULL",
                ..SESSION_B
            },
            NOT_NULL,
        ),
        (
            "user_id = NULL",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                user_id: "NULL",
                ..SESSION_B
            },
            NOT_NULL,
        ),
        (
            "expires_at = NULL",
            SessionRow {
                id: "3",
                token_hash: "'0000000000000000000000000000000000000000000000000000000000000000'",
                expires_at: "NULL",
                ..SESSION_B
            },
            NOT_NULL,
        ),
    ];
    for (case, row, expected) in refused {
        let message = refuse(&mut tx, case, &row.insert(), expected).await;
        if expected == UNIQUE {
            assert_message(case, &message, "sessions.token_hash");
        }
    }

    let sessions = integer(&mut tx, "SELECT count(*) FROM sessions").await;
    assert_eq!(
        sessions, 2,
        "sessions after the accepted row and the refusals"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// `accounts` accepts a second asset under the root, a grandchild of the root's kind, one name
/// under two parents, a child named like a root, an owner on an asset, an archived account, a
/// system key the schema does not know (the eight keys are core's) and a balance-only asset, and
/// lets a child become a root (the count of roots is core's); it refuses with the named code: a
/// kind outside the five, or one of them in capitals (CHECK); the seeded root's system key again
/// (UNIQUE, naming `accounts.system_key`); a second `checking` under Assets (UNIQUE, naming
/// `accounts.parent_id, accounts.name_folded`); a second root folded `assets` (UNIQUE, naming
/// `accounts.name_folded`, the partial index's work); `is_balance_only` outside 0 and 1 and an
/// `archived_at`, `created_at` or `updated_at` of no real instant (CHECK); an owner or a creator
/// that does not exist, and a parent that does not exist, which the foreign key refuses while
/// the kind trigger stays silent (its `<>` meets the missing parent's NULL kind) (FOREIGN KEY);
/// an account that is its own parent (CHECK); NULL in a NOT NULL column.
///
/// Proves: decision 92, INV-10 (schema half), INV-26 (schema half).
#[tokio::test]
async fn accounts_accept_valid_rows_and_refuse_the_rest() {
    let (_dir, storage) = open_migrated().await;
    let label = "the accounts table";
    let mut tx = seeded(&storage, label).await;

    let accepted = [
        ("a second asset under the root", SAVINGS),
        (
            "a grandchild of the root's kind",
            AccountRow {
                id: "9",
                name: "'Joint'",
                name_folded: "'joint'",
                parent_id: "6",
                ..CHECKING
            },
        ),
        (
            "the name Groceries under Assets as well",
            AccountRow {
                id: "10",
                name: "'Groceries'",
                name_folded: "'groceries'",
                ..CHECKING
            },
        ),
        (
            "a child named like a root",
            AccountRow {
                id: "11",
                name: "'Assets'",
                name_folded: "'assets'",
                parent_id: "5",
                kind: "'expense'",
                ..CHECKING
            },
        ),
        (
            "an owner on an asset",
            AccountRow {
                id: "12",
                name: "'Member Checking'",
                name_folded: "'member checking'",
                owner_user_id: "1",
                ..CHECKING
            },
        ),
        (
            "an archived account",
            AccountRow {
                id: "13",
                name: "'Old Card'",
                name_folded: "'old card'",
                parent_id: "2",
                kind: "'liability'",
                archived_at: LATER,
                updated_at: LATER,
                ..CHECKING
            },
        ),
        (
            "a system key the schema does not know",
            AccountRow {
                id: "14",
                name: "'Spare'",
                name_folded: "'spare'",
                parent_id: "3",
                kind: "'equity'",
                system_key: "'spare_key'",
                ..CHECKING
            },
        ),
        (
            "a balance-only asset",
            AccountRow {
                id: "15",
                name: "'Pension'",
                name_folded: "'pension'",
                is_balance_only: "1",
                ..CHECKING
            },
        ),
    ];
    for (case, row) in accepted {
        accept(&mut tx, case, &row.insert()).await;
    }
    accept(
        &mut tx,
        "a child made a root",
        "UPDATE accounts SET parent_id = NULL WHERE id = 9",
    )
    .await;

    let refused = [
        (
            "kind = 'assets'",
            AccountRow {
                kind: "'assets'",
                parent_id: "NULL",
                ..PROBE
            },
            CHECK,
        ),
        (
            "kind = 'ASSET'",
            AccountRow {
                kind: "'ASSET'",
                parent_id: "NULL",
                ..PROBE
            },
            CHECK,
        ),
        (
            "a second account keyed assets",
            AccountRow {
                system_key: "'assets'",
                ..PROBE
            },
            UNIQUE,
        ),
        (
            "a second checking under Assets",
            AccountRow {
                name: "'CHECKING'",
                name_folded: "'checking'",
                ..PROBE
            },
            UNIQUE,
        ),
        (
            "a second root folded assets",
            AccountRow {
                name: "'ASSETS'",
                name_folded: "'assets'",
                parent_id: "NULL",
                ..PROBE
            },
            UNIQUE,
        ),
        (
            "is_balance_only = 2",
            AccountRow {
                is_balance_only: "2",
                ..PROBE
            },
            CHECK,
        ),
        (
            "is_balance_only = -1",
            AccountRow {
                is_balance_only: "-1",
                ..PROBE
            },
            CHECK,
        ),
        (
            "archived_at in the 13th month",
            AccountRow {
                archived_at: NO_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "archived_at on the 30th of February",
            AccountRow {
                archived_at: BAD_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "created_at in the 13th month",
            AccountRow {
                created_at: NO_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "created_at on the 30th of February",
            AccountRow {
                created_at: BAD_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "updated_at in the 13th month",
            AccountRow {
                updated_at: NO_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "updated_at on the 30th of February",
            AccountRow {
                updated_at: BAD_TIME,
                ..PROBE
            },
            CHECK,
        ),
        (
            "owner_user_id = 99",
            AccountRow {
                owner_user_id: "99",
                ..PROBE
            },
            FOREIGN_KEY,
        ),
        (
            "parent_id = 99",
            AccountRow {
                parent_id: "99",
                ..PROBE
            },
            FOREIGN_KEY,
        ),
        (
            "created_by = 99",
            AccountRow {
                created_by: "99",
                ..PROBE
            },
            FOREIGN_KEY,
        ),
        (
            "its own parent",
            AccountRow {
                id: "20",
                parent_id: "20",
                ..PROBE
            },
            CHECK,
        ),
        (
            "name = NULL",
            AccountRow {
                name: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "name_folded = NULL",
            AccountRow {
                name_folded: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "kind = NULL",
            AccountRow {
                kind: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "is_balance_only = NULL",
            AccountRow {
                is_balance_only: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "created_by = NULL",
            AccountRow {
                created_by: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "created_at = NULL",
            AccountRow {
                created_at: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "updated_by = NULL",
            AccountRow {
                updated_by: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
        (
            "updated_at = NULL",
            AccountRow {
                updated_at: "NULL",
                ..PROBE
            },
            NOT_NULL,
        ),
    ];
    for (case, row, expected) in refused {
        let message = refuse(&mut tx, case, &row.insert(), expected).await;
        match case {
            "a second account keyed assets" => {
                assert_message(case, &message, "accounts.system_key");
            }
            "a second checking under Assets" => {
                assert_message(case, &message, "accounts.parent_id, accounts.name_folded");
            }
            "a second root folded assets" => {
                assert_message(case, &message, "accounts.name_folded");
                assert!(
                    !message.contains("parent_id"),
                    "the second root was refused by the sibling index, not the root index: {message}"
                );
            }
            "parent_id = 99" => assert_message(case, &message, FOREIGN_KEY_MESSAGE),
            _ => {}
        }
    }

    let accounts = integer(&mut tx, "SELECT count(*) FROM accounts").await;
    assert_eq!(
        accounts, 15,
        "accounts after the accepted rows and the refusals"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// The three kind triggers (INV-9's Schema half, decision 92): a liability under Assets, an asset
/// under Liabilities and an expense under Checking are refused at creation; moving Checking under
/// Liabilities, Groceries under Assets or the Liabilities root under Assets is refused, moving
/// Checking under Savings and back is accepted; a re-kind of Checking, of the Assets root or of
/// the Liabilities root is refused, a kind set to itself and a rename are accepted; a move and a
/// re-kind in one statement is refused by the kind trigger. Each refusal carries the trigger's
/// code and its message.
///
/// Proves: INV-9 (schema half), decision 92.
#[tokio::test]
async fn accounts_kind_triggers_fire() {
    let (_dir, storage) = open_migrated().await;
    let label = "the kind triggers";
    let mut tx = seeded(&storage, label).await;
    accept(&mut tx, "Savings under Assets", &SAVINGS.insert()).await;

    let refused_rows = [
        (
            "a liability under Assets",
            AccountRow {
                id: "20",
                name: "'Card'",
                name_folded: "'card'",
                kind: "'liability'",
                ..SAVINGS
            },
        ),
        (
            "an expense under Checking",
            AccountRow {
                id: "20",
                name: "'Fees'",
                name_folded: "'fees'",
                parent_id: "6",
                kind: "'expense'",
                ..SAVINGS
            },
        ),
        (
            "an asset under Liabilities",
            AccountRow {
                id: "20",
                name: "'Deposit'",
                name_folded: "'deposit'",
                parent_id: "2",
                kind: "'asset'",
                ..SAVINGS
            },
        ),
    ];
    for (case, row) in refused_rows {
        let message = refuse(&mut tx, case, &row.insert(), TRIGGER).await;
        assert_message(case, &message, KIND_MESSAGE);
    }

    let refused_updates = [
        (
            "Checking moved under Liabilities",
            "UPDATE accounts SET parent_id = 2 WHERE id = 6",
            KIND_MESSAGE,
        ),
        (
            "Groceries moved under Assets",
            "UPDATE accounts SET parent_id = 1 WHERE id = 7",
            KIND_MESSAGE,
        ),
        (
            "the Liabilities root moved under Assets",
            "UPDATE accounts SET parent_id = 1 WHERE id = 2",
            KIND_MESSAGE,
        ),
        (
            "Checking re-kinded",
            "UPDATE accounts SET kind = 'liability' WHERE id = 6",
            FROZEN_MESSAGE,
        ),
        (
            "the Assets root re-kinded",
            "UPDATE accounts SET kind = 'liability' WHERE id = 1",
            FROZEN_MESSAGE,
        ),
        (
            "the Liabilities root re-kinded",
            "UPDATE accounts SET kind = 'asset' WHERE id = 2",
            FROZEN_MESSAGE,
        ),
        (
            "Checking moved and re-kinded in one statement",
            "UPDATE accounts SET parent_id = 2, kind = 'liability' WHERE id = 6",
            FROZEN_MESSAGE,
        ),
    ];
    for (case, sql, needle) in refused_updates {
        let message = refuse(&mut tx, case, sql, TRIGGER).await;
        assert_message(case, &message, needle);
    }

    let accepted_updates = [
        (
            "Checking moved under Savings",
            "UPDATE accounts SET parent_id = 8 WHERE id = 6",
        ),
        (
            "Checking moved back under Assets",
            "UPDATE accounts SET parent_id = 1 WHERE id = 6",
        ),
        (
            "a kind set to itself",
            "UPDATE accounts SET kind = 'asset' WHERE id = 6",
        ),
        (
            "a rename",
            "UPDATE accounts SET name = 'Chequing', name_folded = 'chequing' WHERE id = 6",
        ),
    ];
    for (case, sql) in accepted_updates {
        accept(&mut tx, case, sql).await;
    }

    let kinds = texts(
        &mut tx,
        "SELECT id || ' ' || parent_id || ' ' || kind FROM accounts WHERE id IN (6, 7) ORDER BY id",
    )
    .await;
    assert_eq!(
        kinds,
        ["6 1 asset", "7 5 expense"],
        "Checking and Groceries after the refused and accepted updates"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// `transactions` accepts each of the four origins, the first and last dates of `Date`'s range
/// and two leap days, an empty payee and memo, a deleted transaction (both deleted stamps set,
/// later than its creation), a version written as the text `'1'` and the largest version, and
/// refuses with the named code: a date that is no real day, of another shape, with a time, with
/// a space or empty (CHECK); an origin outside the four, or one of them in capitals (CHECK); a
/// version of 0 or -1 (CHECK) or 1.5 (DATATYPE); one deleted stamp without the other (CHECK); a
/// `created_at`, `updated_at` or `deleted_at` of no real instant (CHECK); a creator or a deleter
/// that does not exist (FOREIGN KEY); NULL in a NOT NULL column.
///
/// Proves: INV-6 (schema half, the transaction date), INV-26 (schema half), decision 92.
#[tokio::test]
async fn transactions_accept_valid_rows_and_refuse_the_rest() {
    let (_dir, storage) = open_migrated().await;
    let label = "the transactions table";
    let mut tx = seeded(&storage, label).await;

    let accepted = [
        ("a manual transaction", LUNCH),
        (
            "an imported transaction",
            TransactionRow {
                id: "3",
                origin: "'import'",
                ..LUNCH
            },
        ),
        (
            "an opening balance",
            TransactionRow {
                id: "4",
                origin: "'opening_balance'",
                ..LUNCH
            },
        ),
        (
            "a balance update",
            TransactionRow {
                id: "5",
                origin: "'balance_update'",
                ..LUNCH
            },
        ),
        (
            "the first date of the range",
            TransactionRow {
                id: "6",
                date: "'1900-01-01'",
                ..LUNCH
            },
        ),
        (
            "the last date of the range",
            TransactionRow {
                id: "7",
                date: "'2199-12-31'",
                ..LUNCH
            },
        ),
        (
            "the leap day of 2000",
            TransactionRow {
                id: "8",
                date: "'2000-02-29'",
                ..LUNCH
            },
        ),
        (
            "the leap day of 2024",
            TransactionRow {
                id: "9",
                date: "'2024-02-29'",
                ..LUNCH
            },
        ),
        (
            "an empty payee and memo",
            TransactionRow {
                id: "10",
                payee: "''",
                ..LUNCH
            },
        ),
        (
            "a deleted transaction",
            TransactionRow {
                id: "11",
                updated_at: LATER,
                deleted_by: "1",
                deleted_at: LATER,
                ..LUNCH
            },
        ),
        (
            "a version written as the text '1'",
            TransactionRow {
                id: "12",
                version: "'1'",
                ..LUNCH
            },
        ),
        (
            "the largest version",
            TransactionRow {
                id: "13",
                version: "9223372036854775807",
                ..LUNCH
            },
        ),
    ];
    for (case, row) in accepted {
        accept(&mut tx, case, &row.insert()).await;
    }
    let versions = texts(
        &mut tx,
        "SELECT typeof(version) || ' ' || version FROM transactions WHERE id = 12",
    )
    .await;
    assert_eq!(
        versions,
        ["integer 1"],
        "the version written as text, stored"
    );

    let refused = [
        (
            "date = '2026-02-30'",
            TransactionRow {
                date: "'2026-02-30'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = '2023-02-29'",
            TransactionRow {
                date: "'2023-02-29'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = '2026-13-01'",
            TransactionRow {
                date: "'2026-13-01'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = '2026-2-3'",
            TransactionRow {
                date: "'2026-2-3'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = '20260203'",
            TransactionRow {
                date: "'20260203'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = '2026-02-03T00:00:00Z'",
            TransactionRow {
                date: "'2026-02-03T00:00:00Z'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = ' 2026-02-03'",
            TransactionRow {
                date: "' 2026-02-03'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "date = ''",
            TransactionRow {
                date: "''",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "origin = 'transfer'",
            TransactionRow {
                origin: "'transfer'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "origin = 'MANUAL'",
            TransactionRow {
                origin: "'MANUAL'",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "version = 0",
            TransactionRow {
                version: "0",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "version = -1",
            TransactionRow {
                version: "-1",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "version = 1.5",
            TransactionRow {
                version: "1.5",
                ..LUNCH
            },
            DATATYPE,
        ),
        (
            "deleted_at without deleted_by",
            TransactionRow {
                deleted_at: NOW,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "deleted_by without deleted_at",
            TransactionRow {
                deleted_by: "1",
                ..LUNCH
            },
            CHECK,
        ),
        (
            "deleted_at in the 13th month",
            TransactionRow {
                deleted_by: "1",
                deleted_at: NO_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "deleted_at on the 30th of February",
            TransactionRow {
                deleted_by: "1",
                deleted_at: BAD_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "created_at in the 13th month",
            TransactionRow {
                created_at: NO_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "created_at on the 30th of February",
            TransactionRow {
                created_at: BAD_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "updated_at in the 13th month",
            TransactionRow {
                updated_at: NO_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "updated_at on the 30th of February",
            TransactionRow {
                updated_at: BAD_TIME,
                ..LUNCH
            },
            CHECK,
        ),
        (
            "deleted_by = 99",
            TransactionRow {
                deleted_by: "99",
                deleted_at: NOW,
                ..LUNCH
            },
            FOREIGN_KEY,
        ),
        (
            "created_by = 99",
            TransactionRow {
                created_by: "99",
                ..LUNCH
            },
            FOREIGN_KEY,
        ),
        (
            "date = NULL",
            TransactionRow {
                date: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "payee = NULL",
            TransactionRow {
                payee: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "memo = NULL",
            TransactionRow {
                memo: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "origin = NULL",
            TransactionRow {
                origin: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "version = NULL",
            TransactionRow {
                version: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "created_by = NULL",
            TransactionRow {
                created_by: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "created_at = NULL",
            TransactionRow {
                created_at: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "updated_by = NULL",
            TransactionRow {
                updated_by: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
        (
            "updated_at = NULL",
            TransactionRow {
                updated_at: "NULL",
                ..LUNCH
            },
            NOT_NULL,
        ),
    ];
    for (case, row, expected) in refused {
        let row = TransactionRow { id: "20", ..row };
        refuse(&mut tx, case, &row.insert(), expected).await;
    }

    let transactions = integer(&mut tx, "SELECT count(*) FROM transactions").await;
    assert_eq!(
        transactions, 13,
        "transactions after the accepted rows and the refusals"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// `postings` accepts a positive amount, a negative amount, the largest and the smallest amount
/// and an amount written as the text `'5'`, stored as the integer 5, and refuses with the named
/// code: a zero amount (CHECK); an amount of 1.5, `'five'` or one past 64 bits (DATATYPE); an
/// account or a transaction that does not exist (FOREIGN KEY); NULL in a NOT NULL column.
///
/// Proves: INV-2 (schema half, no zero amount), decision 56 (a STRICT INTEGER column), decision 92.
#[tokio::test]
async fn postings_accept_valid_rows_and_refuse_the_rest() {
    let (_dir, storage) = open_migrated().await;
    let label = "the postings table";
    let mut tx = seeded(&storage, label).await;
    accept(&mut tx, "the transaction Lunch", &LUNCH.insert()).await;

    let accepted = [
        (
            "a positive amount",
            PostingRow {
                id: "3",
                transaction_id: "2",
                amount: "1",
                ..SPENT
            },
        ),
        (
            "a negative amount",
            PostingRow {
                id: "4",
                transaction_id: "2",
                amount: "-1",
                ..SPENT
            },
        ),
        (
            "the largest amount",
            PostingRow {
                id: "5",
                transaction_id: "2",
                amount: "9223372036854775807",
                ..SPENT
            },
        ),
        (
            "an amount written as the text '5'",
            PostingRow {
                id: "6",
                transaction_id: "2",
                amount: "'5'",
                ..SPENT
            },
        ),
        (
            "the smallest amount",
            PostingRow {
                id: "7",
                transaction_id: "2",
                amount: "-9223372036854775808",
                ..SPENT
            },
        ),
    ];
    for (case, row) in accepted {
        accept(&mut tx, case, &row.insert()).await;
    }
    let stored = texts(
        &mut tx,
        "SELECT typeof(amount) || ' ' || amount FROM postings WHERE id = 6",
    )
    .await;
    assert_eq!(stored, ["integer 5"], "the amount written as text, stored");

    let refused = [
        (
            "amount = 0",
            PostingRow {
                amount: "0",
                ..SPENT
            },
            CHECK,
        ),
        (
            "amount = 1.5",
            PostingRow {
                amount: "1.5",
                ..SPENT
            },
            DATATYPE,
        ),
        (
            "amount = 'five'",
            PostingRow {
                amount: "'five'",
                ..SPENT
            },
            DATATYPE,
        ),
        (
            "amount = 9223372036854775808",
            PostingRow {
                amount: "9223372036854775808",
                ..SPENT
            },
            DATATYPE,
        ),
        (
            "account_id = 99",
            PostingRow {
                account_id: "99",
                ..SPENT
            },
            FOREIGN_KEY,
        ),
        (
            "transaction_id = 99",
            PostingRow {
                transaction_id: "99",
                ..SPENT
            },
            FOREIGN_KEY,
        ),
        (
            "amount = NULL",
            PostingRow {
                amount: "NULL",
                ..SPENT
            },
            NOT_NULL,
        ),
        (
            "transaction_id = NULL",
            PostingRow {
                transaction_id: "NULL",
                ..SPENT
            },
            NOT_NULL,
        ),
        (
            "account_id = NULL",
            PostingRow {
                account_id: "NULL",
                ..SPENT
            },
            NOT_NULL,
        ),
    ];
    for (case, row, expected) in refused {
        let row = PostingRow { id: "20", ..row };
        refuse(&mut tx, case, &row.insert(), expected).await;
    }

    let postings = integer(&mut tx, "SELECT count(*) FROM postings").await;
    assert_eq!(
        postings, 7,
        "postings after the accepted rows and the refusals"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// A foreign key's `ON DELETE RESTRICT` (decision 92; the shape test pins it on every key)
/// refuses a delete at once: deleting the admin (named by the member, accounts, transactions and
/// a session), the Assets root (a parent), Checking (an account with a posting) or the
/// transaction Market (with postings) is refused with SQLite's trigger code and the foreign-key
/// message, and every row stays; once the postings are gone the transaction can go, a session
/// can always go, and so can the member, whom nothing names.
///
/// Proves: decision 92 (a restricted delete is refused, every row stays), INV-26 (the admin stays).
#[tokio::test]
async fn foreign_keys_refuse_deletes_of_referenced_rows() {
    let (_dir, storage) = open_migrated().await;
    let label = "the restrict rule";
    let mut tx = seeded(&storage, label).await;

    let refused = [
        ("deleting the admin", "DELETE FROM users WHERE id = 1"),
        (
            "deleting the Assets root",
            "DELETE FROM accounts WHERE id = 1",
        ),
        ("deleting Checking", "DELETE FROM accounts WHERE id = 6"),
        (
            "deleting the transaction Market",
            "DELETE FROM transactions WHERE id = 1",
        ),
    ];
    for (case, sql) in refused {
        let message = refuse(&mut tx, case, sql, TRIGGER).await;
        assert_message(case, &message, FOREIGN_KEY_MESSAGE);
    }
    let counts = texts(
        &mut tx,
        "SELECT (SELECT count(*) FROM users) || ' ' || (SELECT count(*) FROM accounts) || ' ' || (SELECT count(*) FROM transactions) || ' ' || (SELECT count(*) FROM postings) || ' ' || (SELECT count(*) FROM sessions)",
    )
    .await;
    assert_eq!(
        counts,
        ["2 7 1 2 1"],
        "users, accounts, transactions, postings and sessions after the refused deletes"
    );

    let accepted = [
        (
            "deleting Market's postings",
            "DELETE FROM postings WHERE transaction_id = 1",
        ),
        (
            "deleting Market once its postings are gone",
            "DELETE FROM transactions WHERE id = 1",
        ),
        (
            "deleting the admin's session",
            "DELETE FROM sessions WHERE id = 1",
        ),
        (
            "deleting the member, whom nothing names",
            "DELETE FROM users WHERE id = 2",
        ),
    ];
    for (case, sql) in accepted {
        accept(&mut tx, case, sql).await;
    }
    roll_back(tx, label).await;
    storage.close().await;
}

/// `transactions` and `postings` never give a deleted row's id to a new row (AUTOINCREMENT,
/// decision 92): after the newest transaction is deleted the next insert gets a higher id, and
/// so for postings; `accounts`, a plain rowid table, reuses the freed id, which shows the rule
/// is the two tables' own; `sqlite_sequence` names both tables.
///
/// Proves: decision 92 (AUTOINCREMENT on transactions and postings).
#[tokio::test]
async fn transactions_and_postings_never_reuse_an_id() {
    let (_dir, storage) = open_migrated().await;
    let label = "the ids";
    let mut tx = seeded(&storage, label).await;

    accept(&mut tx, "the transaction Lunch", &LUNCH.insert()).await;
    accept(
        &mut tx,
        "a third transaction",
        &TransactionRow { id: "3", ..LUNCH }.insert(),
    )
    .await;
    accept(
        &mut tx,
        "deleting the third transaction",
        "DELETE FROM transactions WHERE id = 3",
    )
    .await;
    accept(
        &mut tx,
        "a transaction after the delete",
        &TransactionRow {
            id: "NULL",
            ..LUNCH
        }
        .insert(),
    )
    .await;
    let transaction_id = integer(&mut tx, "SELECT last_insert_rowid()").await;
    assert_eq!(transaction_id, 4, "the id after transaction 3 was deleted");

    accept(
        &mut tx,
        "a posting on Lunch",
        &PostingRow {
            transaction_id: "2",
            ..SPENT
        }
        .insert(),
    )
    .await;
    accept(
        &mut tx,
        "a second posting on Lunch",
        &PostingRow {
            id: "4",
            transaction_id: "2",
            ..SPENT
        }
        .insert(),
    )
    .await;
    accept(
        &mut tx,
        "deleting the second posting",
        "DELETE FROM postings WHERE id = 4",
    )
    .await;
    accept(
        &mut tx,
        "a posting after the delete",
        &PostingRow {
            id: "NULL",
            transaction_id: "2",
            ..SPENT
        }
        .insert(),
    )
    .await;
    let posting_id = integer(&mut tx, "SELECT last_insert_rowid()").await;
    assert_eq!(posting_id, 5, "the id after posting 4 was deleted");

    accept(&mut tx, "Savings", &SAVINGS.insert()).await;
    accept(
        &mut tx,
        "deleting Savings",
        "DELETE FROM accounts WHERE id = 8",
    )
    .await;
    accept(
        &mut tx,
        "an account after the delete",
        &AccountRow {
            id: "NULL",
            ..SAVINGS
        }
        .insert(),
    )
    .await;
    let account_id = integer(&mut tx, "SELECT last_insert_rowid()").await;
    assert_eq!(
        account_id, 8,
        "the id after account 8 was deleted (a plain rowid table)"
    );

    let sequences = texts(&mut tx, "SELECT name FROM sqlite_sequence ORDER BY name").await;
    assert_eq!(
        sequences,
        ["postings", "transactions"],
        "the tables sqlite_sequence names"
    );
    roll_back(tx, label).await;
    storage.close().await;
}

/// Every `Date` from 1900-01-01 to 2199-12-31 (109,573 of them, stepped with `Date::tomorrow`)
/// satisfies the date CHECK of `transactions.date`: inserted in batches of a thousand into one
/// rolled-back write transaction, every one is stored and the stored range runs from the first
/// date to the last. Texts `Date` refuses for their shape or their calendar are refused by the
/// CHECK too, while the two dates just outside the range pass the CHECK and are refused by
/// `Date` alone, the schema's years being wider than the type's (decision 88).
///
/// Proves: INV-6 (both halves agree on the transaction date), decisions 88, 89 and 92.
#[tokio::test]
async fn every_date_of_the_range_satisfies_the_date_check() {
    let (_dir, storage) = open_migrated().await;
    let label = "the date check";
    let mut tx = seeded(&storage, label).await;

    let mut dates = Vec::new();
    let mut next = Some(Date::MIN);
    while let Some(date) = next {
        dates.push(date.to_string());
        next = date.tomorrow();
    }
    assert_eq!(
        dates.len(),
        109_573,
        "the dates from Date::MIN to Date::MAX"
    );

    let before = integer(&mut tx, "SELECT count(*) FROM transactions").await;
    for batch in dates.chunks(1_000) {
        let values: Vec<String> = batch
            .iter()
            .map(|date| format!("('{date}', '', '', 'manual', 1, 1, {NOW}, 1, {NOW})"))
            .collect();
        let sql = format!(
            "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES {}",
            values.join(", ")
        );
        accept(&mut tx, "a batch of a thousand dates", &sql).await;
    }
    let after = integer(&mut tx, "SELECT count(*) FROM transactions").await;
    let inserted = usize::try_from(after - before)
        .unwrap_or_else(|error| panic!("fewer transactions after the inserts: {error}"));
    assert_eq!(
        inserted,
        dates.len(),
        "every date of the range stored as a transaction date"
    );
    let bounds = texts(
        &mut tx,
        "SELECT min(date) || ' ' || max(date) FROM transactions WHERE payee = ''",
    )
    .await;
    assert_eq!(
        bounds,
        ["1900-01-01 2199-12-31"],
        "the first and last dates stored"
    );

    let refused = [
        "2026-02-30",
        "2023-02-29",
        "1900-02-29",
        "2100-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-02-00",
        "2026-2-3",
        "20260203",
        " 2026-02-03",
        "2026-02-03 ",
        "2026-02-03T00:00:00Z",
        "2026/02/03",
        "",
        "now",
    ];
    for text in refused {
        assert!(text.parse::<Date>().is_err(), "Date accepts {text:?}");
        let sql = format!(
            "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('{text}', '', '', 'manual', 1, 1, {NOW}, 1, {NOW})"
        );
        refuse(&mut tx, &format!("the date {text:?}"), &sql, CHECK).await;
    }

    for text in ["1899-12-31", "2200-01-01"] {
        assert_eq!(
            text.parse::<Date>(),
            Err(DateError::OutOfRange),
            "Date refuses {text:?} as out of range"
        );
        let sql = format!(
            "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('{text}', 'outside', '', 'manual', 1, 1, {NOW}, 1, {NOW})"
        );
        accept(
            &mut tx,
            &format!("the date {text:?}, which only Date refuses"),
            &sql,
        )
        .await;
    }

    roll_back(tx, label).await;
    storage.close().await;
}
