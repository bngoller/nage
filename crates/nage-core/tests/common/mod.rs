//! Helpers shared by the nage-core integration tests: temporary databases opened through
//! `Storage` (never in place, decision 58), PRAGMA reads and SQLite result codes. The PRAGMA
//! helpers are the unchecked SQL decision 57 allows; the schema tests add their own.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use nage_core::storage::{MigrationReport, Storage, StorageOptions};
use sqlx::{AssertSqlSafe, Executor, Row as _, Sqlite};
use tempfile::TempDir;

/// The file name `serve` gives the database (SPEC 5.2).
pub const FILE_NAME: &str = "nage.sqlite";

/// A wait short enough for a test to let expire (decision 53 shortened through
/// `StorageOptions`).
pub const SHORT_WAIT: Duration = Duration::from_millis(100);

/// The bound an expired [`SHORT_WAIT`] must return within: well under the five-second
/// SQLite busy timeout a wrongly sized pool would hit instead.
pub const SOON: Duration = Duration::from_secs(1);

/// A fresh temporary directory, removed when dropped.
pub fn temp_dir() -> TempDir {
    tempfile::tempdir()
        .unwrap_or_else(|error| panic!("cannot create a temporary directory: {error}"))
}

/// The database path inside `dir`.
pub fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join(FILE_NAME)
}

/// Opens `nage.sqlite` in a fresh temporary directory with `options`; nothing is migrated.
pub async fn open_with(options: &StorageOptions) -> (TempDir, Storage) {
    let dir = temp_dir();
    let path = db_path(&dir);
    let storage = Storage::open(&path, options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    (dir, storage)
}

/// Opens `nage.sqlite` in a fresh temporary directory with the default options; nothing is
/// migrated.
pub async fn open_fresh() -> (TempDir, Storage) {
    open_with(&StorageOptions::default()).await
}

/// Opens `nage.sqlite` in a fresh temporary directory with the default options and applies
/// the migrations.
pub async fn open_migrated() -> (TempDir, Storage) {
    let (dir, storage) = open_fresh().await;
    migrate(&storage).await;
    (dir, storage)
}

/// Applies the migrations, panicking on failure.
pub async fn migrate(storage: &Storage) -> MigrationReport {
    storage
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("cannot migrate: {error}"))
}

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
