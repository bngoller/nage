//! What the process tests of `nage init`, `nage user add` and `nage user reset-password` need
//! (decisions 113 and 115): the password on standard input through a pipe, a run without a
//! terminal, data directories in the states the commands meet (built through the core), what
//! the database holds afterwards (read through the core), and the lines the commands word
//! themselves. The real terminal is not driven here: the prompt's logic is tested in the
//! binary's own unit tests through a reader standing in for it, and the terminal itself is
//! tried by hand.

use std::fs;
use std::future::Future;
use std::io::{PipeWriter, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nage_core::accounts;
use nage_core::book::{self, Book};
use nage_core::money::Money;
use nage_core::password::{self, Verification};
use nage_core::sessions::{self, TokenHash};
use nage_core::setup::Initialized;
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{self, TestDir, fixture_copy};
use nage_core::timestamp::Timestamp;
use nage_core::transactions::{self, NewPosting, NewTransaction};
use nage_core::users::{self, Credentials, User, UserId};
use nage_core::words::AtTheTerminal;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Connection as _, SqliteConnection};

use super::web::{ADMIN, PASSWORD};
use super::{Finished, nage, run};

/// A password the rules refuse: nine characters.
pub const TOO_SHORT: &str = "nine char";

/// What `nage init` says did not happen, the line after `Error:` of each of its refusals.
pub const BOOK_NOT_SET_UP: &str = "The book was not set up.";
/// What `nage user add` says did not happen.
pub const USER_NOT_CREATED: &str = "The user was not created.";
/// What `nage user reset-password` says did not happen.
pub const PASSWORD_NOT_CHANGED: &str = "The password was not changed.";
/// What `nage check` says did not happen when it cannot check the database.
pub const BOOK_NOT_CHECKED: &str = "The book was not checked.";

/// The three labelled lines of a refusal on standard error (decision 157), each ending with a
/// line break.
pub fn refused(what: &str, why: &str, now: &str) -> String {
    format!("Error: {what}\nWhy: {why}\nWhat now: {now}\n")
}

/// A refusal the core words, as the command `what` prints it at the terminal: the command's own
/// first line and the refusal's twin at the terminal (decision 150, the maintainer's pick "The
/// terminal's own words").
#[track_caller]
pub fn at_the_terminal(what: &str, words: Option<AtTheTerminal>) -> String {
    let words =
        words.unwrap_or_else(|| panic!("the refusal has no words at the terminal ({what})"));
    refused(what, &words.why, &words.now)
}

/// What `nage init` says without `--currency`.
pub fn currency_missing() -> String {
    refused(
        BOOK_NOT_SET_UP,
        "No currency was given, and a book's currency never changes once the book is set up.",
        "Add --currency and the code of the book's currency, for example --currency CAD, then run the command again.",
    )
}

/// What `nage init` says about a currency code that is not in the table, the code quoted as
/// Rust quotes a string.
pub fn unknown_currency(typed: &str) -> String {
    refused(
        BOOK_NOT_SET_UP,
        &format!(
            "{typed:?} is not a currency code Nage knows; a code is three capital letters, such as CAD, EUR or USD."
        ),
        "Give the code of the book's currency with --currency, then run the command again.",
    )
}

/// What a command says when it has no terminal to ask on and no `--password-stdin`.
pub fn no_terminal(not_done: &str) -> String {
    refused(
        not_done,
        "The password could not be asked for: this command is not running in a terminal, and --password-stdin was not given.",
        "Run the command in a terminal, or add --password-stdin and send the password on standard input.",
    )
}

/// What a command says when `--password-stdin` brings nothing.
pub fn no_password_on_stdin(not_done: &str) -> String {
    refused(
        not_done,
        "--password-stdin was given and no password arrived on standard input.",
        "Send the password on standard input, for example through a pipe, or leave --password-stdin out and type the password at the terminal.",
    )
}

/// What a command says when standard input is not UTF-8.
pub fn password_not_text(not_done: &str) -> String {
    refused(
        not_done,
        "The password on standard input is not valid UTF-8 text, so Nage cannot tell which characters it holds.",
        "Send the password as UTF-8 text, then run the command again.",
    )
}

/// What `nage user add` and `nage user reset-password` say where the data folder holds no
/// database, in the words `nage check` says it (one sentence per situation); `database` is the
/// path as the command prints it.
pub fn no_database(not_done: &str, database: &str) -> String {
    refused(
        not_done,
        &format!("There is no database at {database}."),
        "Check the folder given with --data-dir or NAGE_DATA_DIR; a new book is set up with nage init.",
    )
}

/// What `nage init`, `nage user add` and `nage user reset-password` say about a database behind
/// this binary, in the sentence `nage check` says it.
pub fn database_behind(not_done: &str, database: &str, version: i64, expects: i64) -> String {
    refused(
        not_done,
        &format!(
            "The database at {database} was left by an older version of Nage: it is at version {version}, and this Nage expects {expects}."
        ),
        "Update it with nage migrate, or start the server, which updates it itself, then run the command again.",
    )
}

/// What `nage check`, `nage init`, `nage user add` and `nage user reset-password` say about a
/// database a newer Nage wrote, in the one sentence every command shares; `version` is the change
/// it carries, and this version knows up to 0002.
pub fn database_newer(not_done: &str, version: i64) -> String {
    refused(
        not_done,
        &format!(
            "A newer Nage wrote this database: it carries the change {version:04}, and this version knows up to 0002."
        ),
        "Run the command with the newer Nage.",
    )
}

/// What `nage user add` and `nage user reset-password` say on a database without a user.
pub fn not_set_up(not_done: &str) -> String {
    refused(
        not_done,
        "Nage has not been set up in this data folder yet, so it has no users.",
        "Set Nage up first, with nage init or on the set-up page, or name the right data folder with --data-dir.",
    )
}

/// What `nage user add` says when `--by` names nobody.
pub fn added_by_nobody(typed: &str) -> String {
    refused(
        USER_NOT_CREATED,
        &format!(
            "Nobody has the username {typed:?}, which --by named as the admin adding the user."
        ),
        "Check the spelling, or name another admin with --by, then run the command again.",
    )
}

/// What `nage user reset-password` says when `--by` names nobody.
pub fn reset_by_nobody(typed: &str) -> String {
    refused(
        PASSWORD_NOT_CHANGED,
        &format!(
            "Nobody has the username {typed:?}, which --by named as the admin resetting the password."
        ),
        "Check the spelling, or name another admin with --by, then run the command again.",
    )
}

/// What `nage user reset-password` says when `--username` names nobody.
pub fn unknown_user(typed: &str) -> String {
    refused(
        PASSWORD_NOT_CHANGED,
        &format!(
            "Nobody has the username {typed:?}, which --username named as the user whose password to reset."
        ),
        "Check the spelling, then run the command again.",
    )
}

/// What `nage check` prints when the book passes every check: the sentence, what was checked and
/// the database, as the command prints its path.
pub fn check_passed(database: &str) -> String {
    format!(
        "Success: The book passed every check.\n  Checked: 10 rules and the links between rows\n  Database: {database}\n"
    )
}

/// What `nage user add` prints when it added `username` (`display_name`) in the name of `by`.
pub fn user_added(username: &str, display_name: &str, by: &str) -> String {
    format!(
        "Success: {username} ({display_name}) was added.\n  Added by: {by}\n  Admin: no\nWhat now: Tell {username} the password; {username} can sign in now and change it afterwards.\n"
    )
}

/// What `nage user reset-password` prints when the admin `by` reset another user's password.
pub fn password_reset(username: &str, by: &str) -> String {
    format!(
        "Success: The password of {username} was reset.\n  Reset by: {by}\n  Signed out: {username}, on every device\nWhat now: Tell {username} the new password; {username} can change it after signing in.\n"
    )
}

/// What `nage user reset-password` prints when an admin reset their own password.
pub fn own_password_reset(username: &str) -> String {
    format!(
        "Success: The password of {username} was reset.\n  Reset by: {username}\n  Signed out: {username}, on every device\nWhat now: Sign in with the new password.\n"
    )
}

/// What `nage check` prints while no book is set up.
pub const NOTHING_TO_CHECK: &str = "Success: Nothing to check: no book is set up yet.\n";

/// Runs `command` to completion with `input` on its standard input, through a pipe whose
/// writing end is closed before the command starts, as `printf ... | nage ...` leaves it. The
/// input must fit the pipe's buffer; every password of the tests does.
#[track_caller]
pub fn run_with_stdin(command: &mut Command, input: &[u8]) -> Finished {
    assert!(
        input.len() <= 32 * 1024,
        "{} bytes do not fit a pipe's buffer",
        input.len()
    );
    let (reader, mut writer) =
        std::io::pipe().unwrap_or_else(|error| panic!("cannot make a pipe: {error}"));
    writer
        .write_all(input)
        .unwrap_or_else(|error| panic!("cannot fill the pipe: {error}"));
    drop(writer);
    command.stdin(reader);
    run(command)
}

/// The `nage` binary started in a session of its own (`setsid --wait`), with an empty
/// environment and stdin on `/dev/null`: whatever terminal the tests were started from, this
/// process has none to ask on.
pub fn nage_without_terminal() -> Command {
    let mut command = Command::new("setsid");
    command
        .arg("--wait")
        .arg(env!("CARGO_BIN_EXE_nage"))
        .env_clear()
        .stdin(Stdio::null());
    command
}

/// `nage <args>` in the working directory `cwd`, with `--password-stdin` when `stdin` holds
/// the password and with no way to ask for one otherwise (the process has no terminal).
#[track_caller]
pub fn command(cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Finished {
    match stdin {
        Some(input) => run_with_stdin(
            nage().args(args).arg("--password-stdin").current_dir(cwd),
            input,
        ),
        None => run(nage_without_terminal().args(args).current_dir(cwd)),
    }
}

/// Runs `future` on a fresh current-thread runtime.
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|error| panic!("cannot build a test runtime: {error}"))
        .block_on(future)
}

/// The system clock cut to the second, as the commands stamp what they write.
pub fn now() -> Timestamp {
    Timestamp::from_jiff(jiff::Timestamp::now())
        .unwrap_or_else(|error| panic!("the system clock is no instant of Nage: {error}"))
}

/// The instant the tests stamp their own scenario data with, 2026-11-09T12:00:00Z: before any
/// instant a command can stamp.
pub fn earlier() -> Timestamp {
    Timestamp::from_second(1_794_225_600)
        .unwrap_or_else(|error| panic!("the test instant: {error}"))
}

/// The database file of the data directory `data` in the working directory `dir`.
pub fn database(dir: &Path) -> PathBuf {
    dir.join("data").join("nage.sqlite")
}

/// The same path as the commands print it: absolute, the working directory as the system
/// names it.
pub fn printed_database(dir: &Path) -> String {
    let dir = fs::canonicalize(dir)
        .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", dir.display()));
    database(&dir).display().to_string()
}

/// Opens the database of `dir` through the core, without migrating it.
pub async fn open(dir: &Path) -> Storage {
    let path = database(dir);
    Storage::open(&path, &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()))
}

/// Creates `data/` in the working directory `dir` and in it a migrated database that holds no
/// book: what `nage serve` leaves behind when it was started once and nobody set Nage up.
pub fn migrate_in(dir: &Path) {
    let data = dir.join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    block_on(async {
        let storage = open(dir).await;
        test_support::migrate(&storage).await;
        storage.close().await;
    });
}

/// A working directory of its own in the state [`migrate_in`] leaves.
pub fn migrated() -> TestDir {
    let dir = TestDir::new();
    migrate_in(dir.path());
    dir
}

/// Leaves the working directory `dir` with a database set up through the core at [`earlier`]
/// (`test_support::set_up`): the book `Household` in CAD with the starter chart and the admin
/// [`ADMIN`], whose password is [`PASSWORD`].
pub fn set_up_in(dir: &Path) -> Initialized {
    migrate_in(dir);
    block_on(async {
        let storage = open(dir).await;
        let initialized = test_support::set_up(&storage, earlier(), ADMIN, PASSWORD).await;
        storage.close().await;
        initialized
    })
}

/// A working directory of its own in the state [`set_up_in`] leaves.
pub fn set_up() -> (TestDir, Initialized) {
    let dir = TestDir::new();
    let initialized = set_up_in(dir.path());
    (dir, initialized)
}

/// Adds the user `username` to the database of `dir` through the core at [`earlier`]
/// (`test_support::add_user`), in the name of the admin `by`.
pub fn add_user(dir: &Path, by: UserId, username: &str, password: &str, is_admin: bool) -> User {
    block_on(async {
        let storage = open(dir).await;
        let user =
            test_support::add_user(&storage, by, earlier(), username, password, is_admin).await;
        storage.close().await;
        user
    })
}

/// Signs `user` in on a device of their own: a session created through the core at
/// [`earlier`] under the token hash made of the byte `device`.
pub fn sign_in(dir: &Path, user: UserId, device: u8) -> TokenHash {
    let token = TokenHash::from_digest([device; 32]);
    block_on(async {
        let storage = open(dir).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin the sign-in: {error}"));
        sessions::create_session(&mut tx, user, earlier(), &token)
            .await
            .unwrap_or_else(|error| panic!("cannot create the session: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the sign-in: {error}"));
        storage.close().await;
    });
    token
}

/// A working directory whose database is a copy of the 0.1.0 fixture: at schema version 1,
/// behind this binary. The committed fixture is copied twice and never opened in place.
pub fn behind() -> TestDir {
    let dir = TestDir::new();
    behind_in(dir.path());
    dir
}

/// Leaves the working directory `dir` with `data/` and in it a copy of the 0.1.0 fixture, as
/// [`behind`] does.
pub fn behind_in(dir: &Path) {
    let (fixture_dir, copy) = fixture_copy("0.1.0");
    let data = dir.join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    fs::copy(copy, database(dir))
        .unwrap_or_else(|error| panic!("cannot copy the fixture copy: {error}"));
    drop(fixture_dir);
}

/// Runs `statements` on the database at `path` through a plain connection with foreign keys off,
/// creating the file when it is missing, and closes it: unchecked SQL that plants a corruption or
/// a file that is not Nage's (decision 57).
pub fn plant(path: &Path, statements: &[&str]) {
    block_on(async {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
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

/// Leaves the working directory `dir` with a book that breaks two rules, as `nage check`'s
/// transcripts plant it: set up through the core, a posting on Groceries written through the core,
/// then a second opening balance planted on Assets:Checking and the account Groceries deleted
/// under its posting, foreign keys off.
pub fn broken_book_in(dir: &Path) {
    let initialized = set_up_in(dir);
    block_on(async {
        let storage = open(dir).await;
        let mut read = storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
        let chart = accounts::accounts(&mut read)
            .await
            .unwrap_or_else(|error| panic!("cannot list the accounts: {error}"));
        drop(read);
        let id = |name: &str| {
            chart
                .iter()
                .find(|account| account.name == name)
                .map(|account| account.id)
                .unwrap_or_else(|| panic!("no account {name} in the starter accounts"))
        };
        let date = "2026-11-09"
            .parse()
            .unwrap_or_else(|error| panic!("the test date: {error:?}"));
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a write: {error}"));
        transactions::create_transaction(
            &mut tx,
            initialized.admin.id,
            earlier(),
            &NewTransaction {
                date,
                postings: vec![
                    NewPosting {
                        account: id("Checking"),
                        amount: Money::from_minor(-4500),
                    },
                    NewPosting {
                        account: id("Groceries"),
                        amount: Money::from_minor(4500),
                    },
                ],
            },
        )
        .await
        .unwrap_or_else(|error| panic!("cannot create the groceries: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the groceries: {error}"));
        storage.close().await;
    });
    let opening = [
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('2026-01-01', '', '', 'opening_balance', 1, 1, '2026-11-09T12:00:00Z', 1, '2026-11-09T12:00:00Z')",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Checking'), 100000)",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Opening Balances'), -100000)",
    ];
    let mut statements: Vec<&str> = Vec::new();
    statements.extend_from_slice(&opening);
    statements.extend_from_slice(&opening);
    statements.push("DELETE FROM accounts WHERE name = 'Groceries'");
    plant(&database(dir), &statements);
}

/// Writes at `path` the one page an init cut off after the open leaves: SQLite's header, written
/// by the switch to WAL, and no table (decision 155).
pub fn one_page_without_tables(path: &Path) {
    block_on(async {
        let conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal),
        )
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        conn.close()
            .await
            .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
    });
    let length = fs::metadata(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .len();
    assert!(
        length > 0,
        "{} holds no byte after the switch to WAL",
        path.display()
    );
}

/// Takes the in-use mark of the data folder `data` for the test, as another Nage would hold it:
/// the file `nage.lock` locked and holding `word` alone (decision 153). The mark is held while the
/// file lives.
pub fn hold_mark(data: &Path, word: &str) -> fs::File {
    let path = data.join("nage.lock");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    file.try_lock()
        .unwrap_or_else(|error| panic!("cannot lock {}: {error}", path.display()));
    file.set_len(0)
        .unwrap_or_else(|error| panic!("cannot empty {}: {error}", path.display()));
    file.write_all(word.as_bytes())
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    file
}

/// A working directory whose database records a migration 9999, as a newer Nage would have
/// left it (unchecked SQL on migration bookkeeping, decision 57).
pub fn ahead() -> TestDir {
    ahead_at(9999)
}

/// A working directory whose database records the migration 3, the one after this binary's
/// last, as the next version of Nage would leave it: a change below 1000, which the commands name
/// in four digits.
pub fn ahead_by_one() -> TestDir {
    ahead_at(3)
}

/// A working directory whose database records the migration `version`, which this binary does
/// not carry.
fn ahead_at(version: i64) -> TestDir {
    let dir = migrated();
    block_on(async {
        let storage = open(dir.path()).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a write: {error}"));
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, 'future', 1, x'00', 0)",
        )
        .bind(version)
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot record migration {version}: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit migration {version}: {error}"));
        storage.close().await;
    });
    dir
}

/// The latest migration applied to the database of `dir`, read through the core without
/// migrating anything: all that can be read of a database behind this binary.
pub fn schema_version(dir: &Path) -> Option<i64> {
    block_on(async {
        let storage = open(dir).await;
        let version = storage
            .schema_version()
            .await
            .unwrap_or_else(|error| panic!("cannot read the schema version: {error}"));
        storage.close().await;
        version
    })
}

/// What the database of a working directory holds.
#[derive(Debug, PartialEq, Eq)]
pub struct Holds {
    /// The latest applied migration.
    pub schema_version: Option<i64>,
    pub book: Option<Book>,
    pub users: i64,
    pub accounts: usize,
    pub sessions: i64,
}

/// Reads what the database of `dir` holds, through the core, and closes it again.
pub fn holds(dir: &Path) -> Holds {
    block_on(async {
        let storage = open(dir).await;
        let schema_version = storage
            .schema_version()
            .await
            .unwrap_or_else(|error| panic!("cannot read the schema version: {error}"));
        let mut read = storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
        let holds = Holds {
            schema_version,
            book: book::book(&mut read)
                .await
                .unwrap_or_else(|error| panic!("cannot read the book: {error}")),
            users: users::count_users(&mut read)
                .await
                .unwrap_or_else(|error| panic!("cannot count the users: {error}")),
            accounts: accounts::accounts(&mut read)
                .await
                .unwrap_or_else(|error| panic!("cannot list the accounts: {error}"))
                .len(),
            sessions: test_support::session_rows(&mut read).await,
        };
        drop(read);
        storage.close().await;
        holds
    })
}

/// The user whose username has the fold of `typed`, with the stored hash, read through the
/// core.
pub fn credentials(dir: &Path, typed: &str) -> Option<Credentials> {
    block_on(async {
        let storage = open(dir).await;
        let mut read = storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
        let found = users::user_by_username(&mut read, typed)
            .await
            .unwrap_or_else(|error| panic!("cannot look {typed} up: {error}"));
        drop(read);
        storage.close().await;
        found
    })
}

/// Whether the session [`sign_in`] created under `token` is still there, asked at [`earlier`],
/// when it had not expired.
pub fn signed_in(dir: &Path, token: &TokenHash) -> bool {
    block_on(async {
        let storage = open(dir).await;
        let mut read = storage
            .begin_read()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
        let found = sessions::valid_session(&mut read, token, earlier())
            .await
            .unwrap_or_else(|error| panic!("cannot look the session up: {error}"));
        drop(read);
        storage.close().await;
        found.is_some()
    })
}

/// Asserts that the command succeeded: exit code 0, exactly `stdout` on standard output and
/// nothing on standard error.
#[track_caller]
pub fn assert_done(out: &Finished, stdout: &str) {
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
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

/// Asserts that the command stopped with the exit code `code` and exactly `lines` on standard
/// error (the three labelled lines of decision 157, from [`refused`] or [`at_the_terminal`]),
/// and printed nothing on standard output.
#[track_caller]
pub fn assert_stopped(out: &Finished, code: i32, lines: &str) {
    assert_eq!(
        out.code(),
        Some(code),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
        out.stderr
    );
    assert_eq!(out.stderr, lines, "stderr of `{}`", out.what);
    assert!(
        out.stdout.is_empty(),
        "stdout of `{}` is not empty:\n{}",
        out.what,
        out.stdout
    );
}

/// Asserts that `nage check` finds the database of `dir` sound: whatever a command wrote is a
/// ledger the checker accepts.
#[track_caller]
pub fn assert_check_ok(dir: &Path) {
    let out = run(nage()
        .args(["check", "--data-dir", "data"])
        .current_dir(dir));
    assert_done(&out, &check_passed(&printed_database(dir)));
}

/// A command started with its standard input on a pipe the test keeps open: it can read what the
/// test sends and waits, as a command whose standard input never ends, for the rest.
pub struct Waiting {
    child: Child,
    what: String,
    stdin: Option<PipeWriter>,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
    start: Instant,
}

/// Starts `command` with its standard input on a pipe the test holds open (see [`Waiting`]).
#[track_caller]
pub fn spawn_waiting(command: &mut Command) -> Waiting {
    let what = super::describe(command);
    let (reader, writer) =
        std::io::pipe().unwrap_or_else(|error| panic!("cannot make a pipe: {error}"));
    command
        .stdin(reader)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("cannot spawn `{what}`: {error}"));
    let stdout = child.stdout.take().map(read_to_text);
    let stderr = child.stderr.take().map(read_to_text);
    Waiting {
        child,
        what,
        stdin: Some(writer),
        stdout,
        stderr,
        start,
    }
}

/// Reads all of `source` on a thread of its own, as text.
fn read_to_text(mut source: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = source.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

impl Waiting {
    /// Whether the command is still running.
    #[track_caller]
    pub fn running(&mut self) -> bool {
        self.child
            .try_wait()
            .unwrap_or_else(|error| panic!("cannot poll `{}`: {error}", self.what))
            .is_none()
    }

    /// Sends `bytes` on the command's standard input, keeping the pipe open.
    #[track_caller]
    pub fn send(&mut self, bytes: &[u8]) {
        let writer = self
            .stdin
            .as_mut()
            .unwrap_or_else(|| panic!("the standard input of `{}` is closed", self.what));
        writer
            .write_all(bytes)
            .unwrap_or_else(|error| panic!("cannot write to `{}`: {error}", self.what));
    }

    /// Closes the command's standard input, as the end of a file.
    pub fn close(&mut self) {
        drop(self.stdin.take());
    }

    /// Waits up to `within` for the command to exit, the pipe left as it is, and returns what it
    /// printed; kills it and panics when it has not exited by then.
    #[track_caller]
    pub fn finish_within(mut self, within: Duration) -> Finished {
        let deadline = Instant::now() + within;
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    panic!(
                        "`{}` did not exit within {within:?} and was killed",
                        self.what
                    );
                }
                Err(error) => panic!("cannot wait for `{}`: {error}", self.what),
            }
        };
        let elapsed = self.start.elapsed();
        self.close();
        let text = |reader: Option<JoinHandle<String>>| {
            reader
                .map(|reader| reader.join().unwrap_or_default())
                .unwrap_or_default()
        };
        Finished {
            what: self.what.clone(),
            status,
            stdout: text(self.stdout.take()),
            stderr: text(self.stderr.take()),
            elapsed,
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Whether another connection gets the writer of the database at `path` now: a `BEGIN
/// IMMEDIATE` granted within a tenth of a second, then rolled back (unchecked SQL for transaction
/// control, decision 57).
pub fn writer_is_free(path: &Path) -> bool {
    block_on(async {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .busy_timeout(Duration::from_millis(100)),
        )
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        let free = sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut conn)
            .await
            .is_ok();
        if free {
            sqlx::query("ROLLBACK")
                .execute(&mut conn)
                .await
                .unwrap_or_else(|error| panic!("cannot roll the probe back: {error}"));
        }
        conn.close()
            .await
            .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
        free
    })
}

/// Asserts that the stored hash was made with SPEC 5.3's parameters, argon2id at m=19456 KiB,
/// t=2, p=1, and from `password`.
#[track_caller]
pub fn assert_hashed_as_spec_5_3(credentials: &Credentials, password: &str) {
    assert!(
        credentials
            .password_hash
            .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "the hash of {} was not made with SPEC 5.3's parameters: {}",
        credentials.user.username,
        credentials.password_hash
    );
    assert_eq!(
        password::verify(password, &credentials.password_hash),
        Verification::Verified,
        "the stored hash of {} against the password {password:?}",
        credentials.user.username
    );
}
