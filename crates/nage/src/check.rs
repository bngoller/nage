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
use nage_core::storage::{self, Storage};

use crate::database::{self, OpenError};

/// Opens the existing database by the rules `user add` and `user reset-password` share
/// (`database::open_existing`: never created, never migrated, refused unless it is at this
/// binary's schema version), runs the check in one read transaction and closes the storage.
async fn check(data_dir: &Path) -> Result<Report, CheckError> {
    let storage = database::open_existing(data_dir).await?;
    let result = check_open(&storage, data_dir).await;
    storage.close().await;
    result
}

/// The check, in one read transaction.
async fn check_open(storage: &Storage, data_dir: &Path) -> Result<Report, CheckError> {
    let mut tx = storage
        .begin_read()
        .await
        .map_err(|source| OpenError::Open {
            path: database::path(data_dir),
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
    /// The database is missing, cannot be opened, or is behind or ahead of this binary.
    Database(OpenError),
    /// A query failed (wrapped in the storage's error, so the binary names no sqlx type).
    Query(storage::Error),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "{error}"),
            Self::Query(source) => write!(f, "cannot check the database: {source}"),
        }
    }
}

impl From<OpenError> for CheckError {
    fn from(error: OpenError) -> Self {
        Self::Database(error)
    }
}
