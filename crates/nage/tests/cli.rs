//! The `nage` command line: `--version`, usage errors and help text (milestone 1 slice 2).

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

/// `nage` without a subcommand is a usage error: exit 2, with the usage on stderr naming
/// both subcommands.
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
    assert!(
        out.stderr.contains("Usage"),
        "stderr of `{}` lacks \"Usage\":\n{}",
        out.what,
        out.stderr
    );
    for name in ["serve", "health"] {
        assert!(
            lists_subcommand(&out.stderr, name),
            "stderr of `{}` does not list the subcommand {name:?}:\n{}",
            out.what,
            out.stderr
        );
    }
}

/// An unknown subcommand is a usage error: exit 2.
///
/// Proves: Decision 45.
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
    for name in ["serve", "health"] {
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
        ],
    );

    assert_documents(
        &run(nage().args(["health", "--help"])),
        &[("--bind", "NAGE_BIND", "127.0.0.1:8787")],
    );
}
