//! `nage check` (SPEC 5.2, W15, decision 97): the ledger's invariants, read from the data
//! directory's database without creating or migrating it, printed by id on stdout; exit 0 when
//! none is violated or the database holds no book row, 1 on a violation or a database that
//! cannot be checked (missing, behind this version, or written by a newer Nage).

use std::path::PathBuf;
use std::process::ExitCode;

/// Runs the check against `<data_dir>/nage.sqlite` and prints the result (decision 97): one
/// line per violation, then one summary line, on stdout; a failure to check is one `nage: ...`
/// line on stderr.
pub(crate) async fn run(data_dir: PathBuf) -> ExitCode {
    match check(&data_dir).await {
        Ok(report) => {
            for violation in report.violations() {
                println!("{violation}");
            }
            println!("{}", report.summary());
            if report.is_sound() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("nage: {error}");
            ExitCode::FAILURE
        }
    }
}

use std::fmt;
use std::path::Path;

use nage_core::check::{self, Report};
use nage_core::storage::{self, Storage, StorageOptions};

use crate::config;

/// Opens the existing database, refuses one that is not at this binary's schema version, runs
/// the check in one read transaction and closes the storage.
async fn check(data_dir: &Path) -> Result<Report, CheckError> {
    let given = data_dir.join(config::DATABASE_FILE);
    let path = std::path::absolute(&given).unwrap_or(given);
    if !path.is_file() {
        return Err(CheckError::Missing(path));
    }
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .map_err(|source| CheckError::Open {
            path: path.clone(),
            source,
        })?;
    let result = check_open(&storage, &path).await;
    storage.close().await;
    result
}

/// The schema version must be this binary's; then the check runs in one read transaction.
async fn check_open(storage: &Storage, path: &Path) -> Result<Report, CheckError> {
    let version = storage
        .schema_version()
        .await
        .map_err(|source| CheckError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    let expects = Storage::latest_schema_version();
    match version {
        Some(version) if version > expects => return Err(CheckError::Newer { version }),
        Some(version) if version == expects => {}
        other => {
            return Err(CheckError::Behind {
                version: other.unwrap_or(0),
                expects,
            });
        }
    }
    let mut tx = storage
        .begin_read()
        .await
        .map_err(|source| CheckError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    let report = check::run(&mut tx)
        .await
        .map_err(|error| CheckError::Query(storage::Error::Sqlx(error)))?;
    tx.rollback()
        .await
        .map_err(|error| CheckError::Query(storage::Error::Sqlx(error)))?;
    Ok(report)
}

/// Why the database could not be checked: exit 1 with one line on stderr (decision 97).
#[derive(Debug)]
enum CheckError {
    /// No file at the path; check never creates one.
    Missing(PathBuf),
    /// The storage could not be opened or its version read.
    Open {
        path: PathBuf,
        source: storage::Error,
    },
    /// The database is behind this binary; check never migrates.
    Behind { version: i64, expects: i64 },
    /// The database carries a migration this binary does not know (decision 54).
    Newer { version: i64 },
    /// A query failed (wrapped in the storage's error, so the binary names no sqlx type).
    Query(storage::Error),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(
                f,
                "no database at {}: start nage serve once to create it",
                path.display()
            ),
            Self::Open { path, source } => {
                write!(f, "cannot open database {}: {source}", path.display())
            }
            Self::Behind { version, expects } => write!(
                f,
                "the database is at schema version {version} and this nage expects {expects}: start nage serve to migrate it"
            ),
            Self::Newer { version } => write!(
                f,
                "the database was written by a newer Nage: it carries migration {version}, which this version does not know"
            ),
            Self::Query(source) => write!(f, "cannot check the database: {source}"),
        }
    }
}
