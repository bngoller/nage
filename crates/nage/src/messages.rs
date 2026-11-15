//! What `nage init`, `nage user add` and `nage user reset-password` say at the terminal (SPEC
//! decisions 102, 113 and 114): the two questions for a password, the refusals a command words
//! itself, the result each prints when it is done, and [`Stop`], everything a command can end
//! with instead of a result. Each refusal says what was refused, why and what to do instead,
//! and begins with the words the core's refusals of the same command begin with ("The book was not
//! set up", "The user was not created", "The password was not changed"). The refusals the core
//! words (a name, a password, a book that is already set up, a username that is taken, an admin who
//! is not one) are printed as the core words them. `nage check` keeps its own short lines for a
//! database it cannot check (decision 97); the three commands say the same three things in three
//! parts.

use std::fmt;
use std::path::PathBuf;

use nage_core::book::BookError;
use nage_core::password::{HashError, PasswordError};
use nage_core::setup::SetupError;
use nage_core::storage;
use nage_core::timestamp::TimestampError;
use nage_core::users::{PasswordChangeError, UserError};

use crate::database::OpenError;

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
    /// What was not done, as the core's refusals of the same command say it.
    pub(crate) fn not_done(self) -> &'static str {
        match self {
            Self::SetUp => "The book was not set up",
            Self::AddUser => "The user was not created",
            Self::ResetPassword => "The password was not changed",
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

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrencyMissing => f.write_str(
                "The book was not set up because no currency was given, and a book's currency never changes once the book is set up. Add --currency and the code of the book's currency, for example --currency CAD, then run the command again.",
            ),
            Self::UnknownCurrency { typed } => write!(
                f,
                "The book was not set up because \"{typed}\" is not a currency code Nage knows; a code is three capital letters, such as CAD, EUR or USD. Give the code of the book's currency with --currency, then run the command again."
            ),
            Self::NoTerminal { task } => write!(
                f,
                "{} because the password could not be asked for: this command is not running in a terminal, and --password-stdin was not given. Run the command in a terminal, or add --password-stdin and send the password on standard input.",
                task.not_done()
            ),
            Self::PasswordsDiffer { task } => write!(
                f,
                "{} because the two passwords typed are not the same, so one of them was mistyped. Run the command again and type the same password both times.",
                task.not_done()
            ),
            Self::NoPasswordOnStdin { task } => write!(
                f,
                "{} because --password-stdin was given and no password arrived on standard input. Send the password on standard input, for example through a pipe, or leave --password-stdin out and type the password at the terminal.",
                task.not_done()
            ),
            Self::PasswordNotText { task } => write!(
                f,
                "{} because the password on standard input is not valid UTF-8 text, so Nage cannot tell which characters it holds. Send the password as UTF-8 text, then run the command again.",
                task.not_done()
            ),
            Self::NoDatabase { task, path } => write!(
                f,
                "{} because there is no Nage database at {}. Run the command in the folder that holds the data directory, or name the data directory with --data-dir; if Nage has never been started there, set it up first with nage init.",
                task.not_done(),
                path.display()
            ),
            Self::DatabaseBehind {
                task,
                path,
                version,
                expects,
            } => write!(
                f,
                "{} because the database at {} was left by an older version of Nage and has not been brought up to date: it is at schema version {version}, and this version of Nage expects {expects}. Start nage serve once, which brings the database up to date, then run the command again.",
                task.not_done(),
                path.display()
            ),
            Self::DatabaseNewer {
                task,
                path,
                version,
            } => write!(
                f,
                "{} because the database at {} was written by a newer version of Nage than this one: it carries migration {version}, which this version does not know. Run the command with that newer version of Nage.",
                task.not_done(),
                path.display()
            ),
            Self::NotSetUp { task } => write!(
                f,
                "{} because Nage has not been set up in this data directory yet, so it has no users. Set Nage up first, with nage init or on the set-up page, or name the right data directory with --data-dir.",
                task.not_done()
            ),
            Self::AddedByNobody { typed } => write!(
                f,
                "The user was not created because nobody has the username \"{typed}\", which --by named as the admin adding the user. Check the spelling, or name another admin with --by, then run the command again."
            ),
            Self::ResetByNobody { typed } => write!(
                f,
                "The password was not changed because nobody has the username \"{typed}\", which --by named as the admin resetting the password. Check the spelling, or name another admin with --by, then run the command again."
            ),
            Self::UnknownUser { typed } => write!(
                f,
                "The password was not changed because nobody has the username \"{typed}\", which --username named as the user whose password to reset. Check the spelling, then run the command again."
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

impl fmt::Display for Done {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetUp {
                book_name,
                currency,
                starter_chart,
                database,
                username,
                display_name,
            } => write!(
                f,
                "The book \"{book_name}\" was set up in {currency}, {} the starter accounts, in {}.\n{username} ({display_name}) is its admin and can now sign in.",
                if *starter_chart { "with" } else { "without" },
                database.display()
            ),
            Self::UserAdded {
                username,
                display_name,
                by,
            } => write!(
                f,
                "{username} ({display_name}) was added by {by} and can now sign in. {username} is not an admin."
            ),
            Self::PasswordReset { username, by } if username == by => write!(
                f,
                "The password of {username} was reset by {by}, and {username} was signed out on every device. Sign in with the new password."
            ),
            Self::PasswordReset { username, by } => write!(
                f,
                "The password of {username} was reset by {by}, and {username} was signed out on every device. Tell {username} the new password; {username} can change it after signing in."
            ),
        }
    }
}

/// Everything a command can end with instead of its result: one `nage: ` line on standard
/// error (decision 113).
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
    /// The database of the data directory cannot be opened, created or migrated, or the data
    /// directory cannot be made: a failure, printed as `database::OpenError` words it.
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
            failure @ (OpenError::Open { .. }
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
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refusal(refusal) => write!(f, "{refusal}"),
            Self::Password(error) => write!(f, "{error}"),
            Self::Book(error) => write!(f, "{error}"),
            Self::Setup(error) => write!(f, "{error}"),
            Self::User(error) => write!(f, "{error}"),
            Self::PasswordChange(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "{error}"),
            Self::Failure(what) => f.write_str(what),
        }
    }
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
