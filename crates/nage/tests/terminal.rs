//! Every state of `nage init`, `nage user add`, `nage user reset-password`, `nage check` and `nage
//! health`, every wrong setting and every kind of mistyped command, run as a process and pinned
//! as a text snapshot of its transcript (decisions 60 and 157): the command line as typed, what
//! standard output and standard error show, and the exit code. The data folder of a test is shown
//! as `/srv/nage/data`, the version as `[version]`, a port the test bound as `[PORT]`.

mod common;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::SocketAddr;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use nage_core::accounts;
use nage_core::money::Money;
use nage_core::test_support::{self, TestDir, assert_no_side_files};
use nage_core::transactions::{self, NewPosting, NewTransaction};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};

use common::commands::{self, block_on, nage_without_terminal, run_with_stdin};
use common::transcript::{starts_with_stamp, transcript};
use common::web::PASSWORD;
use common::{
    BlackHole, ClosedPort, Finished, HealthServer, VERSION, field, http_get, is_request_line,
    is_starting_line, nage, run,
};

/// Asserts that `out` ended with the exit code `code`, showing its standard error otherwise (a
/// stub's panic shows only there).
#[track_caller]
fn assert_exit(out: &Finished, code: i32) {
    assert_eq!(
        out.code(),
        Some(code),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
        out.stderr
    );
}

/// `nage <args>` with the variables `envs`, in the working directory `dir`.
fn command(dir: &TestDir, envs: &[(&str, &OsStr)], args: &[&OsStr]) -> Command {
    let mut command = nage();
    command.current_dir(dir.path());
    for (key, value) in envs {
        command.env(key, value);
    }
    command.args(args);
    command
}

/// Each kind of command line the parser refuses reads as Nage's three lines, `Error: The command
/// was not run.`, the reason in everyday words (a missing option, two and three missing options, a
/// missing command after `nage user` and after `nage`, an option without its value, an unknown
/// command, an unknown option, quoted when it holds a character no option's name has and not
/// when it is one letter long, `--x`) or the library's own sentence for a kind Nage does not
/// foresee (an option given twice), and the `--help` hint of the command as typed, up to the
/// first word that is not a command (an option typed before the command names `nage`, never the
/// command after it); nothing on standard output, exit 2, and nothing created.
///
/// Proves: decision 157 (Nage's three lines for each kind of mistyped command, the library's sentence for an unforeseen one, exit 2)
#[test]
fn mistyped_commands_read_as_their_transcripts() {
    let dir = TestDir::new();
    let data = dir.path().join("data").into_os_string();
    let cases: [(&str, Vec<&OsStr>); 12] = [
        (
            "mistyped_missing_option",
            ["user", "add", "--by", "sam", "--username", "alex"]
                .map(OsStr::new)
                .to_vec(),
        ),
        (
            "mistyped_missing_options",
            ["init", "--book-name", "Household", "--currency", "CAD"]
                .map(OsStr::new)
                .to_vec(),
        ),
        (
            "mistyped_missing_three_options",
            ["init", "--currency", "CAD"].map(OsStr::new).to_vec(),
        ),
        ("mistyped_no_command", vec![OsStr::new("user")]),
        ("mistyped_nothing", Vec::new()),
        (
            "mistyped_option_without_value",
            ["check", "--data-dir"].map(OsStr::new).to_vec(),
        ),
        (
            "mistyped_unforeseen",
            [
                "init",
                "--book-name",
                "Household",
                "--book-name",
                "Home",
                "--currency",
                "CAD",
                "--username",
                "sam",
                "--display-name",
                "Sam",
            ]
            .map(OsStr::new)
            .to_vec(),
        ),
        ("mistyped_unknown_command", vec![OsStr::new("frobnicate")]),
        (
            "mistyped_unknown_option",
            vec![
                OsStr::new("check"),
                OsStr::new("--data-folder"),
                data.as_os_str(),
            ],
        ),
        (
            "mistyped_unknown_option_quoted",
            vec![
                OsStr::new("check"),
                OsStr::new("--data_dir"),
                data.as_os_str(),
            ],
        ),
        (
            "mistyped_unknown_short_option",
            vec![OsStr::new("check"), OsStr::new("--x")],
        ),
        (
            "mistyped_option_before_the_command",
            vec![
                OsStr::new("--data-dir"),
                data.as_os_str(),
                OsStr::new("check"),
            ],
        ),
    ];
    for (name, args) in cases {
        let mut command = command(&dir, &[], &args);
        let shown = transcript(&command).dir(dir.path());
        let out = run(&mut command);
        assert_exit(&out, 2);
        assert!(
            out.stdout.is_empty(),
            "stdout of `{}`:\n{}",
            out.what,
            out.stdout
        );
        assert!(
            !dir.path().join("data").exists(),
            "`{}` created the data folder",
            out.what
        );
        let transcript = shown.of(&out);
        insta::assert_snapshot!(name, transcript);
    }
}

/// A case of a wrong setting: the snapshot's name, the variables and the arguments.
type Setting<'a> = (&'a str, Vec<(&'a str, &'a OsStr)>, Vec<&'a OsStr>);

/// A wrong setting, given as a variable or as an option, refuses to start or to run before
/// anything is looked at: three lines naming the setting's source and quoting its value (a
/// value that is not UTF-8 text by its bytes), what to give instead, exit 2, and nothing created.
/// The first line names what did not happen for the command typed: `nage check`, `nage migrate`
/// and its dry run each say their own.
///
/// Proves: decisions 42, 45 and 159 (a wrong setting names its source and value, exit 2)
#[test]
fn wrong_settings_read_as_their_transcripts() {
    let dir = TestDir::new();
    let data = dir.path().join("data").into_os_string();
    let serve: [&OsStr; 3] = [
        OsStr::new("serve"),
        OsStr::new("--data-dir"),
        data.as_os_str(),
    ];
    let log: [&OsStr; 2] = [OsStr::new("--log"), OsStr::new("nage_web=loud")];
    let not_utf8 = OsString::from(OsStr::from_bytes(b"\xFF"));
    let cases: [Setting<'_>; 8] = [
        (
            "setting_bind_invalid",
            vec![("NAGE_BIND", OsStr::new("nonsense"))],
            serve.to_vec(),
        ),
        (
            "setting_data_dir_not_utf8",
            vec![("NAGE_DATA_DIR", not_utf8.as_os_str())],
            vec![OsStr::new("check")],
        ),
        (
            "setting_data_dir_not_utf8_migrate",
            vec![("NAGE_DATA_DIR", not_utf8.as_os_str())],
            vec![OsStr::new("migrate")],
        ),
        (
            "setting_data_dir_not_utf8_dry_run",
            vec![("NAGE_DATA_DIR", not_utf8.as_os_str())],
            vec![OsStr::new("migrate"), OsStr::new("--dry-run")],
        ),
        (
            "setting_log_filter_invalid",
            Vec::new(),
            [&serve[..], &log[..]].concat(),
        ),
        (
            "setting_port_zero",
            Vec::new(),
            ["health", "--bind", "127.0.0.1:0"].map(OsStr::new).to_vec(),
        ),
        (
            "setting_time_zone_unknown",
            vec![("NAGE_TZ", OsStr::new("Mars/Olympus"))],
            serve.to_vec(),
        ),
        (
            "setting_trust_proxy_not_true_or_false",
            vec![("NAGE_TRUST_PROXY", OsStr::new("yes"))],
            serve.to_vec(),
        ),
    ];
    for (name, envs, args) in cases {
        let mut command = command(&dir, &envs, &args);
        let shown = transcript(&command).dir(dir.path());
        let out = run(&mut command);
        assert_exit(&out, 2);
        assert!(
            out.stdout.is_empty(),
            "stdout of `{}`:\n{}",
            out.what,
            out.stdout
        );
        assert!(
            !dir.path().join("data").exists(),
            "`{}` created the data folder",
            out.what
        );
        let transcript = shown.of(&out);
        insta::assert_snapshot!(name, transcript);
    }
}

/// The instant of a log line's stamp, in seconds since the epoch.
fn stamped_second(line: &str) -> i64 {
    let stamp = &line[..20];
    stamp
        .parse::<jiff::Timestamp>()
        .unwrap_or_else(|error| panic!("{stamp:?} is not an instant: {error}; line: {line}"))
        .as_second()
}

/// Asserts that every line of `lines` begins with a whole-second UTC stamp of the instants from
/// `before` to `after`, then the level, and names no module.
#[track_caller]
fn assert_log_lines(lines: &[String], before: i64, after: i64) {
    let text = lines.join("\n");
    assert!(!lines.is_empty(), "the server logged nothing");
    for line in lines {
        assert!(
            starts_with_stamp(line),
            "a line without a whole-second UTC stamp: {line:?}; stderr:\n{text}"
        );
        let second = stamped_second(line);
        assert!(
            (before..=after).contains(&second),
            "the stamp of {line:?} is not of the run ({before} to {after})"
        );
        let rest = &line[20..];
        assert!(
            ["  INFO ", "  WARN ", " ERROR ", " DEBUG ", " TRACE "]
                .iter()
                .any(|level| rest.starts_with(level)),
            "no level after the stamp: {line:?}"
        );
        for module in ["nage::", "nage_web::", "nage_core::", "serve:", "nage_web:"] {
            assert!(
                !line.contains(module),
                "the line names a module ({module}): {line:?}"
            );
        }
    }
}

/// A free loopback port, found by binding port 0 and letting it go again.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("cannot bind a loopback port: {error}"));
    listener
        .local_addr()
        .unwrap_or_else(|error| panic!("cannot read the port: {error}"))
        .port()
}

/// Every line a server logs, from its start to its stop, begins with a whole-second UTC stamp
/// `YYYY-MM-DDTHH:MM:SSZ` of the moment it was written, then the level, the message and the
/// fields as key=value; no line names a module. A request's line is shown alone first (the log
/// filtered to nage-web), then every line of a start and a stop with the default filter.
///
/// Proves: decision 160 (amending 47: whole-second UTC stamps, no module name, key=value kept)
#[test]
fn the_log_lines_carry_whole_second_utc_stamps_and_no_module_name() {
    let before = jiff::Timestamp::now().as_second();

    // The request's line alone: nage-web's events only, on a port the test chose.
    let port = free_port();
    let mut server = common::serve()
        .bind(&format!("127.0.0.1:{port}"))
        .args(["--log", "nage_web=info"])
        .spawn();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = std::time::Instant::now() + common::LISTEN_TIMEOUT;
    while std::net::TcpStream::connect(addr).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "nage serve did not listen on {addr}; stderr:\n{}",
            server.lines().join("\n")
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let response = server.send(addr, "GET", "/", &[], &[]);
    assert!(
        response.status < 500,
        "GET / answered {}:\n{}",
        response.status,
        response.head
    );
    let line = server.wait_for_line("the request line", |line| {
        is_request_line(line, "GET", "/") || line.contains("panicked")
    });
    let status = server.stop();
    let after = jiff::Timestamp::now().as_second();
    let lines = server.lines().to_vec();
    assert!(
        is_request_line(&line, "GET", "/"),
        "no request line; stderr:\n{}",
        lines.join("\n")
    );
    assert!(status.success(), "nage serve stopped with {status}");
    assert_log_lines(&lines, before, after);
    assert!(
        line[20..].starts_with("  INFO "),
        "the message does not follow the level: {line:?}"
    );

    // A start and a stop with the default filter.
    let mut server = common::serve().spawn();
    let addr = server.wait_listening();
    let response = http_get(addr, "/", &[]);
    assert!(response.status < 500, "GET / answered {}", response.status);
    server.wait_for_line("the request line", |line| is_request_line(line, "GET", "/"));
    let status = server.stop();
    assert!(status.success(), "nage serve stopped with {status}");
    let after = jiff::Timestamp::now().as_second();
    let lines = server.lines().to_vec();
    let text = lines.join("\n");
    assert_log_lines(&lines, before, after);

    let starting = lines
        .iter()
        .find(|line| is_starting_line(line))
        .unwrap_or_else(|| panic!("no starting line; stderr:\n{text}"));
    assert!(
        starting[20..].starts_with("  INFO starting "),
        "the message does not follow the level: {starting:?}"
    );
    assert_eq!(field(starting, "version"), Some(VERSION), "{starting}");
    assert_eq!(field(starting, "tz"), Some("UTC"), "{starting}");
    assert!(
        lines.iter().any(|line| is_request_line(line, "GET", "/")),
        "no request line with key=value fields; stderr:\n{text}"
    );
    assert!(
        lines.iter().any(|line| line.ends_with("  INFO stopped")),
        "no stopped line; stderr:\n{text}"
    );
}

/// What `--password-stdin` is fed in the transcripts: the password of the process tests and a
/// line ending, as `printf '%s\n' ... > password.txt` writes it.
const PASSWORD_TXT: &str = "password.txt";

/// The bytes of [`PASSWORD_TXT`].
fn password_bytes() -> Vec<u8> {
    format!("{PASSWORD}\n").into_bytes()
}

/// Where a command's standard input comes from in a transcript.
#[derive(Clone, Copy)]
enum Input<'a> {
    /// Nothing: standard input is `/dev/null` and the command line shows no redirection.
    Nothing,
    /// A file, shown as `< name`, whose bytes reach the command through a pipe.
    File(&'a str, &'a [u8]),
    /// No terminal: the command runs in a session of its own (`setsid --wait`).
    NoTerminal,
}

/// The arguments `words`, each word `DATA` replaced by the data folder of `dir`.
fn words(dir: &TestDir, words: &[&str]) -> Vec<OsString> {
    let data = dir.path().join("data");
    words
        .iter()
        .map(|word| {
            if *word == "DATA" {
                data.clone().into_os_string()
            } else {
                OsString::from(word)
            }
        })
        .collect()
}

/// Runs `nage <args>` in `dir` with `input`: the process and its transcript, the working
/// directory shown as `/srv/nage`.
fn transcribed(dir: &TestDir, args: &[OsString], input: Input<'_>) -> (Finished, String) {
    let mut command = match input {
        Input::NoTerminal => nage_without_terminal(),
        Input::Nothing | Input::File(..) => nage(),
    };
    command.current_dir(dir.path()).args(args);
    let mut shown = transcript(&command).dir(dir.path());
    let out = match input {
        Input::File(name, bytes) => {
            shown = shown.stdin(name);
            run_with_stdin(&mut command, bytes)
        }
        Input::Nothing | Input::NoTerminal => run(&mut command),
    };
    let transcript = shown.of(&out);
    (out, transcript)
}

/// Asserts that a refused or failed command printed nothing on standard output and three labelled
/// lines on standard error, and that a finished one printed nothing on standard error.
#[track_caller]
fn assert_streams(out: &Finished) {
    if out.code() == Some(0) {
        assert!(
            out.stderr.is_empty(),
            "stderr of `{}`:\n{}",
            out.what,
            out.stderr
        );
    } else {
        assert!(
            out.stdout.is_empty(),
            "stdout of `{}`:\n{}",
            out.what,
            out.stdout
        );
        let lines: Vec<&str> = out.stderr.lines().collect();
        assert!(
            lines.len() >= 3
                && lines[0].starts_with("Error: ")
                && lines[1].starts_with("Why:")
                && lines[lines.len() - 1].starts_with("What now: "),
            "stderr of `{}` is not three labelled lines:\n{}",
            out.what,
            out.stderr
        );
    }
}

/// The arguments of `nage init` for the book Household in CAD, its admin sam (Sam), with `extra`
/// after them.
fn init_words(extra: &[&'static str]) -> Vec<&'static str> {
    let mut words = vec![
        "init",
        "--data-dir",
        "DATA",
        "--book-name",
        "Household",
        "--currency",
        "CAD",
        "--username",
        "sam",
        "--display-name",
        "Sam",
    ];
    words.extend_from_slice(extra);
    words
}

/// `words` with the value after `option` replaced by `value`, or the option and its value left out
/// when `value` is `None`.
fn with_value(
    words: &[&'static str],
    option: &str,
    value: Option<&'static str>,
) -> Vec<&'static str> {
    let at = words
        .iter()
        .position(|word| *word == option)
        .unwrap_or_else(|| panic!("no {option} in {words:?}"));
    let mut changed = words.to_vec();
    match value {
        Some(value) => changed[at + 1] = value,
        None => {
            changed.drain(at..at + 2);
        }
    }
    changed
}

/// A state of `nage init`: the snapshot's name, the working directory and whether it starts
/// empty, the arguments, the input and the exit code.
type InitCase<'a> = (&'a str, (TestDir, bool), Vec<&'static str>, Input<'a>, i32);

/// Every state of `nage init`, run as a process: the book set up with and without the starter
/// accounts (the database then holding the book and its admin), and every refusal: a book already
/// set up, no currency (exit 2), a currency in small letters, an empty username (the core's
/// refusal in the terminal's own words, naming the option), a short password, nothing on standard
/// input, standard input that is not text, no terminal (exit 2), a database behind this version
/// and one a newer Nage wrote. A refusal in a fresh working directory creates nothing, not even the
/// data folder. A username with a colon is set up as typed, as the set-up page allows it. The
/// empty database a cut-off init left (a zero-byte file) is finished and set up; a password that
/// is refused there leaves the file exactly as it was, still empty, no side file beside it.
///
/// Proves: decisions 150 and 157 (every state of nage init, the core's refusals in the terminal's own words; ruling P8)
#[test]
fn init_reads_as_its_transcripts() {
    let password = password_bytes();
    let stdin = init_words(&["--password-stdin"]);
    let fed = Input::File(PASSWORD_TXT, password.as_slice());
    let fresh = || (TestDir::new(), true);
    let cases: Vec<InitCase<'_>> = vec![
        ("init_done", fresh(), stdin.clone(), fed, 0),
        (
            "init_done_without_starter_accounts",
            fresh(),
            init_words(&["--no-starter-accounts", "--password-stdin"]),
            fed,
            0,
        ),
        (
            "init_already_set_up",
            (commands::set_up().0, false),
            stdin.clone(),
            fed,
            1,
        ),
        (
            "init_currency_missing",
            fresh(),
            with_value(&stdin, "--currency", None),
            fed,
            2,
        ),
        (
            "init_unknown_currency",
            fresh(),
            with_value(&stdin, "--currency", Some("cad")),
            fed,
            1,
        ),
        (
            "init_username_empty",
            fresh(),
            with_value(&stdin, "--username", Some(" ")),
            fed,
            1,
        ),
        (
            "init_password_too_short",
            fresh(),
            stdin.clone(),
            Input::File("short-password.txt", b"nine char\n"),
            1,
        ),
        (
            "init_no_password_on_stdin",
            fresh(),
            stdin.clone(),
            Input::File("/dev/null", b""),
            1,
        ),
        (
            "init_password_not_text",
            fresh(),
            stdin.clone(),
            Input::File("not-utf8.bin", b"\xFF\xFEpassword\n"),
            1,
        ),
        (
            "init_no_terminal",
            fresh(),
            init_words(&[]),
            Input::NoTerminal,
            2,
        ),
        (
            "init_database_behind",
            (commands::behind(), false),
            stdin.clone(),
            fed,
            1,
        ),
        (
            "init_database_newer",
            (commands::ahead(), false),
            stdin.clone(),
            fed,
            1,
        ),
        (
            "init_username_with_a_colon",
            fresh(),
            with_value(&stdin, "--username", Some("sam:home")),
            fed,
            0,
        ),
        (
            "init_database_empty",
            (empty_database(), false),
            stdin.clone(),
            fed,
            0,
        ),
        (
            "init_database_empty_password_too_short",
            (empty_database(), false),
            stdin.clone(),
            Input::File("short-password.txt", b"nine char\n"),
            1,
        ),
    ];
    for (name, (dir, fresh), args, input, code) in cases {
        let (out, transcript) = transcribed(&dir, &words(&dir, &args), input);
        assert_exit(&out, code);
        assert_streams(&out);
        if code == 0 {
            let holds = commands::holds(dir.path());
            assert!(
                holds.book.is_some() && holds.users == 1,
                "{name}: the database holds {holds:?}"
            );
        } else if fresh {
            assert!(
                !dir.path().join("data").exists(),
                "{name}: the refused init created the data folder"
            );
        }
        if name == "init_username_with_a_colon" {
            assert!(
                commands::credentials(dir.path(), "sam:home").is_some(),
                "{name}: no user sam:home was stored"
            );
        }
        if name == "init_database_empty_password_too_short" {
            let database = commands::database(dir.path());
            assert!(
                fs::read(&database)
                    .unwrap_or_else(|error| panic!("{name}: cannot read the database: {error}"))
                    .is_empty(),
                "{name}: the refused init changed the empty database"
            );
            assert_no_side_files(&database, name);
        }
        insta::assert_snapshot!(name, transcript);
    }
}

/// A working directory whose data folder holds the empty database a cut-off `nage init` leaves:
/// a zero-byte `nage.sqlite`.
fn empty_database() -> TestDir {
    let dir = TestDir::new();
    fs::create_dir_all(dir.path().join("data"))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    fs::write(commands::database(dir.path()), b"")
        .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
    dir
}

/// `nage init` words the failures of its database in three parts, the system's own words in one
/// pair of brackets at the end of the reason: a data folder that cannot be created (its parent
/// may not be written; nothing is created) and a file at the database's place that is not a
/// database (left exactly as it was).
///
/// Proves: decision 159 (init's failure lines for the database), inherited gap s5-evidence-cli-r1-9
#[test]
fn init_words_the_failures_of_its_database() {
    let password = password_bytes();
    let args = init_words(&["--password-stdin"]);
    let fed = Input::File(PASSWORD_TXT, password.as_slice());

    // The working directory may be read but not written: the data folder cannot be created.
    let dir = TestDir::new();
    let set_mode = |mode: u32| {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode))
            .unwrap_or_else(|error| panic!("cannot change the mode of the test folder: {error}"));
    };
    set_mode(0o500);
    let (out, transcript) = transcribed(&dir, &words(&dir, &args), fed);
    set_mode(0o700);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(
        !dir.path().join("data").exists(),
        "init created the data folder"
    );
    insta::assert_snapshot!("init_data_folder_cannot_be_created", transcript);

    // A file that is not a database stands where the database belongs.
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    let database = commands::database(dir.path());
    let bytes = "This is a shopping list, not a database.\n".repeat(64);
    fs::write(&database, &bytes)
        .unwrap_or_else(|error| panic!("cannot write the stray file: {error}"));
    let (out, transcript) = transcribed(&dir, &words(&dir, &args), fed);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(
        fs::read(&database).unwrap_or_else(|error| panic!("cannot read the stray file: {error}")),
        bytes.as_bytes(),
        "init changed the file that is not a database"
    );
    insta::assert_snapshot!("init_database_cannot_be_opened", transcript);
}

/// The arguments of `nage user add` by sam for alex (Alex).
const USER_ADD: [&str; 11] = [
    "user",
    "add",
    "--data-dir",
    "DATA",
    "--by",
    "sam",
    "--username",
    "alex",
    "--display-name",
    "Alex",
    "--password-stdin",
];

/// A working directory set up with the admin sam and the user `username`, an admin or not.
fn set_up_with(username: &str, is_admin: bool) -> TestDir {
    let (dir, initialized) = commands::set_up();
    commands::add_user(
        dir.path(),
        initialized.admin.id,
        username,
        "another long password",
        is_admin,
    );
    dir
}

/// Every state of `nage user add`, run as a process: alex added by sam (alex can then be found,
/// not an admin), and every refusal: `--by` naming nobody, `--by` naming a user who is not an
/// admin and a username taken (the core's refusals in the terminal's own words, naming the
/// option), no database, and a database that holds no user yet. A display name with a colon is
/// added as typed. An empty username is refused by its name rule before the database is looked
/// at, with a database and without one, and no command leaves a side file beside the database.
///
/// Proves: decisions 150 and 157 (every state of nage user add, the core's refusals in the terminal's own words)
#[test]
fn user_add_reads_as_its_transcripts() {
    let password = password_bytes();
    let fed = Input::File(PASSWORD_TXT, password.as_slice());
    let add = USER_ADD.to_vec();
    let not_an_admin = with_value(
        &with_value(
            &with_value(&add, "--by", Some("alex")),
            "--username",
            Some("robin"),
        ),
        "--display-name",
        Some("Robin"),
    );
    let cases: Vec<(&str, TestDir, Vec<&str>, i32)> = vec![
        ("user_add_done", commands::set_up().0, add.clone(), 0),
        (
            "user_add_by_nobody",
            commands::set_up().0,
            with_value(&add, "--by", Some("sma")),
            1,
        ),
        (
            "user_add_by_not_an_admin",
            set_up_with("alex", false),
            not_an_admin,
            1,
        ),
        (
            "user_add_username_taken",
            set_up_with("alex", false),
            with_value(&add, "--username", Some("Alex")),
            1,
        ),
        ("user_add_no_database", TestDir::new(), add.clone(), 1),
        ("user_add_not_set_up", commands::migrated(), add.clone(), 1),
        (
            "user_add_display_name_with_a_colon",
            commands::set_up().0,
            with_value(&add, "--display-name", Some("Alex: kitchen")),
            0,
        ),
        (
            "user_add_username_empty",
            commands::set_up().0,
            with_value(&add, "--username", Some(" ")),
            1,
        ),
        (
            "user_add_username_empty_no_database",
            TestDir::new(),
            with_value(&add, "--username", Some(" ")),
            1,
        ),
    ];
    for (name, dir, args, code) in cases {
        let (out, transcript) = transcribed(&dir, &words(&dir, &args), fed);
        assert_exit(&out, code);
        assert_streams(&out);
        if code == 0 {
            let alex = commands::credentials(dir.path(), "alex")
                .unwrap_or_else(|| panic!("{name}: alex was not added"));
            assert!(!alex.user.is_admin, "{name}: alex was added as an admin");
            commands::assert_hashed_as_spec_5_3(&alex, PASSWORD);
        }
        let database = commands::database(dir.path());
        if database.exists() {
            assert_no_side_files(&database, name);
        }
        insta::assert_snapshot!(name, transcript);
    }
}

/// The arguments of `nage user reset-password` by sam for alex.
const RESET: [&str; 9] = [
    "user",
    "reset-password",
    "--data-dir",
    "DATA",
    "--by",
    "sam",
    "--username",
    "alex",
    "--password-stdin",
];

/// A working directory set up with the admin sam, the user alex and the admin deputy, whom sam
/// deactivated (planted as the tests plant a deactivation until its operation exists).
fn with_a_deactivated_admin() -> TestDir {
    let (dir, initialized) = commands::set_up();
    let sam = initialized.admin.id;
    commands::add_user(dir.path(), sam, "alex", "another long password", false);
    let deputy = commands::add_user(dir.path(), sam, "deputy", "a third long password", true);
    block_on(async {
        let storage = commands::open(dir.path()).await;
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin the deactivation: {error}"));
        test_support::deactivate_unchecked(&mut tx, deputy.id, sam, commands::earlier()).await;
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit the deactivation: {error}"));
        storage.close().await;
    });
    dir
}

/// Every state of `nage user reset-password`, run as a process: alex's password reset by sam and
/// sam's own (the new password then verifies), and every refusal: `--by` naming nobody, `--by`
/// naming a deactivated admin (the core's refusal in the terminal's own words, given before the
/// password is asked for, so the same without a terminal) and `--username` naming nobody.
///
/// Proves: decisions 150 and 157 (every state of nage user reset-password, the core's refusals in the terminal's own words)
#[test]
fn reset_password_reads_as_its_transcripts() {
    let password = password_bytes();
    let fed = Input::File(PASSWORD_TXT, password.as_slice());
    let reset = RESET.to_vec();
    let cases: Vec<(&str, TestDir, Vec<&str>, i32, &str)> = vec![
        (
            "reset_done",
            set_up_with("alex", false),
            reset.clone(),
            0,
            "alex",
        ),
        (
            "reset_done_own",
            set_up_with("alex", false),
            with_value(&reset, "--username", Some("sam")),
            0,
            "sam",
        ),
        (
            "reset_by_nobody",
            set_up_with("alex", false),
            with_value(&reset, "--by", Some("sma")),
            1,
            "alex",
        ),
        (
            "reset_by_a_deactivated_admin",
            with_a_deactivated_admin(),
            with_value(
                &with_value(&reset, "--by", Some("deputy")),
                "--username",
                Some("sam"),
            ),
            1,
            "sam",
        ),
        (
            "reset_unknown_user",
            set_up_with("alex", false),
            with_value(&reset, "--username", Some("alexx")),
            1,
            "alex",
        ),
    ];
    for (name, dir, args, code, user) in cases {
        let (out, transcript) = transcribed(&dir, &words(&dir, &args), fed);
        assert_exit(&out, code);
        assert_streams(&out);
        if code == 0 {
            let credentials = commands::credentials(dir.path(), user)
                .unwrap_or_else(|| panic!("{name}: {user} is gone"));
            commands::assert_hashed_as_spec_5_3(&credentials, PASSWORD);
        }
        insta::assert_snapshot!(name, transcript);
    }

    // The deactivated admin is refused before the password is asked for: without a terminal and
    // without --password-stdin the refusal is still the core's, never the missing terminal's.
    let dir = with_a_deactivated_admin();
    let args: Vec<&str> = with_value(
        &with_value(&reset, "--by", Some("deputy")),
        "--username",
        Some("sam"),
    )
    .into_iter()
    .filter(|word| *word != "--password-stdin")
    .collect();
    let (out, transcript) = transcribed(&dir, &words(&dir, &args), Input::NoTerminal);
    assert_exit(&out, 1);
    assert_streams(&out);
    insta::assert_snapshot!(
        "reset_by_a_deactivated_admin_before_the_password",
        transcript
    );
}

/// Runs `statements` on the database at `path` through a plain connection with foreign keys off,
/// and closes it: unchecked SQL that plants a corruption (decision 57).
fn plant(path: &Path, statements: &[&str]) {
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

/// A working directory whose book breaks two rules: Assets:Checking carries a second opening
/// balance, and the account Groceries was deleted under a posting with foreign keys off. The
/// posting on Groceries is written through the core; the corruption is planted.
fn broken_book() -> TestDir {
    let (dir, initialized) = commands::set_up();
    block_on(async {
        let storage = commands::open(dir.path()).await;
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
            commands::earlier(),
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
    plant(&commands::database(dir.path()), &statements);
    dir
}

/// A working directory whose book breaks exactly one rule: Assets:Checking carries a second
/// opening balance, planted as [`broken_book`] plants it.
fn one_problem_book() -> TestDir {
    let (dir, _initialized) = commands::set_up();
    let opening = [
        "INSERT INTO transactions (date, payee, memo, origin, version, created_by, created_at, updated_by, updated_at) VALUES ('2026-01-01', '', '', 'opening_balance', 1, 1, '2026-11-09T12:00:00Z', 1, '2026-11-09T12:00:00Z')",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Checking'), 100000)",
        "INSERT INTO postings (transaction_id, account_id, amount) VALUES ((SELECT max(id) FROM transactions), (SELECT id FROM accounts WHERE name = 'Opening Balances'), -100000)",
    ];
    let mut statements: Vec<&str> = Vec::new();
    statements.extend_from_slice(&opening);
    statements.extend_from_slice(&opening);
    plant(&commands::database(dir.path()), &statements);
    dir
}

/// A working directory whose database, at this version and set up, has lost its `postings`
/// table (dropped with foreign keys off): the check's queries cannot run.
fn without_postings() -> TestDir {
    let (dir, _initialized) = commands::set_up();
    plant(&commands::database(dir.path()), &["DROP TABLE postings"]);
    dir
}

/// Every state of `nage check`, run as a process: a book that passes (exit 0, with what was
/// checked and the database), no book yet (exit 0), a book that breaks rules (exit 1, the lines
/// nage-core writes for the planted corruption under a bare `Why:`), a book that breaks exactly
/// one rule (counted as one problem), a database whose check cannot read it (a table dropped),
/// no database, a database behind this version and one a newer Nage wrote (exit 1). The database
/// is never updated.
///
/// Proves: decisions 97 and 157 (the new shape, exit 0 and 1; the broken book a second opening balance planted on Assets:Checking and the account Groceries deleted under a posting, foreign keys off: the real INV-13 and FK lines)
#[test]
fn check_reads_as_its_transcripts() {
    let args = ["check", "--data-dir", "DATA"];
    let cases: Vec<(&str, TestDir, i32)> = vec![
        ("check_passed", commands::set_up().0, 0),
        ("check_not_set_up", commands::migrated(), 0),
        ("check_broken", broken_book(), 1),
        ("check_one_problem", one_problem_book(), 1),
        ("check_cannot_read", without_postings(), 1),
        ("check_no_database", TestDir::new(), 1),
        ("check_database_behind", commands::behind(), 1),
        ("check_database_newer", commands::ahead(), 1),
    ];
    for (name, dir, code) in cases {
        let (out, transcript) = transcribed(&dir, &words(&dir, &args), Input::Nothing);
        assert_exit(&out, code);
        assert_streams(&out);
        if name == "check_no_database" {
            assert!(
                !dir.path().join("data").exists(),
                "check created the data folder"
            );
        }
        if name == "check_database_behind" {
            assert_eq!(
                commands::schema_version(dir.path()),
                Some(1),
                "check updated the database"
            );
        }
        insta::assert_snapshot!(name, transcript);
    }
}

/// Leaves a database of one page and no table at `path`, as a set-up cut off after the open
/// leaves it: the file created and switched to WAL (an unchecked PRAGMA, decision 57).
fn one_page_without_tables(path: &Path) {
    block_on(async {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&mut conn)
            .await
            .unwrap_or_else(|error| panic!("cannot switch {} to WAL: {error}", path.display()));
        conn.close()
            .await
            .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
    });
}

/// `nage check`, `nage user add` and `nage user reset-password` read an empty database alike
/// whatever its bytes: a zero-byte file and a file of one page without tables give the same
/// transcript, in `nage check`'s words, and neither file changes by a byte nor gains a side file.
///
/// Proves: decision 155, ruling P6 (check, user add and user reset-password on a zero-byte file and on a one-page file without tables; neither file changes)
#[test]
fn an_empty_database_reads_alike_whatever_its_bytes() {
    let password = password_bytes();
    let fed = Input::File(PASSWORD_TXT, password.as_slice());
    let cases: [(&str, Vec<&str>, Input<'_>); 3] = [
        (
            "check_database_empty",
            vec!["check", "--data-dir", "DATA"],
            Input::Nothing,
        ),
        ("user_add_database_empty", USER_ADD.to_vec(), fed),
        ("reset_database_empty", RESET.to_vec(), fed),
    ];
    for (name, args, input) in cases {
        let mut seen: Vec<String> = Vec::new();
        for one_page in [false, true] {
            let dir = TestDir::new();
            fs::create_dir(dir.path().join("data"))
                .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
            let database = commands::database(dir.path());
            if one_page {
                one_page_without_tables(&database);
            } else {
                fs::write(&database, b"")
                    .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
            }
            let before = fs::read(&database)
                .unwrap_or_else(|error| panic!("cannot read the empty database: {error}"));
            assert_eq!(
                before.is_empty(),
                !one_page,
                "{name}: the empty database of one page has {} bytes",
                before.len()
            );
            let (out, transcript) = transcribed(&dir, &words(&dir, &args), input);
            assert_exit(&out, 1);
            assert_streams(&out);
            assert_eq!(
                fs::read(&database)
                    .unwrap_or_else(|error| panic!("cannot read the empty database: {error}")),
                before,
                "{name}: the command changed the empty database (one page: {one_page})"
            );
            assert_no_side_files(&database, &format!("after {name} (one page: {one_page})"));
            seen.push(transcript);
        }
        assert_eq!(
            seen[0], seen[1],
            "{name}: a zero-byte file and a page without tables read differently"
        );
        let transcript = seen.swap_remove(0);
        insta::assert_snapshot!(name, transcript);
    }
}

/// `nage health` prints nothing when the server answers 200, and three lines otherwise: a server
/// that answers 503, a port where nothing listens, a listener that never answers (after the
/// two-second deadline), a program that answers something that is not HTTP (the request failed,
/// in the system's own words), and a server that answers 204 or a redirect, exit 1 for each.
///
/// Proves: decisions 44 and 157 (silent when healthy, three lines otherwise)
#[test]
fn health_reads_as_its_transcripts() {
    let dir = TestDir::new();
    let probe = |name: &str, port: u16, code: i32| {
        let bind = format!("127.0.0.1:{port}");
        let mut command = nage();
        command
            .current_dir(dir.path())
            .args(["health", "--bind", bind.as_str()]);
        let shown = transcript(&command).dir(dir.path()).port(port);
        let out = run(&mut command);
        assert_exit(&out, code);
        assert_streams(&out);
        if code == 0 {
            assert!(
                out.stdout.is_empty(),
                "a healthy probe printed {:?}",
                out.stdout
            );
        }
        let transcript = shown.of(&out);
        insta::assert_snapshot!(name, transcript);
    };
    let healthy = HealthServer::start(true);
    probe("health_healthy", healthy.addr().port(), 0);
    drop(healthy);
    let failing = HealthServer::start(false);
    probe("health_answered_503", failing.addr().port(), 1);
    drop(failing);
    let closed = ClosedPort::v4();
    probe("health_nothing_listening", closed.port(), 1);
    drop(closed);
    let hole = BlackHole::new();
    probe("health_no_answer", hole.addr().port(), 1);
    drop(hole);
    // A program that is not Nage answers something that is not HTTP: the request failed.
    let garbage = answering(b"NOT HTTP\r\n\r\n");
    probe("health_request_failed", garbage.port(), 1);
    // Only 200 is healthy: a server that answers 204, or sends the probe elsewhere, is not.
    let no_content = answering(b"HTTP/1.1 204 No Content\r\n\r\n");
    probe("health_answered_204", no_content.port(), 1);
    let redirect = answering(b"HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\n\r\n");
    probe("health_answered_302", redirect.port(), 1);
}

/// A loopback listener that reads the head of each request it accepts, answers `answer` and
/// closes the connection, on a thread of its own for the rest of the test run: a program that is
/// not Nage, or a server that answers a status of its own.
fn answering(answer: &'static [u8]) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("cannot bind a loopback port: {error}"));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("cannot read the listener's address: {error}"));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let _ = stream.write_all(answer);
        }
    });
    addr
}
