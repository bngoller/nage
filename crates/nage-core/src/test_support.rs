//! Helpers shared by the tests of every workspace crate (decision 59): temporary
//! directories, storage opened in them, copies of the committed fixtures, the side-file
//! assertion and the property-test configuration; and, for the tests of sign-in (decisions
//! 107 and 115), the cheap hashing parameters, a book set up with an admin whose password is
//! really hashed, a further user, the deactivation no operation writes yet and the count of
//! session rows.
//!
//! The module exists only with the `test-support` feature, and only dev-dependencies enable
//! that feature, so a production build (`cargo build -p nage`, the Dockerfile) never carries
//! it. Other crates reach it as `nage_core::test_support`; nage-core's own in-crate unit tests
//! reach it as `crate::test_support`, never through the extern name, because the self
//! dev-dependency that turns the feature on links a second copy of the library whose types
//! do not match this one's.

use std::path::{Component, Path, PathBuf};

use proptest::test_runner::FileFailurePersistence;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};
use tempfile::TempDir;

use crate::currency::Currency;
use crate::password::{self, HashParams};
use crate::setup::{self, Initialized, Setup};
use crate::storage::{MigrationReport, Storage, StorageOptions};
use crate::timestamp::Timestamp;
use crate::users::{self, NewUser, User, UserId};

/// The file name `serve` gives the database (SPEC 5.2).
pub const DB_FILE_NAME: &str = "nage.sqlite";

/// A fresh directory under the system temporary directory, named `nage-test-*`, with its
/// path canonicalised at creation so it compares equal to what a child process reports as
/// its working directory. Removed on drop, except when the thread is panicking: a test that
/// panics while it holds the directory leaves it behind for inspection.
#[derive(Debug)]
pub struct TestDir {
    dir: TempDir,
    path: PathBuf,
}

impl TestDir {
    /// Creates the directory, panicking if it cannot be created or resolved.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("nage-test-")
            .tempdir()
            .unwrap_or_else(|error| panic!("cannot create a temporary directory: {error}"));
        let path = std::fs::canonicalize(dir.path())
            .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", dir.path().display()));
        Self { dir, path }
    }

    /// The canonical path of the directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database path inside the directory: [`DB_FILE_NAME`].
    pub fn db_path(&self) -> PathBuf {
        self.path.join(DB_FILE_NAME)
    }
}

impl Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.dir.disable_cleanup(true);
        }
    }
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with `options`; nothing is migrated.
/// Panics if the database cannot be opened.
pub async fn open_with(options: &StorageOptions) -> (TestDir, Storage) {
    let dir = TestDir::new();
    let path = dir.db_path();
    let storage = Storage::open(&path, options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    (dir, storage)
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with the default options; nothing is
/// migrated.
pub async fn open_fresh() -> (TestDir, Storage) {
    open_with(&StorageOptions::default()).await
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with the default options and applies the
/// migrations.
pub async fn open_migrated() -> (TestDir, Storage) {
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

/// Every row of every table of the database at `path`, opened read-only, each as one line
/// `<table> <column>=<value> ...` in table and rowid order, for comparing a committed book with a
/// fresh seed (assumption A15 of PLAN M2 slice 8): sqlx's bookkeeping times
/// (`_sqlx_migrations.installed_on` and `execution_time`) and every password hash with its salt
/// (`users.password_hash`) are left out, since they change on every run. Each table of
/// [`tables_read`] is read by a checked query of its own, never by a statement assembled from the
/// names found in the file (ruling P1 of the pause; decision 57). Panics when the file cannot be
/// read.
pub async fn book_rows(path: &Path) -> Vec<String> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {} read-only: {error}", path.display()));
    let rows = read_rows(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("cannot read the rows of {}: {error}", path.display()));
    conn.close()
        .await
        .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
    rows
}

/// The tables [`book_rows`] reads, by name, in its order: a test compares them with the tables the
/// schema holds, so that a table a later migration adds cannot be forgotten (ruling P1). SQLite's
/// own `sqlite_sequence` is not read: its values follow the ids of the tables read, and sqlx 0.9's
/// query macro brings the compiler down (SIGSEGV) when it describes a query on it.
pub fn tables_read() -> &'static [&'static str] {
    &[
        "_sqlx_migrations",
        "book",
        "users",
        "sessions",
        "accounts",
        "transactions",
        "postings",
    ]
}

/// A value of a row as [`book_rows`] shows it: `NULL` for none, else its `Debug` text.
fn shown<T: std::fmt::Debug>(value: &Option<T>) -> String {
    match value {
        Some(value) => format!("{value:?}"),
        None => String::from("NULL"),
    }
}

/// [`book_rows`] on `conn`: one checked query per table of [`tables_read`], in its order.
async fn read_rows(conn: &mut SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    let mut rows = Vec::new();
    for row in sqlx::query!(
        r#"SELECT version AS "version!: i64", description AS "description!: String", success AS "success!: i64", checksum AS "checksum!: Vec<u8>" FROM _sqlx_migrations ORDER BY version"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        let checksum: String = row.checksum.iter().map(|byte| format!("{byte:02x}")).collect();
        rows.push(format!(
            "_sqlx_migrations version={} description={:?} success={} checksum={checksum}",
            row.version, row.description, row.success
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", name AS "name!: String", currency AS "currency!: String", minor_digits AS "minor_digits!: i64", created_at AS "created_at!: String" FROM book ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "book id={} name={:?} currency={:?} minor_digits={} created_at={}",
            row.id, row.name, row.currency, row.minor_digits, row.created_at
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", username AS "username!: String", username_folded AS "username_folded!: String", display_name AS "display_name!: String", is_admin AS "is_admin!: i64", created_by AS "created_by: i64", created_at AS "created_at!: String", updated_by AS "updated_by: i64", updated_at AS "updated_at: String", disabled_by AS "disabled_by: i64", disabled_at AS "disabled_at: String" FROM users ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "users id={} username={:?} username_folded={:?} display_name={:?} is_admin={} created_by={} created_at={} updated_by={} updated_at={} disabled_by={} disabled_at={}",
            row.id,
            row.username,
            row.username_folded,
            row.display_name,
            row.is_admin,
            shown(&row.created_by),
            row.created_at,
            shown(&row.updated_by),
            shown(&row.updated_at),
            shown(&row.disabled_by),
            shown(&row.disabled_at)
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", token_hash AS "token_hash!: String", user_id AS "user_id!: i64", created_at AS "created_at!: String", last_seen_at AS "last_seen_at!: String", expires_at AS "expires_at!: String" FROM sessions ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "sessions id={} token_hash={:?} user_id={} created_at={} last_seen_at={} expires_at={}",
            row.id, row.token_hash, row.user_id, row.created_at, row.last_seen_at, row.expires_at
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", name AS "name!: String", name_folded AS "name_folded!: String", parent_id AS "parent_id: i64", kind AS "kind!: String", system_key AS "system_key: String", owner_user_id AS "owner_user_id: i64", is_balance_only AS "is_balance_only!: i64", archived_at AS "archived_at: String", created_by AS "created_by!: i64", created_at AS "created_at!: String", updated_by AS "updated_by!: i64", updated_at AS "updated_at!: String" FROM accounts ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "accounts id={} name={:?} name_folded={:?} parent_id={} kind={} system_key={} owner_user_id={} is_balance_only={} archived_at={} created_by={} created_at={} updated_by={} updated_at={}",
            row.id,
            row.name,
            row.name_folded,
            shown(&row.parent_id),
            row.kind,
            shown(&row.system_key),
            shown(&row.owner_user_id),
            row.is_balance_only,
            shown(&row.archived_at),
            row.created_by,
            row.created_at,
            row.updated_by,
            row.updated_at
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", date AS "date!: String", payee AS "payee!: String", memo AS "memo!: String", origin AS "origin!: String", version AS "version!: i64", created_by AS "created_by!: i64", created_at AS "created_at!: String", updated_by AS "updated_by!: i64", updated_at AS "updated_at!: String", deleted_by AS "deleted_by: i64", deleted_at AS "deleted_at: String" FROM transactions ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "transactions id={} date={} payee={:?} memo={:?} origin={} version={} created_by={} created_at={} updated_by={} updated_at={} deleted_by={} deleted_at={}",
            row.id,
            row.date,
            row.payee,
            row.memo,
            row.origin,
            row.version,
            row.created_by,
            row.created_at,
            row.updated_by,
            row.updated_at,
            shown(&row.deleted_by),
            shown(&row.deleted_at)
        ));
    }
    for row in sqlx::query!(
        r#"SELECT id AS "id!: i64", transaction_id AS "transaction_id!: i64", account_id AS "account_id!: i64", amount AS "amount!: i64" FROM postings ORDER BY id"#
    )
    .fetch_all(&mut *conn)
    .await?
    {
        rows.push(format!(
            "postings id={} transaction_id={} account_id={} amount={}",
            row.id, row.transaction_id, row.account_id, row.amount
        ));
    }
    Ok(rows)
}

/// The argon2id parameters a test hands in unless it pins another set (decision 107): m=8 KiB,
/// t=1, p=1, the cheapest set argon2 accepts, so that a test's hash takes about a millisecond
/// in a debug build where SPEC 5.3's takes about half a second. The binary never uses them: it
/// hands in `HashParams::SPEC`. A hash made with them names them, so it verifies like any other.
pub const CHEAP_HASH_PARAMS: HashParams = HashParams {
    memory_kib: 8,
    passes: 1,
    lanes: 1,
};

/// The name of the book [`set_up`] creates.
pub const TEST_BOOK_NAME: &str = "Household";

/// The hash of `password` made with [`CHEAP_HASH_PARAMS`], panicking when the password breaks
/// a rule or the hasher fails.
pub fn cheap_hash(password: &str) -> String {
    password::hash(CHEAP_HASH_PARAMS, password)
        .unwrap_or_else(|error| panic!("cannot hash the test password: {error}"))
}

/// Sets a migrated database up at `now` through `setup::initialize`, in one committed write
/// transaction: the book [`TEST_BOOK_NAME`] in CAD with the starter chart, and the admin
/// `username` (its display name the same) with `password` hashed by [`cheap_hash`]. Panics if
/// the set-up is refused. For the tests of sign-in and of the commands, which need a stored
/// hash that verifies; a test that never verifies a password may store any text.
pub async fn set_up(
    storage: &Storage,
    now: Timestamp,
    username: &str,
    password: &str,
) -> Initialized {
    let setup = Setup {
        book_name: String::from(TEST_BOOK_NAME),
        currency: Currency::from_code("CAD")
            .unwrap_or_else(|| panic!("CAD is not in the currency table")),
        admin: NewUser {
            username: username.to_owned(),
            display_name: username.to_owned(),
            password_hash: cheap_hash(password),
            is_admin: true,
        },
        starter_chart: true,
    };
    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin the set-up: {error}"));
    let initialized = setup::initialize(&mut tx, now, &setup)
        .await
        .unwrap_or_else(|error| panic!("cannot set the test book up: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit the set-up: {error}"));
    initialized
}

/// Adds the user `username` (its display name the same) at `now` through
/// `users::create_user`, created by the admin `by`, with `password` hashed by [`cheap_hash`],
/// in one committed write transaction. Panics if the user is refused.
pub async fn add_user(
    storage: &Storage,
    by: UserId,
    now: Timestamp,
    username: &str,
    password: &str,
    is_admin: bool,
) -> User {
    let new = NewUser {
        username: username.to_owned(),
        display_name: username.to_owned(),
        password_hash: cheap_hash(password),
        is_admin,
    };
    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin adding {username}: {error}"));
    let user = users::create_user(&mut tx, Some(by), now, &new)
        .await
        .unwrap_or_else(|error| panic!("cannot add {username}: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("cannot commit adding {username}: {error}"));
    user
}

/// Deactivates `user` as if `by` had done so at `at`, with an unchecked update on `conn` (a
/// write transaction, which the caller commits or rolls back): the deactivation operation is
/// milestone 8 slice 1's, and until it exists the tests that need a deactivated user plant the
/// pair `disabled_by`/`disabled_at` this way (decisions 98 and 115). Panics unless exactly one
/// row changed.
pub async fn deactivate_unchecked(
    conn: &mut sqlx::SqliteConnection,
    user: UserId,
    by: UserId,
    at: Timestamp,
) {
    let changed = sqlx::query("UPDATE users SET disabled_by = ?1, disabled_at = ?2 WHERE id = ?3")
        .bind(by)
        .bind(at)
        .bind(user)
        .execute(conn)
        .await
        .unwrap_or_else(|error| panic!("cannot deactivate user {user}: {error}"))
        .rows_affected();
    assert_eq!(changed, 1, "rows changed by deactivating user {user}");
}

/// How many rows `sessions` holds as `conn` sees them, valid, expired and deactivated users'
/// alike: what a refused sign-in must leave as it found it (INV-28) and what a deletion must
/// lower. A checked query.
pub async fn session_rows(conn: &mut sqlx::SqliteConnection) -> i64 {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM sessions"#)
        .fetch_one(conn)
        .await
        .unwrap_or_else(|error| panic!("cannot count the sessions: {error}"))
}

/// Copies the committed fixture `fixtures/books/v<version>.sqlite` (the version without a
/// leading `v`, for example `"0.1.0"`; never a path), resolved from nage-core's
/// `CARGO_MANIFEST_DIR`, into a fresh [`TestDir`] and returns the directory and the copy's
/// path. The copy lies at the directory's [`TestDir::db_path`], the name `serve` gives the
/// database. The committed file is never opened in place, only read as bytes by
/// `std::fs::copy` (decision 58). Panics, naming the path, if the fixture is missing or
/// cannot be copied.
pub fn fixture_copy(version: &str) -> (TestDir, PathBuf) {
    let fixture = workspace_root()
        .join("fixtures/books")
        .join(format!("v{version}.sqlite"));
    assert!(
        fixture.is_file(),
        "the fixture {} does not exist",
        fixture.display()
    );
    let dir = TestDir::new();
    let copy = dir.db_path();
    std::fs::copy(&fixture, &copy).unwrap_or_else(|error| {
        panic!(
            "cannot copy the fixture {} to {}: {error}",
            fixture.display(),
            copy.display()
        )
    });
    (dir, copy)
}

/// The workspace root: the grandparent of nage-core's `CARGO_MANIFEST_DIR`, fixed at compile
/// time, so it does not depend on the working directory.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("{} has no grandparent", manifest.display()))
        .to_path_buf()
}

/// Asserts that neither `<db>-wal` nor `<db>-shm` exists next to the database file `db`: a
/// clean close checkpoints the WAL and removes both (decision 53). `when` names the moment
/// in the message.
#[track_caller]
pub fn assert_no_side_files(db: &Path, when: &str) {
    for suffix in ["-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        let side = PathBuf::from(name);
        assert!(
            !side.exists(),
            "the SQLite side file {} exists {when}",
            side.display()
        );
    }
}

/// The proptest regression file for the workspace-relative source path `file` (what
/// `file!()` gives under cargo): `<workspace root>/<crate dir>/proptest-regressions/<src or
/// tests>/<rest>.txt`. The workspace root is the grandparent of nage-core's
/// `CARGO_MANIFEST_DIR`, a compile-time constant, so the result is absolute and does not
/// depend on the working directory; `<crate dir>` is the leading `crates/<name>` of `file`,
/// and the rest is the path from its `src/` or `tests/` component with the extension
/// replaced by `.txt`. A path without those components maps to
/// `proptest-regressions/<file name>.txt` under the crate directory (decision 61). Panics if
/// `file` does not start with `crates/<name>/` or has no file name.
pub fn regression_path(file: &str) -> PathBuf {
    let path = Path::new(file);
    let mut components = path.components();
    let crate_dir = match (components.next(), components.next()) {
        (Some(Component::Normal(first)), Some(Component::Normal(name))) if first == "crates" => {
            Path::new(first).join(name)
        }
        _ => panic!("regression_path({file}): the path does not start with crates/<name>/"),
    };
    let inner = components.as_path();
    let start = inner
        .components()
        .position(|component| {
            matches!(component, Component::Normal(name) if name == "src" || name == "tests")
        });
    let rest = match start {
        Some(start) => inner.components().skip(start).collect::<PathBuf>(),
        None => PathBuf::from(
            inner
                .file_name()
                .unwrap_or_else(|| panic!("regression_path({file}): the path has no file name")),
        ),
    };
    workspace_root()
        .join(crate_dir)
        .join("proptest-regressions")
        .join(rest)
        .with_extension("txt")
}

/// The configuration every property declares through
/// `#![proptest_config(test_support::proptest_config(file!()))]`: `Config::default()` (256
/// cases unless `PROPTEST_CASES` overrides it) with failure persistence set to
/// `FileFailurePersistence::Direct` of [`regression_path`] for `file`. `Direct` takes a
/// `&'static str`, so each call leaks one path string; that is one small string per
/// property per test run (decision 61). `Config::default()` is proptest's contextualised
/// default, so `PROPTEST_CASES` and the other `PROPTEST_*` variables still apply.
pub fn proptest_config(file: &'static str) -> proptest::test_runner::Config {
    let path = regression_path(file)
        .into_os_string()
        .into_string()
        .unwrap_or_else(|path| panic!("the regression path {path:?} is not UTF-8"));
    let leaked: &'static str = Box::leak(path.into_boxed_str());
    proptest::test_runner::Config {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(leaked))),
        ..proptest::test_runner::Config::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use proptest::test_runner::FileFailurePersistence;

    use super::{TestDir, assert_no_side_files, proptest_config, regression_path};

    /// The workspace root, computed here from nage-core's manifest directory and anchored
    /// against the tree: it must hold the workspace `Cargo.toml` and nage-core's manifest, so
    /// the mapping below is checked against the real root, not against the helper's formula.
    fn workspace_root() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| panic!("{} has no grandparent", manifest.display()))
            .to_path_buf();
        assert!(
            root.join("Cargo.toml").is_file() && root.join("crates/nage-core/Cargo.toml").is_file(),
            "{} is not the workspace root",
            root.display()
        );
        root
    }

    /// Asserts that `file` maps to the absolute path `expected` under the workspace root.
    #[track_caller]
    fn assert_maps(file: &str, expected: &str) {
        let path = regression_path(file);
        assert!(
            path.is_absolute(),
            "regression_path({file}) is not absolute: {}",
            path.display()
        );
        let root = workspace_root();
        let suffix = path.strip_prefix(&root).unwrap_or_else(|_| {
            panic!(
                "regression_path({file}) = {} lies outside the workspace root {}",
                path.display(),
                root.display()
            )
        });
        assert_eq!(
            suffix,
            Path::new(expected),
            "regression_path({file}) under the workspace root"
        );
    }

    /// `regression_path` mirrors the source tree under each crate's `proptest-regressions/`,
    /// absolute under the workspace root: a `src/` file, a `tests/` file and a nested file of
    /// another crate. `proptest_config(file!())` persists failures directly to this file's
    /// regression path and runs `PROPTEST_CASES` cases when that variable is set and parses
    /// (the test reads it and never sets it), else 256.
    ///
    /// Proves: decision 61.
    #[test]
    fn regression_path_mirrors_the_source_tree() {
        assert_maps(
            "crates/nage-core/src/storage.rs",
            "crates/nage-core/proptest-regressions/src/storage.txt",
        );
        assert_maps(
            "crates/nage-core/tests/props.rs",
            "crates/nage-core/proptest-regressions/tests/props.txt",
        );
        assert_maps(
            "crates/nage-web/src/import/fingerprint.rs",
            "crates/nage-web/proptest-regressions/src/import/fingerprint.txt",
        );

        let config = proptest_config(file!());
        let expected =
            workspace_root().join("crates/nage-core/proptest-regressions/src/test_support.txt");
        let persistence = config
            .failure_persistence
            .as_ref()
            .unwrap_or_else(|| panic!("proptest_config(file!()) sets no failure persistence"));
        match persistence
            .as_any()
            .downcast_ref::<FileFailurePersistence>()
        {
            Some(FileFailurePersistence::Direct(path)) => assert_eq!(
                Path::new(path),
                expected.as_path(),
                "the Direct failure persistence of proptest_config(file!())"
            ),
            other => panic!(
                "proptest_config(file!()) persists failures through {other:?}, not FileFailurePersistence::Direct"
            ),
        }

        let cases = std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(256);
        assert_eq!(
            config.cases, cases,
            "proptest_config(file!()).cases (PROPTEST_CASES or 256)"
        );
    }

    /// `assert_no_side_files` fails when `<db>-wal` or `<db>-shm` exists beside the database
    /// path, naming that file, and returns when neither does: the callers that assert a clean
    /// close left no side files rely on it being able to fail. Each file is created empty and
    /// removed again before the next case.
    ///
    /// Proves: decision 59.
    #[test]
    fn assert_no_side_files_fails_when_a_side_file_exists() {
        let dir = TestDir::new();
        let db = dir.db_path();
        for suffix in ["-wal", "-shm"] {
            let side = dir.path().join(format!("nage.sqlite{suffix}"));
            std::fs::write(&side, b"")
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", side.display()));
            let outcome =
                std::panic::catch_unwind(|| assert_no_side_files(&db, "in the self-test"));
            std::fs::remove_file(&side)
                .unwrap_or_else(|error| panic!("cannot remove {}: {error}", side.display()));
            let payload = match outcome {
                Ok(()) => panic!(
                    "assert_no_side_files returned although {} exists",
                    side.display()
                ),
                Err(payload) => payload,
            };
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or_else(|| panic!("assert_no_side_files panicked without a message"));
            let expected = format!(
                "the SQLite side file {} exists in the self-test",
                side.display()
            );
            assert_eq!(
                message, expected,
                "the message of assert_no_side_files with {suffix} present"
            );
        }
        assert_no_side_files(&db, "in the self-test with neither side file present");
    }

    /// The tables `book_rows` reads are exactly the tables a freshly migrated database holds
    /// (asked of `sqlite_schema`, so a table a later migration adds fails here until it is read);
    /// two databases set up alike at the same instant, each with a second user, give the same
    /// rows although their password hashes and sqlx's bookkeeping times differ; every table read
    /// has a line, none of them shows a hash or `installed_on`, and a value a row holds is shown:
    /// each posting's line is pinned whole, its amount with its sign.
    ///
    /// Proves: decision 161, ruling P1 (each table read by a checked query of its own; the tables read equal the tables the schema holds; hashes and bookkeeping times left out)
    #[tokio::test]
    async fn book_rows_reads_every_table_the_schema_holds() {
        let mut read: Vec<&str> = super::tables_read().to_vec();
        read.sort_unstable();

        let mut books = Vec::new();
        for _ in 0..2 {
            let (dir, storage) = super::open_migrated().await;
            let now = crate::timestamp::Timestamp::from_second(1_794_225_600)
                .unwrap_or_else(|error| panic!("the test instant: {error:?}"));
            let initialized = super::set_up(&storage, now, "sam", "correct horse battery").await;
            super::add_user(
                &storage,
                initialized.admin.id,
                now,
                "alex",
                "another long password",
                false,
            )
            .await;
            // A session and a transaction with its postings, through the core's operations, so
            // that every table read holds a row.
            let mut tx = storage
                .begin_write()
                .await
                .unwrap_or_else(|error| panic!("cannot begin: {error}"));
            crate::sessions::create_session(
                &mut tx,
                initialized.admin.id,
                now,
                &crate::sessions::TokenHash::from_digest([7; 32]),
            )
            .await
            .unwrap_or_else(|error| panic!("cannot create a session: {error}"));
            let on: crate::date::Date = "2026-11-09"
                .parse()
                .unwrap_or_else(|error| panic!("the test date: {error:?}"));
            crate::transactions::create_transaction(
                &mut tx,
                initialized.admin.id,
                now,
                &crate::transactions::NewTransaction {
                    date: on,
                    postings: vec![
                        crate::transactions::NewPosting {
                            account: crate::accounts::AccountId(9),
                            amount: crate::money::Money::from_minor(-4500),
                        },
                        crate::transactions::NewPosting {
                            account: crate::accounts::AccountId(8),
                            amount: crate::money::Money::from_minor(4500),
                        },
                    ],
                },
            )
            .await
            .unwrap_or_else(|error| panic!("cannot create a transaction: {error}"));
            tx.commit()
                .await
                .unwrap_or_else(|error| panic!("cannot commit: {error}"));
            let mut conn = storage
                .reader()
                .acquire()
                .await
                .unwrap_or_else(|error| panic!("cannot acquire a reader: {error}"));
            // A schema question in unchecked SQL, a literal (decision 57). SQLite's own tables
            // (`sqlite_sequence`) are not the schema's and are left out.
            let schema: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
            )
            .fetch_all(&mut *conn)
            .await
            .unwrap_or_else(|error| panic!("cannot list the tables: {error}"));
            drop(conn);
            assert_eq!(
                read, schema,
                "the tables book_rows reads and the tables the schema holds"
            );
            storage.close().await;
            let rows = super::book_rows(&dir.db_path()).await;
            books.push((dir, rows));
        }
        let (_, first) = &books[0];
        let (_, second) = &books[1];
        assert_eq!(first, second, "the rows of two books set up alike");
        for table in &read {
            assert!(
                first
                    .iter()
                    .any(|line| line.starts_with(&format!("{table} "))),
                "no line of the table {table} in {first:#?}"
            );
        }
        for line in first {
            assert!(
                !line.contains("argon2") && !line.contains("password_hash"),
                "a password hash is shown: {line}"
            );
            assert!(
                !line.contains("installed_on") && !line.contains("execution_time"),
                "a bookkeeping time is shown: {line}"
            );
        }
        assert!(
            first.iter().any(|line| line.starts_with("users ")
                && line.contains("alex")
                && line.contains("2026-11-09T12:00:00Z")),
            "the user alex is not shown with its creation time: {first:#?}"
        );
        assert!(
            first
                .iter()
                .any(|line| line.starts_with("book ") && line.contains("Household")),
            "the book is not shown with its name: {first:#?}"
        );
        // Each posting's line whole, its amount as it is stored, sign and all.
        for posting in [
            "postings id=1 transaction_id=1 account_id=9 amount=-4500",
            "postings id=2 transaction_id=1 account_id=8 amount=4500",
        ] {
            assert!(
                first.iter().any(|line| line == posting),
                "no line {posting:?} in {first:#?}"
            );
        }
    }

    /// A `TestDir` exists at its canonical path and its `db_path` is `nage.sqlite` inside it;
    /// dropping it removes the directory, but a thread that panics while holding one leaves
    /// the directory behind for inspection (the test removes it afterwards).
    ///
    /// Proves: decision 59.
    #[test]
    fn test_dir_is_canonical_removed_on_drop_and_kept_on_panic() {
        let dir = TestDir::new();
        let path = dir.path().to_path_buf();
        assert!(path.is_dir(), "{} does not exist", path.display());
        let canonical = std::fs::canonicalize(&path)
            .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", path.display()));
        assert_eq!(path, canonical, "the TestDir path is not canonical");
        assert_eq!(
            dir.db_path(),
            path.join("nage.sqlite"),
            "the TestDir database path"
        );
        drop(dir);
        assert!(
            !path.exists(),
            "{} still exists after the TestDir was dropped",
            path.display()
        );

        let (sender, receiver) = mpsc::channel();
        let joined = std::thread::spawn(move || {
            let dir = TestDir::new();
            sender
                .send(dir.path().to_path_buf())
                .unwrap_or_else(|error| panic!("cannot send the TestDir path: {error}"));
            panic!("deliberate panic while holding a TestDir");
        })
        .join();
        let kept = receiver
            .recv()
            .unwrap_or_else(|error| panic!("the panicking thread sent no path: {error}"));
        assert!(
            joined.is_err(),
            "the thread holding the TestDir did not panic"
        );
        let survived = kept.is_dir();
        if survived {
            std::fs::remove_dir_all(&kept)
                .unwrap_or_else(|error| panic!("cannot remove {}: {error}", kept.display()));
        }
        assert!(
            survived,
            "{} was removed although its thread panicked",
            kept.display()
        );
    }
}
