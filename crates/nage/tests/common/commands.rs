//! What the process tests of `nage init`, `nage user add` and `nage user reset-password` need
//! (decisions 113 and 115): the password on standard input through a pipe, a run without a
//! terminal, data directories in the states the commands meet (built through the core), what
//! the database holds afterwards (read through the core), and the lines the commands word
//! themselves. The real terminal is not driven here: the prompt's logic is tested in the
//! binary's own unit tests through a reader standing in for it, and the terminal itself is
//! tried by hand.

use std::fs;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nage_core::accounts;
use nage_core::book::{self, Book};
use nage_core::password::{self, Verification};
use nage_core::sessions::{self, TokenHash};
use nage_core::setup::Initialized;
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{self, TestDir, fixture_copy};
use nage_core::timestamp::Timestamp;
use nage_core::users::{self, Credentials, User, UserId};

use super::web::{ADMIN, PASSWORD};
use super::{Finished, nage, run};

/// A password the rules refuse: nine characters.
pub const TOO_SHORT: &str = "nine char";

/// The first words of every refusal `nage init` words itself.
pub const BOOK_NOT_SET_UP: &str = "The book was not set up";
/// The first words of every refusal `nage user add` words itself.
pub const USER_NOT_CREATED: &str = "The user was not created";
/// The first words of every refusal `nage user reset-password` words itself.
pub const PASSWORD_NOT_CHANGED: &str = "The password was not changed";

/// What `nage init` says without `--currency`.
pub const CURRENCY_MISSING: &str = "The book was not set up because no currency was given, and a book's currency never changes once the book is set up. Add --currency and the code of the book's currency, for example --currency CAD, then run the command again.";

/// What `nage init` says about a currency code that is not in the table.
pub fn unknown_currency(typed: &str) -> String {
    format!(
        "The book was not set up because \"{typed}\" is not a currency code Nage knows; a code is three capital letters, such as CAD, EUR or USD. Give the code of the book's currency with --currency, then run the command again."
    )
}

/// What a command says when it has no terminal to ask on and no `--password-stdin`.
pub fn no_terminal(not_done: &str) -> String {
    format!(
        "{not_done} because the password could not be asked for: this command is not running in a terminal, and --password-stdin was not given. Run the command in a terminal, or add --password-stdin and send the password on standard input."
    )
}

/// What a command says when `--password-stdin` brings nothing.
pub fn no_password_on_stdin(not_done: &str) -> String {
    format!(
        "{not_done} because --password-stdin was given and no password arrived on standard input. Send the password on standard input, for example through a pipe, or leave --password-stdin out and type the password at the terminal."
    )
}

/// What a command says when standard input is not UTF-8.
pub fn password_not_text(not_done: &str) -> String {
    format!(
        "{not_done} because the password on standard input is not valid UTF-8 text, so Nage cannot tell which characters it holds. Send the password as UTF-8 text, then run the command again."
    )
}

/// What `nage user add` and `nage user reset-password` say where the data directory holds no
/// database; `database` is the path as the command prints it.
pub fn no_database(not_done: &str, database: &str) -> String {
    format!(
        "{not_done} because there is no Nage database at {database}. Run the command in the folder that holds the data directory, or name the data directory with --data-dir; if Nage has never been started there, set it up first with nage init."
    )
}

/// What a command says about a database behind this binary.
pub fn database_behind(not_done: &str, database: &str, version: i64, expects: i64) -> String {
    format!(
        "{not_done} because the database at {database} was left by an older version of Nage and has not been brought up to date: it is at schema version {version}, and this version of Nage expects {expects}. Start nage serve once, which brings the database up to date, then run the command again."
    )
}

/// What a command says about a database a newer Nage wrote.
pub fn database_newer(not_done: &str, database: &str, version: i64) -> String {
    format!(
        "{not_done} because the database at {database} was written by a newer version of Nage than this one: it carries migration {version}, which this version does not know. Run the command with that newer version of Nage."
    )
}

/// What `nage user add` and `nage user reset-password` say on a database without a user.
pub fn not_set_up(not_done: &str) -> String {
    format!(
        "{not_done} because Nage has not been set up in this data directory yet, so it has no users. Set Nage up first, with nage init or on the set-up page, or name the right data directory with --data-dir."
    )
}

/// What `nage user add` says when `--by` names nobody.
pub fn added_by_nobody(typed: &str) -> String {
    format!(
        "The user was not created because nobody has the username \"{typed}\", which --by named as the admin adding the user. Check the spelling, or name another admin with --by, then run the command again."
    )
}

/// What `nage user reset-password` says when `--by` names nobody.
pub fn reset_by_nobody(typed: &str) -> String {
    format!(
        "The password was not changed because nobody has the username \"{typed}\", which --by named as the admin resetting the password. Check the spelling, or name another admin with --by, then run the command again."
    )
}

/// What `nage user reset-password` says when `--username` names nobody.
pub fn unknown_user(typed: &str) -> String {
    format!(
        "The password was not changed because nobody has the username \"{typed}\", which --username named as the user whose password to reset. Check the spelling, then run the command again."
    )
}

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
    let (fixture_dir, copy) = fixture_copy("0.1.0");
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    fs::copy(copy, database(dir.path()))
        .unwrap_or_else(|error| panic!("cannot copy the fixture copy: {error}"));
    drop(fixture_dir);
    dir
}

/// A working directory whose database records a migration 9999, as a newer Nage would have
/// left it (unchecked SQL on migration bookkeeping, decision 57).
pub fn ahead() -> TestDir {
    let dir = migrated();
    block_on(async {
        let storage = open(dir.path()).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a write: {error}"));
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'future', 1, x'00', 0)",
        )
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| panic!("cannot record migration 9999: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit migration 9999: {error}"));
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

/// Asserts that the command stopped with the exit code `code` and exactly one line on
/// standard error, `nage: ` followed by `line`, and printed nothing on standard output.
#[track_caller]
pub fn assert_stopped(out: &Finished, code: i32, line: &str) {
    assert_eq!(
        out.code(),
        Some(code),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
        out.stderr
    );
    assert_eq!(
        out.stderr,
        format!("nage: {line}\n"),
        "stderr of `{}`",
        out.what
    );
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
    assert_done(&out, "check: ok (10 invariants)\n");
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
