//! Helpers shared by the nage-core integration tests beyond `nage_core::test_support`
//! (decision 59), which opens the temporary databases: the shortened waits, PRAGMA reads,
//! SQLite result codes and the bookkeeping write; the instants, the runtime bridge, the standard
//! setup and the transaction, reader and account-lookup helpers of the slice 3 tests; the
//! unchecked-SQL fixtures of decision 98 (a transaction with its postings, the raw roots); the
//! plain connection and the corruption step; and the check run with its labels. The PRAGMA
//! helpers are the unchecked SQL decision 57 allows; the schema tests add their own.
#![allow(dead_code)]

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use nage_core::accounts::AccountId;
use nage_core::check::{self, Report};
use nage_core::currency::Currency;
use nage_core::setup::{Initialized, Setup, initialize};
use nage_core::storage::{Storage, WriteTransaction};
use nage_core::test_support::{TestDir, open_migrated};
use nage_core::timestamp::Timestamp;
use nage_core::users::NewUser;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{AssertSqlSafe, Connection as _, Executor, Row as _, Sqlite, SqliteConnection};

/// A wait short enough for a test to let expire (decision 53 shortened through
/// `StorageOptions`).
pub const SHORT_WAIT: Duration = Duration::from_millis(100);

/// The bound an expired [`SHORT_WAIT`] must return within: well under the five-second
/// SQLite busy timeout a wrongly sized pool would hit instead.
pub const SOON: Duration = Duration::from_secs(1);

/// Reads the text value of `PRAGMA name` on `executor` (a pool, or `&mut *tx` inside a
/// write transaction). The PRAGMA name is a test literal; the statement is unchecked SQL
/// (decision 57) because sqlx cannot check a PRAGMA, and the decode is unchecked because a
/// PRAGMA row carries no declared type.
pub async fn pragma_text<'e, E>(executor: E, name: &str) -> String
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(AssertSqlSafe(format!("PRAGMA {name}")))
        .fetch_one(executor)
        .await
        .unwrap_or_else(|error| panic!("PRAGMA {name} failed: {error}"));
    row.try_get_unchecked::<String, _>(0)
        .unwrap_or_else(|error| panic!("PRAGMA {name} did not answer text: {error}"))
}

/// Reads the integer value of `PRAGMA name` on `executor`; see [`pragma_text`].
pub async fn pragma_int<'e, E>(executor: E, name: &str) -> i64
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(AssertSqlSafe(format!("PRAGMA {name}")))
        .fetch_one(executor)
        .await
        .unwrap_or_else(|error| panic!("PRAGMA {name} failed: {error}"));
    row.try_get_unchecked::<i64, _>(0)
        .unwrap_or_else(|error| panic!("PRAGMA {name} did not answer an integer: {error}"))
}

/// The extended SQLite result code of a database error, `None` for any other sqlx error.
pub fn sqlite_code(error: &sqlx::Error) -> Option<i32> {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .and_then(|code| code.parse().ok())
}

/// The primary result code of an extended one (the low byte).
pub fn primary_code(extended: i32) -> i32 {
    extended & 0xFF
}

/// Runs `sql` in one write transaction through `Storage::begin_write` and commits it,
/// panicking on failure. For the tests that need a row the public API cannot yet write
/// (migration bookkeeping and schema tests, decision 57); never for scenario data.
pub async fn write(storage: &Storage, sql: &'static str) {
    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction: {error}"));
    sqlx::query(sql)
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit `{sql}`: {error}"));
}

/// The seconds of 2026-11-09T12:00:00Z, the instant the tests use as "now".
pub const NOW_SECOND: i64 = 1_794_225_600;
/// The seconds of 2026-11-10T08:30:00Z, an instant after [`NOW_SECOND`] for later edits.
pub const LATER_SECOND: i64 = 1_794_299_400;
/// The text of [`NOW_SECOND`], for rows written with unchecked SQL.
pub const NOW_TEXT: &str = "2026-11-09T12:00:00Z";
/// The text of [`LATER_SECOND`].
pub const LATER_TEXT: &str = "2026-11-10T08:30:00Z";

/// The instant [`NOW_SECOND`].
pub fn now() -> Timestamp {
    stamp(NOW_SECOND)
}

/// The instant [`LATER_SECOND`].
pub fn later() -> Timestamp {
    stamp(LATER_SECOND)
}

/// The instant `second` seconds after the epoch, which must lie in the range.
pub fn stamp(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap_or_else(|error| panic!("second {second}: {error}"))
}

/// Runs `future` to completion on a fresh current-thread runtime: the bridge from a synchronous
/// proptest case to the async storage (decision 98).
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"))
        .block_on(future)
}

/// The currency `code`, which must be in the table.
pub fn currency(code: &str) -> Currency {
    Currency::from_code(code).unwrap_or_else(|| panic!("{code} is not in the currency table"))
}

/// The standard setup of these tests: the book "Household" in CAD, the admin `admin` with the
/// display name "Admin" and the opaque hash `hash`, the starter chart when asked.
pub fn standard_setup(starter_chart: bool) -> Setup {
    Setup {
        book_name: String::from("Household"),
        currency: currency("CAD"),
        admin: NewUser {
            username: String::from("admin"),
            display_name: String::from("Admin"),
            password_hash: String::from("hash"),
            is_admin: true,
        },
        starter_chart,
    }
}

/// Begins a write transaction on `storage`, naming `label` on failure.
pub async fn begin(storage: &Storage, label: &str) -> WriteTransaction {
    storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction for {label}: {error}"))
}

/// Commits `tx`, naming `label` on failure.
pub async fn commit(tx: WriteTransaction, label: &str) {
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit {label}: {error}"));
}

/// Rolls `tx` back, naming `label` on failure.
pub async fn roll_back(tx: WriteTransaction, label: &str) {
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back {label}: {error}"));
}

/// A reader connection of `storage`, for the read functions of the core.
pub async fn reader(storage: &Storage) -> PoolConnection<Sqlite> {
    storage
        .reader()
        .acquire()
        .await
        .unwrap_or_else(|error| panic!("cannot acquire a reader connection: {error}"))
}

/// A fresh migrated database with the standard setup committed at [`now`]: the directory, the
/// storage and what setup created.
pub async fn seeded(starter_chart: bool) -> (TestDir, Storage, Initialized) {
    let (dir, storage) = open_migrated().await;
    let mut tx = begin(&storage, "the standard setup").await;
    let initialized = initialize(&mut tx, now(), &standard_setup(starter_chart))
        .await
        .unwrap_or_else(|error| panic!("cannot set up the test book: {error}"));
    commit(tx, "the standard setup").await;
    (dir, storage, initialized)
}

/// The account named `full_name`, which must exist.
pub async fn account_named(conn: &mut SqliteConnection, full_name: &str) -> AccountId {
    nage_core::accounts::resolve(conn, full_name)
        .await
        .unwrap_or_else(|error| panic!("cannot resolve {full_name}: {error}"))
        .unwrap_or_else(|| panic!("no account {full_name}"))
        .id
}

/// Inserts a transaction and its postings with unchecked SQL and returns the transaction's id:
/// the test fixture of the balance, archive and check tests while the transaction operation is
/// slice 4's (decision 98's exception). The transaction is created and updated by user
/// 1 at `created_at`; a deleted one is deleted by user 1 at the same instant.
pub async fn insert_transaction(
    tx: &mut WriteTransaction,
    date: &str,
    origin: &str,
    deleted: bool,
    created_at: &str,
    postings: &[(AccountId, i64)],
) -> i64 {
    let deleted_pair = if deleted {
        format!("1, '{created_at}'")
    } else {
        String::from("NULL, NULL")
    };
    let sql = format!(
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at, deleted_by, deleted_at) VALUES ('{date}', '', '', '{origin}', 1, 1, '{created_at}', 1, '{created_at}', {deleted_pair})"
    );
    let id = sqlx::query(AssertSqlSafe(sql.clone()))
        .execute(&mut **tx)
        .await
        .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"))
        .last_insert_rowid();
    for (account, amount) in postings {
        let sql = format!(
            "INSERT INTO postings (transaction_id, account_id, amount) VALUES ({id}, {}, {amount})",
            account.0
        );
        sqlx::query(AssertSqlSafe(sql.clone()))
            .execute(&mut **tx)
            .await
            .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"));
    }
    id
}

/// Inserts one user (id 1, the admin) and the five roots (ids 1 to 5, in SPEC 6.2's order) with
/// unchecked SQL, for the tests of the column forms, which must not depend on setup.
pub async fn raw_roots(storage: &Storage) {
    let mut tx = begin(storage, "the raw roots").await;
    let statements = [
        format!(
            "INSERT INTO users (id, username, username_folded, display_name, password_hash, is_admin, created_by, created_at, updated_by, updated_at, disabled_by, disabled_at) VALUES (1, 'admin', 'admin', 'Admin', 'hash', 1, NULL, '{NOW_TEXT}', NULL, NULL, NULL, NULL)"
        ),
        root_sql(1, "Assets", "asset", "assets"),
        root_sql(2, "Liabilities", "liability", "liabilities"),
        root_sql(3, "Equity", "equity", "equity"),
        root_sql(4, "Income", "income", "income"),
        root_sql(5, "Expenses", "expense", "expenses"),
    ];
    for sql in statements {
        sqlx::query(AssertSqlSafe(sql.clone()))
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"));
    }
    commit(tx, "the raw roots").await;
}

/// The INSERT of a root account with unchecked SQL.
fn root_sql(id: i64, name: &str, kind: &str, key: &str) -> String {
    let folded = name.to_lowercase();
    format!(
        "INSERT INTO accounts (id, name, name_folded, parent_id, kind, system_key, owner_user_id, is_balance_only, archived_at, created_by, created_at, updated_by, updated_at) VALUES ({id}, '{name}', '{folded}', NULL, '{kind}', '{key}', NULL, 0, NULL, 1, '{NOW_TEXT}', 1, '{NOW_TEXT}')"
    )
}

/// A plain connection of the test's own to the database at `path`, for planting corruptions
/// (decision 98): foreign keys on or off as asked, otherwise sqlx's defaults.
pub async fn raw_connection(path: &Path, foreign_keys: bool) -> SqliteConnection {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(foreign_keys);
    SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()))
}

/// Runs each statement on `conn` in order, panicking on the first failure.
pub async fn exec(conn: &mut SqliteConnection, statements: &[&str]) {
    for sql in statements {
        sqlx::query(AssertSqlSafe((*sql).to_owned()))
            .execute(&mut *conn)
            .await
            .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"));
    }
}

/// Plants `statements` through a plain connection to `path` (foreign keys off when `fk_off`) and
/// closes it: the corruption step of the check tests (decision 98).
pub async fn corrupt(path: &Path, fk_off: bool, statements: &[&str]) {
    let mut conn = raw_connection(path, !fk_off).await;
    exec(&mut conn, statements).await;
    conn.close()
        .await
        .unwrap_or_else(|error| panic!("cannot close the corrupting connection: {error}"));
}

/// Runs the check in one read transaction of `storage`.
pub async fn check_report(storage: &Storage) -> Report {
    let mut tx = storage
        .begin_read()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a read transaction: {error}"));
    let report = check::run(&mut tx)
        .await
        .unwrap_or_else(|error| panic!("check failed: {error}"));
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot end the read transaction: {error}"));
    report
}

/// The labels of a report's violations, in order: `INV-n` or `FK`.
pub fn labels(report: &Report) -> Vec<&'static str> {
    report
        .violations()
        .iter()
        .map(nage_core::check::Violation::label)
        .collect()
}
