//! The password's way into a command (decision 113), with a script standing in for the
//! terminal and a byte slice for standard input.

use std::collections::VecDeque;
use std::io::{self, Read};

use nage_core::password::{HashError, PasswordError};

use super::{AskError, Source, Terminal, new_password};
use crate::messages::{QUESTION_AGAIN, Refusal, Stop, Task, question};

/// The three commands' tasks.
const TASKS: [Task; 3] = [Task::SetUp, Task::AddUser, Task::ResetPassword];

/// A terminal that answers from a script and remembers what it was asked. Asked more often
/// than the script has answers, it fails the test.
struct Script {
    answers: VecDeque<Result<String, AskError>>,
    asked: Vec<String>,
}

impl Script {
    /// A terminal at which `answers` are typed, one per question.
    fn typing(answers: &[&str]) -> Self {
        Self {
            answers: answers
                .iter()
                .map(|answer| Ok((*answer).to_owned()))
                .collect(),
            asked: Vec::new(),
        }
    }

    /// A terminal that answers `answers`, in order.
    fn answering(answers: Vec<Result<String, AskError>>) -> Self {
        Self {
            answers: answers.into(),
            asked: Vec::new(),
        }
    }
}

impl Terminal for Script {
    fn ask(&mut self, question: &str) -> Result<String, AskError> {
        self.asked.push(question.to_owned());
        self.answers
            .pop_front()
            .unwrap_or_else(|| panic!("the terminal was asked once more: {question:?}"))
    }
}

/// Asks `terminal` for the password `task` sets for `username`.
fn ask(terminal: &mut Script, task: Task, username: &str) -> Result<String, Stop> {
    new_password(&mut Source::Terminal(terminal), task, username)
}

/// Reads the password `task` sets for sam from `input`, as `--password-stdin` reads standard
/// input.
fn read(mut input: &[u8], task: Task) -> Result<String, Stop> {
    new_password(&mut Source::Stdin(&mut input), task, "sam")
}

/// A standard input that fails when it is read.
struct Broken;

impl Read for Broken {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the pipe broke"))
    }
}

/// At the terminal each command asks twice, first with its own question, which names the
/// user, then with the second question, and answers the password exactly as typed: the
/// spaces at both ends stay.
///
/// Proves: decision 113 (each command asks for the password twice at the terminal), decision
/// 107 (a password is never trimmed).
#[test]
fn the_terminal_is_asked_twice_and_gives_the_password_as_typed() {
    for task in TASKS {
        let mut terminal = Script::typing(&["  correct horse 42 ", "  correct horse 42 "]);
        let password = ask(&mut terminal, task, "alex")
            .unwrap_or_else(|stop| panic!("the password for {task:?} was refused: {stop:?}"));
        assert_eq!(password, "  correct horse 42 ", "the password for {task:?}");
        assert_eq!(
            terminal.asked,
            [question(task, "alex"), String::from(QUESTION_AGAIN)],
            "what the terminal was asked for {task:?}"
        );
    }
}

/// Two answers that are not the same are refused as different and nothing is asked a third
/// time: a different last character, a space more at the end, and two different answers that
/// are both too short, which are refused as different and not as too short.
///
/// Proves: decision 113 (when the two differ the password is refused as different, before any
/// rule is applied, and nothing is asked again).
#[test]
fn two_answers_that_differ_are_refused_and_not_asked_again() {
    for task in TASKS {
        for (first, second) in [
            ("correct horse 42", "correct horse 43"),
            ("correct horse 42", "correct horse 42 "),
            ("short", "shirt"),
        ] {
            let mut terminal = Script::typing(&[first, second]);
            let stop = ask(&mut terminal, task, "sam").err();
            assert!(
                matches!(
                    &stop,
                    Some(Stop::Refusal(Refusal::PasswordsDiffer { task: refused })) if *refused == task
                ),
                "{first:?} then {second:?} for {task:?}: {stop:?}"
            );
            assert_eq!(
                terminal.asked.len(),
                2,
                "questions asked for {first:?} then {second:?}"
            );
        }
    }
}

/// Two answers that differ only in the case of their letters are not the same password: each
/// command refuses them as different and asks nothing a third time, whether the first letter,
/// every letter or the last letter changes case.
///
/// Proves: decision 113 (the two answers compared exactly as typed), inherited gap s5-evidence-cli-r2-9
#[test]
fn two_answers_that_differ_only_in_capitals_are_refused() {
    for task in TASKS {
        for (first, second) in [
            ("correct horse 42", "Correct horse 42"),
            ("correct horse 42", "CORRECT HORSE 42"),
            ("Correct Horse", "Correct HorsE"),
        ] {
            let mut terminal = Script::typing(&[first, second]);
            let stop = ask(&mut terminal, task, "sam").err();
            assert!(
                matches!(
                    &stop,
                    Some(Stop::Refusal(Refusal::PasswordsDiffer { task: refused })) if *refused == task
                ),
                "{first:?} then {second:?} for {task:?}: {stop:?}"
            );
            assert_eq!(
                terminal.asked,
                [question(task, "sam"), String::from(QUESTION_AGAIN)],
                "questions asked for {first:?} then {second:?}"
            );
        }
    }
}

/// `nage init` on a database the server created (the data folder brought up to date by the one
/// update operation `serve` calls, under the server's mark, which is then let go: every change
/// applied, no book and no user) asks its first question for the user it creates by the username,
/// "Password for sam: ", not by the display name, then the second question, and sets the book up:
/// the database then holds the book Household and one user, sam (Sam Lee), an admin.
///
/// Proves: decision 113 (the question names the username as typed), inherited gap s5-final-cli-r1-4
#[tokio::test]
async fn init_asks_its_first_question_on_a_database_the_server_created() {
    let dir = nage_core::test_support::TestDir::new();
    let data = dir.path().join("data");
    let at = nage_core::timestamp::Timestamp::from_second(1_794_225_600)
        .unwrap_or_else(|error| panic!("the test instant: {error}"));
    let updated = nage_core::update::open_and_update(nage_core::update::Update {
        data_dir: &data,
        now: at,
        version: env!("CARGO_PKG_VERSION"),
        missing: nage_core::update::Missing::Create,
        holder: nage_core::update::Holder::Serve,
        options: &nage_core::storage::StorageOptions::default(),
    })
    .await
    .unwrap_or_else(|error| panic!("the server's start could not create the database: {error}"));
    assert!(
        updated.report.created,
        "the server's start did not create the database: {:?}",
        updated.report
    );
    let nage_core::update::Updated { storage, mark, .. } = updated;
    storage.close().await;
    drop(mark);

    let mut terminal = Script::typing(&["correct horse 42", "correct horse 42"]);
    let database = data.join("nage.sqlite");
    let init = crate::init::Init {
        data_dir: data,
        book_name: String::from("Household"),
        currency: Some(String::from("CAD")),
        username: String::from("sam"),
        display_name: String::from("Sam Lee"),
        starter_chart: true,
    };
    let done = crate::init::run(init, &mut Source::Terminal(&mut terminal)).await;
    assert!(
        done.is_ok(),
        "the set-up on a database the server created was refused: {:?}",
        done.err()
    );
    assert_eq!(
        terminal.asked,
        [question(Task::SetUp, "sam"), String::from(QUESTION_AGAIN)],
        "what the terminal was asked for --username sam --display-name \"Sam Lee\" on a database the server created"
    );

    // The book is set up: the database holds the book Household and its admin sam (Sam Lee).
    let storage = nage_core::storage::Storage::open(
        &database,
        &nage_core::storage::StorageOptions::default(),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot open {}: {error}", database.display()));
    let mut read = storage
        .begin_read()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a read: {error}"));
    let book = nage_core::book::book(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot read the book: {error}"));
    let sam = nage_core::users::user_by_username(&mut read, "sam")
        .await
        .unwrap_or_else(|error| panic!("cannot look sam up: {error}"));
    let users = nage_core::users::count_users(&mut read)
        .await
        .unwrap_or_else(|error| panic!("cannot count the users: {error}"));
    drop(read);
    storage.close().await;
    assert_eq!(
        book.map(|book| book.name),
        Some(String::from("Household")),
        "the book set up"
    );
    let sam = sam.unwrap_or_else(|| panic!("no user sam was set up"));
    assert_eq!(
        (sam.user.display_name.as_str(), sam.user.is_admin, users),
        ("Sam Lee", true, 1),
        "the admin set up"
    );
}

/// Without a terminal the password is a usage error, whichever of the two questions meets
/// the missing terminal, and no further question is asked.
///
/// Proves: decision 113 (with no terminal and no `--password-stdin` the command is a usage
/// error).
#[test]
fn without_a_terminal_the_password_is_a_usage_error() {
    for task in TASKS {
        let mut terminal = Script::answering(vec![Err(AskError::NoTerminal)]);
        let stop = ask(&mut terminal, task, "sam").err();
        assert!(
            matches!(
                &stop,
                Some(Stop::Refusal(Refusal::NoTerminal { task: refused })) if *refused == task
            ),
            "no terminal at the first question for {task:?}: {stop:?}"
        );
        assert_eq!(
            terminal.asked.len(),
            1,
            "questions asked without a terminal"
        );

        let mut terminal = Script::answering(vec![
            Ok(String::from("correct horse 42")),
            Err(AskError::NoTerminal),
        ]);
        let stop = ask(&mut terminal, task, "sam").err();
        assert!(
            matches!(
                &stop,
                Some(Stop::Refusal(Refusal::NoTerminal { task: refused })) if *refused == task
            ),
            "no terminal at the second question for {task:?}: {stop:?}"
        );
    }
}

/// A terminal that is there and fails, and a standard input that fails, are failures and not
/// refusals: each names where the password could not be read and passes the cause on.
///
/// Proves: decision 113 (the terminal and standard input are the two places a password comes
/// from), decision 102 (a failure nobody caused is not a refusal and passes its cause on).
#[test]
fn a_terminal_or_a_standard_input_that_fails_is_a_failure_with_its_cause() {
    let mut terminal = Script::answering(vec![Err(AskError::Failed(io::Error::other(
        "input/output error",
    )))]);
    let stop = ask(&mut terminal, Task::AddUser, "alex").err();
    assert!(
        matches!(
            &stop,
            Some(Stop::Failure(what))
                if what == "cannot read the password from the terminal: input/output error"
        ),
        "a terminal that fails: {stop:?}"
    );
    assert_eq!(
        terminal.asked.len(),
        1,
        "questions asked of a failing terminal"
    );

    let stop = new_password(&mut Source::Stdin(&mut Broken), Task::AddUser, "alex").err();
    assert!(
        matches!(
            &stop,
            Some(Stop::Failure(what))
                if what == "cannot read the password from standard input: the pipe broke"
        ),
        "a standard input that fails: {stop:?}"
    );
}

/// Standard input is read once, whole, and exactly one trailing line ending is removed: a
/// line feed, a carriage return with a line feed, or a carriage return alone. Everything
/// else is the password: the spaces at both ends, a line ending in the middle, and the first
/// of two line endings at the end.
///
/// Proves: decision 113 (`--password-stdin` reads the password once from standard input, all
/// of it with one trailing line ending removed, as `docker login --password-stdin` reads
/// it), decision 107 (a password is never trimmed).
#[test]
fn standard_input_is_the_password_but_for_one_line_ending() {
    let cases: [(&[u8], &str); 8] = [
        (b"correct horse 42", "correct horse 42"),
        (b"correct horse 42\n", "correct horse 42"),
        (b"correct horse 42\r\n", "correct horse 42"),
        (b"correct horse 42\r", "correct horse 42"),
        (b"correct horse 42\n\n", "correct horse 42\n"),
        (b"correct horse 42\n\r\n", "correct horse 42\n"),
        (b"  correct horse 42  \n", "  correct horse 42  "),
        (b"correct\nhorse\n42\n", "correct\nhorse\n42"),
    ];
    for (input, expected) in cases {
        let password = read(input, Task::SetUp)
            .unwrap_or_else(|stop| panic!("the standard input {input:?} was refused: {stop:?}"));
        assert_eq!(
            password, expected,
            "the password read from the standard input {input:?}"
        );
    }
    let accented = "caf\u{e9} au lait 42\n".as_bytes();
    let password = read(accented, Task::SetUp)
        .unwrap_or_else(|stop| panic!("the standard input {accented:?} was refused: {stop:?}"));
    assert_eq!(
        password, "caf\u{e9} au lait 42",
        "a password with an accented letter"
    );
}

/// A standard input that holds nothing, or one line ending alone, is refused as holding no
/// password; bytes that are not UTF-8 are refused as not being text. Each refusal names the
/// command's task.
///
/// Proves: decision 113 (`--password-stdin` reads the password from standard input; an input
/// that holds no password or is not UTF-8 is a refusal of the command's own, naming its task).
#[test]
fn a_standard_input_without_a_password_or_without_text_is_refused() {
    for task in TASKS {
        for input in [&b""[..], b"\n", b"\r\n", b"\r"] {
            let stop = read(input, task).err();
            assert!(
                matches!(
                    &stop,
                    Some(Stop::Refusal(Refusal::NoPasswordOnStdin { task: refused })) if *refused == task
                ),
                "the standard input {input:?} for {task:?}: {stop:?}"
            );
        }
        let stop = read(b"correct \xff\xfe horse 42\n", task).err();
        assert!(
            matches!(
                &stop,
                Some(Stop::Refusal(Refusal::PasswordNotText { task: refused })) if *refused == task
            ),
            "a standard input that is not UTF-8 for {task:?}: {stop:?}"
        );
    }
}

/// A password that breaks a rule is refused in the core's words wherever it comes from, as
/// typed: nine characters typed twice at the terminal or sent on standard input are too
/// short, while nine characters and a line ending that stays are ten and pass; 1,025 bytes
/// are too long, typed or sent, and so is a megabyte on standard input; exactly ten
/// characters and exactly 1,024 bytes pass.
///
/// Proves: decision 107 (the password rules live in the core, so that every place that sets
/// a password applies the same rules with the same messages; counted as typed), SPEC 5.3
/// (at least 10 characters, at most 1 KiB).
#[test]
fn a_password_that_breaks_a_rule_is_refused_in_the_cores_words() {
    let too_short = |stop: &Option<Stop>| {
        matches!(
            stop,
            Some(Stop::Password(HashError::Password(PasswordError::TooShort)))
        )
    };
    let too_long = |stop: &Option<Stop>| {
        matches!(
            stop,
            Some(Stop::Password(HashError::Password(PasswordError::TooLong)))
        )
    };
    let longest = "x".repeat(1024);
    let longer = "x".repeat(1025);

    let stop = ask(
        &mut Script::typing(&["nine char", "nine char"]),
        Task::SetUp,
        "sam",
    )
    .err();
    assert!(
        too_short(&stop),
        "nine characters at the terminal: {stop:?}"
    );
    let stop = ask(&mut Script::typing(&[&longer, &longer]), Task::SetUp, "sam").err();
    assert!(too_long(&stop), "1,025 bytes at the terminal: {stop:?}");
    let typed = ask(
        &mut Script::typing(&[&longest, &longest]),
        Task::SetUp,
        "sam",
    );
    assert!(
        typed.as_ref().is_ok_and(|password| *password == longest),
        "1,024 bytes at the terminal: {:?}",
        typed.as_ref().err()
    );

    let stop = read(b"nine char\n", Task::SetUp).err();
    assert!(
        too_short(&stop),
        "nine characters on standard input: {stop:?}"
    );
    let sent = read(b"nine char\n\n", Task::SetUp);
    assert!(
        sent.as_ref()
            .is_ok_and(|password| password == "nine char\n"),
        "nine characters and a line ending that stays, ten in all: {:?}",
        sent.as_ref().err()
    );
    let sent = read(b"ten chars!\n", Task::SetUp);
    assert!(
        sent.as_ref().is_ok_and(|password| password == "ten chars!"),
        "ten characters on standard input: {:?}",
        sent.as_ref().err()
    );
    let stop = read(longer.as_bytes(), Task::SetUp).err();
    assert!(too_long(&stop), "1,025 bytes on standard input: {stop:?}");
    let sent = read(format!("{longest}\n").as_bytes(), Task::SetUp);
    assert!(
        sent.as_ref().is_ok_and(|password| *password == longest),
        "1,024 bytes and a line ending on standard input: {:?}",
        sent.as_ref().err()
    );
    let sent = read(format!("{longest}\r\n").as_bytes(), Task::SetUp);
    assert!(
        sent.as_ref().is_ok_and(|password| *password == longest),
        "1,024 bytes and a carriage return with a line feed on standard input: {:?}",
        sent.as_ref().err()
    );
    let megabyte = vec![b'x'; 1024 * 1024];
    let stop = read(&megabyte, Task::SetUp).err();
    assert!(too_long(&stop), "a megabyte on standard input: {stop:?}");
    let accented = "\u{e9}".repeat(512 * 1024);
    let stop = read(accented.as_bytes(), Task::SetUp).err();
    assert!(
        too_long(&stop),
        "a megabyte of two-byte letters on standard input: {stop:?}"
    );
}

/// A reset asks for the new password of the user as stored, whatever was typed for
/// `--username`: `--username ALEX` finds alex, and the terminal is asked "New password for
/// alex: ", then the second question. The other two commands ask for the password of the
/// user they create: `nage user add --by sam --username robin` asks "Password for robin: ",
/// not for the admin's, and `nage init --username sam --display-name Sam` asks "Password for
/// sam: ", by the username and not the display name; each then asks the second question.
///
/// Proves: decision 113 (each command asks for the password twice at the terminal; `--username`
/// is found by the fold of what is typed, and the question names the user as stored; the
/// first question of `init` and of `user add` names the user being created by the username).
#[tokio::test]
async fn a_reset_asks_for_the_new_password_of_the_user_as_stored() {
    let dir = nage_core::test_support::TestDir::new();
    let data = dir.path().join("data");
    let storage = crate::database::create(&data)
        .await
        .unwrap_or_else(|error| panic!("cannot create the database: {error:?}"));
    let at = nage_core::timestamp::Timestamp::from_second(1_794_225_600)
        .unwrap_or_else(|error| panic!("the test instant: {error}"));
    let initialized =
        nage_core::test_support::set_up(&storage, at, "sam", "correct horse 42").await;
    nage_core::test_support::add_user(
        &storage,
        initialized.admin.id,
        at,
        "alex",
        "tea and biscuits",
        false,
    )
    .await;
    storage.close().await;
    let mut terminal = Script::typing(&["kettle on the hob", "kettle on the hob"]);
    let reset = crate::user::Reset {
        data_dir: data.clone(),
        by: String::from("sam"),
        username: String::from("ALEX"),
    };
    let done = crate::user::reset_password(reset, &mut Source::Terminal(&mut terminal)).await;
    assert!(done.is_ok(), "the reset was refused: {:?}", done.err());
    assert_eq!(
        terminal.asked,
        [
            question(Task::ResetPassword, "alex"),
            String::from(QUESTION_AGAIN)
        ],
        "what the terminal was asked for --username ALEX"
    );

    let mut terminal = Script::typing(&["tea and biscuits", "tea and biscuits"]);
    let add = crate::user::Add {
        data_dir: data,
        by: String::from("sam"),
        username: String::from("robin"),
        display_name: String::from("Robin"),
    };
    let done = crate::user::add(add, &mut Source::Terminal(&mut terminal)).await;
    assert!(done.is_ok(), "the add was refused: {:?}", done.err());
    assert_eq!(
        terminal.asked,
        [
            question(Task::AddUser, "robin"),
            String::from(QUESTION_AGAIN)
        ],
        "what the terminal was asked for --by sam --username robin"
    );

    let fresh = nage_core::test_support::TestDir::new();
    let mut terminal = Script::typing(&["correct horse 42", "correct horse 42"]);
    let init = crate::init::Init {
        data_dir: fresh.path().join("data"),
        book_name: String::from("Household"),
        currency: Some(String::from("CAD")),
        username: String::from("sam"),
        display_name: String::from("Sam"),
        starter_chart: false,
    };
    let done = crate::init::run(init, &mut Source::Terminal(&mut terminal)).await;
    assert!(done.is_ok(), "the set-up was refused: {:?}", done.err());
    assert_eq!(
        terminal.asked,
        [question(Task::SetUp, "sam"), String::from(QUESTION_AGAIN)],
        "what the terminal was asked for --username sam --display-name Sam"
    );
}
