//! `nage user add` and `nage user reset-password` on the command line (SPEC W1 step 4, 5.3,
//! decisions 93, 96, 107, 112 and 113): another user added in an admin's name, a password
//! reset in an admin's name with every session of the user ended, `--by` and `--username`
//! found by the fold of what is typed, and each refusal: an admin or a user nobody is, an
//! admin who is not one, a username that is taken, a name or a password the rules refuse, a
//! database that cannot be used or is not set up, no terminal.

mod common;

use std::path::Path;

use nage_core::fold::NameError;
use nage_core::password::{self, PasswordError, Verification};
use nage_core::storage::Storage;
use nage_core::test_support::{TestDir, assert_no_side_files};
use nage_core::users::{PasswordChangeError, UserError};

use common::commands::{
    PASSWORD_NOT_CHANGED, TOO_SHORT, USER_NOT_CREATED, add_user, added_by_nobody, ahead,
    assert_check_ok, assert_done, assert_hashed_as_spec_5_3, assert_stopped, behind, command,
    credentials, database, database_behind, database_newer, holds, migrated, no_database,
    no_password_on_stdin, no_terminal, not_set_up, now, printed_database, reset_by_nobody,
    run_with_stdin, schema_version, set_up, set_up_in, sign_in, signed_in, unknown_user,
};
use common::web::{ADMIN, PASSWORD, header, session_cookie};
use common::{Finished, nage, serve};

/// The password the tests give alex through the core, and the one a command sets.
const ALEX_PASSWORD: &str = "tea and biscuits";
const NEW_PASSWORD: &str = "kettle on the hob";

/// The arguments of `nage user add --data-dir data --by <by> --username <username>
/// --display-name <display_name>`.
fn add_args<'a>(by: &'a str, username: &'a str, display_name: &'a str) -> [&'a str; 10] {
    [
        "user",
        "add",
        "--data-dir",
        "data",
        "--by",
        by,
        "--username",
        username,
        "--display-name",
        display_name,
    ]
}

/// The arguments of `nage user reset-password --data-dir data --by <by> --username
/// <username>`.
fn reset_args<'a>(by: &'a str, username: &'a str) -> [&'a str; 8] {
    [
        "user",
        "reset-password",
        "--data-dir",
        "data",
        "--by",
        by,
        "--username",
        username,
    ]
}

/// `nage user add` in `cwd` with [`add_args`] and `stdin` as the password on standard input.
#[track_caller]
fn add(cwd: &Path, by: &str, username: &str, display_name: &str, stdin: &[u8]) -> Finished {
    command(cwd, &add_args(by, username, display_name), Some(stdin))
}

/// `nage user reset-password` in `cwd` with [`reset_args`] and `stdin` as the new password on
/// standard input.
#[track_caller]
fn reset(cwd: &Path, by: &str, username: &str, stdin: &[u8]) -> Finished {
    command(cwd, &reset_args(by, username), Some(stdin))
}

/// The line `nage user add` prints when it is done.
fn added(username: &str, display_name: &str, by: &str) -> String {
    format!(
        "{username} ({display_name}) was added by {by} and can now sign in. {username} is not an admin.\n"
    )
}

/// The line `nage user reset-password` prints when an admin reset another user's password.
fn reset_done(username: &str, by: &str) -> String {
    format!(
        "The password of {username} was reset by {by}, and {username} was signed out on every device. Tell {username} the new password; {username} can change it after signing in.\n"
    )
}

/// The line `nage user reset-password` prints when an admin reset their own password.
fn reset_own_done(username: &str) -> String {
    format!(
        "The password of {username} was reset by {username}, and {username} was signed out on every device. Sign in with the new password.\n"
    )
}

/// On a set-up book, `nage user add --by sam --username alex --display-name Alex` with the
/// password on standard input adds alex: not an admin, created by sam at the system clock's
/// instant, active, with the password, sent with a space at each end, stored as it arrived,
/// the spaces included, and hashed with SPEC 5.3's parameters. The command prints its line on
/// standard output and nothing on standard error; sam's row stays as it was, nobody is signed
/// in, no `-wal` or `-shm` file stays, and `nage check` finds the book sound.
///
/// Proves: SPEC W1 step 4 and decision 113 (`nage user add` takes `--by`, `--username` and
/// `--display-name` and adds a user who is not an admin; a result is printed on standard
/// output; the password given through `--password-stdin` is hashed with the spaces at its
/// ends), decision 93 and INV-26's Core half (the user names the admin who created them),
/// decision 107 (m=19456 KiB, t=2, p=1; a password is never trimmed), decision 53 (a clean
/// close).
#[test]
fn user_add_adds_a_user_who_is_not_an_admin() {
    let (dir, initialized) = set_up();
    let typed = format!(" {ALEX_PASSWORD} ");
    let before = now();
    let out = add(
        dir.path(),
        "sam",
        "alex",
        "Alex",
        format!("{typed}\n").as_bytes(),
    );
    let after = now();
    assert_done(&out, &added("alex", "Alex", "sam"));
    assert_no_side_files(&database(dir.path()), "after nage user add");

    let alex = credentials(dir.path(), "alex")
        .unwrap_or_else(|| panic!("nage user add created no user alex"));
    assert_eq!(alex.user.username, "alex", "the new user's username");
    assert_eq!(
        alex.user.display_name, "Alex",
        "the new user's display name"
    );
    assert!(!alex.user.is_admin, "the new user is an admin");
    assert!(alex.user.is_enabled(), "the new user is deactivated");
    assert_eq!(
        alex.user.created_by,
        Some(initialized.admin.id),
        "who created the new user"
    );
    assert!(
        before <= alex.user.created_at && alex.user.created_at <= after,
        "the new user was created at {}, outside {before} to {after}",
        alex.user.created_at
    );
    assert_eq!(
        (alex.user.updated_by, alex.user.updated_at),
        (None, None),
        "the new user's updated pair"
    );
    assert_hashed_as_spec_5_3(&alex, &typed);

    let holds = holds(dir.path());
    assert_eq!(holds.users, 2, "users after nage user add");
    assert_eq!(holds.sessions, 0, "sessions after nage user add");
    let sam = credentials(dir.path(), ADMIN)
        .unwrap_or_else(|| panic!("the admin is gone after nage user add"));
    assert_eq!(sam.user, initialized.admin, "the admin's row");
    assert_check_ok(dir.path());
}

/// `--by` and `--username` are found by the fold of what is typed: `--by SAM` adds a user in
/// the name of the stored admin `sam`, and `--by Ｓａｍ --username ALEX`, with full-width
/// letters and capitals, resets the password of the stored user `alex` in sam's name. Each
/// line prints the usernames as stored.
///
/// Proves: decision 113 (`--by` and `--username` are found by the fold of what is typed),
/// decision 95 (the fold: NFKC and case).
#[test]
fn the_admin_and_the_user_are_found_by_the_fold_of_what_is_typed() {
    let (dir, initialized) = set_up();
    let out = add(
        dir.path(),
        "SAM",
        "alex",
        "  Alex ",
        ALEX_PASSWORD.as_bytes(),
    );
    assert_done(&out, &added("alex", "Alex", "sam"));
    let alex = credentials(dir.path(), "alex")
        .unwrap_or_else(|| panic!("nage user add --by SAM created no user alex"));
    assert_eq!(
        alex.user.created_by,
        Some(initialized.admin.id),
        "who created the user added with --by SAM"
    );

    let out = reset(
        dir.path(),
        "\u{ff33}\u{ff41}\u{ff4d}",
        "ALEX",
        NEW_PASSWORD.as_bytes(),
    );
    assert_done(&out, &reset_done("alex", "sam"));
    let alex =
        credentials(dir.path(), "alex").unwrap_or_else(|| panic!("alex is gone after the reset"));
    assert_eq!(
        alex.user.updated_by,
        Some(initialized.admin.id),
        "who reset the password with --by in full-width letters"
    );
    assert_hashed_as_spec_5_3(&alex, NEW_PASSWORD);
}

/// `nage user add --by sma`, a username nobody has, is refused with the command's own line,
/// which quotes what was typed, exit code 1, before the password is read (the password on
/// standard input is one the rules refuse, and the line is the admin's). Nobody is added.
///
/// Proves: decision 113 (`--by` must name an active admin: one that names nobody is refused
/// in the command's own words, quoting what was typed; a refusal prints one line in three
/// parts and exits 1).
#[test]
fn user_add_refuses_a_by_that_names_nobody() {
    let (dir, _) = set_up();
    let before = holds(dir.path());
    let out = add(dir.path(), "sma", "alex", "Alex", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &added_by_nobody("sma"));
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after a refused nage user add"
    );
}

/// `nage user add --by alex`, where alex is a user and not an admin, is refused in the core's
/// words, exit code 1, before the password is read. Nobody is added.
///
/// Proves: decision 96 (`--by` must name an active admin; the command refuses another before the
/// password, in the core's words), decision 113 (for an add as for a reset), decision 114 (the
/// core's message).
#[test]
fn user_add_refuses_a_by_who_is_not_an_admin_in_the_cores_words() {
    let (dir, initialized) = set_up();
    add_user(
        dir.path(),
        initialized.admin.id,
        "alex",
        ALEX_PASSWORD,
        false,
    );
    let before = holds(dir.path());
    let out = add(dir.path(), "ALEX", "robin", "Robin", TOO_SHORT.as_bytes());
    assert_stopped(
        &out,
        1,
        &UserError::CreatorNotAdmin {
            username: String::from("alex"),
        }
        .to_string(),
    );
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after a refused nage user add"
    );
    assert!(
        credentials(dir.path(), "robin").is_none(),
        "a refused nage user add created the user robin"
    );

    // An admin who is deactivated is refused the same way.
    let deputy = add_user(
        dir.path(),
        initialized.admin.id,
        "deputy",
        ALEX_PASSWORD,
        true,
    );
    common::commands::block_on(async {
        let storage = common::commands::open(dir.path()).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin the deactivation: {error}"));
        nage_core::test_support::deactivate_unchecked(
            &mut tx,
            deputy.id,
            initialized.admin.id,
            common::commands::earlier(),
        )
        .await;
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the deactivation: {error}"));
        storage.close().await;
    });
    let before = holds(dir.path());
    let out = add(dir.path(), "deputy", "robin", "Robin", TOO_SHORT.as_bytes());
    assert_stopped(
        &out,
        1,
        &UserError::CreatorNotAdmin {
            username: String::from("deputy"),
        }
        .to_string(),
    );
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after nage user add --by a deactivated admin"
    );
}

/// `nage user add --username Sam`, when the admin is already called sam, is refused in the
/// core's words, exit code 1, before the password is read. Nobody is added and sam's row
/// stays as it was.
///
/// Proves: decision 95 (usernames are unique by the fold), decision 113 (a refusal exits 1
/// with one line), decision 114 (the core's message).
#[test]
fn user_add_refuses_a_username_that_is_taken_in_the_cores_words() {
    let (dir, initialized) = set_up();
    let before = holds(dir.path());
    let out = add(dir.path(), "sam", "Sam", "Samantha", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &UserError::UsernameTaken.to_string());
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after a refused nage user add"
    );
    let sam = credentials(dir.path(), ADMIN)
        .unwrap_or_else(|| panic!("the admin is gone after a refused nage user add"));
    assert_eq!(sam.user, initialized.admin, "the admin's row");
}

/// A username or a display name that the core's name rules refuse is refused in the core's
/// words, exit code 1, before the password is read: an empty username, a display name of 65
/// characters. Nobody is added.
///
/// Proves: decision 113 (a refusal exits 1 with one line), decision 114 (the user refusals
/// `user add` shows first, in the three parts), decision 95 (the name rules).
#[test]
fn user_add_refuses_a_name_the_name_rules_refuse_in_the_cores_words() {
    let (dir, _) = set_up();
    let before = holds(dir.path());
    let long = "a".repeat(65);
    let cases: [(&str, &str, String); 2] = [
        (
            "",
            "Alex",
            UserError::Username(NameError::Empty).to_string(),
        ),
        (
            "alex",
            &long,
            UserError::DisplayName(NameError::TooLong { chars: 65 }).to_string(),
        ),
    ];
    for (username, display_name, line) in cases {
        let out = add(
            dir.path(),
            "sam",
            username,
            display_name,
            TOO_SHORT.as_bytes(),
        );
        assert_stopped(&out, 1, &line);
        assert_eq!(
            holds(dir.path()),
            before,
            "the database after nage user add --username {username:?}"
        );
    }
}

/// A password that breaks a rule is refused in the core's words, and an empty standard input
/// with the command's own line, each with exit code 1. Nobody is added.
///
/// Proves: decision 107 (every place that sets a password applies the same rules with the
/// same messages), decision 113 (`--password-stdin`; a refusal exits 1 with one line).
#[test]
fn user_add_refuses_an_unusable_password_and_adds_nobody() {
    let (dir, _) = set_up();
    let before = holds(dir.path());
    let cases: [(&[u8], String); 2] = [
        (TOO_SHORT.as_bytes(), PasswordError::TooShort.to_string()),
        (b"", no_password_on_stdin(USER_NOT_CREATED)),
    ];
    for (typed, line) in cases {
        let out = add(dir.path(), "sam", "alex", "Alex", typed);
        assert_stopped(&out, 1, &line);
        assert_eq!(
            holds(dir.path()),
            before,
            "the database after nage user add with the standard input {typed:?}"
        );
    }
}

/// `nage user add` and `nage user reset-password` open the database by `check`'s rules: a
/// data directory that does not exist, a database behind this binary and one a newer Nage
/// wrote are each refused with the command's own line, which names the database and what to
/// do, exit code 1, before the password is read. Nothing is created and nothing is migrated:
/// the missing data directory is still missing, and the two databases are at the schema
/// version they had.
///
/// Proves: decision 113 (`user add` and `user reset-password` open an existing database by
/// `check`'s rules; a refusal prints one line in three parts and exits 1), decision 97 (never
/// created, never migrated, refused unless at this binary's schema version).
#[test]
fn the_user_commands_refuse_a_database_they_cannot_use_and_touch_nothing() {
    let expects = Storage::latest_schema_version();
    let missing = TestDir::new();
    let behind = behind();
    let ahead = ahead();
    let lines = |not_done: &str| {
        [
            no_database(not_done, &printed_database(missing.path())),
            database_behind(not_done, &printed_database(behind.path()), 1, expects),
            database_newer(not_done, &printed_database(ahead.path()), 9999),
        ]
    };
    let dirs = [missing.path(), behind.path(), ahead.path()];
    let versions = [None, schema_version(dirs[1]), schema_version(dirs[2])];
    for (dir, line) in dirs.iter().zip(lines(USER_NOT_CREATED)) {
        let out = add(dir, "sam", "alex", "Alex", TOO_SHORT.as_bytes());
        assert_stopped(&out, 1, &line);
    }
    for (dir, line) in dirs.iter().zip(lines(PASSWORD_NOT_CHANGED)) {
        let out = reset(dir, "sam", "sam", TOO_SHORT.as_bytes());
        assert_stopped(&out, 1, &line);
    }
    assert!(
        !missing.path().join("data").exists(),
        "a user command created the data directory"
    );
    for dir in &dirs[1..] {
        assert_no_side_files(
            &database(dir),
            "after the user commands refused the database",
        );
    }
    for (dir, version) in dirs.iter().zip(versions).skip(1) {
        assert_eq!(
            schema_version(dir),
            version,
            "the schema version of {} after the user commands",
            database(dir).display()
        );
    }
}

/// On a database that `nage serve` created and nobody set up, both commands are refused with
/// the command's own line, which says to set Nage up first or to name the right data
/// directory, exit code 1, before the password is read. Nothing is written.
///
/// Proves: decision 113 (a refusal prints one line in three parts and exits 1; `--by` must
/// name an active admin, and a book without users has none).
#[test]
fn the_user_commands_refuse_a_database_that_is_not_set_up() {
    let dir = migrated();
    let before = holds(dir.path());
    let out = add(dir.path(), "sam", "alex", "Alex", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &not_set_up(USER_NOT_CREATED));
    let out = reset(dir.path(), "sam", "sam", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &not_set_up(PASSWORD_NOT_CHANGED));
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after the refused user commands"
    );
}

/// Without `--password-stdin` and without a terminal (each command runs in a session of its own,
/// with standard input on `/dev/null`), `nage user add` and `nage user reset-password` are usage
/// errors: exit code 2 and one line naming both ways out. Nobody is added, and the password and the
/// sessions of the user to reset stay as they were.
///
/// Proves: decision 113 (with no terminal and no `--password-stdin` the command is a usage
/// error, exit 2), decision 45.
#[test]
fn the_user_commands_without_a_terminal_and_without_password_stdin_are_usage_errors() {
    let (dir, initialized) = set_up();
    let device = sign_in(dir.path(), initialized.admin.id, 1);
    let before = holds(dir.path());
    let out = command(dir.path(), &add_args("sam", "alex", "Alex"), None);
    assert_stopped(&out, 2, &no_terminal(USER_NOT_CREATED));
    let out = command(dir.path(), &reset_args("sam", "sam"), None);
    assert_stopped(&out, 2, &no_terminal(PASSWORD_NOT_CHANGED));
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after the user commands without a terminal"
    );
    assert!(
        signed_in(dir.path(), &device),
        "a refused reset ended the admin's session"
    );
    let sam = credentials(dir.path(), ADMIN)
        .unwrap_or_else(|| panic!("the admin is gone after the refused commands"));
    assert_eq!(
        password::verify(PASSWORD, &sam.password_hash),
        Verification::Verified,
        "the admin's password after a refused reset"
    );
}

/// `nage user add` without `--data-dir` and `NAGE_DATA_DIR` uses `./data` relative to the
/// working directory; `nage user reset-password` uses `NAGE_DATA_DIR`, and its `--data-dir`
/// wins over the variable; so does the `--data-dir` of `nage user add`.
///
/// Proves: decision 113 (`user add` defaults to `./data` and takes `--data-dir` over
/// `NAGE_DATA_DIR`; `user reset-password` takes `NAGE_DATA_DIR` and `--data-dir`), decision
/// 42 (flag over variable over default).
#[test]
fn the_user_commands_read_the_data_dir_from_the_flag_the_variable_and_the_default() {
    let (dir, _) = set_up();
    let data = dir.path().join("data");
    let elsewhere = TestDir::new();

    let default = [
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
    let out = run_with_stdin(
        nage().args(default).current_dir(dir.path()),
        ALEX_PASSWORD.as_bytes(),
    );
    assert_done(&out, &added("alex", "Alex", "sam"));

    let by_variable = [
        "user",
        "reset-password",
        "--by",
        "sam",
        "--username",
        "alex",
        "--password-stdin",
    ];
    let out = run_with_stdin(
        nage()
            .args(by_variable)
            .env("NAGE_DATA_DIR", &data)
            .current_dir(elsewhere.path()),
        NEW_PASSWORD.as_bytes(),
    );
    assert_done(&out, &reset_done("alex", "sam"));

    let out = run_with_stdin(
        nage()
            .args(by_variable)
            .arg("--data-dir")
            .arg(&data)
            .env("NAGE_DATA_DIR", "nowhere")
            .current_dir(elsewhere.path()),
        ALEX_PASSWORD.as_bytes(),
    );
    assert_done(&out, &reset_done("alex", "sam"));
    let out = run_with_stdin(
        nage()
            .args([
                "user",
                "add",
                "--by",
                "sam",
                "--username",
                "robin",
                "--display-name",
                "Robin",
                "--password-stdin",
            ])
            .arg("--data-dir")
            .arg(&data)
            .env("NAGE_DATA_DIR", "nowhere")
            .current_dir(elsewhere.path()),
        ALEX_PASSWORD.as_bytes(),
    );
    assert_done(&out, &added("robin", "Robin", "sam"));
    assert!(
        !elsewhere.path().join("nowhere").exists() && !elsewhere.path().join("data").exists(),
        "a user command created a data directory"
    );
    let alex = credentials(dir.path(), "alex").unwrap_or_else(|| panic!("alex is gone"));
    assert_eq!(
        password::verify(ALEX_PASSWORD, &alex.password_hash),
        Verification::Verified,
        "alex's password after the reset through --data-dir"
    );
}

/// `nage user reset-password --by sam --username alex` with the new password on standard
/// input, sent with a space at each end, stores the new hash of the password as it arrived,
/// the spaces included, made with SPEC 5.3's parameters, writes that sam changed alex at the
/// system clock's instant, and ends both of alex's sessions while sam's stays. The old
/// password no longer verifies; nothing else about alex changes. The command prints its line
/// on standard output and nothing on standard error, leaves no `-wal` or `-shm` file, and
/// `nage check` finds the book sound.
///
/// Proves: SPEC 5.3 (a password reset revokes the user's sessions), decision 112 (an admin's
/// reset stores the new hash, writes `updated_by` and `updated_at` and ends every session of
/// the user; nothing forces a change afterwards), decision 113 (`nage user reset-password`
/// takes `--by` and `--username`; the password given through `--password-stdin` is hashed
/// with the spaces at its ends), INV-26's Core half (who changed the user), decision 107 (a
/// password is never trimmed).
#[test]
fn reset_password_gives_the_user_a_new_password_and_ends_their_sessions() {
    let (dir, initialized) = set_up();
    let sam = initialized.admin.id;
    let alex = add_user(dir.path(), sam, "alex", ALEX_PASSWORD, false);
    let phone = sign_in(dir.path(), alex.id, 1);
    let laptop = sign_in(dir.path(), alex.id, 2);
    let sams = sign_in(dir.path(), sam, 3);

    let typed = format!(" {NEW_PASSWORD} ");
    let before = now();
    let out = reset(dir.path(), "sam", "alex", format!("{typed}\n").as_bytes());
    let after = now();
    assert_done(&out, &reset_done("alex", "sam"));
    assert_no_side_files(&database(dir.path()), "after nage user reset-password");

    let found =
        credentials(dir.path(), "alex").unwrap_or_else(|| panic!("alex is gone after the reset"));
    assert_hashed_as_spec_5_3(&found, &typed);
    assert_eq!(
        password::verify(ALEX_PASSWORD, &found.password_hash),
        Verification::WrongPassword,
        "alex's old password after the reset"
    );
    assert_eq!(found.user.updated_by, Some(sam), "who changed alex");
    assert!(
        found
            .user
            .updated_at
            .is_some_and(|at| before <= at && at <= after),
        "alex was changed at {:?}, outside {before} to {after}",
        found.user.updated_at
    );
    let mut unchanged = found.user.clone();
    unchanged.updated_by = None;
    unchanged.updated_at = None;
    assert_eq!(unchanged, alex, "the rest of alex's row");

    assert!(
        !signed_in(dir.path(), &phone) && !signed_in(dir.path(), &laptop),
        "the reset left a session of alex's"
    );
    assert!(
        signed_in(dir.path(), &sams),
        "the reset ended the admin's session"
    );
    assert_eq!(holds(dir.path()).sessions, 1, "sessions after the reset");
    assert_check_ok(dir.path());
}

/// `--by` may name the user being reset: the only admin, having forgotten the password,
/// resets it at the server's terminal. The new password verifies, the admin is the one who
/// changed the row, the admin's own sessions are ended, and the line says to sign in with the
/// new password.
///
/// Proves: decision 112 (the acting admin may be the user being reset, so an admin who
/// forgets their password recovers at the server's terminal), decision 113 (`--by` may name
/// the user being reset).
#[test]
fn reset_password_may_name_the_admin_being_reset() {
    let (dir, initialized) = set_up();
    let sam = initialized.admin.id;
    let device = sign_in(dir.path(), sam, 1);
    let out = reset(dir.path(), "sam", "sam", NEW_PASSWORD.as_bytes());
    assert_done(&out, &reset_own_done("sam"));
    let found = credentials(dir.path(), "sam")
        .unwrap_or_else(|| panic!("the admin is gone after the reset"));
    assert_hashed_as_spec_5_3(&found, NEW_PASSWORD);
    assert_eq!(found.user.updated_by, Some(sam), "who changed the admin");
    assert!(found.user.is_admin, "the admin is no longer an admin");
    assert!(
        !signed_in(dir.path(), &device),
        "the reset left the admin's session"
    );
}

/// A `--by` nobody has and a `--username` nobody has are each refused with the command's own
/// line, which quotes what was typed, exit code 1, before the password is read. No password
/// changes and no session ends.
///
/// Proves: decision 113 (`--by` and `--username` must name a user: one that names nobody is
/// refused in the command's own words, quoting what was typed; a refusal prints one line in
/// three parts and exits 1).
#[test]
fn reset_password_refuses_a_by_or_a_username_that_names_nobody() {
    let (dir, initialized) = set_up();
    let device = sign_in(dir.path(), initialized.admin.id, 1);
    let before = holds(dir.path());
    let out = reset(dir.path(), "sma", "sam", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &reset_by_nobody("sma"));
    let out = reset(dir.path(), "sam", "alexx", TOO_SHORT.as_bytes());
    assert_stopped(&out, 1, &unknown_user("alexx"));
    assert_eq!(
        holds(dir.path()),
        before,
        "the database after the refused resets"
    );
    assert!(
        signed_in(dir.path(), &device),
        "a refused reset ended the admin's session"
    );
}

/// `nage user reset-password --by alex`, where alex is a user and not an admin, is refused in
/// the core's words, which name alex, exit code 1, before the password is read: alex resets
/// neither the admin's password nor their own this way. No password changes and no session
/// ends.
///
/// Proves: decision 113 (`--by` must name an active admin, for a reset as for an add),
/// decision 112, decision 114 (the core's message).
#[test]
fn reset_password_refuses_a_by_who_is_not_an_admin_in_the_cores_words() {
    let (dir, initialized) = set_up();
    add_user(
        dir.path(),
        initialized.admin.id,
        "alex",
        ALEX_PASSWORD,
        false,
    );
    let device = sign_in(dir.path(), initialized.admin.id, 1);
    let line = PasswordChangeError::NotAnAdmin {
        username: String::from("alex"),
    }
    .to_string();
    for username in ["sam", "alex"] {
        let out = reset(dir.path(), "ALEX", username, TOO_SHORT.as_bytes());
        assert_stopped(&out, 1, &line);
    }
    assert!(
        signed_in(dir.path(), &device),
        "a refused reset ended the admin's session"
    );
    for (username, password) in [("sam", PASSWORD), ("alex", ALEX_PASSWORD)] {
        let found = credentials(dir.path(), username)
            .unwrap_or_else(|| panic!("{username} is gone after the refused resets"));
        assert_eq!(
            password::verify(password, &found.password_hash),
            Verification::Verified,
            "the password of {username} after the refused resets"
        );
        assert_eq!(
            found.user.updated_by, None,
            "who changed {username} after the refused resets"
        );
    }
}

/// A new password that breaks a rule is refused in the core's words, and an empty standard
/// input with the command's own line, each with exit code 1: the old password still
/// verifies, the user's row is as it was and the user's session stays.
///
/// Proves: decision 107 (the same rules with the same messages wherever a password is set),
/// decision 113 (a refusal exits 1 with one line and the command changes nothing).
#[test]
fn reset_password_refuses_an_unusable_password_and_changes_nothing() {
    let (dir, initialized) = set_up();
    let device = sign_in(dir.path(), initialized.admin.id, 1);
    let cases: [(&[u8], String); 2] = [
        (TOO_SHORT.as_bytes(), PasswordError::TooShort.to_string()),
        (b"\n", no_password_on_stdin(PASSWORD_NOT_CHANGED)),
    ];
    for (typed, line) in cases {
        let out = reset(dir.path(), "sam", "sam", typed);
        assert_stopped(&out, 1, &line);
    }
    let sam = credentials(dir.path(), ADMIN)
        .unwrap_or_else(|| panic!("the admin is gone after the refused resets"));
    assert_eq!(sam.user, initialized.admin, "the admin's row");
    assert_eq!(
        password::verify(PASSWORD, &sam.password_hash),
        Verification::Verified,
        "the admin's password after the refused resets"
    );
    assert!(
        signed_in(dir.path(), &device),
        "a refused reset ended the admin's session"
    );
}

/// A reset beside the running server: alex is signed in through the sign-in page; the admin
/// resets alex's password at the terminal while the server runs; alex's cookie is then sent
/// to the sign-in page, the old password is refused there and the new one signs alex in.
///
/// Proves: SPEC 5.3 (a password reset revokes the user's sessions), decision 112 (the person
/// signs in with the password the admin chose), decision 113 (the command opens the database
/// the server is using), INV-27 (a session that was ended is no session).
#[test]
fn a_reset_beside_the_running_server_signs_the_user_out_everywhere() {
    let server = serve();
    let dir = server.dir().to_path_buf();
    let initialized = set_up_in(&dir);
    add_user(&dir, initialized.admin.id, "alex", ALEX_PASSWORD, false);
    let mut server = server.spawn();
    let addr = server.wait_listening();

    let sign_in_as = |server: &mut common::ServeProcess, password: &str| {
        server.post_form(
            addr,
            "/login",
            &[],
            &[("username", "alex"), ("password", password)],
        )
    };
    let first = sign_in_as(&mut server, ALEX_PASSWORD);
    assert_eq!(
        (first.status, header(&first, "location")),
        (303, Some("/")),
        "alex's sign-in before the reset:\n{}\n{}",
        first.head,
        String::from_utf8_lossy(&first.body)
    );
    let (token, _secure) = session_cookie(&first);
    let cookie = format!("nage_session={token}");
    let home = server.get(addr, "/", &[("Cookie", &cookie)]);
    assert_eq!(
        home.status, 200,
        "GET / with alex's session before the reset:\n{}",
        home.head
    );

    let out = reset(&dir, "sam", "alex", NEW_PASSWORD.as_bytes());
    assert_done(&out, &reset_done("alex", "sam"));

    let after = server.get(addr, "/", &[("Cookie", &cookie)]);
    assert_eq!(
        (after.status, header(&after, "location")),
        (303, Some("/login")),
        "GET / with alex's session after the reset:\n{}",
        after.head
    );
    let old = sign_in_as(&mut server, ALEX_PASSWORD);
    assert_eq!(
        old.status, 422,
        "alex's sign-in with the old password after the reset:\n{}",
        old.head
    );
    let new = sign_in_as(&mut server, NEW_PASSWORD);
    assert_eq!(
        (new.status, header(&new, "location")),
        (303, Some("/")),
        "alex's sign-in with the new password:\n{}\n{}",
        new.head,
        String::from_utf8_lossy(&new.body)
    );
}
