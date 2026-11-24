//! What `nage init`, `nage user add` and `nage user reset-password` say at the terminal (SPEC
//! decisions 102, 113, 114 and 157): the two questions for a password, the refusals a command
//! words itself, the result each prints when it is done, and [`Stop`], everything a command can
//! end with instead of a result. Each refusal is three labelled lines: the first names what the
//! command was doing ("The book was not set up", "The user was not created", "The password was
//! not changed"), then why and what to do. The refusals the core words (a name, a password, a
//! book that is already set up, a username that is taken, an admin who is not one) are printed in
//! the core's words for the terminal (`at_the_terminal`, the twins of the pages' sentences: the
//! maintainer's pick "The terminal's own words" at the pause), never in the page's.

use std::path::PathBuf;

use nage_core::book::BookError;
use nage_core::password::{HashError, PasswordError};
use nage_core::setup::SetupError;
use nage_core::storage::{self, Storage};
use nage_core::timestamp::TimestampError;
use nage_core::users::{PasswordChangeError, UserError};
use nage_core::words::AtTheTerminal;

use crate::database::{self, OpenError};
use crate::shape::{Doing, Finished, IF_IT_FAILS_AGAIN, Refused, capitalized, shown, system_words};

#[cfg(test)]
mod tests;

/// What a command was run to do. It decides the first words of a refusal and the question the
/// password is asked for with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Task {
    /// `nage init`.
    SetUp,
    /// `nage user add`.
    AddUser,
    /// `nage user reset-password`.
    ResetPassword,
}

impl Task {
    /// The command, for the terminal's first line (decision 157).
    pub(crate) fn doing(self) -> Doing {
        match self {
            Self::SetUp => Doing::Init,
            Self::AddUser => Doing::AddUser,
            Self::ResetPassword => Doing::ResetPassword,
        }
    }
}

/// The first question for the password of `username`, written on the terminal before the
/// answer is typed unseen: `Password for sam: `, and `New password for alex: ` for a reset.
pub(crate) fn question(task: Task, username: &str) -> String {
    match task {
        Task::SetUp | Task::AddUser => format!("Password for {username}: "),
        Task::ResetPassword => format!("New password for {username}: "),
    }
}

/// The second question, the same for every command.
pub(crate) const QUESTION_AGAIN: &str = "The same password again: ";

/// A refusal a command words itself (decision 113). Two of them are usage errors, which exit
/// with code 2 (decision 45): the command line or the place it was run in must change. The
/// others exit with code 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// `nage init` without `--currency`: a usage error. The currency has no default at the
    /// terminal, where nothing would show the default before it is used.
    CurrencyMissing,
    /// `--currency` names a code that is not in the currency table.
    UnknownCurrency { typed: String },
    /// No `--password-stdin`, and no terminal to ask for the password on: a usage error.
    NoTerminal { task: Task },
    /// The two passwords typed at the terminal are not the same. Nothing is asked again.
    PasswordsDiffer { task: Task },
    /// `--password-stdin`, and standard input held nothing, or only one line ending.
    NoPasswordOnStdin { task: Task },
    /// `--password-stdin`, and standard input is not valid UTF-8.
    PasswordNotText { task: Task },
    /// `nage user add` or `nage user reset-password` where the data directory holds no
    /// database (`nage init` creates one instead).
    NoDatabase { task: Task, path: PathBuf },
    /// The database is behind this binary: only `nage serve` migrates one.
    DatabaseBehind {
        task: Task,
        path: PathBuf,
        version: i64,
        expects: i64,
    },
    /// The database carries a migration this binary does not know.
    DatabaseNewer {
        task: Task,
        path: PathBuf,
        version: i64,
    },
    /// `nage user add` or `nage user reset-password` on a database that holds no user yet.
    NotSetUp { task: Task },
    /// `nage user add --by` names nobody.
    AddedByNobody { typed: String },
    /// `nage user reset-password --by` names nobody.
    ResetByNobody { typed: String },
    /// `nage user reset-password --username` names nobody.
    UnknownUser { typed: String },
}

impl Refusal {
    /// The refusal as the terminal prints it, in three labelled lines (decision 157): the reason
    /// a sentence of its own, a typed value quoted as Rust quotes a string and a path shown by
    /// `shape::shown`, so that no line break either holds breaks the line, and the hints for a
    /// database behind this version naming `nage migrate` and the server's start (assumption A14
    /// of PLAN M2 slice 8).
    pub(crate) fn refused(&self) -> Refused {
        match self {
            Self::CurrencyMissing => Refused::new(
                Doing::Init.not_done(),
                "No currency was given, and a book's currency never changes once the book is set up.",
                "Add --currency and the code of the book's currency, for example --currency CAD, then run the command again.",
            ),
            Self::UnknownCurrency { typed } => Refused::new(
                Doing::Init.not_done(),
                format!(
                    "{typed:?} is not a currency code Nage knows; a code is three capital letters, such as CAD, EUR or USD."
                ),
                "Give the code of the book's currency with --currency, then run the command again.",
            ),
            Self::NoTerminal { task } => Refused::new(
                task.doing().not_done(),
                "The password could not be asked for: this command is not running in a terminal, and --password-stdin was not given.",
                "Run the command in a terminal, or add --password-stdin and send the password on standard input.",
            ),
            Self::PasswordsDiffer { task } => Refused::new(
                task.doing().not_done(),
                "The two passwords typed are not the same, so one of them was mistyped.",
                "Run the command again and type the same password both times.",
            ),
            Self::NoPasswordOnStdin { task } => Refused::new(
                task.doing().not_done(),
                "--password-stdin was given and no password arrived on standard input.",
                "Send the password on standard input, for example through a pipe, or leave --password-stdin out and type the password at the terminal.",
            ),
            Self::PasswordNotText { task } => Refused::new(
                task.doing().not_done(),
                "The password on standard input is not valid UTF-8 text, so Nage cannot tell which characters it holds.",
                "Send the password as UTF-8 text, then run the command again.",
            ),
            // The database's situations in the sentences every command shares (the maintainer's
            // answer "One sentence per situation"), each with the command's own first line.
            Self::NoDatabase { task, path } => Refused::new(
                task.doing().not_done(),
                database::no_database(path),
                database::NO_DATABASE_NOW,
            ),
            Self::DatabaseBehind {
                task,
                path,
                version,
                expects,
            } => Refused::new(
                task.doing().not_done(),
                database::behind(path, *version, *expects),
                "Update it with nage migrate, or start the server, which updates it itself, then run the command again.",
            ),
            Self::DatabaseNewer { task, version, .. } => Refused::new(
                task.doing().not_done(),
                database::newer(*version, Storage::latest_schema_version()),
                database::NEWER_NOW,
            ),
            Self::NotSetUp { task } => Refused::new(
                task.doing().not_done(),
                "Nage has not been set up in this data folder yet, so it has no users.",
                "Set Nage up first, with nage init or on the set-up page, or name the right data folder with --data-dir.",
            ),
            Self::AddedByNobody { typed } => Refused::new(
                Doing::AddUser.not_done(),
                format!(
                    "Nobody has the username {typed:?}, which --by named as the admin adding the user."
                ),
                "Check the spelling, or name another admin with --by, then run the command again.",
            ),
            Self::ResetByNobody { typed } => Refused::new(
                Doing::ResetPassword.not_done(),
                format!(
                    "Nobody has the username {typed:?}, which --by named as the admin resetting the password."
                ),
                "Check the spelling, or name another admin with --by, then run the command again.",
            ),
            Self::UnknownUser { typed } => Refused::new(
                Doing::ResetPassword.not_done(),
                format!(
                    "Nobody has the username {typed:?}, which --username named as the user whose password to reset."
                ),
                "Check the spelling, then run the command again.",
            ),
        }
    }
}

/// What a command prints on standard output when it is done (decision 113). The usernames and
/// names are the stored ones, as the core normalized them, so the line also shows who was
/// found for `--by` and `--username`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Done {
    /// `nage init`: two lines.
    SetUp {
        book_name: String,
        /// The currency's code.
        currency: &'static str,
        starter_chart: bool,
        /// The database file, as an absolute path.
        database: PathBuf,
        username: String,
        display_name: String,
    },
    /// `nage user add`.
    UserAdded {
        username: String,
        display_name: String,
        /// The admin who added the user.
        by: String,
    },
    /// `nage user reset-password`. The admin may be the user (decision 112): the line then
    /// ends differently.
    PasswordReset {
        username: String,
        /// The admin who reset the password.
        by: String,
    },
}

impl Done {
    /// The result as the terminal prints it (decision 157): a sentence, its facts and the next
    /// step, with the names as the core stored them.
    pub(crate) fn finished(&self) -> Finished {
        match self {
            Self::SetUp {
                book_name,
                currency,
                starter_chart,
                database,
                username,
                display_name,
            } => Finished::new(format!("The book \"{book_name}\" was set up."))
                .fact("Currency", *currency)
                .fact(
                    "Starter accounts",
                    if *starter_chart { "added" } else { "left out" },
                )
                .fact("Admin", format!("{username} ({display_name})"))
                .fact("Database", shown(database))
                .then(format!(
                    "Start the server if it is not running; {username} can then sign in."
                )),
            Self::UserAdded {
                username,
                display_name,
                by,
            } => Finished::new(format!("{username} ({display_name}) was added."))
                .fact("Added by", by.as_str())
                .fact("Admin", "no")
                .then(format!(
                    "Tell {username} the password; {username} can sign in now and change it afterwards."
                )),
            Self::PasswordReset { username, by } if username == by => {
                Finished::new(format!("The password of {username} was reset."))
                    .fact("Reset by", by.as_str())
                    .fact("Signed out", format!("{username}, on every device"))
                    .then("Sign in with the new password.")
            }
            Self::PasswordReset { username, by } => {
                Finished::new(format!("The password of {username} was reset."))
                    .fact("Reset by", by.as_str())
                    .fact("Signed out", format!("{username}, on every device"))
                    .then(format!(
                        "Tell {username} the new password; {username} can change it after signing in."
                    ))
            }
        }
    }
}

/// Everything a command can end with instead of its result: three labelled lines on standard
/// error (decisions 113 and 157).
#[derive(Debug)]
pub(crate) enum Stop {
    /// A refusal the command words itself.
    Refusal(Refusal),
    /// The new password breaks a rule (the core's words), or the hasher failed.
    Password(HashError),
    /// The book name breaks a name rule (the core's words), or the book row could not be read.
    Book(BookError),
    /// `initialize` refused or failed: a book that is already set up, in the core's words.
    Setup(SetupError),
    /// A username or display name breaks a name rule, a username is taken, the admin named is
    /// not an active admin, or `create_user` failed: the core's words.
    User(UserError),
    /// The admin named is not an active admin, or `reset_password` failed: the core's words.
    PasswordChange(PasswordChangeError),
    /// The database of the data folder cannot be opened, created or updated, or the data folder
    /// cannot be made: a failure, printed as `database::OpenError` words it.
    Database(OpenError),
    /// Anything else that failed and that nobody at the terminal caused: the terminal, standard
    /// input, the clock, a read or a commit. Its text is `what: cause`, or the cause alone.
    Failure(String),
}

impl Stop {
    /// What `task` ends with when the database of the data directory cannot be used (decision
    /// 113). Three of the reasons are refusals, in the command's own words and in three parts:
    /// no database at the path ([`Refusal::NoDatabase`]; `nage init` never gets here with it,
    /// since it creates the database instead), a database behind this binary
    /// ([`Refusal::DatabaseBehind`]) and one ahead of it ([`Refusal::DatabaseNewer`]), each
    /// naming the database by its path. Every other reason (the database cannot be opened,
    /// created or migrated, the data directory cannot be made) is a failure and stays as it
    /// is: [`Stop::Database`].
    pub(crate) fn database(task: Task, error: OpenError) -> Stop {
        match error {
            OpenError::Missing(path) => Self::Refusal(Refusal::NoDatabase { task, path }),
            OpenError::Behind {
                path,
                version,
                expects,
            } => Self::Refusal(Refusal::DatabaseBehind {
                task,
                path,
                version,
                expects,
            }),
            OpenError::Newer { path, version } => Self::Refusal(Refusal::DatabaseNewer {
                task,
                path,
                version,
            }),
            // Named one by one, so that a reason added to `OpenError` is sorted here on purpose.
            // An empty database (a set-up cut off before it finished, decision 155) is said as
            // `nage check` says it (ruling P6 of the pause); `init` finishes it and never gets
            // here with it.
            failure @ (OpenError::Empty(_)
            | OpenError::Open { .. }
            | OpenError::DataDir { .. }
            | OpenError::Create { .. }
            | OpenError::Migrate { .. }) => Self::Database(failure),
        }
    }

    /// The command's exit code (decision 45): 2 for the two usage errors
    /// ([`Refusal::CurrencyMissing`], [`Refusal::NoTerminal`]), 1 for every other refusal and
    /// every failure.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Refusal(Refusal::CurrencyMissing | Refusal::NoTerminal { .. }) => 2,
            Self::Refusal(_)
            | Self::Password(_)
            | Self::Book(_)
            | Self::Setup(_)
            | Self::User(_)
            | Self::PasswordChange(_)
            | Self::Database(_)
            | Self::Failure(_) => 1,
        }
    }

    /// What the command ends with, as the terminal prints it for `task` (decision 157): a refusal
    /// in its three parts, the command's own first line before a refusal the core words and the
    /// core's words for the terminal after it (`at_the_terminal`, never the page's sentence), and
    /// every failure in three parts too, its own words kept at the end of the reason (decision
    /// 159).
    pub(crate) fn refused(&self, task: Task) -> Refused {
        let doing = task.doing();
        let core =
            |words: Option<AtTheTerminal>, failure: &(dyn std::error::Error + 'static)| match words
            {
                Some(words) => Refused::at_the_terminal(doing, &words),
                None => failed(doing, failure),
            };
        match self {
            Self::Refusal(refusal) => refusal.refused(),
            Self::Password(error) => core(error.at_the_terminal(), error),
            Self::Book(error) => core(error.at_the_terminal(), error),
            Self::Setup(error) => core(error.at_the_terminal(), error),
            Self::User(error) => core(error.at_the_terminal(), error),
            Self::PasswordChange(error) => core(error.at_the_terminal(), error),
            Self::Database(error) => error.refused(doing),
            Self::Failure(what) => Refused::new(
                doing.not_done(),
                format!("{}.", capitalized(what)),
                format!("Run the command again; {IF_IT_FAILS_AGAIN}"),
            ),
        }
    }
}

/// A failure that nobody at the terminal caused, in three parts (decision 159): the system's own
/// words in one pair of brackets at the end of the reason.
pub(crate) fn failed(doing: Doing, failure: &(dyn std::error::Error + 'static)) -> Refused {
    Refused::new(
        doing.not_done(),
        format!(
            "Something failed that nobody at the terminal caused ({}).",
            system_words(failure)
        ),
        format!("Run the command again; {IF_IT_FAILS_AGAIN}"),
    )
}

impl From<Refusal> for Stop {
    fn from(refusal: Refusal) -> Self {
        Self::Refusal(refusal)
    }
}

impl From<PasswordError> for Stop {
    fn from(error: PasswordError) -> Self {
        Self::Password(HashError::Password(error))
    }
}

impl From<HashError> for Stop {
    fn from(error: HashError) -> Self {
        Self::Password(error)
    }
}

impl From<SetupError> for Stop {
    fn from(error: SetupError) -> Self {
        Self::Setup(error)
    }
}

impl From<UserError> for Stop {
    fn from(error: UserError) -> Self {
        Self::User(error)
    }
}

impl From<PasswordChangeError> for Stop {
    fn from(error: PasswordChangeError) -> Self {
        Self::PasswordChange(error)
    }
}

impl From<storage::Error> for Stop {
    fn from(error: storage::Error) -> Self {
        Self::Failure(error.to_string())
    }
}

impl From<BookError> for Stop {
    fn from(error: BookError) -> Self {
        Self::Book(error)
    }
}

impl From<TimestampError> for Stop {
    fn from(error: TimestampError) -> Self {
        Self::Failure(format!("the system clock cannot be used: {error}"))
    }
}
