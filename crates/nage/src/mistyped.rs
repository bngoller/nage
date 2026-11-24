//! A command line the parser refuses, in Nage's own three lines (decision 157, the maintainer's
//! pick "Nage's three lines"): `Error: The command was not run.`, the reason in everyday words for
//! each kind of mistake Nage foresees (a missing option, an option or a command it does not know, a
//! missing command, an option without its value), the library's own sentence for any other kind,
//! and the `--help` hint of the command as typed. Exit 2, as before (decision 45). `--help` and
//! `--version` keep the library's layout and exit 0; their colours follow Nage's own rule, set in
//! `main` (decision 158): only when standard output is a terminal and `NO_COLOR` is unset.

use std::ffi::OsString;
use std::process::ExitCode;

use clap::CommandFactory as _;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use crate::cli::Cli;
use crate::shape::{Doing, Refused, Screen, capitalized};

/// What the binary ends with when the parser refused the command line `typed` (the program's
/// arguments, the program's own name first): the library's help or version text, or Nage's three
/// lines with exit 2.
pub(crate) fn stop(error: &clap::Error, typed: &[OsString], screen: Screen) -> ExitCode {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // A closed standard output changes nothing: the text was all there was to do.
            let _ = error.print();
            ExitCode::SUCCESS
        }
        _ => screen.refused(&refused(error, typed), 2),
    }
}

/// The three lines for a refused command line (decision 157).
pub(crate) fn refused(error: &clap::Error, typed: &[OsString]) -> Refused {
    let command = typed_command(typed);
    let options = format!("{command} --help shows every option.");
    let commands = format!("{command} --help shows every command.");
    match error.kind() {
        ErrorKind::MissingRequiredArgument => {
            let flags = flags(error);
            let it = if flags.len() == 1 { "it" } else { "them" };
            Refused::new(
                Doing::Command.not_done(),
                missing(&flags),
                format!("Add {it} and run the command again; {options}"),
            )
        }
        ErrorKind::UnknownArgument => Refused::new(
            Doing::Command.not_done(),
            format!("{} is not an option of {command}.", invalid(error)),
            format!("Check the spelling and run the command again; {options}"),
        ),
        ErrorKind::InvalidSubcommand => Refused::new(
            Doing::Command.not_done(),
            format!("{} is not a command of {command}.", invalid(error)),
            format!("Check the spelling and run the command again; {commands}"),
        ),
        // A missing command: `nage` and `nage user` take no option but `--help` and `--version`,
        // so the library always reports it as this kind, never as `MissingSubcommand` (which it
        // gives only when another option was matched); that kind gets the library's sentence.
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => Refused::new(
            Doing::Command.not_done(),
            format!("No command was given after {command}."),
            format!("Name one and run it again; {commands}"),
        ),
        ErrorKind::InvalidValue => Refused::new(
            Doing::Command.not_done(),
            format!("{} needs a value after it.", invalid(error)),
            format!("Give one and run the command again; {options}"),
        ),
        _ => Refused::new(
            Doing::Command.not_done(),
            format!("{}.", capitalized(&library_sentence(error))),
            format!("Correct the command line and run it again; {options}"),
        ),
    }
}

/// The reason for the options `flags` left out: "--username is missing.", "--username and
/// --display-name are missing.", "--book-name, --username and --display-name are missing.".
pub(crate) fn missing(flags: &[String]) -> String {
    match flags {
        [] => String::from("An option it needs is missing."),
        [one] => format!("{one} is missing."),
        [first @ .., last] => format!("{} and {last} are missing.", first.join(", ")),
    }
}

/// The command as typed, up to its last command word, for the `--help` hint: `nage`, `nage init`,
/// `nage user add`; the program is always called `nage`, whatever path it was started by.
fn typed_command(typed: &[OsString]) -> String {
    let mut words = vec![String::from("nage")];
    let mut command = Cli::command();
    for word in typed.iter().skip(1) {
        let Some(word) = word.to_str() else { break };
        let Some(sub) = command.find_subcommand(word) else {
            break;
        };
        words.push(sub.get_name().to_owned());
        let sub = sub.clone();
        command = sub;
    }
    words.join(" ")
}

/// The options the library says are missing, by their long names alone (`--username`, without
/// `<NAME>`), in the library's order.
fn flags(error: &clap::Error) -> Vec<String> {
    match error.get(ContextKind::InvalidArg) {
        Some(ContextValue::Strings(args)) => args.iter().map(|arg| long_name(arg)).collect(),
        Some(ContextValue::String(arg)) => vec![long_name(arg)],
        _ => Vec::new(),
    }
}

/// An argument as the library names it (`--username <NAME>`), by its long name alone.
fn long_name(arg: &str) -> String {
    arg.split_whitespace().next().unwrap_or(arg).to_owned()
}

/// The option or command the library refused, as typed, quoted as Rust quotes a string when it is
/// not an option's long name.
fn invalid(error: &clap::Error) -> String {
    let refused = match error.get(ContextKind::InvalidSubcommand) {
        Some(ContextValue::String(command)) => command.clone(),
        _ => match error.get(ContextKind::InvalidArg) {
            Some(ContextValue::String(arg)) => long_name(arg),
            Some(ContextValue::Strings(args)) => {
                args.first().map(|arg| long_name(arg)).unwrap_or_default()
            }
            _ => String::new(),
        },
    };
    let option = refused.len() > 2
        && refused.starts_with("--")
        && refused
            .chars()
            .skip(2)
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if option {
        refused
    } else {
        format!("{refused:?}")
    }
}

/// The library's own sentence for a kind of mistake Nage does not foresee: its first line,
/// without its `error: ` and without any colour.
fn library_sentence(error: &clap::Error) -> String {
    // The rendered text's `Display` carries no colour.
    let text = error.render().to_string();
    let first = text.lines().next().unwrap_or_default();
    first
        .strip_prefix("error: ")
        .unwrap_or(first)
        .trim_end_matches('.')
        .to_owned()
}
