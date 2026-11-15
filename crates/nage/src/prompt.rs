//! The password a command sets (SPEC W1 step 4, decision 113): asked for twice at the
//! terminal without being shown, or read once from standard input with `--password-stdin`.
//! The terminal stands behind a small trait, so that tests can answer in its place; the real
//! one is `rpassword`, which asks on the terminal itself and never on standard input, so a
//! piped password is read only when `--password-stdin` says so.

use std::io::{self, Read};

use crate::messages::{Stop, Task};

#[cfg(test)]
mod tests;

/// The terminal a password is asked for on.
pub(crate) trait Terminal {
    /// Writes `question` on the terminal and reads one line from it without showing what is
    /// typed. Returns the line without its line ending and otherwise exactly as typed: nothing
    /// is trimmed.
    fn ask(&mut self, question: &str) -> Result<String, AskError>;
}

/// Why a question was not answered.
#[derive(Debug)]
pub(crate) enum AskError {
    /// The process has no terminal to ask on (a cron job, `docker exec` without `-t`, a test).
    NoTerminal,
    /// The terminal is there and writing the question or reading the answer failed, or the
    /// input ended before any answer.
    Failed(io::Error),
}

/// The process's own terminal, through `rpassword` (decision 113): the question is written to
/// the controlling terminal and the answer is read from it with the echo off, whatever
/// standard input, output and error are connected to.
pub(crate) struct Tty;

impl Terminal for Tty {
    /// Asks on the controlling terminal. A process without one answers
    /// [`AskError::NoTerminal`] and writes nothing anywhere.
    fn ask(&mut self, question: &str) -> Result<String, AskError> {
        // `rpassword` reports a process without a terminal only as the error of opening one,
        // like any other failure; opening the terminal first tells the two apart, before
        // anything is written.
        if OpenOptions::new()
            .read(true)
            .write(true)
            .open(TERMINAL)
            .is_err()
        {
            return Err(AskError::NoTerminal);
        }
        rpassword::prompt_password(question).map_err(AskError::Failed)
    }
}

/// Where the password comes from: the terminal, or standard input with `--password-stdin`.
pub(crate) enum Source<'a> {
    /// Asked for twice.
    Terminal(&'a mut dyn Terminal),
    /// Read once.
    Stdin(&'a mut dyn Read),
}

/// The new password `task` sets for `username`, which has passed the core's password rules
/// (decisions 107 and 113).
///
/// From the terminal it is asked for twice, first with `messages::question(task, username)`,
/// then with `messages::QUESTION_AGAIN`, and the two answers are compared exactly as typed.
/// Answers that differ are `Refusal::PasswordsDiffer`, and nothing is asked a third time.
/// [`AskError::NoTerminal`] at either question is `Refusal::NoTerminal`, a usage error, and
/// no further question is asked; any other failure of the terminal is `Stop::Failure`
/// (`cannot read the password from the terminal: ...`).
///
/// From standard input it is read once, to the end, and one trailing line ending is removed
/// (`\r\n`, else `\n`, else `\r`), as `docker login --password-stdin` does; everything else is
/// the password, spaces and further line endings included. Nothing left is
/// `Refusal::NoPasswordOnStdin`; bytes that are not UTF-8 are `Refusal::PasswordNotText`; a
/// read that fails is `Stop::Failure` (`cannot read the password from standard input: ...`).
/// Reading may stop once more bytes have arrived than a password may have: the password is
/// then refused as too long.
///
/// Either way the password is then checked with `nage_core::password::check`, as typed, and
/// one that breaks a rule is refused in the core's words (`Stop::Password`), so that no hash
/// is made of a password the rules refuse. At the terminal the comparison comes first: two
/// different answers are refused as different, whatever their length.
pub(crate) fn new_password(
    source: &mut Source<'_>,
    task: Task,
    username: &str,
) -> Result<String, Stop> {
    let password = match source {
        Source::Terminal(terminal) => asked_twice(&mut **terminal, task, username)?,
        Source::Stdin(input) => read_once(&mut **input, task)?,
    };
    password::check(&password)?;
    Ok(password)
}

use std::fs::OpenOptions;

use nage_core::password::{self, MAX_PASSWORD_BYTES, PasswordError};

use crate::messages::{QUESTION_AGAIN, Refusal, question};

/// The controlling terminal, on which `rpassword` asks.
const TERMINAL: &str = "/dev/tty";

/// The most bytes `--password-stdin` reads: the longest password the rules allow and a `\r\n`,
/// and one byte more, which proves the password too long without reading the rest.
const STDIN_LIMIT: usize = MAX_PASSWORD_BYTES + "\r\n".len() + 1;

/// The password asked for twice at `terminal` and compared as typed (`new_password`'s order).
fn asked_twice(terminal: &mut dyn Terminal, task: Task, username: &str) -> Result<String, Stop> {
    let first = answer(terminal.ask(&question(task, username)), task)?;
    let again = answer(terminal.ask(QUESTION_AGAIN), task)?;
    if first != again {
        return Err(Refusal::PasswordsDiffer { task }.into());
    }
    Ok(first)
}

/// One answer of the terminal, or what the command ends with instead.
fn answer(asked: Result<String, AskError>, task: Task) -> Result<String, Stop> {
    asked.map_err(|error| match error {
        AskError::NoTerminal => Refusal::NoTerminal { task }.into(),
        AskError::Failed(cause) => Stop::Failure(format!(
            "cannot read the password from the terminal: {cause}"
        )),
    })
}

/// The password read once from `input`, to its end or to [`STDIN_LIMIT`], without one trailing
/// line ending (`new_password`'s order).
fn read_once(input: &mut dyn Read, task: Task) -> Result<String, Stop> {
    let mut bytes = Vec::new();
    input
        .take(STDIN_LIMIT as u64)
        .read_to_end(&mut bytes)
        .map_err(|cause| {
            Stop::Failure(format!(
                "cannot read the password from standard input: {cause}"
            ))
        })?;
    if bytes.len() == STDIN_LIMIT {
        return Err(PasswordError::TooLong.into());
    }
    let password = bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .or_else(|| bytes.strip_suffix(b"\r"))
        .unwrap_or(&bytes);
    if password.is_empty() {
        return Err(Refusal::NoPasswordOnStdin { task }.into());
    }
    std::str::from_utf8(password)
        .map(str::to_owned)
        .map_err(|_| Refusal::PasswordNotText { task }.into())
}
