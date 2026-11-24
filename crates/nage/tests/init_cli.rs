//! `nage init` on the command line (SPEC W1 step 4, 5.2, decisions 107 and 113): a fresh data
//! directory or a database `serve` created is set up as the set-up page sets it up, with the
//! currency typed and the password from standard input; and each refusal: no currency, an
//! unknown one, a name or a password the rules refuse, a book that is already set up, a
//! database behind or ahead of this binary, no password on standard input, no terminal.

mod common;

use std::fs;
use std::path::Path;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::time::{Duration, Instant};

use nage_core::book::BookError;
use nage_core::currency::Currency;
use nage_core::fold::NameError;
use nage_core::password::{self, HashError, PasswordError, Verification};
use nage_core::setup::{STARTER_CHART, SetupError};
use nage_core::storage::Storage;
use nage_core::test_support::{CHEAP_HASH_PARAMS, TEST_BOOK_NAME, TestDir, assert_no_side_files};
use nage_core::users::{self, NewUser, UserError};

use common::commands::{
    BOOK_NOT_SET_UP, PASSWORD_NOT_CHANGED, TOO_SHORT, USER_NOT_CREATED, added_by_nobody, ahead,
    assert_check_ok, assert_done, assert_hashed_as_spec_5_3, assert_stopped, at_the_terminal,
    behind, block_on, command, credentials, currency_missing, database, database_behind,
    database_newer, earlier, holds, migrated, no_password_on_stdin, no_terminal, now, open,
    password_not_text, printed_database, refused, run_with_stdin, schema_version, set_up,
    spawn_waiting, unknown_currency, writer_is_free,
};
use common::web::{ADMIN, PASSWORD, header, session_cookie};
use common::{Finished, nage, run, serve};

/// The arguments of `nage init --data-dir data --book-name Household --currency EUR
/// --username sam --display-name Sam`, each value replaced by `with` where it names the flag
/// (`None` leaves the flag out), followed by `more`.
fn init_args<'a>(with: &[(&'a str, Option<&'a str>)], more: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["init"];
    for (flag, standard) in [
        ("--data-dir", "data"),
        ("--book-name", "Household"),
        ("--currency", "EUR"),
        ("--username", ADMIN),
        ("--display-name", "Sam"),
    ] {
        let value = with
            .iter()
            .find(|(name, _)| *name == flag)
            .map_or(Some(standard), |(_, value)| *value);
        if let Some(value) = value {
            args.extend([flag, value]);
        }
    }
    args.extend(more);
    args
}

/// `nage init` in `cwd` with [`init_args`] and `stdin` as the password on standard input
/// (`--password-stdin`).
#[track_caller]
fn init(cwd: &Path, with: &[(&str, Option<&str>)], more: &[&str], stdin: &[u8]) -> Finished {
    command(cwd, &init_args(with, more), Some(stdin))
}

/// What `nage init` prints when it is done (decision 157): the sentence, its facts and the next
/// step; `starter` is `added` or `left out`.
fn done(dir: &Path, book: &str, currency: &str, starter: &str, user: &str, name: &str) -> String {
    format!(
        "Success: The book \"{book}\" was set up.\n  Currency: {currency}\n  Starter accounts: {starter}\n  Admin: {user} ({name})\n  Database: {}\nWhat now: Start the server if it is not running; {user} can then sign in.\n",
        printed_database(dir)
    )
}

/// How many accounts a set-up creates: the five roots, the three other system accounts and,
/// when asked for, the starter chart.
fn accounts_after_set_up(starter_chart: bool) -> usize {
    let starter: usize = STARTER_CHART.iter().map(|(_, names)| names.len()).sum();
    if starter_chart { 8 + starter } else { 8 }
}

/// What the command `what` prints for a password that breaks the rule `refusal`: the core's
/// words at the terminal.
fn password_refused(what: &str, refusal: PasswordError) -> String {
    at_the_terminal(what, HashError::Password(refusal).at_the_terminal())
}

/// Asserts that a refused `nage init` in the fresh working directory `dir` created nothing:
/// no data directory, so no database either.
#[track_caller]
fn assert_nothing_created(dir: &Path, when: &str) {
    assert!(
        !dir.join("data").exists(),
        "nage init created the data directory {when}"
    );
}

/// In a working directory without a data directory, `nage init` with the names, the currency
/// EUR and the password on standard input creates `data/` with `backups/` and the database,
/// migrates it to this binary's schema version and sets Nage up: the book in EUR with the
/// table's minor digits, the admin sam with no creator, stamped by the system clock, the
/// eight system accounts and the starter chart, and no session. The password is stored
/// hashed with SPEC 5.3's parameters and verifies. The command prints its two lines on
/// standard output, with the database's absolute path, and nothing on standard error; it
/// leaves no `-wal` or `-shm` file, and `nage check` finds the new book sound.
///
/// Proves: SPEC W1 step 4, decision 113 (`init` creates the data directory with `backups/`
/// and the database and migrates a new database; a result is printed on standard output),
/// decision 107 (the binary hashes with m=19456 KiB, t=2, p=1), INV-5's Core half (the
/// currency and its digits are written at set-up, from the table), INV-26's Core half (the
/// first admin has no creator), decision 53 (a clean close).
#[test]
fn init_sets_nage_up_in_a_fresh_data_directory() {
    let dir = TestDir::new();
    let before = now();
    let out = init(dir.path(), &[], &[], b"correct horse 42\n");
    let after = now();
    assert_done(
        &out,
        &done(dir.path(), "Household", "EUR", "added", "sam", "Sam"),
    );
    assert!(
        dir.path().join("data").join("backups").is_dir(),
        "nage init did not create data/backups"
    );
    assert_no_side_files(&database(dir.path()), "after nage init");

    let holds = holds(dir.path());
    assert_eq!(
        holds.schema_version,
        Some(Storage::latest_schema_version()),
        "the schema version of the new database"
    );
    let book = holds
        .book
        .unwrap_or_else(|| panic!("nage init wrote no book"));
    assert_eq!(book.name, "Household", "the book's name");
    assert_eq!(book.currency_code, "EUR", "the book's currency");
    let euro = Currency::from_code("EUR").unwrap_or_else(|| panic!("EUR is not in the table"));
    assert_eq!(
        book.minor_digits,
        euro.minor_digits(),
        "the book's minor digits"
    );
    assert!(
        before <= book.created_at && book.created_at <= after,
        "the book was created at {}, outside {before} to {after}",
        book.created_at
    );
    assert_eq!(holds.users, 1, "users after nage init");
    assert_eq!(
        holds.accounts,
        accounts_after_set_up(true),
        "accounts after nage init"
    );
    assert_eq!(holds.sessions, 0, "sessions after nage init");

    let admin =
        credentials(dir.path(), "sam").unwrap_or_else(|| panic!("nage init created no user sam"));
    assert_eq!(admin.user.username, "sam", "the admin's username");
    assert_eq!(admin.user.display_name, "Sam", "the admin's display name");
    assert!(admin.user.is_admin, "the first user is not an admin");
    assert!(admin.user.is_enabled(), "the first user is deactivated");
    assert_eq!(admin.user.created_by, None, "the first admin's creator");
    assert_eq!(
        (admin.user.updated_by, admin.user.updated_at),
        (None, None),
        "the first admin's updated pair"
    );
    assert!(
        before <= admin.user.created_at && admin.user.created_at <= after,
        "the admin was created at {}, outside {before} to {after}",
        admin.user.created_at
    );
    assert_hashed_as_spec_5_3(&admin, "correct horse 42");
    assert_check_ok(dir.path());
}

/// With `--no-starter-accounts` the set-up creates the eight system accounts and nothing else,
/// and the first line says so; the names are stored as the core normalizes them (the spaces
/// around the book name and inside the display name collapse) and printed as stored; a
/// currency without minor digits, JPY, is stored with none.
///
/// Proves: decision 113 (`--no-starter-accounts` leaves the starter chart out, which is
/// otherwise created as on the page), SPEC W1 step 3 (the roots and the three system
/// accounts always), INV-5's Core half (the digits come from the table entry chosen).
#[test]
fn init_leaves_the_starter_chart_out_when_told_to() {
    let dir = TestDir::new();
    let out = init(
        dir.path(),
        &[
            ("--book-name", Some("  Our  Household ")),
            ("--currency", Some("JPY")),
            ("--display-name", Some("Sam   Doe")),
            ("--username", Some("  sam ")),
        ],
        &["--no-starter-accounts"],
        PASSWORD.as_bytes(),
    );
    assert_done(
        &out,
        &done(
            dir.path(),
            "Our Household",
            "JPY",
            "left out",
            "sam",
            "Sam Doe",
        ),
    );
    let holds = holds(dir.path());
    assert_eq!(
        holds.accounts,
        accounts_after_set_up(false),
        "accounts after nage init --no-starter-accounts"
    );
    let book = holds
        .book
        .unwrap_or_else(|| panic!("nage init wrote no book"));
    assert_eq!(book.name, "Our Household", "the book's name as stored");
    assert_eq!(book.currency_code, "JPY", "the book's currency");
    assert_eq!(book.minor_digits.get(), 0, "the minor digits of JPY");
    assert_check_ok(dir.path());
}

/// A database that `nage serve` created and nobody set up (migrated, no book, no user) is
/// set up in place: the same file afterwards holds the book and the admin.
///
/// Proves: decision 113 (`init` sets up a database of this version that holds neither a book nor
/// a user).
#[test]
fn init_sets_up_a_database_that_serve_created() {
    let dir = migrated();
    let empty = holds(dir.path());
    assert_eq!(
        (empty.book.is_some(), empty.users),
        (false, 0),
        "the database before nage init"
    );
    let out = init(dir.path(), &[], &[], PASSWORD.as_bytes());
    assert_done(
        &out,
        &done(dir.path(), "Household", "EUR", "added", "sam", "Sam"),
    );
    let holds = holds(dir.path());
    assert!(holds.book.is_some(), "nage init wrote no book");
    assert_eq!(holds.users, 1, "users after nage init");
    assert_no_side_files(&database(dir.path()), "after nage init");
    assert_check_ok(dir.path());
}

/// The data directory comes from `--data-dir`, else `NAGE_DATA_DIR`, else `./data` relative
/// to the working directory; the flag wins over the variable. The run with the default prints
/// the absolute path of the database it set up; the other two are held to the file they made.
///
/// Proves: decision 113 (`init` takes `--data-dir`, `NAGE_DATA_DIR`, default `./data`, as
/// `serve` and `check` do), decision 42 (flag over variable over default).
#[test]
fn init_reads_the_data_dir_from_the_flag_the_variable_and_the_default() {
    let names = init_args(&[("--data-dir", None)], &["--password-stdin"]);

    let default = TestDir::new();
    let out = run_with_stdin(
        nage().args(&names).current_dir(default.path()),
        PASSWORD.as_bytes(),
    );
    assert_done(
        &out,
        &done(default.path(), "Household", "EUR", "added", "sam", "Sam"),
    );
    assert!(
        database(default.path()).is_file(),
        "nage init without --data-dir did not use ./data"
    );

    let variable = TestDir::new();
    let books = variable.path().join("books");
    let out = run_with_stdin(
        nage()
            .args(&names)
            .env("NAGE_DATA_DIR", &books)
            .current_dir(variable.path()),
        PASSWORD.as_bytes(),
    );
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(
        books.join("nage.sqlite").is_file() && !variable.path().join("data").exists(),
        "nage init did not use NAGE_DATA_DIR"
    );

    let flag = TestDir::new();
    let out = run_with_stdin(
        nage()
            .args(&names)
            .args(["--data-dir", "here"])
            .env("NAGE_DATA_DIR", "nowhere")
            .current_dir(flag.path()),
        PASSWORD.as_bytes(),
    );
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(
        flag.path().join("here").join("nage.sqlite").is_file()
            && !flag.path().join("nowhere").exists(),
        "--data-dir did not win over NAGE_DATA_DIR"
    );
}

/// Without `--currency`, `nage init` is a usage error: exit code 2 and three lines saying that a
/// book's currency never changes and how to add the flag. Nothing else is looked at and
/// nothing is written: a fresh working directory gets no data directory, and a database that
/// is already set up, which would be refused for that, is refused for the missing currency
/// and stays as it is. It is refused before the password is read: the password on standard
/// input is one the rules refuse, and the line is the currency's.
///
/// Proves: decision 113 (`--currency` has no default: without it `init` stops before writing
/// anything, a usage error whose message says that a book's currency never changes and how
/// to add the flag; it is checked before the password is asked for or read), decision 45
/// (exit 2).
#[test]
fn init_without_a_currency_is_a_usage_error_and_writes_nothing() {
    let fresh = TestDir::new();
    let out = init(
        fresh.path(),
        &[("--currency", None)],
        &[],
        TOO_SHORT.as_bytes(),
    );
    assert_stopped(&out, 2, &currency_missing());
    assert_nothing_created(fresh.path(), "without --currency");

    let (dir, _) = set_up();
    let before = holds(dir.path());
    let out = init(
        dir.path(),
        &[("--currency", None)],
        &[],
        TOO_SHORT.as_bytes(),
    );
    assert_stopped(&out, 2, &currency_missing());
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after nage init without --currency"
    );
}

/// A currency code that is not in the table is refused, exit code 1, with three lines that
/// quotes what was typed: an unknown code, a known code in small letters (the table's codes
/// are compared exactly) and an empty value. It is refused before the password is read (the
/// password on standard input is one the rules refuse, and the line is the currency's), and
/// nothing is created.
///
/// Proves: decision 113 (a refusal prints decision 102's three parts on standard error, as the
/// three labelled lines of decision 157, and exits 1), SPEC 6.10 (the book's currency is one of
/// the table's).
#[test]
fn init_refuses_a_currency_code_it_does_not_know() {
    for typed in ["XYZ", "cad", ""] {
        let dir = TestDir::new();
        let out = init(
            dir.path(),
            &[("--currency", Some(typed))],
            &[],
            TOO_SHORT.as_bytes(),
        );
        assert_stopped(&out, 1, &unknown_currency(typed));
        assert_nothing_created(dir.path(), &format!("for --currency {typed:?}"));
    }
}

/// A book name, a username or a display name that the core's name rules refuse is refused in
/// the core's words, exit code 1: an empty book name, a username of 65 characters, a display
/// name with a tab in it. Each is refused before the password is read (the password on
/// standard input is one the rules refuse, and the line is the name's), and nothing is
/// created.
///
/// Proves: decision 113 (a refusal prints one line and exits 1), decision 114 (the user and
/// book refusals `init` shows first, in the three parts), decision 95 (the name rules).
#[test]
fn init_refuses_a_name_the_name_rules_refuse_in_the_cores_words() {
    let long = "a".repeat(65);
    let cases: [(&str, &str, String); 3] = [
        (
            "--book-name",
            "",
            at_the_terminal(
                BOOK_NOT_SET_UP,
                BookError::Name(NameError::Empty).at_the_terminal(),
            ),
        ),
        (
            "--username",
            &long,
            at_the_terminal(
                BOOK_NOT_SET_UP,
                UserError::Username(NameError::TooLong { chars: 65 }).at_the_terminal(),
            ),
        ),
        (
            "--display-name",
            "Sa\tm",
            at_the_terminal(
                BOOK_NOT_SET_UP,
                UserError::DisplayName(NameError::Control).at_the_terminal(),
            ),
        ),
    ];
    for (flag, typed, line) in cases {
        let dir = TestDir::new();
        let out = init(
            dir.path(),
            &[(flag, Some(typed))],
            &[],
            TOO_SHORT.as_bytes(),
        );
        assert_stopped(&out, 1, &line);
        assert_nothing_created(dir.path(), &format!("for {flag} {typed:?}"));
    }
}

/// On a database whose book is already set up, `nage init` is refused in the core's words
/// for a second set-up, exit code 1, before the password is read (the password on standard
/// input is one the rules refuse, and the line is the book's). The book, its currency, the
/// one user and that user's password stay exactly as they were.
///
/// Proves: INV-5 (the book and its currency are written once: a second set-up through the command
/// is refused before the password and changes nothing), decision 113 (a refusal exits 1 with one
/// line), decision 114 (the book's message), SPEC W1 step 4.
#[test]
fn init_refuses_a_book_that_is_already_set_up() {
    let (dir, initialized) = set_up();
    let before = holds(dir.path());
    let out = init(
        dir.path(),
        &[
            ("--book-name", Some("Another")),
            ("--currency", Some("USD")),
            ("--username", Some("alex")),
        ],
        &[],
        TOO_SHORT.as_bytes(),
    );
    assert_stopped(
        &out,
        1,
        &at_the_terminal(
            BOOK_NOT_SET_UP,
            SetupError::AlreadyInitialized.at_the_terminal(),
        ),
    );
    let after = holds(dir.path());
    assert_eq!(after, before, "the database after a refused second set-up");
    assert_eq!(
        after.book.map(|book| (book.name, book.currency_code)),
        Some((String::from(TEST_BOOK_NAME), String::from("CAD"))),
        "the book after a refused second set-up"
    );
    assert!(
        credentials(dir.path(), "alex").is_none(),
        "a refused set-up created the user alex"
    );
    let admin = credentials(dir.path(), ADMIN)
        .unwrap_or_else(|| panic!("the admin is gone after a refused set-up"));
    assert_eq!(admin.user, initialized.admin, "the admin's row");
    assert_eq!(
        password::verify(PASSWORD, &admin.password_hash),
        Verification::Verified,
        "the admin's password after a refused set-up"
    );
}

/// A database behind this binary (a copy of the 0.1.0 fixture, at schema version 1) and one
/// that a newer Nage wrote are each refused with the command's own line, which names the
/// database, the versions and what to do, exit code 1, before the password is read (the
/// password on standard input is one the rules refuse). Neither is migrated: each is at the
/// schema version it had, and `nage check` still refuses it as before.
///
/// Proves: decision 113 (`init` refuses a database behind or ahead of the binary as `check`
/// does, so that the pre-migration backup is never skipped; a refusal prints one line in
/// three parts and exits 1), decision 97 (never migrated).
#[test]
fn init_refuses_a_database_behind_or_ahead_of_this_binary_and_migrates_nothing() {
    let expects = Storage::latest_schema_version();
    let behind = behind();
    let ahead = ahead();
    let cases = [
        (
            "behind",
            behind.path(),
            database_behind(
                BOOK_NOT_SET_UP,
                &printed_database(behind.path()),
                1,
                expects,
            ),
        ),
        ("ahead", ahead.path(), database_newer(BOOK_NOT_SET_UP, 9999)),
    ];
    for (state, dir, line) in cases {
        let version = schema_version(dir);
        let out = init(dir, &[], &[], TOO_SHORT.as_bytes());
        assert_stopped(&out, 1, &line);
        assert_eq!(
            schema_version(dir),
            version,
            "the schema version of the database {state} after nage init"
        );
        let check = run(nage()
            .args(["check", "--data-dir", "data"])
            .current_dir(dir));
        assert_eq!(
            check.code(),
            Some(1),
            "exit code of `{}` on the database {state} after nage init; stderr:\n{}",
            check.what,
            check.stderr
        );
    }
}

/// A password that breaks a rule is refused in the core's words, exit code 1, and nothing is
/// created: nine characters, 1,025 bytes, and twenty thousand bytes, of which the command
/// need not read more than a password may have.
///
/// Proves: decision 107 (every place that sets a password applies the same rules with the
/// same messages; a password the rules refuse is refused before anything is created),
/// decision 113 (a refusal exits 1 with one line), SPEC 5.3 (at least 10 characters, at most
/// 1 KiB).
#[test]
fn init_refuses_a_password_that_breaks_a_rule_and_creates_nothing() {
    let cases: [(Vec<u8>, PasswordError); 3] = [
        (TOO_SHORT.as_bytes().to_vec(), PasswordError::TooShort),
        (vec![b'x'; 1025], PasswordError::TooLong),
        (vec![b'x'; 20_000], PasswordError::TooLong),
    ];
    for (typed, refusal) in cases {
        let dir = TestDir::new();
        let out = init(dir.path(), &[], &[], &typed);
        assert_stopped(&out, 1, &password_refused(BOOK_NOT_SET_UP, refusal));
        assert_nothing_created(
            dir.path(),
            &format!("for a password of {} bytes", typed.len()),
        );
    }
    let dir = migrated();
    let out = init(dir.path(), &[], &[], TOO_SHORT.as_bytes());
    assert_stopped(
        &out,
        1,
        &password_refused(BOOK_NOT_SET_UP, PasswordError::TooShort),
    );
    let holds = holds(dir.path());
    assert_eq!(
        (holds.book.is_some(), holds.users),
        (false, 0),
        "the database after a refused password"
    );
}

/// With `--password-stdin` the password is all of standard input but for one trailing line
/// ending: without a line ending it is the bytes as they are; with two line endings the first
/// stays part of the password, as do the spaces at both ends. The stored hash verifies the
/// password so read and not the trimmed one.
///
/// Proves: decision 113 (`--password-stdin` reads the password once from standard input, all
/// of it with one trailing line ending removed), decision 107 (a password is never trimmed).
#[test]
fn init_takes_the_password_from_standard_input_whole_but_for_one_line_ending() {
    let dir = TestDir::new();
    let out = init(dir.path(), &[], &[], b"no line ending 42");
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let admin =
        credentials(dir.path(), "sam").unwrap_or_else(|| panic!("nage init created no user sam"));
    assert_hashed_as_spec_5_3(&admin, "no line ending 42");

    let dir = TestDir::new();
    let out = init(dir.path(), &[], &[], b"  two  spaces each side  \r\n\n");
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let admin =
        credentials(dir.path(), "sam").unwrap_or_else(|| panic!("nage init created no user sam"));
    assert_hashed_as_spec_5_3(&admin, "  two  spaces each side  \r\n");
    assert_eq!(
        password::verify("two  spaces each side", &admin.password_hash),
        Verification::WrongPassword,
        "the trimmed password against the stored hash"
    );
}

/// With `--password-stdin` and nothing on standard input (an empty pipe, or one line ending
/// alone) the command is refused with its own line, exit code 1; bytes that are not UTF-8 are
/// refused with theirs. Nothing is created.
///
/// Proves: decision 113 (`--password-stdin` reads the password from standard input; a
/// refusal prints one line in three parts and exits 1).
#[test]
fn init_refuses_a_standard_input_without_a_password_or_without_text() {
    let cases: [(&[u8], String); 3] = [
        (b"", no_password_on_stdin(BOOK_NOT_SET_UP)),
        (b"\n", no_password_on_stdin(BOOK_NOT_SET_UP)),
        (
            b"correct \xff\xfe horse 42\n",
            password_not_text(BOOK_NOT_SET_UP),
        ),
    ];
    for (typed, line) in cases {
        let dir = TestDir::new();
        let out = init(dir.path(), &[], &[], typed);
        assert_stopped(&out, 1, &line);
        assert_nothing_created(dir.path(), &format!("for the standard input {typed:?}"));
    }
}

/// Without `--password-stdin` and without a terminal (the command runs in a session of its
/// own, with standard input on `/dev/null`), `nage init` is a usage error: exit code 2 and three
/// lines naming both ways out. Nothing is created, and nothing is read from standard input.
///
/// Proves: decision 113 (with no terminal and no `--password-stdin` the command is a usage
/// error, exit 2; the terminal is asked on the terminal itself, never on standard input).
#[test]
fn init_without_a_terminal_and_without_password_stdin_is_a_usage_error() {
    let dir = TestDir::new();
    let out = command(dir.path(), &init_args(&[], &[]), None);
    assert_stopped(&out, 2, &no_terminal(BOOK_NOT_SET_UP));
    assert_nothing_created(dir.path(), "without a terminal");
}

/// A book set up by `nage init` is what `nage serve` then serves: the start page leads to the
/// sign-in page, never to the set-up page, which is closed with 404; the admin signs in there
/// with the username and the password the command was given and gets the session cookie; the
/// start page then answers 200.
///
/// Proves: SPEC W1 step 4 (`nage init` does the same as the set-up page), decision 113,
/// INV-27 (once a user exists `/setup` is unavailable and every other page needs a session).
#[test]
fn a_book_set_up_by_init_is_signed_into_through_serve() {
    let server = serve();
    let out = init(server.dir(), &[], &[], PASSWORD.as_bytes());
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let mut server = server.spawn();
    let addr = server.wait_listening();

    let start = server.get(addr, "/", &[]);
    assert_eq!(
        (start.status, header(&start, "location")),
        (303, Some("/login")),
        "GET / on a server whose book nage init set up:\n{}",
        start.head
    );
    let closed = server.get(addr, "/setup", &[]);
    assert_eq!(
        closed.status, 404,
        "GET /setup after nage init:\n{}",
        closed.head
    );
    let signed_in = server.post_form(
        addr,
        "/login",
        &[],
        &[("username", ADMIN), ("password", PASSWORD)],
    );
    assert_eq!(
        (signed_in.status, header(&signed_in, "location")),
        (303, Some("/")),
        "the sign-in with the password nage init was given:\n{}\n{}",
        signed_in.head,
        String::from_utf8_lossy(&signed_in.body)
    );
    let (token, _secure) = session_cookie(&signed_in);
    let home = server.get(addr, "/", &[("Cookie", &format!("nage_session={token}"))]);
    assert_eq!(
        home.status, 200,
        "GET / with the admin's session:\n{}",
        home.head
    );
}

/// `nage init` beside a running server that nobody has set up: the server, which showed the
/// set-up page, sends the next visitor of the start page to the sign-in page and answers
/// `/setup` with 404, without a restart.
///
/// Proves: decision 113 (`init` sets up a database of this version that holds neither a book nor
/// a user, the server's own), decision 109 (a book set up by `nage init` is seen at the next
/// request), INV-27.
#[test]
fn init_beside_a_running_server_closes_its_set_up_page() {
    let mut server = serve().spawn();
    let addr = server.wait_listening();
    let form = server.get(addr, "/setup", &[]);
    assert_eq!(
        form.status, 200,
        "GET /setup on a fresh server:\n{}",
        form.head
    );
    let dir = server.dir().to_path_buf();
    let out = init(&dir, &[], &[], PASSWORD.as_bytes());
    assert_done(&out, &done(&dir, "Household", "EUR", "added", "sam", "Sam"));

    let start = server.get(addr, "/", &[]);
    assert_eq!(
        (start.status, header(&start, "location")),
        (303, Some("/login")),
        "GET / after nage init beside the server:\n{}",
        start.head
    );
    let closed = server.get(addr, "/setup", &[]);
    assert_eq!(
        closed.status, 404,
        "GET /setup after nage init beside the server:\n{}",
        closed.head
    );
    assert!(
        fs::metadata(database(&dir)).is_ok(),
        "the server's database is gone"
    );
}

/// The arguments of `nage user add --by sam --username alex --display-name Alex
/// --password-stdin`, without a data folder, and of `nage user reset-password --by sam
/// --username alex --password-stdin`.
const USER_ADD: [&str; 9] = [
    "user",
    "add",
    "--by",
    "sam",
    "--username",
    "alex",
    "--display-name",
    "Alex",
    "--password-stdin",
];
const RESET: [&str; 7] = [
    "user",
    "reset-password",
    "--by",
    "sam",
    "--username",
    "alex",
    "--password-stdin",
];

/// Without `--currency` and with a book name the name rules refuse, `nage init` is refused for
/// the currency, a usage error with exit code 2: nothing else, the names included, is looked at
/// first. Nothing is created.
///
/// Proves: decision 113 (nothing else is looked at without a currency, exit 2), inherited gap
/// s5-evidence-cli-r1-5
#[test]
fn init_refuses_a_missing_currency_before_a_refused_name() {
    let dir = TestDir::new();
    let out = init(
        dir.path(),
        &[("--currency", None), ("--book-name", Some(""))],
        &[],
        TOO_SHORT.as_bytes(),
    );
    assert_stopped(&out, 2, &currency_missing());
    assert_nothing_created(dir.path(), "without --currency and with an empty book name");
}

/// The currency code is taken exactly as typed: a known code with a space before it, one with a
/// space after it and one in mixed capitals are each refused as a code Nage does not know, the
/// code quoted as typed, exit code 1, and nothing is created.
///
/// Proves: decision 113 (the code taken exactly as typed, a space included), inherited gap
/// s5-evidence-cli-r1-6
#[test]
fn init_takes_the_currency_code_exactly_as_typed() {
    for typed in [" CAD", "CAD ", "Cad"] {
        let dir = TestDir::new();
        let out = init(
            dir.path(),
            &[("--currency", Some(typed))],
            &[],
            TOO_SHORT.as_bytes(),
        );
        assert_stopped(&out, 1, &unknown_currency(typed));
        assert_nothing_created(dir.path(), &format!("for --currency {typed:?}"));
    }
}

/// A book name with a colon is set up as the set-up page sets it up: `  Our:  Household ` is
/// stored as `Our: Household` and printed so, the colon kept.
///
/// Proves: decision 95 (a colon allowed in the book name), inherited gap s5-evidence-cli-r2-5
#[test]
fn init_keeps_a_colon_in_the_book_name() {
    let dir = TestDir::new();
    let out = init(
        dir.path(),
        &[("--book-name", Some("  Our:  Household "))],
        &[],
        PASSWORD.as_bytes(),
    );
    assert_done(
        &out,
        &done(dir.path(), "Our: Household", "EUR", "added", "sam", "Sam"),
    );
    let book = holds(dir.path())
        .book
        .unwrap_or_else(|| panic!("nage init wrote no book"));
    assert_eq!(book.name, "Our: Household", "the book's name as stored");
    assert_check_ok(dir.path());
}

/// `NAGE_DATA_DIR` holding bytes that are not UTF-8 text is a wrong setting for each of the
/// three commands: exit code 2, three lines naming the variable and quoting its value, nothing
/// on standard output, and nothing created.
///
/// Proves: decision 45 (a configuration error exits 2), inherited gap s5-evidence-cli-r2-7
#[test]
fn a_wrong_setting_of_the_three_commands_exits_2() {
    let not_utf8 = OsStr::from_bytes(b"\xFF\xFE");
    let init_words = init_args(&[("--data-dir", None)], &["--password-stdin"]);
    let cases: [(&[&str], &str); 3] = [
        (&init_words, BOOK_NOT_SET_UP),
        (&USER_ADD, USER_NOT_CREATED),
        (&RESET, PASSWORD_NOT_CHANGED),
    ];
    for (args, what) in cases {
        let dir = TestDir::new();
        let out = run_with_stdin(
            nage()
                .args(args)
                .env("NAGE_DATA_DIR", not_utf8)
                .current_dir(dir.path()),
            PASSWORD.as_bytes(),
        );
        assert_stopped(
            &out,
            2,
            &refused(
                what,
                "NAGE_DATA_DIR holds \"\\xFF\\xFE\", which is not valid UTF-8 text.",
                "Set NAGE_DATA_DIR to UTF-8 text, or unset it, then run the command again.",
            ),
        );
        let entries = fs::read_dir(dir.path())
            .unwrap_or_else(|error| panic!("cannot list the working directory: {error}"))
            .count();
        assert_eq!(entries, 0, "`{}` created something", out.what);
    }
}

/// Runs `nage <args> --password-stdin` in `dir` with its standard input held open, and for a
/// second and a half, while the command waits for its password, asks again and again whether
/// another connection gets the writer of the database; it must, every time. Then the password
/// is sent, and what the command printed is returned.
#[track_caller]
fn writer_free_while_waiting(dir: &Path, args: &[&str], password: &str) -> Finished {
    let mut waiting = spawn_waiting(nage().args(args).current_dir(dir));
    let path = database(dir);
    let until = Instant::now() + Duration::from_millis(1500);
    let mut probes = 0;
    while Instant::now() < until {
        if !waiting.running() {
            let out = waiting.finish_within(Duration::from_secs(1));
            panic!(
                "`{}` ended before its password was sent ({}); stderr:\n{}",
                out.what, out.status, out.stderr
            );
        }
        assert!(
            writer_is_free(&path),
            "another connection did not get the writer after {probes} probe(s) while {args:?} waited for its password"
        );
        probes += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
    waiting.send(format!("{password}\n").as_bytes());
    waiting.close();
    waiting.finish_within(Duration::from_secs(10))
}

/// `nage init` on a database `nage serve` created, `nage user add` and `nage user
/// reset-password` each wait for the password with the database open and no write transaction
/// begun: another connection gets the writer the whole time. Once the password arrives each
/// finishes as usual.
///
/// Proves: SPEC 5.4 (hashing before the write transaction opens), inherited gap
/// s5-final-cli-r1-1
#[test]
fn the_password_is_read_and_hashed_before_the_write_transaction_opens() {
    let dir = migrated();
    let mut args = init_args(&[], &["--password-stdin"]);
    let out = writer_free_while_waiting(dir.path(), &args, PASSWORD);
    assert_done(
        &out,
        &done(dir.path(), "Household", "EUR", "added", "sam", "Sam"),
    );

    args = vec!["--data-dir", "data"];
    let add: Vec<&str> = [&USER_ADD[..], &args[..]].concat();
    let out = writer_free_while_waiting(dir.path(), &add, "tea and biscuits");
    assert_done(&out, &common::commands::user_added("alex", "Alex", "sam"));

    let reset: Vec<&str> = [&RESET[..], &args[..]].concat();
    let out = writer_free_while_waiting(dir.path(), &reset, "kettle on the hob");
    assert_done(&out, &common::commands::password_reset("alex", "sam"));
    assert_check_ok(dir.path());
}

/// With `--password-stdin` and a standard input that does not end (the test keeps the pipe open
/// after four kilobytes), `nage init` stops reading past the longest password and a line ending
/// and refuses the password as too long, at once: exit code 1, the core's words, and nothing
/// created.
///
/// Proves: decision 113 (reading stops past the longest password and a line ending), inherited
/// gap s5-final-cli-r1-2
#[test]
fn init_stops_reading_standard_input_past_the_longest_password() {
    let dir = TestDir::new();
    let mut waiting = spawn_waiting(
        nage()
            .args(init_args(&[], &["--password-stdin"]))
            .current_dir(dir.path()),
    );
    waiting.send(&[b'x'; 4096]);
    let out = waiting.finish_within(Duration::from_secs(5));
    assert_stopped(
        &out,
        1,
        &password_refused(BOOK_NOT_SET_UP, PasswordError::TooLong),
    );
    assert_nothing_created(dir.path(), "for a standard input that does not end");
}

/// `NAGE_DATA_DIR` alone, with no `--data-dir`, names the data folder of each of the three
/// commands, run from another working directory: `nage init` sets the book up there, `nage
/// user add` adds alex there and `nage user reset-password` resets alex's password there; no
/// data folder appears in the working directory.
///
/// Proves: decision 42 (NAGE_DATA_DIR alone names the data folder), inherited gap
/// s5-final-cli-r1-3
#[test]
fn the_commands_read_nage_data_dir_alone() {
    let home = TestDir::new();
    let home_path = fs::canonicalize(home.path())
        .unwrap_or_else(|error| panic!("cannot resolve the test folder: {error}"));
    let data = home_path.join("data");
    let elsewhere = TestDir::new();
    let in_elsewhere = |args: &[&str], password: &str| {
        run_with_stdin(
            nage()
                .args(args)
                .env("NAGE_DATA_DIR", &data)
                .current_dir(elsewhere.path()),
            password.as_bytes(),
        )
    };
    let out = in_elsewhere(
        &init_args(&[("--data-dir", None)], &["--password-stdin"]),
        PASSWORD,
    );
    assert_done(
        &out,
        &done(&home_path, "Household", "EUR", "added", "sam", "Sam"),
    );
    let out = in_elsewhere(&USER_ADD, "tea and biscuits");
    assert_done(&out, &common::commands::user_added("alex", "Alex", "sam"));
    let out = in_elsewhere(&RESET, "kettle on the hob");
    assert_done(&out, &common::commands::password_reset("alex", "sam"));
    let alex = credentials(&home_path, "alex")
        .unwrap_or_else(|| panic!("nage user add through NAGE_DATA_DIR created no user alex"));
    assert_hashed_as_spec_5_3(&alex, "kettle on the hob");
    let entries = fs::read_dir(elsewhere.path())
        .unwrap_or_else(|error| panic!("cannot list the other working directory: {error}"))
        .count();
    assert_eq!(entries, 0, "a command wrote into the working directory");
}

/// In a working directory whose data folder is there and empty, as a deployment's mounted folder
/// is, `nage init` creates `backups/` and the database in it and sets Nage up; `nage check` finds
/// the book sound.
///
/// Proves: decision 113 (an existing empty data folder is set up), inherited gap
/// s5-final-cli-r2-2
#[test]
fn init_sets_nage_up_in_an_existing_empty_data_folder() {
    let dir = TestDir::new();
    fs::create_dir(dir.path().join("data"))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    let out = init(dir.path(), &[], &[], PASSWORD.as_bytes());
    assert_done(
        &out,
        &done(dir.path(), "Household", "EUR", "added", "sam", "Sam"),
    );
    assert!(
        dir.path().join("data").join("backups").is_dir(),
        "nage init did not create data/backups in the empty data folder"
    );
    let holds = holds(dir.path());
    assert!(
        holds.book.is_some() && holds.users == 1,
        "the database after nage init in an empty data folder: {holds:?}"
    );
    assert_no_side_files(&database(dir.path()), "after nage init");
    assert_check_ok(dir.path());
}

/// Without a terminal and without `--password-stdin`, each command still refuses first what it can
/// refuse without the password: `nage init` without `--currency` (exit 2, the currency's line) and
/// with an empty book name (exit 1, the name's line), `nage user add --by` naming nobody and `nage
/// user reset-password --username` naming nobody (exit 1, their own lines). The missing terminal
/// is never named.
///
/// Proves: decision 113 (everything that can be refused without the password is refused first),
/// inherited gap s5-final-cli-r2-4
#[test]
fn a_command_looks_for_the_terminal_only_when_it_reaches_the_password() {
    let fresh = TestDir::new();
    let out = command(fresh.path(), &init_args(&[("--currency", None)], &[]), None);
    assert_stopped(&out, 2, &currency_missing());
    let out = command(
        fresh.path(),
        &init_args(&[("--book-name", Some(""))], &[]),
        None,
    );
    assert_stopped(
        &out,
        1,
        &at_the_terminal(
            BOOK_NOT_SET_UP,
            BookError::Name(NameError::Empty).at_the_terminal(),
        ),
    );
    assert_nothing_created(fresh.path(), "without a terminal");

    let (dir, _) = set_up();
    let out = command(
        dir.path(),
        &[
            "user",
            "add",
            "--data-dir",
            "data",
            "--by",
            "sma",
            "--username",
            "alex",
            "--display-name",
            "Alex",
        ],
        None,
    );
    assert_stopped(&out, 1, &added_by_nobody("sma"));
    let out = command(
        dir.path(),
        &[
            "user",
            "reset-password",
            "--data-dir",
            "data",
            "--by",
            "sam",
            "--username",
            "alexx",
        ],
        None,
    );
    assert_stopped(&out, 1, &common::commands::unknown_user("alexx"));
}

/// A database of this version that holds a user and no book (the first admin created through the
/// core, no set-up) is refused as already set up, exit code 1, before the password is read; the
/// database stays as it was.
///
/// Proves: decision 113 (a database that holds a user is refused as already set up), inherited
/// gap s5-authors-row113
#[test]
fn init_refuses_a_database_with_a_user_and_no_book() {
    let dir = migrated();
    block_on(async {
        let storage = open(dir.path()).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin the write: {error}"));
        let password_hash = password::hash(CHEAP_HASH_PARAMS, PASSWORD)
            .unwrap_or_else(|error| panic!("cannot hash the password: {error}"));
        users::create_user(
            &mut tx,
            None,
            earlier(),
            &NewUser {
                username: String::from("robin"),
                display_name: String::from("Robin"),
                password_hash,
                is_admin: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("cannot create the first admin: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the first admin: {error}"));
        storage.close().await;
    });
    let before = holds(dir.path());
    assert_eq!(
        (before.book.is_some(), before.users),
        (false, 1),
        "the database before nage init"
    );
    let out = init(dir.path(), &[], &[], TOO_SHORT.as_bytes());
    assert_stopped(
        &out,
        1,
        &at_the_terminal(
            BOOK_NOT_SET_UP,
            SetupError::AlreadyInitialized.at_the_terminal(),
        ),
    );
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after the refused nage init"
    );
}
