//! The texts the three commands word themselves, pinned (decisions 102, 113, 114 and 157), and
//! each outcome of the commands as it reads at the terminal: three labelled lines for a refusal
//! or a failure, a sentence with its facts and the next step for a result.

use std::io;
use std::path::PathBuf;

use nage_core::book::BookError;
use nage_core::fold::NameError;
use nage_core::password::{HashError, PasswordError};
use nage_core::setup::SetupError;
use nage_core::storage::{self, BusyCause, Storage};
use nage_core::timestamp::TimestampError;
use nage_core::users::{PasswordChangeError, UserError};

use super::{Done, QUESTION_AGAIN, Refusal, Stop, Task, question};
use crate::database::OpenError;
use crate::shape::{Doing, Paint, Refused, Why};

/// The data folder's database of the examples.
const DATABASE: &str = "/srv/nage/data/nage.sqlite";

/// The three tasks.
const TASKS: [Task; 3] = [Task::SetUp, Task::AddUser, Task::ResetPassword];

/// The lines `refused` prints, without colour.
fn rendered(refused: &Refused) -> String {
    refused.render(Paint::PLAIN)
}

/// Asserts that `text` is three labelled lines, `Error: `, `Why: ` and `What now: `, each part a
/// sentence of its own that ends with a full stop, and returns the three parts.
#[track_caller]
fn three_lines(what: &str, text: &str) -> [String; 3] {
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{what} is not three lines:\n{text}");
    assert!(
        text.ends_with('\n'),
        "{what} does not end its last line:\n{text}"
    );
    let mut parts = Vec::new();
    for (line, label) in lines.iter().zip(["Error: ", "Why: ", "What now: "]) {
        let part = line
            .strip_prefix(label)
            .unwrap_or_else(|| panic!("{what}: {line:?} does not begin with {label:?}"));
        assert!(
            !part.is_empty() && !part.starts_with(' '),
            "{what}: the part after {label:?} is empty or begins with a space: {line:?}"
        );
        assert!(
            part.ends_with('.'),
            "{what}: the part after {label:?} does not end with a full stop: {line:?}"
        );
        parts.push(part.to_owned());
    }
    [parts[0].clone(), parts[1].clone(), parts[2].clone()]
}

/// Every refusal the commands word themselves, for every task it can end, with `typed` as each
/// typed value and `path` as each path.
fn refusals(typed: &str, path: &str) -> Vec<Refusal> {
    let typed = || typed.to_owned();
    let path = || PathBuf::from(path);
    let mut refusals = vec![
        Refusal::CurrencyMissing,
        Refusal::UnknownCurrency { typed: typed() },
        Refusal::AddedByNobody { typed: typed() },
        Refusal::ResetByNobody { typed: typed() },
        Refusal::UnknownUser { typed: typed() },
    ];
    for task in TASKS {
        refusals.push(Refusal::NoTerminal { task });
        refusals.push(Refusal::PasswordsDiffer { task });
        refusals.push(Refusal::NoPasswordOnStdin { task });
        refusals.push(Refusal::PasswordNotText { task });
        refusals.push(Refusal::NoDatabase { task, path: path() });
        refusals.push(Refusal::DatabaseBehind {
            task,
            path: path(),
            version: 1,
            expects: 2,
        });
        refusals.push(Refusal::DatabaseNewer {
            task,
            path: path(),
            version: 3,
        });
        refusals.push(Refusal::NotSetUp { task });
    }
    refusals
}

/// The task a refusal ends.
fn task_of(refusal: &Refusal) -> Task {
    match refusal {
        Refusal::CurrencyMissing | Refusal::UnknownCurrency { .. } => Task::SetUp,
        Refusal::AddedByNobody { .. } => Task::AddUser,
        Refusal::ResetByNobody { .. } | Refusal::UnknownUser { .. } => Task::ResetPassword,
        Refusal::NoTerminal { task }
        | Refusal::PasswordsDiffer { task }
        | Refusal::NoPasswordOnStdin { task }
        | Refusal::PasswordNotText { task }
        | Refusal::NoDatabase { task, .. }
        | Refusal::DatabaseBehind { task, .. }
        | Refusal::DatabaseNewer { task, .. }
        | Refusal::NotSetUp { task } => *task,
    }
}

/// Every failure of the database a command can meet, its paths `path`.
fn database_failures(path: &str) -> Vec<OpenError> {
    let path = || PathBuf::from(path);
    vec![
        OpenError::Missing(path()),
        OpenError::Empty(path()),
        OpenError::Open {
            path: path(),
            source: storage::Error::Busy(BusyCause::WriteQueue),
        },
        OpenError::Behind {
            path: path(),
            version: 1,
            expects: 2,
        },
        OpenError::Newer {
            path: path(),
            version: 3,
        },
        OpenError::DataDir {
            path: path(),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        },
        OpenError::Create {
            path: path(),
            source: io::Error::from(io::ErrorKind::AlreadyExists),
        },
        OpenError::Migrate {
            path: path(),
            source: storage::Error::Busy(BusyCause::WriteQueue),
        },
    ]
}

/// Every way a command can stop that the core words or that nobody at the terminal caused.
fn core_stops_and_failures(path: &str) -> Vec<Stop> {
    let mut stops = vec![
        Stop::Password(HashError::Password(PasswordError::TooShort)),
        Stop::Password(HashError::Hasher(String::from("no salt"))),
        Stop::Book(BookError::Name(NameError::Empty)),
        Stop::Setup(SetupError::AlreadyInitialized),
        Stop::User(UserError::UsernameTaken),
        Stop::User(UserError::CreatorNotAdmin {
            username: String::from("alex"),
        }),
        Stop::PasswordChange(PasswordChangeError::NotAnAdmin {
            username: String::from("alex"),
        }),
        Stop::Failure(String::from("the database is busy")),
        Stop::Failure(String::from(
            "the system clock cannot be used: out of range",
        )),
    ];
    stops.extend(database_failures(path).into_iter().map(Stop::Database));
    stops
}

/// Every refusal and every failure a command can end with renders as exactly three labelled
/// lines, each part one line ending with a full stop, whatever a typed value or a path holds: a
/// typed value with a line break and a quotation mark is quoted as Rust quotes a string, a path
/// with a line break shows it escaped, so neither breaks its line. The two passwords that differ
/// are pinned as the lines `nage init` prints, and a database a newer Nage wrote names its change
/// in four digits (0003), for the commands and for `nage check`.
///
/// Proves: decisions 157 and 159, ruling P9 (each part one line whatever a typed value or a path holds; the two passwords that differ pinned as rendered lines)
#[test]
fn every_refusal_and_failure_renders_as_three_lines() {
    let typed = "sam\nWhat now: \"run\" rm";
    let path = "/srv/nage/da\nta/nage.sqlite";
    let (mut quoted, mut escaped) = (0_usize, 0_usize);
    for refusal in refusals(typed, path) {
        let task = task_of(&refusal);
        let what = format!("{refusal:?}");
        let text = rendered(&Stop::Refusal(refusal.clone()).refused(task));
        let [error, why, _] = three_lines(&what, &text);
        assert_eq!(error, task.doing().not_done(), "the first line of {what}");
        if why.contains("What now") {
            assert!(
                why.contains(r#""sam\nWhat now: \"run\" rm""#),
                "{what}: the typed value is not quoted as Rust quotes it: {why}"
            );
            quoted += 1;
        }
        if why.contains("/srv/nage") {
            assert!(
                why.contains(r"/srv/nage/da\nta/nage.sqlite"),
                "{what}: the path is not shown escaped: {why}"
            );
            escaped += 1;
        }
    }
    // The four refusals that name a typed value, and the two of each task that name a path (a
    // database a newer Nage wrote is said in the sentence every command shares, which names none).
    assert_eq!(
        (quoted, escaped),
        (4, 6),
        "the refusals that show a typed value and a path"
    );
    for task in TASKS {
        for stop in core_stops_and_failures(path) {
            let what = format!("{stop:?} for {task:?}");
            let text = rendered(&stop.refused(task));
            let [error, why, _] = three_lines(&what, &text);
            assert_eq!(error, task.doing().not_done(), "the first line of {what}");
            if why.contains("/srv/nage") {
                assert!(
                    why.contains(r"/srv/nage/da\nta"),
                    "{what}: the path is not shown escaped: {why}"
                );
            }
        }
    }
    let lines = rendered(
        &Stop::Refusal(Refusal::PasswordsDiffer { task: Task::SetUp }).refused(Task::SetUp),
    );
    insta::assert_snapshot!("passwords_differ", lines);

    // A change a newer Nage wrote is named in four digits, as migrations/ numbers it, whether the
    // command words it (init and the user commands) or nage check does, in the one sentence every
    // command shares, with the newest change this version knows as the core reads it.
    let newer_why = format!(
        "A newer Nage wrote this database: it carries the change 0003, and this version knows up to {:04}.",
        Storage::latest_schema_version()
    );
    let newer = Stop::Refusal(Refusal::DatabaseNewer {
        task: Task::SetUp,
        path: PathBuf::from(DATABASE),
        version: 3,
    });
    let [_, why, now] = three_lines("a newer database", &rendered(&newer.refused(Task::SetUp)));
    assert_eq!(why, newer_why);
    assert_eq!(now, "Run the command with the newer Nage.");
    let newer = OpenError::Newer {
        path: PathBuf::from(DATABASE),
        version: 3,
    };
    let [_, why, now] = three_lines(
        "a newer database, checked",
        &rendered(&newer.refused(Doing::Check)),
    );
    assert_eq!(why, newer_why);
    assert_eq!(now, "Run the command with the newer Nage.");
}

/// A failure nobody at the terminal caused prints its three parts: what the command was doing,
/// what failed with the system's own words in one pair of brackets at the end (never the
/// wrapping words around them), and to run the command again and, when it fails again, to keep
/// the lines for the deployment guide; a failure the command words as text is a sentence of its
/// own, its first letter a capital: a clock the core cannot stamp with and a busy database, as the
/// core's errors become failures, read in plain words; and a database file another `nage init`
/// created first is said whole.
///
/// Proves: decision 159 (what a command prints for a failure), inherited gap s5-evidence-cli-r2-6
#[test]
fn a_failure_prints_its_three_parts() {
    let again = "Run the command again; if it fails again, keep the lines above: the deployment guide says where to report them.";
    let failure = Stop::Failure(String::from(
        "the terminal could not be read: no such device",
    ));
    assert_eq!(
        rendered(&failure.refused(Task::AddUser)),
        format!(
            "Error: The user was not created.\nWhy: The terminal could not be read: no such device.\nWhat now: {again}\n"
        )
    );
    let hasher = Stop::Password(HashError::Hasher(String::from("no salt")));
    let [error, why, now] = three_lines("the hasher", &rendered(&hasher.refused(Task::SetUp)));
    assert_eq!(error, "The book was not set up.");
    assert!(
        why.starts_with("Something failed that nobody at the terminal caused (")
            && why.ends_with(").")
            && why.matches('(').count() == 1,
        "the hasher's reason: {why}"
    );
    assert_eq!(now, again);
    let data_dir = Stop::Database(OpenError::DataDir {
        path: PathBuf::from("/srv/nage/data"),
        source: io::Error::from_raw_os_error(13),
    });
    assert_eq!(
        rendered(&data_dir.refused(Task::SetUp)),
        "Error: The book was not set up.\nWhy: The data folder /srv/nage/data could not be created (Permission denied, os error 13).\nWhat now: Check that this account may create it, or name another folder with --data-dir, then run the command again.\n"
    );
    let migrate = Stop::Database(OpenError::Migrate {
        path: PathBuf::from(DATABASE),
        source: storage::Error::Busy(BusyCause::WriteQueue),
    });
    let [error, why, now] = three_lines("the update", &rendered(&migrate.refused(Task::SetUp)));
    assert_eq!(error, "The book was not set up.");
    assert!(
        why.starts_with(&format!(
            "The new database at {DATABASE} could not be prepared ("
        )) && why.ends_with(").")
            && why.matches('(').count() == 1,
        "the update's reason: {why}"
    );
    assert_eq!(
        now,
        "Run the command again, which finishes it; if it fails again, keep the lines above: the deployment guide says where to report them."
    );

    // The clock and a storage failure, as the core's errors become a failure: each in plain words,
    // never the program's inner names.
    let clock = Stop::from(TimestampError::OutOfRange);
    assert_eq!(
        rendered(&clock.refused(Task::SetUp)),
        format!(
            "Error: The book was not set up.\nWhy: The system clock cannot be used: outside 2020-01-01T00:00:00Z to 2199-12-31T23:59:59Z.\nWhat now: {again}\n"
        )
    );
    let busy = Stop::from(storage::Error::Busy(BusyCause::WriteQueue));
    assert_eq!(
        rendered(&busy.refused(Task::ResetPassword)),
        format!(
            "Error: The password was not changed.\nWhy: The database is busy: the write queue timed out.\nWhat now: {again}\n"
        )
    );

    // Another nage init created the database file first.
    let create = Stop::Database(OpenError::Create {
        path: PathBuf::from(DATABASE),
        source: io::Error::from_raw_os_error(17),
    });
    assert_eq!(
        rendered(&create.refused(Task::SetUp)),
        format!(
            "Error: The book was not set up.\nWhy: The database file {DATABASE} could not be created (File exists, os error 17).\nWhat now: If another nage init is running on this folder, wait for it to end; otherwise check the folder's permissions, then run the command again.\n"
        )
    );
}

/// Each refusal a command words itself says what was not done (the command's own first line),
/// why and what to do instead, each part one line of its own; no two refusals of one task share
/// their reason and what now.
///
/// Proves: decision 102 (the three parts), decision 113 (what each command says), decision 157
/// (a refusal is three labelled lines whose first names what the command was doing).
#[test]
fn command_refusals_explain_themselves() {
    let mut seen: Vec<(String, String, String)> = Vec::new();
    for refusal in refusals("sma", DATABASE) {
        let task = task_of(&refusal);
        let what = format!("{refusal:?}");
        let parts = refusal.refused();
        assert_eq!(
            parts.what,
            task.doing().not_done(),
            "what {what} did not do"
        );
        assert!(
            matches!(parts.why, Why::Line(_)),
            "{what} gives its reason as a list"
        );
        let [error, why, now] = three_lines(&what, &rendered(&parts));
        println!("[{what}]\n{}", rendered(&parts));
        let key = (error, why, now);
        assert!(!seen.contains(&key), "two refusals share the text {key:?}");
        seen.push(key);
    }
}

/// What each command prints when it is done names what was done, its facts with the names as the
/// core stored them, and the next step: `nage init` with or without the starter accounts and the
/// database's path, which stays on one line when the data folder's name holds a line break; `nage
/// user add`; `nage user reset-password` for another user and for the admin's own password.
///
/// Proves: decision 113 (what each command's result says, with the names it is handed), decision
/// 112 (an admin who reset their own password is told to sign in with it, and another user can
/// change theirs after signing in), decision 157 (a sentence, its facts, the next step).
#[test]
fn command_results_say_what_was_done() {
    let set_up = |starter_chart| Done::SetUp {
        book_name: String::from("Household"),
        currency: "CAD",
        starter_chart,
        database: PathBuf::from(DATABASE),
        username: String::from("sam"),
        display_name: String::from("Sam"),
    };
    let cases: [(Done, &str); 5] = [
        (
            set_up(true),
            "Success: The book \"Household\" was set up.\n  Currency: CAD\n  Starter accounts: added\n  Admin: sam (Sam)\n  Database: /srv/nage/data/nage.sqlite\nWhat now: Start the server if it is not running; sam can then sign in.\n",
        ),
        (
            set_up(false),
            "Success: The book \"Household\" was set up.\n  Currency: CAD\n  Starter accounts: left out\n  Admin: sam (Sam)\n  Database: /srv/nage/data/nage.sqlite\nWhat now: Start the server if it is not running; sam can then sign in.\n",
        ),
        (
            Done::UserAdded {
                username: String::from("alex"),
                display_name: String::from("Alex"),
                by: String::from("sam"),
            },
            "Success: alex (Alex) was added.\n  Added by: sam\n  Admin: no\nWhat now: Tell alex the password; alex can sign in now and change it afterwards.\n",
        ),
        (
            Done::PasswordReset {
                username: String::from("alex"),
                by: String::from("sam"),
            },
            "Success: The password of alex was reset.\n  Reset by: sam\n  Signed out: alex, on every device\nWhat now: Tell alex the new password; alex can change it after signing in.\n",
        ),
        (
            Done::PasswordReset {
                username: String::from("sam"),
                by: String::from("sam"),
            },
            "Success: The password of sam was reset.\n  Reset by: sam\n  Signed out: sam, on every device\nWhat now: Sign in with the new password.\n",
        ),
    ];
    for (done, text) in cases {
        assert_eq!(
            done.finished().render(Paint::PLAIN),
            text,
            "the result of {done:?}"
        );
    }

    // A data folder whose name holds a line break: the database's fact stays on one line.
    let broken_folder = Done::SetUp {
        book_name: String::from("Household"),
        currency: "CAD",
        starter_chart: true,
        database: PathBuf::from("/srv/nage/da\nta/nage.sqlite"),
        username: String::from("sam"),
        display_name: String::from("Sam"),
    };
    assert_eq!(
        broken_folder.finished().render(Paint::PLAIN),
        "Success: The book \"Household\" was set up.\n  Currency: CAD\n  Starter accounts: added\n  Admin: sam (Sam)\n  Database: /srv/nage/da\\nta/nage.sqlite\nWhat now: Start the server if it is not running; sam can then sign in.\n",
        "the result in a data folder whose name holds a line break"
    );
}

/// The first question names whose password is asked for, and for a reset that it is a new
/// one; the second question is the same for every command; both end with a colon and a space,
/// after which the terminal waits.
///
/// Proves: decision 113 (the words of the two questions a command asks for the password with).
#[test]
fn the_two_questions_say_whose_password_is_asked_for() {
    assert_eq!(question(Task::SetUp, "sam"), "Password for sam: ");
    assert_eq!(question(Task::AddUser, "alex"), "Password for alex: ");
    assert_eq!(
        question(Task::ResetPassword, "alex"),
        "New password for alex: "
    );
    assert_eq!(QUESTION_AGAIN, "The same password again: ");
}

/// A database that cannot be used ends a command in the command's own words when the reason
/// is one the person at the terminal can act on: no database at the path, a database behind
/// this binary, a database ahead of it; each refusal carries the task and the path, and the
/// last two the database's version (the one behind also the version this binary expects).
/// `nage init` never meets the first, since it creates a missing database. Every other reason
/// (an empty database, the database cannot be opened, created or updated, the data folder
/// cannot be made) stays a failure and prints as `database::OpenError` words it for the command.
///
/// Proves: decision 113 (a database that is missing, behind or ahead of the binary becomes the
/// command's own refusal, carrying the task and the path, and the version of one behind or
/// ahead), decision 102 (a failure is not a refusal and passes the inner message on).
#[test]
fn a_database_that_cannot_be_used_is_refused_in_the_commands_own_words() {
    let path = || PathBuf::from(DATABASE);
    for task in TASKS {
        let stop = Stop::database(task, OpenError::Missing(path()));
        assert!(
            matches!(
                &stop,
                Stop::Refusal(Refusal::NoDatabase { task: refused, path: named })
                    if *refused == task && *named == path()
            ),
            "a missing database for {task:?}: {stop:?}"
        );
        let stop = Stop::database(
            task,
            OpenError::Behind {
                path: path(),
                version: 1,
                expects: 2,
            },
        );
        assert!(
            matches!(
                &stop,
                Stop::Refusal(Refusal::DatabaseBehind {
                    task: refused,
                    path: named,
                    version: 1,
                    expects: 2,
                }) if *refused == task && *named == path()
            ),
            "a database behind for {task:?}: {stop:?}"
        );
        let stop = Stop::database(
            task,
            OpenError::Newer {
                path: path(),
                version: 9999,
            },
        );
        assert!(
            matches!(
                &stop,
                Stop::Refusal(Refusal::DatabaseNewer {
                    task: refused,
                    path: named,
                    version: 9999,
                }) if *refused == task && *named == path()
            ),
            "a database ahead for {task:?}: {stop:?}"
        );
    }
    let busy = || storage::Error::Busy(BusyCause::WriteQueue);
    let failures = [
        OpenError::Empty(path()),
        OpenError::Open {
            path: path(),
            source: busy(),
        },
        OpenError::DataDir {
            path: PathBuf::from("/srv/nage/data"),
            source: io::Error::other("permission denied"),
        },
        OpenError::Create {
            path: path(),
            source: io::Error::other("file exists"),
        },
        OpenError::Migrate {
            path: path(),
            source: busy(),
        },
    ];
    for error in failures {
        let expected = error.refused(Doing::Init);
        let stop = Stop::database(Task::SetUp, error);
        assert!(
            matches!(&stop, Stop::Database(_)),
            "the failure {expected:?} became {stop:?}"
        );
        assert_eq!(
            stop.refused(Task::SetUp),
            expected,
            "what the failure prints"
        );
    }
}

/// A usage error exits with code 2 and there are exactly two of them: no currency for `nage
/// init`, and no terminal without `--password-stdin`, for each of the three commands. Every
/// other refusal a command words itself, every refusal in the core's words (a password, a
/// name, a book already set up, a username taken, an admin who is not one), a database that
/// cannot be used and a failure exit with code 1.
///
/// Proves: decision 113 (a missing currency and a missing terminal are usage errors, exit 2;
/// a refusal exits 1), decision 45 (2 for usage errors, 1 for runtime failures).
#[test]
fn only_the_two_usage_errors_exit_with_code_2() {
    assert_eq!(
        Stop::Refusal(Refusal::CurrencyMissing).exit_code(),
        2,
        "the exit code without a currency"
    );
    for task in TASKS {
        assert_eq!(
            Stop::Refusal(Refusal::NoTerminal { task }).exit_code(),
            2,
            "the exit code without a terminal for {task:?}"
        );
    }
    let typed = || String::from("x");
    let mut others = vec![
        Stop::Refusal(Refusal::UnknownCurrency { typed: typed() }),
        Stop::Refusal(Refusal::AddedByNobody { typed: typed() }),
        Stop::Refusal(Refusal::ResetByNobody { typed: typed() }),
        Stop::Refusal(Refusal::UnknownUser { typed: typed() }),
        Stop::Password(HashError::Password(PasswordError::TooShort)),
        Stop::Password(HashError::Hasher(String::from("no salt"))),
        Stop::Book(BookError::Name(NameError::Empty)),
        Stop::Setup(SetupError::AlreadyInitialized),
        Stop::User(UserError::UsernameTaken),
        Stop::PasswordChange(PasswordChangeError::NotAnAdmin { username: typed() }),
        Stop::Database(OpenError::Create {
            path: PathBuf::from(DATABASE),
            source: io::Error::other("file exists"),
        }),
        Stop::Failure(String::from("the database is busy")),
    ];
    for task in TASKS {
        others.push(Stop::Refusal(Refusal::PasswordsDiffer { task }));
        others.push(Stop::Refusal(Refusal::NoPasswordOnStdin { task }));
        others.push(Stop::Refusal(Refusal::PasswordNotText { task }));
        others.push(Stop::Refusal(Refusal::NotSetUp { task }));
        others.push(Stop::Refusal(Refusal::NoDatabase {
            task,
            path: PathBuf::from(DATABASE),
        }));
        others.push(Stop::Refusal(Refusal::DatabaseBehind {
            task,
            path: PathBuf::from(DATABASE),
            version: 1,
            expects: 2,
        }));
        others.push(Stop::Refusal(Refusal::DatabaseNewer {
            task,
            path: PathBuf::from(DATABASE),
            version: 3,
        }));
    }
    for stop in others {
        assert_eq!(stop.exit_code(), 1, "the exit code of {stop:?}");
    }
}

/// One line of a terminal transcript.
enum Line {
    /// What is typed at the shell.
    Shell(&'static str),
    /// A question on the terminal; the answer typed after it is not shown.
    Asks(String),
    /// What the command prints on standard output when it is done.
    Prints(String),
    /// The three labelled lines on standard error.
    Stops(String),
}

/// The lines `stop` prints for `task`, without colour.
fn stops(stop: Stop, task: Task) -> Line {
    Line::Stops(rendered(&stop.refused(task)))
}

/// The lines `done` prints, without colour.
fn prints(done: Done) -> Line {
    Line::Prints(done.finished().render(Paint::PLAIN))
}

/// Each outcome of the three commands as it reads at the terminal, put together from the
/// binary's own texts in the order the commands keep: the command line, the two questions when
/// the command gets as far as the password, then either the result or three labelled lines. The
/// test prints every transcript (`cargo test -p nage --bin nage transcripts -- --nocapture`) and
/// checks the form of what it put together: each refusal is three lines, each question ends with
/// a colon and a space, and each transcript holds the command line, two questions or none, and
/// one outcome that fits its exit code. It runs no command: the transcripts of
/// `tests/terminal.rs` hold the real commands to their results and refusals.
///
/// Proves: decision 157 (the text of each refusal, worded by the command or by the core, is three
/// labelled lines; a result is a sentence with its facts).
#[test]
fn the_commands_read_as_these_terminal_transcripts() {
    let asks = |task, username: &str| {
        [
            Line::Asks(question(task, username)),
            Line::Asks(String::from(QUESTION_AGAIN)),
        ]
    };
    let set_up = |starter_chart| {
        prints(Done::SetUp {
            book_name: String::from("Household"),
            currency: "CAD",
            starter_chart,
            database: PathBuf::from(DATABASE),
            username: String::from("sam"),
            display_name: String::from("Sam"),
        })
    };
    let refused = |refusal: Refusal, task| stops(Stop::Refusal(refusal), task);
    let [init_asks_1, init_asks_2] = asks(Task::SetUp, "sam");
    let [differ_asks_1, differ_asks_2] = asks(Task::SetUp, "sam");
    let [short_asks_1, short_asks_2] = asks(Task::SetUp, "sam");
    let [add_asks_1, add_asks_2] = asks(Task::AddUser, "alex");
    let [reset_asks_1, reset_asks_2] = asks(Task::ResetPassword, "alex");
    let [own_asks_1, own_asks_2] = asks(Task::ResetPassword, "sam");
    let [reset_differ_1, reset_differ_2] = asks(Task::ResetPassword, "alex");
    let init = "nage init --book-name Household --currency CAD --username sam --display-name Sam";
    let add = "nage user add --by sam --username alex --display-name Alex";
    let reset = "nage user reset-password --by sam --username alex";
    let scenes: Vec<(&str, &str, u8, Vec<Line>)> = vec![
        (
            "init",
            "Setting Nage up at the server's terminal. The password is typed twice and never shown.",
            0,
            vec![Line::Shell(init), init_asks_1, init_asks_2, set_up(true)],
        ),
        (
            "init",
            "The same from a script: the password comes from a file, and the starter accounts are left out.",
            0,
            vec![
                Line::Shell(
                    "nage init --book-name Household --currency CAD --username sam --display-name Sam --no-starter-accounts --password-stdin < password.txt",
                ),
                set_up(false),
            ],
        ),
        (
            "init",
            "The currency is forgotten. Nothing is asked and nothing is written.",
            2,
            vec![
                Line::Shell("nage init --book-name Household --username sam --display-name Sam"),
                refused(Refusal::CurrencyMissing, Task::SetUp),
            ],
        ),
        (
            "init",
            "The currency is typed in small letters.",
            1,
            vec![
                Line::Shell(
                    "nage init --book-name Household --currency cad --username sam --display-name Sam",
                ),
                refused(
                    Refusal::UnknownCurrency {
                        typed: String::from("cad"),
                    },
                    Task::SetUp,
                ),
            ],
        ),
        (
            "init",
            "The book name is left empty.",
            1,
            vec![
                Line::Shell(
                    "nage init --book-name \"\" --currency CAD --username sam --display-name Sam",
                ),
                stops(Stop::Book(BookError::Name(NameError::Empty)), Task::SetUp),
            ],
        ),
        (
            "init",
            "The second password is typed differently from the first.",
            1,
            vec![
                Line::Shell(init),
                differ_asks_1,
                differ_asks_2,
                refused(Refusal::PasswordsDiffer { task: Task::SetUp }, Task::SetUp),
            ],
        ),
        (
            "init",
            "The password typed, both times the same, has nine characters.",
            1,
            vec![
                Line::Shell(init),
                short_asks_1,
                short_asks_2,
                stops(Stop::from(PasswordError::TooShort), Task::SetUp),
            ],
        ),
        (
            "init",
            "Nage is already set up in this data folder. The password is not asked for.",
            1,
            vec![
                Line::Shell(init),
                stops(Stop::Setup(SetupError::AlreadyInitialized), Task::SetUp),
            ],
        ),
        (
            "init",
            "The command is run where there is no terminal (a script, `docker exec` without `-t`), without `--password-stdin`.",
            2,
            vec![
                Line::Shell(init),
                refused(Refusal::NoTerminal { task: Task::SetUp }, Task::SetUp),
            ],
        ),
        (
            "init",
            "`--password-stdin` is given and nothing is sent.",
            1,
            vec![
                Line::Shell(
                    "nage init --book-name Household --currency CAD --username sam --display-name Sam --password-stdin < /dev/null",
                ),
                refused(
                    Refusal::NoPasswordOnStdin { task: Task::SetUp },
                    Task::SetUp,
                ),
            ],
        ),
        (
            "init",
            "The database in the data folder was left by an older version of Nage and has not been brought up to date yet.",
            1,
            vec![
                Line::Shell(init),
                refused(
                    Refusal::DatabaseBehind {
                        task: Task::SetUp,
                        path: PathBuf::from(DATABASE),
                        version: 1,
                        expects: Storage::latest_schema_version(),
                    },
                    Task::SetUp,
                ),
            ],
        ),
        (
            "user add",
            "sam, an admin, adds alex at the server's terminal and types alex's password twice.",
            0,
            vec![
                Line::Shell(add),
                add_asks_1,
                add_asks_2,
                prints(Done::UserAdded {
                    username: String::from("alex"),
                    display_name: String::from("Alex"),
                    by: String::from("sam"),
                }),
            ],
        ),
        (
            "user add",
            "The admin's username is mistyped. The password is not asked for.",
            1,
            vec![
                Line::Shell("nage user add --by sma --username alex --display-name Alex"),
                refused(
                    Refusal::AddedByNobody {
                        typed: String::from("sma"),
                    },
                    Task::AddUser,
                ),
            ],
        ),
        (
            "user add",
            "`--by` names alex, who is not an admin.",
            1,
            vec![
                Line::Shell("nage user add --by alex --username robin --display-name Robin"),
                stops(
                    Stop::User(UserError::CreatorNotAdmin {
                        username: String::from("alex"),
                    }),
                    Task::AddUser,
                ),
            ],
        ),
        (
            "user add",
            "The new user is to be called Sam, and the admin is already called sam.",
            1,
            vec![
                Line::Shell("nage user add --by sam --username Sam --display-name Samantha"),
                stops(Stop::User(UserError::UsernameTaken), Task::AddUser),
            ],
        ),
        (
            "user add",
            "Nage has not been set up yet: the server was started once and nobody has used the set-up page or `nage init`.",
            1,
            vec![
                Line::Shell(add),
                refused(
                    Refusal::NotSetUp {
                        task: Task::AddUser,
                    },
                    Task::AddUser,
                ),
            ],
        ),
        (
            "user add",
            "The command is run in the wrong folder, where there is no data folder.",
            1,
            vec![
                Line::Shell(add),
                refused(
                    Refusal::NoDatabase {
                        task: Task::AddUser,
                        path: PathBuf::from(DATABASE),
                    },
                    Task::AddUser,
                ),
            ],
        ),
        (
            "user reset-password",
            "alex forgot the password. sam resets it at the server's terminal and types the new one twice.",
            0,
            vec![
                Line::Shell(reset),
                reset_asks_1,
                reset_asks_2,
                prints(Done::PasswordReset {
                    username: String::from("alex"),
                    by: String::from("sam"),
                }),
            ],
        ),
        (
            "user reset-password",
            "sam, the only admin, forgot the password and resets it at the server's terminal.",
            0,
            vec![
                Line::Shell("nage user reset-password --by sam --username sam"),
                own_asks_1,
                own_asks_2,
                prints(Done::PasswordReset {
                    username: String::from("sam"),
                    by: String::from("sam"),
                }),
            ],
        ),
        (
            "user reset-password",
            "The username of the user to reset is mistyped. The password is not asked for.",
            1,
            vec![
                Line::Shell("nage user reset-password --by sam --username alexx"),
                refused(
                    Refusal::UnknownUser {
                        typed: String::from("alexx"),
                    },
                    Task::ResetPassword,
                ),
            ],
        ),
        (
            "user reset-password",
            "`--by` names alex, who is not an admin.",
            1,
            vec![
                Line::Shell("nage user reset-password --by alex --username sam"),
                stops(
                    Stop::PasswordChange(PasswordChangeError::NotAnAdmin {
                        username: String::from("alex"),
                    }),
                    Task::ResetPassword,
                ),
            ],
        ),
        (
            "user reset-password",
            "The second password is typed differently from the first. The old password stays.",
            1,
            vec![
                Line::Shell(reset),
                reset_differ_1,
                reset_differ_2,
                refused(
                    Refusal::PasswordsDiffer {
                        task: Task::ResetPassword,
                    },
                    Task::ResetPassword,
                ),
            ],
        ),
    ];
    for (number, (command, moment, exit, lines)) in scenes.iter().enumerate() {
        let tag = format!("[terminal {} {command}]", number + 1);
        println!("{tag} # {moment}");
        let mut questions = 0_usize;
        let mut results = 0_usize;
        let mut refusals = 0_usize;
        for (index, line) in lines.iter().enumerate() {
            match line {
                Line::Shell(typed) => {
                    assert_eq!(index, 0, "{moment}: the command line is not first");
                    assert!(
                        typed.starts_with(&format!("nage {command} ")),
                        "{moment}: the command line is not `nage {command}`: {typed}"
                    );
                    println!("{tag} $ {typed}");
                }
                Line::Asks(text) => {
                    assert_eq!(
                        index,
                        questions + 1,
                        "{moment}: a question does not come right after the command line"
                    );
                    assert!(
                        text.ends_with(": ") && !text.contains('\n'),
                        "{moment}: the question {text:?} is not one line ending with a colon and a space"
                    );
                    questions += 1;
                    println!("{tag} ? {text}");
                }
                Line::Prints(text) => {
                    assert!(
                        text.starts_with("Success: "),
                        "{moment}: the result does not begin with Success: {text:?}"
                    );
                    results += 1;
                    for printed in text.lines() {
                        println!("{tag} > {printed}");
                    }
                }
                Line::Stops(text) => {
                    three_lines(moment, text);
                    refusals += 1;
                    for printed in text.lines() {
                        println!("{tag} ! {printed}");
                    }
                }
            }
        }
        println!("{tag} = {exit}");
        assert!(
            questions == 0 || questions == 2,
            "{moment}: {questions} question(s)"
        );
        assert_eq!(
            (results, refusals),
            if *exit == 0 { (1, 0) } else { (0, 1) },
            "{moment}: results and refusals for the exit code {exit}"
        );
        assert_eq!(
            lines.len(),
            questions + 2,
            "{moment}: the transcript holds something beside the command line, the questions and the outcome"
        );
    }
}
