//! The form of everything a command prints at the terminal (SPEC 5.2, decisions 157 and 158, the
//! mock-ups the maintainer picked at PLAN M2 slice 8's interview). A refused or failed command
//! prints three labelled lines on standard error, each at the left edge: `Error:` with what did not
//! happen, `Why:` with the reason (the system's own words, when there are any, at its end) and
//! `What now:` with what to do; a reason of several lines (`nage check`'s broken rules) stands under
//! a bare `Why:`, each line indented by two spaces. A finished command prints on standard output
//! `Success:` with one sentence, its facts one per line indented by two spaces as `Label: value`,
//! then `What now:` with the next step when there is one. Nage never breaks a line itself.
//!
//! Colour is on the labels only, and only when the stream is a terminal and `NO_COLOR` is unset
//! (decision 47's rule): `Error:` red, `Success:` green, `Why:` and `What now:` bold, in the
//! terminal's own colours; the sentences and the facts stay plain.

use std::io::{self, Write as _};
use std::process::ExitCode;

#[cfg(test)]
mod tests;

/// The label of what did not happen.
pub(crate) const ERROR: &str = "Error:";
/// The label of the reason.
pub(crate) const WHY: &str = "Why:";
/// The label of what to do.
pub(crate) const WHAT_NOW: &str = "What now:";
/// The label of a finished command's sentence.
pub(crate) const SUCCESS: &str = "Success:";

/// ANSI red, for `Error:`.
pub(crate) const RED: &str = "\u{1b}[31m";
/// ANSI green, for `Success:`.
pub(crate) const GREEN: &str = "\u{1b}[32m";
/// ANSI bold, for `Why:` and `What now:`.
pub(crate) const BOLD: &str = "\u{1b}[1m";
/// ANSI reset, after a coloured label.
pub(crate) const RESET: &str = "\u{1b}[0m";

/// Whether a stream's labels are coloured (decision 158).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Paint {
    colour: bool,
}

impl Paint {
    /// No colour, whatever the stream: the tests' rendering of a command's lines (every command
    /// judges its streams through [`Paint::new`]).
    #[cfg(test)]
    pub(crate) const PLAIN: Paint = Paint { colour: false };

    /// Colour exactly when the stream is a terminal and `NO_COLOR` is unset (or empty).
    pub(crate) fn new(terminal: bool, no_color: bool) -> Paint {
        Paint {
            colour: terminal && !no_color,
        }
    }

    /// Whether the labels are coloured.
    pub(crate) fn colours(self) -> bool {
        self.colour
    }

    /// `label` in `colour` and then reset, or plain.
    fn label(self, label: &str, colour: &str) -> String {
        if self.colour {
            format!("{colour}{label}{RESET}")
        } else {
            label.to_owned()
        }
    }
}

/// Whether `NO_COLOR`, as the environment holds it, turns colour off (decisions 42 and 158): any
/// value that is not empty does, an unset or empty one does not. The value is never read as text,
/// so one that is not UTF-8 turns colour off too.
pub(crate) fn no_color(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// What a command was run to do (decision 157): it decides the first line of its refusals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Doing {
    /// `nage serve`.
    Serve,
    /// `nage health`.
    Health,
    /// `nage check`.
    Check,
    /// `nage migrate`.
    Migrate,
    /// `nage migrate --dry-run`, which only looks (ruling P7 of the pause).
    DryRun,
    /// `nage init`.
    Init,
    /// `nage user add`.
    AddUser,
    /// `nage user reset-password`.
    ResetPassword,
    /// A command line the parser refused before any command ran.
    Command,
}

impl Doing {
    /// What did not happen, the sentence after `Error:`.
    pub(crate) fn not_done(self) -> &'static str {
        match self {
            Self::Serve => "Nage did not start.",
            Self::Health => "The server's health was not confirmed.",
            Self::Check => "The book was not checked.",
            Self::Migrate => "The database was not updated.",
            Self::DryRun => "The waiting changes could not be listed.",
            Self::Init => "The book was not set up.",
            Self::AddUser => "The user was not created.",
            Self::ResetPassword => "The password was not changed.",
            Self::Command => "The command was not run.",
        }
    }
}

/// The reason of a refusal: one sentence on the `Why:` line, or lines under a bare `Why:`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// One line after `Why: `.
    Line(String),
    /// Lines under a bare `Why:`, each indented by two spaces.
    Lines(Vec<String>),
}

/// A refused or failed command's three parts, each a whole sentence or more (decision 157).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    /// What did not happen, with its full stop.
    pub(crate) what: String,
    /// Why.
    pub(crate) why: Why,
    /// What to do, with its full stop.
    pub(crate) now: String,
}

impl Refused {
    /// A refusal whose reason is one sentence.
    pub(crate) fn new(
        what: impl Into<String>,
        why: impl Into<String>,
        now: impl Into<String>,
    ) -> Refused {
        Refused {
            what: what.into(),
            why: Why::Line(why.into()),
            now: now.into(),
        }
    }

    /// A refusal whose reason is a list of lines.
    pub(crate) fn listing(
        what: impl Into<String>,
        lines: Vec<String>,
        now: impl Into<String>,
    ) -> Refused {
        Refused {
            what: what.into(),
            why: Why::Lines(lines),
            now: now.into(),
        }
    }

    /// A refusal the core words, as the terminal says it (decision 150 as the pause amended it):
    /// the command's own first line, and the refusal's twin at the terminal after `Why:` and
    /// `What now:`.
    pub(crate) fn at_the_terminal(
        doing: Doing,
        words: &nage_core::words::AtTheTerminal,
    ) -> Refused {
        Refused::new(doing.not_done(), words.why.clone(), words.now.clone())
    }

    /// The three labelled lines, each ending with a line break, the labels coloured by `paint`.
    pub(crate) fn render(&self, paint: Paint) -> String {
        let mut text = format!("{} {}\n", paint.label(ERROR, RED), self.what);
        match &self.why {
            Why::Line(why) => {
                text.push_str(&format!("{} {why}\n", paint.label(WHY, BOLD)));
            }
            Why::Lines(lines) => {
                text.push_str(&paint.label(WHY, BOLD));
                text.push('\n');
                for line in lines {
                    text.push_str(&format!("  {line}\n"));
                }
            }
        }
        text.push_str(&format!("{} {}\n", paint.label(WHAT_NOW, BOLD), self.now));
        text
    }
}

/// A finished command's sentence, facts and next step (decision 157).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finished {
    /// What was done, one sentence with its full stop.
    pub(crate) sentence: String,
    /// The facts, in order: a label without its colon and a value.
    pub(crate) facts: Vec<(&'static str, String)>,
    /// The next step, when there is one.
    pub(crate) now: Option<String>,
}

impl Finished {
    /// A finished command with only its sentence.
    pub(crate) fn new(sentence: impl Into<String>) -> Finished {
        Finished {
            sentence: sentence.into(),
            facts: Vec::new(),
            now: None,
        }
    }

    /// The same with one more fact.
    pub(crate) fn fact(mut self, label: &'static str, value: impl Into<String>) -> Finished {
        self.facts.push((label, value.into()));
        self
    }

    /// The same with its next step.
    pub(crate) fn then(mut self, now: impl Into<String>) -> Finished {
        self.now = Some(now.into());
        self
    }

    /// The lines, each ending with a line break, the labels coloured by `paint`.
    pub(crate) fn render(&self, paint: Paint) -> String {
        let mut text = format!("{} {}\n", paint.label(SUCCESS, GREEN), self.sentence);
        for (label, value) in &self.facts {
            text.push_str(&format!("  {label}: {value}\n"));
        }
        if let Some(now) = &self.now {
            text.push_str(&format!("{} {now}\n", paint.label(WHAT_NOW, BOLD)));
        }
        text
    }
}

/// How the two streams are painted, for one run of the binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Screen {
    /// Standard output's paint.
    pub(crate) out: Paint,
    /// Standard error's paint.
    pub(crate) err: Paint,
}

impl Screen {
    /// Prints a finished command on standard output and answers exit code 0.
    pub(crate) fn finished(self, finished: &Finished) -> ExitCode {
        let text = finished.render(self.out);
        // A closed standard output (a pipe whose reader left) changes nothing the command did.
        let _ = io::stdout().lock().write_all(text.as_bytes());
        ExitCode::SUCCESS
    }

    /// Prints a refusal on standard error and answers `code`.
    pub(crate) fn refused(self, refused: &Refused, code: u8) -> ExitCode {
        let text = refused.render(self.err);
        let _ = io::stderr().lock().write_all(text.as_bytes());
        ExitCode::from(code)
    }
}

/// The next step after a failure nobody at the terminal caused (decision 159, the maintainer's
/// pick "Point to the guide" at the pause): no address in the binary.
pub(crate) const KEEP_THE_LINES: &str =
    "Keep the lines above: the deployment guide says where to report them.";

/// The same, after "Run the command again; " or "Start Nage again; ".
pub(crate) const IF_IT_FAILS_AGAIN: &str =
    "if it fails again, keep the lines above: the deployment guide says where to report them.";

/// A path as the terminal and the log show it (decision 157, ruling P10 of the pause): without
/// quotation marks, its control characters escaped as Rust escapes them in a string (a line break
/// as `\n`), so that a data folder whose name holds a line break keeps each part on one line.
pub(crate) fn shown(path: &std::path::Path) -> String {
    let mut text = String::new();
    for c in path.to_string_lossy().chars() {
        if c.is_control() {
            text.extend(c.escape_debug());
        } else {
            text.push(c);
        }
    }
    text
}

/// The system's own words for `error`, as they stand in the one pair of brackets at the end of
/// `Why:` (decision 159, ruling P9 of the pause): the innermost message alone, never a bracket
/// inside a bracket and never sqlx's wrapping words. An operating system error reads
/// `Address in use, os error 98`; a database error `unable to open database file, code 14`.
pub(crate) fn system_words(error: &(dyn std::error::Error + 'static)) -> String {
    let mut innermost = error;
    while let Some(source) = innermost.source() {
        innermost = source;
    }
    let words = innermost.to_string();
    // SQLite's words as sqlx writes them: `(code: 14) unable to open database file`.
    if let Some(rest) = words.strip_prefix("(code: ")
        && let Some((code, message)) = rest.split_once(") ")
    {
        return format!("{message}, code {code}");
    }
    // An operating system's words as Rust writes them: `Address in use (os error 98)`.
    if let Some(rest) = words.strip_suffix(')')
        && let Some((message, code)) = rest.rsplit_once(" (os error ")
    {
        return format!("{message}, os error {code}");
    }
    words
}

/// `text` with its first letter made a capital, the way a reason that is a whole sentence begins
/// at the terminal; text that begins with anything else is returned as it is.
pub(crate) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => text.to_owned(),
    }
}
