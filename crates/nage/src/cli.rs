//! The command line (SPEC 5.2, decision 42). Flags only: the variables they stand for are
//! resolved by hand in `config`, never by clap's `env` feature, so an empty variable counts
//! as unset and every error names its source. The help texts name each variable and
//! default by hand.

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
    /// Run the web server until it is stopped
    Serve(ServeArgs),
    /// Probe the local server's /health; exit 0 only on 200
    Health(HealthArgs),
}

#[derive(Args)]
pub(crate) struct ServeArgs {
    /// Data directory, created with its backups/ folder if missing [env: NAGE_DATA_DIR] [default: ./data]
    #[arg(long, value_name = "DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// Address to listen on, an IP address and port; IPv6 in brackets [env: NAGE_BIND] [default: 127.0.0.1:8787]
    #[arg(long, value_name = "ADDR")]
    pub(crate) bind: Option<String>,

    /// Time zone, an IANA name such as Europe/Berlin [env: NAGE_TZ, then TZ] [default: UTC]
    #[arg(long, value_name = "NAME")]
    pub(crate) tz: Option<String>,

    /// Log filter, a tracing directive such as warn,nage_web=debug [env: NAGE_LOG] [default: info]
    #[arg(long, value_name = "FILTER")]
    pub(crate) log: Option<String>,
}

#[derive(Args)]
pub(crate) struct HealthArgs {
    /// The server's bind address; 0.0.0.0 is probed as 127.0.0.1 and :: as ::1 [env: NAGE_BIND] [default: 127.0.0.1:8787]
    #[arg(long, value_name = "ADDR")]
    pub(crate) bind: Option<String>,
}
