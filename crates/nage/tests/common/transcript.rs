//! The transcript of a process, as the text snapshots of the binary's tests pin it (decisions 60,
//! 157 and 162): the command line as typed after `$ `, what standard output and standard error
//! show under `[stdout]` and `[stderr]`, and the exit code after `[exit]`.
//!
//! What varies from run to run is shown the same every time: a test's working directory as
//! `/srv/nage` (so its data folder reads `/srv/nage/data`), the version as `[version]`, a log
//! line's stamp as `[TIME]`, and, when the test names them, a port it bound as `[PORT]` and a
//! backup's stamp as `[STAMP]`. A word of the command line that a shell would not take as it is
//! stands quoted as Rust quotes a string, so a line break it holds keeps the line whole; a value
//! that is not UTF-8 text is named by its bytes (`<the byte FF>`).

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::process::Command;

use super::{Finished, VERSION};

/// How a test's working directory is shown.
pub const WORKING_DIR: &str = "/srv/nage";
/// How the version is shown.
pub const VERSION_SHOWN: &str = "[version]";
/// How a log line's stamp is shown.
pub const TIME_SHOWN: &str = "[TIME]";
/// How a port the test bound is shown.
pub const PORT_SHOWN: &str = "[PORT]";
/// How a backup's stamp is shown.
pub const STAMP_SHOWN: &str = "[STAMP]";

/// The command line of a process and what its transcript shows in place of what varies.
#[derive(Clone, Debug)]
pub struct Transcript {
    line: String,
    redactions: Vec<(String, String)>,
}

/// The transcript of `command`: its variables and its arguments after the binary's own path; a
/// command started through another program (`setsid --wait`) shows that program's name, without
/// its options, before `nage`.
pub fn transcript(command: &Command) -> Transcript {
    let mut words = Vec::new();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            words.push(format!("{}={}", key.to_string_lossy(), shell_word(value)));
        }
    }
    let binary = OsStr::new(env!("CARGO_BIN_EXE_nage"));
    let args: Vec<&OsStr> = command.get_args().collect();
    let after = if command.get_program() == binary {
        0
    } else {
        let program = Path::new(command.get_program())
            .file_name()
            .unwrap_or_else(|| command.get_program());
        words.push(shell_word(program));
        args.iter()
            .position(|arg| *arg == binary)
            .map_or(0, |at| at + 1)
    };
    words.push(String::from("nage"));
    words.extend(args[after..].iter().map(|arg| shell_word(arg)));
    Transcript {
        line: words.join(" "),
        redactions: Vec::new(),
    }
}

/// A word of the command line as a shell user would type it: as it is when it holds only
/// letters, digits and `-_./:,=@+%[]`, quoted as Rust quotes a string otherwise; a value that
/// is not UTF-8 text is named by its bytes.
pub fn shell_word(word: &OsStr) -> String {
    match word.to_str() {
        Some(text)
            if !text.is_empty()
                && text.chars().all(|c| {
                    c.is_alphanumeric()
                        || matches!(
                            c,
                            '-' | '_' | '.' | '/' | ':' | ',' | '=' | '@' | '+' | '%' | '[' | ']'
                        )
                }) =>
        {
            text.to_owned()
        }
        Some(text) => format!("{text:?}"),
        None => {
            let bytes: Vec<String> = word
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect();
            if bytes.len() == 1 {
                format!("<the byte {}>", bytes.join(" "))
            } else {
                format!("<the bytes {}>", bytes.join(" "))
            }
        }
    }
}

/// Whether `text` begins with a log stamp `YYYY-MM-DDTHH:MM:SSZ` (decision 160).
pub fn starts_with_stamp(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:ddZ";
    text.len() >= shape.len()
        && text
            .chars()
            .zip(shape.chars())
            .all(|(found, want)| match want {
                'd' => found.is_ascii_digit(),
                other => found == other,
            })
}

/// The stamps `YYYY-MM-DDTHH-MM-SSZ` of the backups `text` names (each right after `backups/`),
/// those of `old` left out, each once: the stamps of the backups a process wrote, which its
/// transcript shows as `[STAMP]`, while the fixed stamps of the copies a test planted stay.
pub fn new_stamps(text: &str, old: &[&str]) -> Vec<String> {
    let shape = "dddd-dd-ddTdd-dd-ddZ";
    let mut stamps: Vec<String> = Vec::new();
    for (at, _) in text.match_indices("backups/") {
        let Some(candidate) = text.get(at + "backups/".len()..at + "backups/".len() + shape.len())
        else {
            continue;
        };
        let fits = candidate
            .chars()
            .zip(shape.chars())
            .all(|(found, want)| match want {
                'd' => found.is_ascii_digit(),
                other => found == other,
            });
        if fits && !old.contains(&candidate) && !stamps.iter().any(|stamp| stamp == candidate) {
            stamps.push(candidate.to_owned());
        }
    }
    stamps
}

impl Transcript {
    /// Shows each stamp of `stamps` as `[STAMP]`.
    pub fn stamps(self, stamps: &[String]) -> Self {
        stamps
            .iter()
            .fold(self, |transcript, stamp| transcript.stamp(stamp))
    }

    /// Shows the working directory `dir` as [`WORKING_DIR`], under the name it was given and
    /// under the name the system resolves it to.
    pub fn dir(self, dir: &Path) -> Self {
        self.path(dir, WORKING_DIR)
    }

    /// Shows `path` as `shown`, under the name it was given and the name the system resolves it
    /// to.
    pub fn path(mut self, path: &Path, shown: &str) -> Self {
        self.redactions
            .push((path.display().to_string(), shown.to_owned()));
        if let Ok(resolved) = std::fs::canonicalize(path) {
            self.redactions
                .push((resolved.display().to_string(), shown.to_owned()));
        }
        self
    }

    /// Shows the port `port` as `:[PORT]` wherever it follows a colon.
    pub fn port(self, port: u16) -> Self {
        self.redact(format!(":{port}"), format!(":{PORT_SHOWN}"))
    }

    /// Shows a backup's stamp, `YYYY-MM-DDTHH-MM-SSZ`, as `[STAMP]`.
    pub fn stamp(self, stamp: &str) -> Self {
        self.redact(stamp, STAMP_SHOWN)
    }

    /// Shows the command's standard input as a shell user would give it: ` < source` after the
    /// command line (the test feeds the bytes through a pipe).
    pub fn stdin(mut self, source: &str) -> Self {
        self.line.push_str(" < ");
        self.line.push_str(source);
        self
    }

    /// Shows `from` as `to` wherever it stands.
    pub fn redact(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.redactions.push((from.into(), to.into()));
        self
    }

    /// The command line as the transcript shows it, after `$ `.
    pub fn line(&self) -> String {
        self.shown(&self.line)
    }

    /// The transcript of `out`, the process this transcript's command ran.
    pub fn of(&self, out: &Finished) -> String {
        let exit = out
            .code()
            .map_or_else(|| out.status.to_string(), |code| code.to_string());
        format!(
            "$ {}\n[stdout]\n{}[stderr]\n{}[exit] {exit}\n",
            self.line(),
            self.stream(&out.stdout),
            self.stream(&out.stderr)
        )
    }

    /// The log of a server up to its listening line (decision 160): the command line, then the
    /// lines `lines` under `[stderr, up to the listening line]`, with their stamps and redactions.
    pub fn log(&self, lines: &[String]) -> String {
        format!(
            "$ {}\n[stderr, up to the listening line]\n{}",
            self.line(),
            self.stream(&lines.join("\n"))
        )
    }

    /// A stream's text with its stamps and redactions, ending with a line break when it holds
    /// anything.
    fn stream(&self, text: &str) -> String {
        let mut shown = String::new();
        for line in text.lines() {
            if starts_with_stamp(line) {
                shown.push_str(TIME_SHOWN);
                shown.push_str(&self.shown(&line[20..]));
            } else {
                shown.push_str(&self.shown(line));
            }
            shown.push('\n');
        }
        shown
    }

    /// `text` with every redaction applied, the longest first, then the version.
    fn shown(&self, text: &str) -> String {
        let mut redactions: Vec<&(String, String)> = self
            .redactions
            .iter()
            .filter(|(from, _)| !from.is_empty())
            .collect();
        redactions.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        let mut shown = text.to_owned();
        for (from, to) in redactions {
            shown = shown.replace(from.as_str(), to);
        }
        shown.replace(VERSION, VERSION_SHOWN)
    }
}
