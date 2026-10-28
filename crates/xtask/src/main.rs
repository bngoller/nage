//! Development tooling: seeding scenario databases, generating fixtures (SPEC 5.2).
//! Never part of the shipped binary; run as `cargo xtask <command>` (the alias in
//! `.cargo/config.toml`).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use nage_core::storage::{self, Storage, StorageOptions};

#[derive(Parser)]
#[command(name = "xtask", about = "Nage development tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a freshly migrated, empty database file and close it cleanly
    Seed(SeedArgs),
}

#[derive(Args)]
struct SeedArgs {
    /// Where to write the database; the parent directory is created, an existing file is refused
    #[arg(long, value_name = "PATH")]
    out: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Seed(args) => seed(&args.out).await,
    }
}

/// `xtask seed --out <path>` (decision 58): creates the parent directory, refuses an existing
/// file, opens the storage through nage-core, migrates, closes it cleanly (no `-wal` or
/// `-shm` left) and prints the path; exit 1 with a message on failure.
async fn seed(out: &Path) -> ExitCode {
    match write_seed(out).await {
        Ok(()) => {
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("xtask seed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The work of [`seed`]: refuse anything already at `out` (a file, a directory or a dangling
/// symlink, which SQLite would otherwise follow and create), create the parent directory,
/// then open, migrate and close through [`Storage`] so the result is exactly what `serve`
/// would create on first start (decision 58). A failed migration still closes the storage,
/// so no side files remain beside the partial file the message names.
async fn write_seed(out: &Path) -> Result<(), SeedError> {
    match out.symlink_metadata() {
        Ok(_) => return Err(SeedError::Exists(out.to_path_buf())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(SeedError::Inspect {
                path: out.to_path_buf(),
                source,
            });
        }
    }
    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|source| SeedError::ParentDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let storage = Storage::open(out, &StorageOptions::default())
        .await
        .map_err(|source| SeedError::Open {
            path: out.to_path_buf(),
            source,
        })?;
    let migrated = storage.migrate().await;
    storage.close().await;
    migrated.map_err(|source| SeedError::Migrate {
        path: out.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// A failure of `xtask seed`: exit code 1 with the message on stderr.
#[derive(Debug)]
enum SeedError {
    /// Something is already at the output path; `seed` never overwrites.
    Exists(PathBuf),
    /// The output path could not be inspected.
    Inspect { path: PathBuf, source: io::Error },
    /// The parent directory could not be created.
    ParentDir { path: PathBuf, source: io::Error },
    /// The storage could not be opened at the path.
    Open {
        path: PathBuf,
        source: storage::Error,
    },
    /// The migrations could not be applied.
    Migrate {
        path: PathBuf,
        source: storage::Error,
    },
}

impl fmt::Display for SeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exists(path) => write!(
                f,
                "{} already exists; seed never overwrites, delete it first",
                path.display()
            ),
            Self::Inspect { path, source } => {
                write!(f, "cannot inspect {}: {source}", path.display())
            }
            Self::ParentDir { path, source } => {
                write!(f, "cannot create directory {}: {source}", path.display())
            }
            Self::Open { path, source } => {
                write!(f, "cannot open database {}: {source}", path.display())
            }
            Self::Migrate { path, source } => {
                write!(f, "cannot migrate database {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for SeedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Exists(_) => None,
            Self::Inspect { source, .. } | Self::ParentDir { source, .. } => Some(source),
            Self::Open { source, .. } | Self::Migrate { source, .. } => Some(source),
        }
    }
}
