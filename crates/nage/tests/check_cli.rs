//! `nage check` on the command line (SPEC 5.2, W15, decisions 97 and 157): `Success: Nothing to
//! check: no book is set up yet.` on a fresh database, `Success: The book passed every check.` on a
//! set-up book, a book that breaks rules as `Error:`, a bare `Why:` with its lines by id and `What
//! now:`, exit 1, the refusals of a missing database and of a database behind or ahead of this
//! binary in three labelled lines, and the data folder from the flag, the variable and the
//! default.

mod common;

use std::fs;
use std::path::Path;

use nage_core::currency::Currency;
use nage_core::setup::{Setup, initialize};
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files, fixture_copy};
use nage_core::timestamp::Timestamp;
use nage_core::users::NewUser;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};

use common::commands::{BOOK_NOT_CHECKED, NOTHING_TO_CHECK, check_passed, database_newer, refused};
use common::transcript::transcript;
use common::{Finished, nage, run};

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
    database_in(&dir.path().join("data"), set_up);
    dir
}

/// Creates the data folder `data` with `nage.sqlite` migrated and, when asked, set up with the
/// standard book, as [`data_dir`] leaves its `data/`.
fn database_in(data: &Path, set_up: bool) {
    fs::create_dir_all(data)
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

/// The database file of the working directory `dir`'s data folder as `nage check` prints it:
/// absolute, the working directory as the system names it.
fn printed(dir: &Path, data: &str) -> String {
    let dir = fs::canonicalize(dir)
        .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", dir.display()));
    dir.join(data).join("nage.sqlite").display().to_string()
}

/// Asserts exit 1, nothing on stdout and exactly `stderr` on stderr.
#[track_caller]
fn assert_refused(out: &Finished, stderr: &str) {
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert_eq!(out.stderr, stderr, "stderr of `{}`", out.what);
    assert!(
        out.stdout.is_empty(),
        "stdout of `{}` is not empty:\n{}",
        out.what,
        out.stdout
    );
}

/// What `nage check` prints for a book that breaks rules: the count, the lines under a bare
/// `Why:`, and what now.
fn broken(lines: &[&str]) -> String {
    let count = if lines.len() == 1 {
        String::from("1 problem was found")
    } else {
        format!("{} problems were found", lines.len())
    };
    let mut text = format!("Error: The book did not pass every check: {count}.\nWhy:\n");
    for line in lines {
        text.push_str(&format!("  {line}\n"));
    }
    text.push_str("What now: Nothing was changed. Keep a copy of the data folder as it is, and keep the lines above: the deployment guide says where to report them.\n");
    text
}

/// What `nage check` prints for a database behind this binary, at `version`.
fn behind_lines(database: &str, version: i64) -> String {
    refused(
        BOOK_NOT_CHECKED,
        &format!(
            "The database at {database} was left by an older version of Nage: it is at version {version}, and this Nage expects 2."
        ),
        "Update it with nage migrate, or start the server, which updates it itself, then run nage check again.",
    )
}

/// What `nage check` prints where the data folder holds no database.
fn missing_lines(database: &str) -> String {
    refused(
        BOOK_NOT_CHECKED,
        &format!("There is no database at {database}."),
        "Check the folder given with --data-dir or NAGE_DATA_DIR; a new book is set up with nage init.",
    )
}

/// A migrated database without a book row: nothing to check, exit 0, nothing on stderr.
///
/// Proves: SPEC 5.2 ("not initialized" with exit 0), decisions 97 and 157.
#[test]
fn check_reports_not_initialized_with_exit_0() {
    let dir = data_dir(false);
    assert_ok(
        &check(dir.path(), &["--data-dir", "data"], &[]),
        NOTHING_TO_CHECK,
    );
}

/// A set-up book passes every check: the sentence, the ten rules and the database, exit 0,
/// nothing on stderr; the database is closed cleanly, no `-wal` or `-shm` file left beside it.
///
/// Proves: W15 (check from the command line), decisions 97 and 157, decision 53 (a clean close).
#[test]
fn check_reports_ok_on_a_set_up_book() {
    let dir = data_dir(true);
    assert_ok(
        &check(dir.path(), &["--data-dir", "data"], &[]),
        &check_passed(&printed(dir.path(), "data")),
    );
    assert_no_side_files(
        &dir.path().join("data").join("nage.sqlite"),
        "after nage check",
    );
}

/// Two planted violations (a colon in two names) print one `INV-10` line each, in the order
/// found, under a bare `Why:` on stderr, after the count of problems; the run exits 1 and writes
/// nothing on stdout.
///
/// Proves: W15 (violations by id, non-zero exit), decision 45 (exit 1), decisions 97 and 157
/// (one line per violation under the count).
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
    let lines: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(lines.len(), 5, "stderr of `{}`:\n{}", out.what, out.stderr);
    assert_eq!(
        (lines[0], lines[1]),
        (
            "Error: The book did not pass every check: 2 problems were found.",
            "Why:"
        ),
        "the first two lines of `{}`",
        out.what
    );
    assert!(
        lines[2].starts_with("  INV-10 ") && lines[2].contains("Food:Fruit"),
        "the first violation line of `{}`: {}",
        out.what,
        lines[2]
    );
    assert!(
        lines[3].starts_with("  INV-10 ") && lines[3].contains("Eat:Out"),
        "the second violation line of `{}`: {}",
        out.what,
        lines[3]
    );
    assert!(
        lines[4].starts_with("What now: "),
        "the last line of `{}`: {}",
        out.what,
        lines[4]
    );
    assert!(
        out.stdout.is_empty(),
        "stdout of `{}` is not empty:\n{}",
        out.what,
        out.stdout
    );
}

/// A data folder without `nage.sqlite`, or no data folder at all, is refused with three lines
/// naming the path and `nage init`, exit 1, and nothing is created.
///
/// Proves: decision 97 (check never creates the database), decision 45 (exit 1), decision 157.
#[test]
fn check_refuses_a_missing_database_and_creates_nothing() {
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_refused(&out, &missing_lines(&printed(dir.path(), "data")));
    assert!(
        !data.join("nage.sqlite").exists(),
        "check created the database"
    );
    let entries = fs::read_dir(&data)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", data.display()))
        .count();
    assert_eq!(entries, 0, "check wrote into the data folder");
    let out = check(dir.path(), &["--data-dir", "missing"], &[]);
    assert_refused(&out, &missing_lines(&printed(dir.path(), "missing")));
    assert!(
        !dir.path().join("missing").exists(),
        "check created the data folder"
    );
}

/// A database at schema version 1 (a copy of the 0.1.0 fixture) is refused with three lines
/// naming both versions, `nage migrate` and the server's start, exit 1, and is not migrated; a
/// bookkeeping table without rows is behind at version 0.
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
    assert_refused(&out, &behind_lines(&printed(dir.path(), "data"), 1));
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

    // A database whose bookkeeping records no migration at all is behind at version 0.
    let dir = data_dir(true);
    exec(
        &dir.path().join("data").join("nage.sqlite"),
        &["DELETE FROM _sqlx_migrations"],
    );
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_refused(&out, &behind_lines(&printed(dir.path(), "data"), 0));
}

/// A database recording a migration this binary does not carry (change 3, the next after this
/// version's) is refused as written by a newer Nage, with three lines naming the change it carries
/// and the last this version knows, each in four digits, in the sentence every command shares,
/// exit 1.
///
/// Proves: decision 97, decision 54 (the newer-database message).
#[test]
fn check_refuses_a_database_written_by_a_newer_nage() {
    let dir = data_dir(true);
    exec(
        &dir.path().join("data").join("nage.sqlite"),
        &[
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (3, 'from the future', 1, X'00', 0)",
        ],
    );
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_refused(&out, &database_newer(BOOK_NOT_CHECKED, 3));
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
    let resolved = fs::canonicalize(dir.path())
        .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", dir.path().display()));
    let data = resolved.join("data").display().to_string();
    let passed = check_passed(&printed(dir.path(), "data"));
    assert_ok(&check(dir.path(), &[], &[]), &passed);
    let elsewhere = TestDir::new();
    assert_ok(
        &check(elsewhere.path(), &[], &[("NAGE_DATA_DIR", data.as_str())]),
        &passed,
    );
    assert_ok(
        &check(
            elsewhere.path(),
            &["--data-dir", data.as_str()],
            &[("NAGE_DATA_DIR", "nowhere")],
        ),
        &passed,
    );
    assert_ok(&check(dir.path(), &[], &[("NAGE_DATA_DIR", "")]), &passed);
    let out = check(elsewhere.path(), &[], &[("NAGE_DATA_DIR", "nowhere")]);
    assert_refused(&out, &missing_lines(&printed(elsewhere.path(), "nowhere")));
}

/// A book that breaks several rules at once (two names with a colon, a second opening balance on
/// Assets:Checking, and the account Groceries deleted under a posting with foreign keys off)
/// prints every line whole, word for word as nage-core writes it: the INV-10, INV-13 and FK
/// lines under a bare `Why:`, after the count of problems, then what now; exit 1, nothing on
/// stdout. A book that breaks exactly one rule (only the second opening balance) is counted as
/// "1 problem was found", its line pinned whole, exit 1.
///
/// Proves: decision 97 (every INV and FK line printed whole), inherited gap s5-final-cli-r2-3
#[test]
fn check_lines_are_pinned_whole() {
    let dir = data_dir(true);
    let path = dir.path().join("data").join("nage.sqlite");
    let opening = [
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('2026-01-01', '', '', 'opening_balance', 1, 1, '2026-11-09T12:00:00Z', 1, '2026-11-09T12:00:00Z')",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Checking'), 100000)",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Opening Balances'), -100000)",
    ];
    let groceries = [
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('2026-11-09', '', '', 'manual', 1, 1, '2026-11-09T12:00:00Z', 1, '2026-11-09T12:00:00Z')",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Checking'), -4500)",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Groceries'), 4500)",
    ];
    let mut statements: Vec<&str> =
        vec!["UPDATE accounts SET name = 'Eat:Out', name_folded = 'eat:out' WHERE name = 'Dining'"];
    statements.extend_from_slice(&groceries);
    statements.extend_from_slice(&opening);
    statements.extend_from_slice(&opening);
    statements.push("DELETE FROM accounts WHERE name = 'Groceries'");
    exec(&path, &statements);
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_refused(
        &out,
        &broken(&[
            "INV-10 account 16 (Expenses:Eat:Out) has a colon in its name",
            "INV-13 transaction 3: a second opening balance for account 9 (Assets:Checking)",
            "FK postings row 2 references a missing accounts row",
        ]),
    );

    // A book that breaks exactly one rule: a second opening balance on Assets:Checking. One
    // problem is counted in the singular, and the book does not pass.
    let dir = data_dir(true);
    let mut statements: Vec<&str> = Vec::new();
    statements.extend_from_slice(&opening);
    statements.extend_from_slice(&opening);
    exec(&dir.path().join("data").join("nage.sqlite"), &statements);
    let out = check(dir.path(), &["--data-dir", "data"], &[]);
    assert_refused(
        &out,
        &format!(
            "Error: The book did not pass every check: 1 problem was found.\nWhy:\n  INV-13 transaction 2: a second opening balance for account 9 (Assets:Checking)\nWhat now: {}\n",
            "Nothing was changed. Keep a copy of the data folder as it is, and keep the lines above: the deployment guide says where to report them."
        ),
    );
}

/// A data folder whose name holds a line break is shown on one line, the break escaped as
/// `\n`, in each of `nage check`'s three lines; holding a book that passes, the folder's database
/// fact stays on one line too.
///
/// Proves: decision 157, ruling P10 (a data folder whose name holds a line break, each part one
/// line)
#[test]
fn check_shows_a_data_folder_with_a_line_break_on_one_line() {
    let dir = TestDir::new();
    let mut command = nage();
    command
        .args(["check", "--data-dir"])
        .arg(dir.path().join("two\nlines"))
        .current_dir(dir.path());
    let shown = transcript(&command).path(dir.path(), "/srv/nage");
    let out = run(&mut command);
    assert_eq!(
        out.stderr.lines().count(),
        3,
        "stderr of `{}`:\n{}",
        out.what,
        out.stderr
    );
    let transcript = shown.of(&out);
    insta::assert_snapshot!("data_folder_with_a_line_break", transcript);

    // The same folder holding a book that passes: its Database fact stays on one line.
    database_in(&dir.path().join("two\nlines"), true);
    let shown = common::transcript::transcript(&command).path(dir.path(), "/srv/nage");
    let out = run(&mut command);
    assert_eq!(
        out.stdout.lines().count(),
        3,
        "stdout of `{}`:\n{}",
        out.what,
        out.stdout
    );
    let passed = shown.of(&out);
    insta::assert_snapshot!("data_folder_with_a_line_break_passed", passed);
}
