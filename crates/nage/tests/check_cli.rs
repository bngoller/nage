//! `nage check` on the command line (SPEC 5.2, W15, decision 97): "not initialized" on a fresh
//! database, `ok` on a set-up book, violations by id with exit 1, the refusals of a missing
//! database and of a database behind or ahead of this binary, and the data directory from the
//! flag, the variable and the default.

mod common;

use std::fs;
use std::path::Path;

use nage_core::currency::Currency;
use nage_core::setup::{Setup, initialize};
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, fixture_copy};
use nage_core::timestamp::Timestamp;
use nage_core::users::NewUser;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};

use common::{Finished, assert_runtime_error, nage, run};

/// Runs `future` on a fresh current-thread runtime.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"))
        .block_on(future)
}

/// A working directory with `data/nage.sqlite` migrated and, when asked, set up with the
/// standard book, written through the core and closed cleanly.
fn data_dir(set_up: bool) -> TestDir {
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    block_on(async {
        let storage = Storage::open(&data.join("nage.sqlite"), &StorageOptions::default())
            .await
            .unwrap_or_else(|error| panic!("cannot open the test database: {error}"));
        storage
            .migrate()
            .await
            .unwrap_or_else(|error| panic!("cannot migrate the test database: {error}"));
        if set_up {
            let mut tx = storage
                .begin_write()
                .await
                .unwrap_or_else(|error| panic!("cannot begin the setup: {error}"));
            let now = Timestamp::from_second(1_794_225_600)
                .unwrap_or_else(|error| panic!("the test instant: {error}"));
            initialize(
                &mut tx,
                now,
                &Setup {
                    book_name: String::from("Household"),
                    currency: Currency::from_code("CAD")
                        .unwrap_or_else(|| panic!("CAD is not in the currency table")),
                    admin: NewUser {
                        username: String::from("admin"),
                        display_name: String::from("Admin"),
                        password_hash: String::from("hash"),
                        is_admin: true,
                    },
                    starter_chart: true,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("cannot set up the test book: {error}"));
            tx.commit()
                .await
                .unwrap_or_else(|error| panic!("cannot commit the setup: {error}"));
        }
        storage.close().await;
    });
    dir
}

/// Runs `statements` on the database at `path` through a plain connection and closes it.
fn exec(path: &Path, statements: &[&str]) {
    block_on(async {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .foreign_keys(false),
        )
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        for sql in statements {
            sqlx::query(sqlx::AssertSqlSafe((*sql).to_owned()))
                .execute(&mut conn)
                .await
                .unwrap_or_else(|error| panic!("cannot execute `{sql}`: {error}"));
        }
        conn.close()
            .await
            .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
    });
}

/// `nage check` with `args`, run in `cwd` with the variables `envs` and nothing else.
fn check(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Finished {
    let mut command = nage();
    command.arg("check").args(args).current_dir(cwd);
    for (key, value) in envs {
        command.env(key, value);
    }
    run(&mut command)
}

/// Asserts exit 0, an empty stderr and exactly `stdout` on stdout.
#[track_caller]
fn assert_ok(out: &Finished, stdout: &str) {
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert_eq!(out.stdout, stdout, "stdout of `{}`", out.what);
    assert!(
        out.stderr.is_empty(),
        "stderr of `{}` is not empty:\n{}",
        out.what,
        out.stderr
    );
}

/// A migrated database without a book row: `check: not initialized`, exit 0, nothing on stderr.
///
/// Proves: SPEC 5.2 ("not initialized" with exit 0), decision 97.
#[test]
fn check_reports_not_initialized_with_exit_0() {
    let dir = data_dir(false);
    assert_ok(
        &check(dir.path(), &["--data-dir", "data"], &[]),
        "check: not initialized\n",
    );
}

/// A set-up book: `check: ok (10 invariants)`, exit 0, nothing on stderr.
///
/// Proves: W15 (check from the command line), decision 97.
#[test]
fn check_reports_ok_on_a_set_up_book() {
    let dir = data_dir(true);
    assert_ok(
        &check(dir.path(), &["--data-dir", "data"], &[]),
        "check: ok (10 invariants)\n",
    );
}

/// Two planted violations (a colon in two names) print one `INV-10` line each, in the order
/// found, and then `check: 2 violation(s)` on stdout; the run exits 1 and writes nothing on
/// stderr.
///
/// Proves: W15 (violations by id, non-zero exit), decision 45 (exit 1), decision 97 (one line
/// per violation, then the summary line).
#[test]
fn check_reports_violations_by_id_with_exit_1() {
    let dir = data_dir(true);
    exec(
        &dir.path().join("data").join("nage.sqlite"),
        &[
            "UPDATE accounts SET name = 'Food:Fruit', name_folded = 'food:fruit' WHERE name = 'Groceries'",
            "UPDATE accounts SET name = 'Eat:Out', name_folded = 'eat:out' WHERE name = 'Dining'",
        ],
    );
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 3, "stdout of `{}`:\n{}", out.what, out.stdout);
    assert!(
        lines[0].starts_with("INV-10 ") && lines[0].contains("Food:Fruit"),
        "the first violation line of `{}`: {}",
        out.what,
        lines[0]
    );
    assert!(
        lines[1].starts_with("INV-10 ") && lines[1].contains("Eat:Out"),
        "the second violation line of `{}`: {}",
        out.what,
        lines[1]
    );
    assert_eq!(lines[2], "check: 2 violation(s)");
    assert!(
        out.stderr.is_empty(),
        "stderr of `{}` is not empty:\n{}",
        out.what,
        out.stderr
    );
}

/// A data directory without `nage.sqlite`, or no data directory at all, is refused with one
/// `nage: ` line naming the path and `nage serve`, exit 1, and nothing is created.
///
/// Proves: decision 97 (check never creates the database), decision 45 (exit 1).
#[test]
fn check_refuses_a_missing_database_and_creates_nothing() {
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_runtime_error(&out, "nage: no database at ", "nage serve");
    assert_eq!(
        out.stderr.lines().count(),
        1,
        "stderr of `{}`:\n{}",
        out.what,
        out.stderr
    );
    assert!(
        out.stderr
            .contains(&data.join("nage.sqlite").display().to_string()),
        "the error does not name the path: {}",
        out.stderr
    );
    assert!(
        out.stdout.is_empty(),
        "stdout of `{}`: {}",
        out.what,
        out.stdout
    );
    assert!(
        !data.join("nage.sqlite").exists(),
        "check created the database"
    );
    let entries = fs::read_dir(&data)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", data.display()))
        .count();
    assert_eq!(entries, 0, "check wrote into the data directory");
    let out = check(dir.path(), &["--data-dir", "missing"], &[]);
    assert_runtime_error(&out, "nage: no database at ", "nage serve");
    assert_eq!(
        out.stderr.lines().count(),
        1,
        "stderr of `{}`:\n{}",
        out.what,
        out.stderr
    );
    assert!(
        !dir.path().join("missing").exists(),
        "check created the data directory"
    );
}

/// A database at schema version 1 (a copy of the 0.1.0 fixture) is refused with a line naming
/// both versions and `nage serve`, exit 1, and is not migrated.
///
/// Proves: decision 97 (check never migrates: a database behind this binary keeps its version).
#[test]
fn check_refuses_a_database_behind_this_version_without_migrating_it() {
    let (fixture_dir, copy) = fixture_copy("0.1.0");
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let path = data.join("nage.sqlite");
    fs::copy(copy, &path).unwrap_or_else(|error| panic!("cannot copy the fixture copy: {error}"));
    drop(fixture_dir);
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_runtime_error(&out, "nage: ", "schema version 1");
    assert!(
        out.stderr.contains("expects 2") && out.stderr.contains("nage serve"),
        "the error does not name the expected version and the remedy: {}",
        out.stderr
    );
    let version: i64 = block_on(async {
        let mut conn = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        let version = sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&mut conn)
            .await
            .unwrap_or_else(|error| panic!("cannot read the schema version: {error}"));
        conn.close()
            .await
            .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
        version
    });
    assert_eq!(version, 1, "check migrated the database");
}

/// A database recording a migration this binary does not carry is refused as written by a
/// newer Nage, exit 1.
///
/// Proves: decision 97, decision 54 (the newer-database message).
#[test]
fn check_refuses_a_database_written_by_a_newer_nage() {
    let dir = data_dir(true);
    exec(
        &dir.path().join("data").join("nage.sqlite"),
        &[
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'from the future', 1, X'00', 0)",
        ],
    );
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_runtime_error(&out, "nage: ", "newer Nage");
    assert!(
        out.stderr.contains("9999"),
        "the error does not name the migration: {}",
        out.stderr
    );
}

/// The data directory comes from `--data-dir`, else `NAGE_DATA_DIR`, else `./data` relative to
/// the working directory; the flag wins over the variable, and an empty variable counts as
/// unset.
///
/// Proves: decision 42 (flag over variable over default; an empty variable is unset), decision
/// 97.
#[test]
fn check_reads_the_data_dir_from_the_flag_the_variable_and_the_default() {
    let dir = data_dir(true);
    let data = dir.path().join("data").display().to_string();
    assert_ok(&check(dir.path(), &[], &[]), "check: ok (10 invariants)\n");
    let elsewhere = TestDir::new();
    assert_ok(
        &check(elsewhere.path(), &[], &[("NAGE_DATA_DIR", data.as_str())]),
        "check: ok (10 invariants)\n",
    );
    assert_ok(
        &check(
            elsewhere.path(),
            &["--data-dir", data.as_str()],
            &[("NAGE_DATA_DIR", "nowhere")],
        ),
        "check: ok (10 invariants)\n",
    );
    assert_ok(
        &check(dir.path(), &[], &[("NAGE_DATA_DIR", "")]),
        "check: ok (10 invariants)\n",
    );
    let out = check(elsewhere.path(), &[], &[("NAGE_DATA_DIR", "nowhere")]);
    assert_runtime_error(&out, "nage: no database at ", "nowhere");
}
