//! The texts the three commands word themselves, pinned (decisions 102, 113 and 114), and
//! each outcome of the commands as it reads at the terminal.

use std::io;
use std::path::PathBuf;

use nage_core::book::BookError;
use nage_core::fold::NameError;
use nage_core::password::{HashError, PasswordError};
use nage_core::setup::SetupError;
use nage_core::storage::{self, BusyCause, Storage};
use nage_core::users::{PasswordChangeError, UserError};

use super::{Done, QUESTION_AGAIN, Refusal, Stop, Task, question};
use crate::database::OpenError;

/// The data directory of the examples.
const DATABASE: &str = "/srv/nage/data/nage.sqlite";

/// Each refusal a command words itself says what was refused, why and what to do instead, on
/// one line that begins with the words the core's refusals of the same command begin with,
/// and no two share a text. The test prints each moment with its text (`cargo test -p nage
/// --bin nage explain -- --nocapture` shows them).
///
/// Proves: decision 102 (the three parts), decision 113 (what each command says; a refusal is
/// one line), decision 114 (every refusal this slice adds is three-part).
#[test]
fn command_refusals_explain_themselves() {
    let cases: [(&str, &str, Refusal, &str); 15] = [
        (
            "Refusal::CurrencyMissing",
            "On the server, `nage init --book-name Household --username sam --display-name Sam` is run without saying which currency the book is in.",
            Refusal::CurrencyMissing,
            "The book was not set up because no currency was given, and a book's currency never changes once the book is set up. Add --currency and the code of the book's currency, for example --currency CAD, then run the command again.",
        ),
        (
            "Refusal::UnknownCurrency",
            "`nage init` is given `--currency cad`, in small letters.",
            Refusal::UnknownCurrency {
                typed: String::from("cad"),
            },
            "The book was not set up because \"cad\" is not a currency code Nage knows; a code is three capital letters, such as CAD, EUR or USD. Give the code of the book's currency with --currency, then run the command again.",
        ),
        (
            "Refusal::NoTerminal",
            "`nage init` is run by a script, where there is no terminal to type a password in, and without `--password-stdin`.",
            Refusal::NoTerminal { task: Task::SetUp },
            "The book was not set up because the password could not be asked for: this command is not running in a terminal, and --password-stdin was not given. Run the command in a terminal, or add --password-stdin and send the password on standard input.",
        ),
        (
            "Refusal::PasswordsDiffer",
            "At the two password questions of `nage init`, correct horse 42 is typed the first time and correct horse 43 the second time.",
            Refusal::PasswordsDiffer { task: Task::SetUp },
            "The book was not set up because the two passwords typed are not the same, so one of them was mistyped. Run the command again and type the same password both times.",
        ),
        (
            "Refusal::PasswordsDiffer",
            "The same slip at the two password questions of `nage user add`.",
            Refusal::PasswordsDiffer {
                task: Task::AddUser,
            },
            "The user was not created because the two passwords typed are not the same, so one of them was mistyped. Run the command again and type the same password both times.",
        ),
        (
            "Refusal::PasswordsDiffer",
            "The same slip at the two password questions of `nage user reset-password`.",
            Refusal::PasswordsDiffer {
                task: Task::ResetPassword,
            },
            "The password was not changed because the two passwords typed are not the same, so one of them was mistyped. Run the command again and type the same password both times.",
        ),
        (
            "Refusal::NoPasswordOnStdin",
            "`nage user add` is run with `--password-stdin`, and nothing is sent on standard input.",
            Refusal::NoPasswordOnStdin {
                task: Task::AddUser,
            },
            "The user was not created because --password-stdin was given and no password arrived on standard input. Send the password on standard input, for example through a pipe, or leave --password-stdin out and type the password at the terminal.",
        ),
        (
            "Refusal::PasswordNotText",
            "`nage user reset-password --password-stdin` is sent a file that is not text.",
            Refusal::PasswordNotText {
                task: Task::ResetPassword,
            },
            "The password was not changed because the password on standard input is not valid UTF-8 text, so Nage cannot tell which characters it holds. Send the password as UTF-8 text, then run the command again.",
        ),
        (
            "Refusal::NoDatabase",
            "`nage user add` is run in a folder that holds no data directory.",
            Refusal::NoDatabase {
                task: Task::AddUser,
                path: PathBuf::from(DATABASE),
            },
            "The user was not created because there is no Nage database at /srv/nage/data/nage.sqlite. Run the command in the folder that holds the data directory, or name the data directory with --data-dir; if Nage has never been started there, set it up first with nage init.",
        ),
        (
            "Refusal::DatabaseBehind",
            "`nage init` is run where an older version of Nage left its database, before `nage serve` has brought it up to date.",
            Refusal::DatabaseBehind {
                task: Task::SetUp,
                path: PathBuf::from(DATABASE),
                version: 1,
                expects: 2,
            },
            "The book was not set up because the database at /srv/nage/data/nage.sqlite was left by an older version of Nage and has not been brought up to date: it is at schema version 1, and this version of Nage expects 2. Start nage serve once, which brings the database up to date, then run the command again.",
        ),
        (
            "Refusal::DatabaseNewer",
            "`nage user reset-password` is run with an older version of Nage than the one that last served the database.",
            Refusal::DatabaseNewer {
                task: Task::ResetPassword,
                path: PathBuf::from(DATABASE),
                version: 3,
            },
            "The password was not changed because the database at /srv/nage/data/nage.sqlite was written by a newer version of Nage than this one: it carries migration 3, which this version does not know. Run the command with that newer version of Nage.",
        ),
        (
            "Refusal::NotSetUp",
            "`nage user add` is run after `nage serve` was started for the first time and before anyone has set Nage up.",
            Refusal::NotSetUp {
                task: Task::AddUser,
            },
            "The user was not created because Nage has not been set up in this data directory yet, so it has no users. Set Nage up first, with nage init or on the set-up page, or name the right data directory with --data-dir.",
        ),
        (
            "Refusal::AddedByNobody",
            "`nage user add --by sma` is run, and nobody is called sma.",
            Refusal::AddedByNobody {
                typed: String::from("sma"),
            },
            "The user was not created because nobody has the username \"sma\", which --by named as the admin adding the user. Check the spelling, or name another admin with --by, then run the command again.",
        ),
        (
            "Refusal::ResetByNobody",
            "`nage user reset-password --by sma --username alex` is run, and nobody is called sma.",
            Refusal::ResetByNobody {
                typed: String::from("sma"),
            },
            "The password was not changed because nobody has the username \"sma\", which --by named as the admin resetting the password. Check the spelling, or name another admin with --by, then run the command again.",
        ),
        (
            "Refusal::UnknownUser",
            "`nage user reset-password --by sam --username alexx` is run, and nobody is called alexx.",
            Refusal::UnknownUser {
                typed: String::from("alexx"),
            },
            "The password was not changed because nobody has the username \"alexx\", which --username named as the user whose password to reset. Check the spelling, then run the command again.",
        ),
    ];
    let mut seen: Vec<String> = Vec::new();
    for (key, moment, refusal, text) in cases {
        let message = refusal.to_string();
        println!("[{key}] {moment} => {message}");
        assert_eq!(message, text, "the message of {key}");
        assert!(
            !message.contains('\n'),
            "the message of {key} is more than one line"
        );
        assert!(
            [Task::SetUp, Task::AddUser, Task::ResetPassword]
                .iter()
                .any(|task| message.starts_with(&format!("{} because ", task.not_done()))),
            "the message of {key} does not begin with what was not done and why: {message}"
        );
        assert!(
            message.ends_with('.'),
            "the message of {key} does not end with a full stop"
        );
        assert!(
            !seen.contains(&message),
            "two refusals share the text {message:?}"
        );
        seen.push(message);
    }
}

/// What each command prints when it is done names what was done, to whom and by whom, with
/// the names as stored: `nage init` on two lines, with or without the starter accounts and with
/// the database's path; `nage user add`; `nage user reset-password` for another user and for
/// the admin's own password. The test prints each moment with its text.
///
/// Proves: decision 113 (what each command's result says, word for word, with the names it is
/// handed), decision 112 (the reset's lines: an admin who reset their own password is told to
/// sign in with it, and another user can change theirs after signing in).
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
    let cases: [(&str, &str, Done, &str); 5] = [
        (
            "Done::SetUp",
            "`nage init` has set Nage up for sam, in Canadian dollars.",
            set_up(true),
            "The book \"Household\" was set up in CAD, with the starter accounts, in /srv/nage/data/nage.sqlite.\nsam (Sam) is its admin and can now sign in.",
        ),
        (
            "Done::SetUp",
            "The same with `--no-starter-accounts`.",
            set_up(false),
            "The book \"Household\" was set up in CAD, without the starter accounts, in /srv/nage/data/nage.sqlite.\nsam (Sam) is its admin and can now sign in.",
        ),
        (
            "Done::UserAdded",
            "`nage user add --by sam --username alex --display-name Alex` has added alex.",
            Done::UserAdded {
                username: String::from("alex"),
                display_name: String::from("Alex"),
                by: String::from("sam"),
            },
            "alex (Alex) was added by sam and can now sign in. alex is not an admin.",
        ),
        (
            "Done::PasswordReset",
            "`nage user reset-password --by sam --username alex` has given alex a new password.",
            Done::PasswordReset {
                username: String::from("alex"),
                by: String::from("sam"),
            },
            "The password of alex was reset by sam, and alex was signed out on every device. Tell alex the new password; alex can change it after signing in.",
        ),
        (
            "Done::PasswordReset",
            "sam, the only admin, forgot the password and has reset it with `nage user reset-password --by sam --username sam`.",
            Done::PasswordReset {
                username: String::from("sam"),
                by: String::from("sam"),
            },
            "The password of sam was reset by sam, and sam was signed out on every device. Sign in with the new password.",
        ),
    ];
    for (key, moment, done, text) in cases {
        let message = done.to_string();
        println!("[{key}] {moment} => {}", message.replace('\n', "\\n"));
        assert_eq!(message, text, "the result of {key}");
    }
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
/// (the database cannot be opened, created or migrated, the data directory cannot be made)
/// stays a failure and prints as `database::OpenError` words it.
///
/// Proves: decision 113 (a database that is missing, behind or ahead of the binary becomes the
/// command's own refusal, carrying the task and the path, and the version of one behind or
/// ahead), decision 102 (a failure is not a refusal and passes the inner message on).
#[test]
fn a_database_that_cannot_be_used_is_refused_in_the_commands_own_words() {
    let path = || PathBuf::from(DATABASE);
    for task in [Task::SetUp, Task::AddUser, Task::ResetPassword] {
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
        let line = error.to_string();
        let stop = Stop::database(Task::SetUp, error);
        assert!(
            matches!(&stop, Stop::Database(_)),
            "the failure {line:?} became {stop:?}"
        );
        assert_eq!(stop.to_string(), line, "what the failure prints");
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
    let tasks = [Task::SetUp, Task::AddUser, Task::ResetPassword];
    assert_eq!(
        Stop::Refusal(Refusal::CurrencyMissing).exit_code(),
        2,
        "the exit code without a currency"
    );
    for task in tasks {
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
    for task in tasks {
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
    /// A line on standard output.
    Prints(String),
    /// The line on standard error, after `nage: `.
    Stops(String),
}

/// Each outcome of the three commands as it reads at the terminal, put together from the
/// binary's own texts in the order the commands keep: the command line, the two questions
/// when the command gets as far as the password, then either the result or one `nage: ` line.
/// The test prints every transcript for MSG.md (`cargo test -p nage --bin nage transcripts --
/// --nocapture`) and checks the form of what it put together: each refusal's text is one line, each
/// question ends with a colon and a space, and each transcript holds the command line, two
/// questions or none, and one outcome that fits its exit code. It runs no command: the process
/// tests of `init_cli.rs` and `user_cli.rs` hold the real commands to their results and refusals.
///
/// Proves: decision 113 (the text of each refusal, worded by the command or by the core, is one
/// line).
#[test]
fn the_commands_read_as_these_terminal_transcripts() {
    let asks = |task, username: &str| {
        [
            Line::Asks(question(task, username)),
            Line::Asks(String::from(QUESTION_AGAIN)),
        ]
    };
    let set_up = |starter_chart| {
        Line::Prints(
            Done::SetUp {
                book_name: String::from("Household"),
                currency: "CAD",
                starter_chart,
                database: PathBuf::from(DATABASE),
                username: String::from("sam"),
                display_name: String::from("Sam"),
            }
            .to_string(),
        )
    };
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
                Line::Stops(Refusal::CurrencyMissing.to_string()),
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
                Line::Stops(
                    Refusal::UnknownCurrency {
                        typed: String::from("cad"),
                    }
                    .to_string(),
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
                Line::Stops(Stop::Book(BookError::Name(NameError::Empty)).to_string()),
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
                Line::Stops(Refusal::PasswordsDiffer { task: Task::SetUp }.to_string()),
            ],
        ),
        (
            "init",
            "The password typed, both times the same, has eight characters.",
            1,
            vec![
                Line::Shell(init),
                short_asks_1,
                short_asks_2,
                Line::Stops(Stop::from(PasswordError::TooShort).to_string()),
            ],
        ),
        (
            "init",
            "Nage is already set up in this data directory. The password is not asked for.",
            1,
            vec![
                Line::Shell(init),
                Line::Stops(Stop::Setup(SetupError::AlreadyInitialized).to_string()),
            ],
        ),
        (
            "init",
            "The command is run where there is no terminal (a script, `docker exec` without `-t`), without `--password-stdin`.",
            2,
            vec![
                Line::Shell(init),
                Line::Stops(Refusal::NoTerminal { task: Task::SetUp }.to_string()),
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
                Line::Stops(Refusal::NoPasswordOnStdin { task: Task::SetUp }.to_string()),
            ],
        ),
        (
            "init",
            "The database in the data directory was left by an older version of Nage and has not been brought up to date yet.",
            1,
            vec![
                Line::Shell(init),
                Line::Stops(
                    Refusal::DatabaseBehind {
                        task: Task::SetUp,
                        path: PathBuf::from(DATABASE),
                        version: 1,
                        expects: Storage::latest_schema_version(),
                    }
                    .to_string(),
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
                Line::Prints(
                    Done::UserAdded {
                        username: String::from("alex"),
                        display_name: String::from("Alex"),
                        by: String::from("sam"),
                    }
                    .to_string(),
                ),
            ],
        ),
        (
            "user add",
            "The admin's username is mistyped. The password is not asked for.",
            1,
            vec![
                Line::Shell("nage user add --by sma --username alex --display-name Alex"),
                Line::Stops(
                    Refusal::AddedByNobody {
                        typed: String::from("sma"),
                    }
                    .to_string(),
                ),
            ],
        ),
        (
            "user add",
            "`--by` names alex, who is not an admin.",
            1,
            vec![
                Line::Shell("nage user add --by alex --username robin --display-name Robin"),
                Line::Stops(
                    Stop::User(UserError::CreatorNotAdmin {
                        username: String::from("alex"),
                    })
                    .to_string(),
                ),
            ],
        ),
        (
            "user add",
            "The new user is to be called Sam, and the admin is already called sam.",
            1,
            vec![
                Line::Shell("nage user add --by sam --username Sam --display-name Samantha"),
                Line::Stops(Stop::User(UserError::UsernameTaken).to_string()),
            ],
        ),
        (
            "user add",
            "Nage has not been set up yet: the server was started once and nobody has used the set-up page or `nage init`.",
            1,
            vec![
                Line::Shell(add),
                Line::Stops(
                    Refusal::NotSetUp {
                        task: Task::AddUser,
                    }
                    .to_string(),
                ),
            ],
        ),
        (
            "user add",
            "The command is run in the wrong folder, where there is no data directory.",
            1,
            vec![
                Line::Shell(add),
                Line::Stops(
                    Refusal::NoDatabase {
                        task: Task::AddUser,
                        path: PathBuf::from(DATABASE),
                    }
                    .to_string(),
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
                Line::Prints(
                    Done::PasswordReset {
                        username: String::from("alex"),
                        by: String::from("sam"),
                    }
                    .to_string(),
                ),
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
                Line::Prints(
                    Done::PasswordReset {
                        username: String::from("sam"),
                        by: String::from("sam"),
                    }
                    .to_string(),
                ),
            ],
        ),
        (
            "user reset-password",
            "The username of the user to reset is mistyped. The password is not asked for.",
            1,
            vec![
                Line::Shell("nage user reset-password --by sam --username alexx"),
                Line::Stops(
                    Refusal::UnknownUser {
                        typed: String::from("alexx"),
                    }
                    .to_string(),
                ),
            ],
        ),
        (
            "user reset-password",
            "`--by` names alex, who is not an admin.",
            1,
            vec![
                Line::Shell("nage user reset-password --by alex --username sam"),
                Line::Stops(
                    Stop::PasswordChange(PasswordChangeError::NotAnAdmin {
                        username: String::from("alex"),
                    })
                    .to_string(),
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
                Line::Stops(
                    Refusal::PasswordsDiffer {
                        task: Task::ResetPassword,
                    }
                    .to_string(),
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
                    results += 1;
                    for printed in text.lines() {
                        println!("{tag} > {printed}");
                    }
                }
                Line::Stops(text) => {
                    assert!(
                        !text.contains('\n') && !text.is_empty(),
                        "{moment}: the refusal is not one line: {text:?}"
                    );
                    refusals += 1;
                    println!("{tag} ! nage: {text}");
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
