//! The `nage` command line: `--version`, usage errors and help text (milestone 1 slice 2),
//! and the flags of `init`, `user add` and `user reset-password` (milestone 2 slice 5).

mod common;

use common::{Finished, VERSION, nage, run};

/// True when `help` lists `name` as a subcommand: an indented line whose first word is the
/// name. A plain substring search would not do, since `server` contains `serve`.
fn lists_subcommand(help: &str, name: &str) -> bool {
    help.lines()
        .any(|line| line.starts_with(' ') && line.split_whitespace().next() == Some(name))
}

/// `nage --version` prints `nage <version>` and a newline on stdout and exits 0.
///
/// Proves: PLAN M1 slice 2.
#[test]
fn version_flag_prints_name_and_version() {
    let out = run(nage().arg("--version"));
    assert_eq!(
        out.stdout,
        format!("nage {VERSION}\n"),
        "stdout of `{}`",
        out.what
    );
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
}

/// `nage` without a subcommand is a usage error: exit 2, with Nage's three lines on stderr
/// pointing to `nage --help` (decision 157), which names every subcommand.
///
/// Proves: SPEC 5.2, decision 45.
#[test]
fn no_subcommand_is_a_usage_error() {
    let out = run(&mut nage());
    assert_eq!(
        out.code(),
        Some(2),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert_eq!(
        out.stderr,
        "Error: The command was not run.\n\
         Why: No command was given after nage.\n\
         What now: Name one and run it again; nage --help shows every command.\n",
        "stderr of `{}`",
        out.what
    );
    let help = run(nage().arg("--help"));
    for name in ["serve", "health", "init", "user", "check", "migrate"] {
        assert!(
            lists_subcommand(&help.stdout, name),
            "stdout of `{}` does not list the subcommand {name:?}:\n{}",
            help.what,
            help.stdout
        );
    }
}

/// An unknown subcommand is a usage error: exit 2.
///
/// Proves: decision 45.
#[test]
fn unknown_subcommand_is_a_usage_error() {
    let out = run(nage().arg("frobnicate"));
    assert_eq!(
        out.code(),
        Some(2),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
}

/// The line of `help` that documents the option `flag`: an indented line whose first word is
/// the flag, as clap prints it when the help of an option fits on its own line.
fn option_line<'a>(help: &'a str, flag: &str) -> Option<&'a str> {
    help.lines()
        .find(|line| line.starts_with(' ') && line.split_whitespace().next() == Some(flag))
}

/// Asserts that `out` exited 0 and that, for each `(flag, variable, default)`, the line
/// documenting the flag names the variable it stands for and its default.
fn assert_documents(out: &Finished, options: &[(&str, &str, &str)]) {
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    for &(flag, variable, default) in options {
        let Some(line) = option_line(&out.stdout, flag) else {
            panic!("`{}` does not document {flag:?}:\n{}", out.what, out.stdout);
        };
        for needle in [variable, default] {
            assert!(
                line.contains(needle),
                "`{}` documents {flag:?} without {needle:?}:\n{line}",
                out.what
            );
        }
    }
}

/// The help texts list the subcommands, and each subcommand's help names its flags, the
/// variables they stand for and the defaults.
///
/// Proves: SPEC 5.2, decision 42.
#[test]
fn help_documents_subcommands_defaults_and_env_vars() {
    let top = run(nage().arg("--help"));
    for name in ["serve", "health", "init", "user", "check"] {
        assert!(
            lists_subcommand(&top.stdout, name),
            "`{}` does not list the subcommand {name:?}:\n{}",
            top.what,
            top.stdout
        );
    }
    assert_eq!(
        top.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        top.what,
        top.status,
        top.stderr
    );

    assert_documents(
        &run(nage().args(["serve", "--help"])),
        &[
            ("--data-dir", "NAGE_DATA_DIR", "./data"),
            ("--bind", "NAGE_BIND", "127.0.0.1:8787"),
            ("--tz", "NAGE_TZ", "UTC"),
            ("--log", "NAGE_LOG", "info"),
            ("--trust-proxy", "NAGE_TRUST_PROXY", "off"),
        ],
    );

    assert_documents(
        &run(nage().args(["health", "--help"])),
        &[("--bind", "NAGE_BIND", "127.0.0.1:8787")],
    );

    assert_documents(
        &run(nage().args(["check", "--help"])),
        &[("--data-dir", "NAGE_DATA_DIR", "./data")],
    );

    let user = run(nage().args(["user", "--help"]));
    for name in ["add", "reset-password"] {
        assert!(
            lists_subcommand(&user.stdout, name),
            "`{}` does not list the subcommand {name:?}:\n{}",
            user.what,
            user.stdout
        );
    }
    for command in [
        &["init", "--help"][..],
        &["user", "add", "--help"],
        &["user", "reset-password", "--help"],
    ] {
        assert_documents(
            &run(nage().args(command)),
            &[("--data-dir", "NAGE_DATA_DIR", "./data")],
        );
    }
}

/// The flags each of the three commands takes besides `--data-dir`, as its help lists them.
const INIT_FLAGS: [&str; 6] = [
    "--book-name",
    "--currency",
    "--username",
    "--display-name",
    "--no-starter-accounts",
    "--password-stdin",
];
const USER_ADD_FLAGS: [&str; 4] = ["--by", "--username", "--display-name", "--password-stdin"];
const USER_RESET_FLAGS: [&str; 3] = ["--by", "--username", "--password-stdin"];

/// The help of `init`, `user add` and `user reset-password` documents each flag the command
/// takes: the names as flags, `--no-starter-accounts` for `init`, `--by` for the two user
/// commands and `--password-stdin` for all three; `init`'s line for `--currency` says that
/// the currency never changes, and its usage line names the flag. The test prints the help
/// of `nage`, of `nage user` and of the three commands (`cargo test -p nage --test cli
/// help_documents_the_flags -- --nocapture` shows them).
///
/// Proves: decision 113 (the three commands take their names as flags: `--book-name`,
/// `--currency`, `--username`, `--display-name`, `--no-starter-accounts`, `--by`,
/// `--password-stdin`), SPEC 5.2.
#[test]
fn help_documents_the_flags_of_init_and_the_user_commands() {
    let cases: [(&[&str], &[&str]); 5] = [
        (&["--help"], &[]),
        (&["init", "--help"], &INIT_FLAGS),
        (&["user", "--help"], &[]),
        (&["user", "add", "--help"], &USER_ADD_FLAGS),
        (&["user", "reset-password", "--help"], &USER_RESET_FLAGS),
    ];
    for (command, flags) in cases {
        let out = run(nage().args(command));
        assert_eq!(
            out.code(),
            Some(0),
            "exit code of `{}` ({}); stderr:\n{}",
            out.what,
            out.status,
            out.stderr
        );
        for line in out.stdout.lines() {
            println!("[help {}] {line}", out.what);
        }
        for flag in flags {
            assert!(
                option_line(&out.stdout, flag).is_some(),
                "`{}` does not document {flag:?}:\n{}",
                out.what,
                out.stdout
            );
        }
    }
    let init = run(nage().args(["init", "--help"]));
    let currency = option_line(&init.stdout, "--currency").unwrap_or_default();
    assert!(
        currency.contains("never changes"),
        "`{}` documents --currency without saying that it never changes:\n{currency}",
        init.what
    );
    let usage = init
        .stdout
        .lines()
        .find(|line| line.starts_with("Usage: "))
        .unwrap_or_default();
    assert!(
        usage.contains("--currency <CODE>"),
        "the usage line of `{}` does not name --currency: {usage}",
        init.what
    );
}

/// `nage --help`, `nage user --help` and the help of every command read as written, each pinned
/// as a text snapshot of its transcript: the library's layout, in everyday words (the data
/// folder, the database updated, the rules of the book), exit 0 and nothing on standard error.
/// Into a pipe, with CLICOLOR_FORCE=1 and a colour terminal in TERM, the help and the version hold
/// no colour code.
///
/// Proves: decision 157, ruling P8 (nage --help and every command's help pinned, in everyday words)
#[test]
fn help_texts_read_as_written() {
    let cases: [(&str, &[&str]); 9] = [
        ("help_nage", &["--help"]),
        ("help_serve", &["serve", "--help"]),
        ("help_health", &["health", "--help"]),
        ("help_init", &["init", "--help"]),
        ("help_user", &["user", "--help"]),
        ("help_user_add", &["user", "add", "--help"]),
        (
            "help_user_reset_password",
            &["user", "reset-password", "--help"],
        ),
        ("help_check", &["check", "--help"]),
        ("help_migrate", &["migrate", "--help"]),
    ];
    let dir = nage_core::test_support::TestDir::new();
    for (name, args) in cases {
        let mut command = nage();
        command.current_dir(dir.path()).args(args);
        let shown = common::transcript::transcript(&command).dir(dir.path());
        let out = run(&mut command);
        assert_eq!(
            out.code(),
            Some(0),
            "exit code of `{}` ({}); stderr:\n{}",
            out.what,
            out.status,
            out.stderr
        );
        let transcript = shown.of(&out);
        insta::assert_snapshot!(name, transcript);
    }

    // Into a pipe the help stays plain whatever CLICOLOR_FORCE or TERM say (decision 158): Nage
    // sets the library's colour itself.
    for args in [&["--help"][..], &["init", "--help"], &["--version"]] {
        let out = run(nage()
            .current_dir(dir.path())
            .env("CLICOLOR_FORCE", "1")
            .env("TERM", "xterm-256color")
            .args(args));
        assert_eq!(
            out.code(),
            Some(0),
            "exit code of `{}` ({}); stderr:\n{}",
            out.what,
            out.status,
            out.stderr
        );
        assert!(
            !out.stdout.contains('\u{1b}') && !out.stderr.contains('\u{1b}'),
            "`{}` coloured a pipe:\n{:?}",
            out.what,
            out.stdout
        );
    }
}

/// One of the three commands: its words, every name it takes as a flag with a value, and the
/// flags it cannot do without.
struct Names {
    command: &'static [&'static str],
    flags: &'static [(&'static str, &'static str)],
    required: &'static [&'static str],
}

/// Each command refuses a command line that leaves out one of its names, before anything is
/// looked at: `init` without `--book-name`, `--username` or `--display-name`, `user add`
/// without `--by`, `--username` or `--display-name`, `user reset-password` without `--by` or
/// `--username`, and `user` without a subcommand are usage errors, exit code 2, with Nage's
/// three lines on standard error naming what is missing (decision 157); nothing is created in the working
/// directory. The password is never a flag: `--password` is an unknown argument. The test
/// prints what `nage user add` says without `--by`.
///
/// Proves: decision 113 (the three commands take their names as flags, and the password
/// from the terminal or standard input only), decision 45 (a usage error exits 2).
#[test]
fn the_three_commands_require_their_names_as_flags() {
    let cases = [
        Names {
            command: &["init"],
            flags: &[
                ("--book-name", "Household"),
                ("--currency", "CAD"),
                ("--username", "sam"),
                ("--display-name", "Sam"),
            ],
            required: &["--book-name", "--username", "--display-name"],
        },
        Names {
            command: &["user", "add"],
            flags: &[
                ("--by", "sam"),
                ("--username", "alex"),
                ("--display-name", "Alex"),
            ],
            required: &["--by", "--username", "--display-name"],
        },
        Names {
            command: &["user", "reset-password"],
            flags: &[("--by", "sam"), ("--username", "alex")],
            required: &["--by", "--username"],
        },
    ];
    let dir = nage_core::test_support::TestDir::new();
    let usage_error = |command: &mut std::process::Command, needle: &str| {
        let out = run(command.current_dir(dir.path()));
        if out.what.starts_with("nage user add") && needle == "--by" {
            for line in out.stderr.lines() {
                println!("[usage {}] {line}", out.what);
            }
        }
        assert_eq!(
            out.code(),
            Some(2),
            "exit code of `{}` ({}); stderr:\n{}",
            out.what,
            out.status,
            out.stderr
        );
        let lines: Vec<&str> = out.stderr.lines().collect();
        assert!(
            lines.len() == 3
                && lines[0] == "Error: The command was not run."
                && lines[1].starts_with("Why: ")
                && lines[2].starts_with("What now: ")
                && out.stderr.contains(needle),
            "stderr of `{}` is not Nage's three lines naming {needle:?}:\n{}",
            out.what,
            out.stderr
        );
        assert!(
            out.stdout.is_empty(),
            "stdout of `{}`: {}",
            out.what,
            out.stdout
        );
    };
    for Names {
        command,
        flags,
        required,
    } in cases
    {
        for missing in required {
            let mut line = nage();
            line.args(command);
            for (flag, value) in flags {
                if flag != missing {
                    line.args([flag, value]);
                }
            }
            line.arg("--password-stdin");
            usage_error(&mut line, missing);
        }
        let mut line = nage();
        line.args(command);
        for (flag, value) in flags {
            line.args([flag, value]);
        }
        line.args(["--password", "correct horse 42"]);
        usage_error(&mut line, "--password");
    }
    usage_error(nage().arg("user"), "nage user --help");
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.path().display()))
        .collect();
    assert!(
        left.is_empty(),
        "a usage error left something in the working directory: {left:?}"
    );
}
