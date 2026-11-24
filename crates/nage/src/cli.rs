//! The command line (SPEC 5.2, decisions 42 and 113). Flags only: the variables they stand
//! for are resolved by hand in `config`, never by clap's `env` feature, so an empty variable
//! counts as unset and every error names its source. The help texts name each variable and
//! default by hand, in everyday words (the data folder, the rules, an update; the maintainer's
//! pick "Everyday words everywhere" at PLAN M2 slice 8's pause), and a snapshot pins each of them.
//! `init`, `user add` and `user reset-password` take their names as flags and never the
//! password: it is asked for at the terminal or read from standard input (`--password-stdin`), so
//! it reaches neither the shell's history nor the process list.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nage", version, about = "Household accounting")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Run the web server until it is stopped; it updates the database first, after a backup
    Serve(ServeArgs),
    /// Ask the server on this machine whether it is healthy; exit 0 only when it is
    Health(HealthArgs),
    /// Set Nage up at the terminal: the book, its currency and the first user, an admin
    // The usage line is written by hand so that it names `--currency`: clap cannot require
    // the flag, since leaving it out gets a message of its own (decision 113).
    #[command(
        override_usage = "nage init [OPTIONS] --book-name <NAME> --currency <CODE> --username <NAME> --display-name <NAME>"
    )]
    Init(InitArgs),
    /// Add a user or reset a user's password at the terminal
    User(UserArgs),
    /// Check that the book keeps every rule
    Check(CheckArgs),
    /// Update the database to this version of Nage, after a backup
    Migrate(MigrateArgs),
}

#[derive(Args)]
pub(crate) struct MigrateArgs {
    /// The data folder that holds nage.sqlite; refused while a server runs on it [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// List the changes that wait and change nothing; works beside a running server
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(Args)]
pub(crate) struct ServeArgs {
    /// The data folder, created with its backups folder when it is missing [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// The address to listen on, an IP address and port; IPv6 in brackets [env: NAGE_BIND] [default: 127.0.0.1:8787]
    #[arg(long, value_name = "ADDR")]
    pub(crate) bind: Option<String>,

    /// The time zone, a name such as Europe/Berlin [env: NAGE_TZ, then TZ] [default: UTC]
    #[arg(long, value_name = "NAME")]
    pub(crate) tz: Option<String>,

    /// What the log shows, a level such as info or a filter such as warn,nage_web=debug [env: NAGE_LOG] [default: info]
    #[arg(long, value_name = "FILTER")]
    pub(crate) log: Option<String>,

    /// Believe a reverse proxy's X-Forwarded-Proto and X-Forwarded-Host headers [env: NAGE_TRUST_PROXY=true|false] [default: off]
    #[arg(long)]
    pub(crate) trust_proxy: bool,
}

#[derive(Args)]
pub(crate) struct HealthArgs {
    /// The address the server listens on; 0.0.0.0 is asked as 127.0.0.1 and :: as ::1 [env: NAGE_BIND] [default: 127.0.0.1:8787]
    #[arg(long, value_name = "ADDR")]
    pub(crate) bind: Option<String>,
}

#[derive(Args)]
pub(crate) struct CheckArgs {
    /// The data folder that holds nage.sqlite; check only reads it [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct InitArgs {
    /// The data folder, created with its backups folder and the database when they are missing [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// The book's name, such as Household
    #[arg(long, value_name = "NAME")]
    pub(crate) book_name: String,

    /// The book's currency, a code such as CAD; it never changes afterwards, so there is no default
    #[arg(long, value_name = "CODE")]
    pub(crate) currency: Option<String>,

    /// The username of the first user, who is an admin
    #[arg(long, value_name = "NAME")]
    pub(crate) username: String,

    /// The first user's display name
    #[arg(long, value_name = "NAME")]
    pub(crate) display_name: String,

    /// Leave the starter accounts out
    #[arg(long = "no-starter-accounts")]
    pub(crate) no_starter_chart: bool,

    /// Read the password once from standard input instead of asking for it twice at the terminal
    #[arg(long)]
    pub(crate) password_stdin: bool,
}

#[derive(Args)]
pub(crate) struct UserArgs {
    #[command(subcommand)]
    pub(crate) command: UserCommand,
}

#[derive(Subcommand)]
pub(crate) enum UserCommand {
    /// Add a user who is not an admin
    Add(UserAddArgs),
    /// Give a user a new password and sign them out everywhere
    ResetPassword(UserResetPasswordArgs),
}

#[derive(Args)]
pub(crate) struct UserAddArgs {
    /// The data folder that holds nage.sqlite, which must exist and be set up [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// The username of the admin who adds the user
    #[arg(long, value_name = "ADMIN")]
    pub(crate) by: String,

    /// The new user's username
    #[arg(long, value_name = "NAME")]
    pub(crate) username: String,

    /// The new user's display name
    #[arg(long, value_name = "NAME")]
    pub(crate) display_name: String,

    /// Read the password once from standard input instead of asking for it twice at the terminal
    #[arg(long)]
    pub(crate) password_stdin: bool,
}

#[derive(Args)]
pub(crate) struct UserResetPasswordArgs {
    /// The data folder that holds nage.sqlite, which must exist and be set up [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// The username of the admin who resets the password; it may be the user being reset
    #[arg(long, value_name = "ADMIN")]
    pub(crate) by: String,

    /// The username of the user whose password is reset
    #[arg(long, value_name = "NAME")]
    pub(crate) username: String,

    /// Read the new password once from standard input instead of asking for it twice at the terminal
    #[arg(long)]
    pub(crate) password_stdin: bool,
}
