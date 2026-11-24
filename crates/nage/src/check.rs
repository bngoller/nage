//! `nage check` (SPEC 5.2, W15, decisions 97 and 157): the book's rules (the ledger's invariants),
//! read from the data folder's database without creating or updating it; exit 0 when none is
//! broken or the
//! database holds no book row, 1 on a violation or a database that cannot be checked (missing,
//! empty, behind this version, or written by a newer Nage). A book that passes, and one with no
//! book yet, is a `Success:` on standard output; a book that breaks rules is an `Error:` on
//! standard error, with the `INV-n` and `FK` lines word for word as nage-core writes them, each
//! indented under a bare `Why:`.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::shape::Screen;

/// Runs the check against `<data_dir>/nage.sqlite` and prints the result (decisions 97 and 157).
pub(crate) async fn run(data_dir: PathBuf, screen: Screen) -> ExitCode {
    match check(&data_dir).await {
        Ok(report) => match said(&report, &database::path(&data_dir)) {
            Said::Finished(finished) => screen.finished(&finished),
            Said::Refused(refused) => screen.refused(&refused, 1),
        },
        Err(error) => screen.refused(&error.refused(), 1),
    }
}

use std::path::Path;

use nage_core::check::{self, Outcome, Report};
use nage_core::storage::{self, Storage};

use crate::database::{self, OpenError};
use crate::shape::{Doing, Finished, IF_IT_FAILS_AGAIN, Refused, shown, system_words};

/// What a run of the check says at the terminal (decision 157).
pub(crate) enum Said {
    /// The book passed, or there is no book yet: standard output, exit 0.
    Finished(Finished),
    /// The book breaks rules: standard error, exit 1.
    Refused(Refused),
}

/// What `report`, a run of the check on the database `database`, says at the terminal: the
/// book passed every check, with how much was checked; nothing to check while no book is set up;
/// or the broken rules, their lines word for word. `serve`'s start and `nage migrate` say the same
/// facts in their own words.
pub(crate) fn said(report: &Report, database: &Path) -> Said {
    match &report.outcome {
        Outcome::NotInitialized => {
            Said::Finished(Finished::new("Nothing to check: no book is set up yet."))
        }
        Outcome::Checked { violations } if violations.is_empty() => Said::Finished(
            Finished::new("The book passed every check.")
                .fact(
                    "Checked",
                    format!("{} rules and the links between rows", report.invariants),
                )
                .fact("Database", shown(database)),
        ),
        Outcome::Checked { violations } => Said::Refused(Refused::listing(
            problems_found(violations.len()),
            violations.iter().map(ToString::to_string).collect(),
            KEEP_A_COPY,
        )),
    }
}

/// What now after the broken rules of `nage check` (the maintainer's pick "Point to the guide").
pub(crate) const KEEP_A_COPY: &str = "Nothing was changed. Keep a copy of the data folder as it is, and keep the lines above: the deployment guide says where to report them.";

/// The sentence after `Error:` for a book that breaks rules in `count` places.
pub(crate) fn problems_found(count: usize) -> String {
    if count == 1 {
        String::from("The book did not pass every check: 1 problem was found.")
    } else {
        format!("The book did not pass every check: {count} problems were found.")
    }
}

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

/// Why the database could not be checked: exit 1 with three lines on stderr (decisions 97 and
/// 157).
#[derive(Debug)]
enum CheckError {
    /// The database is missing, empty, cannot be opened, or is behind or ahead of this binary.
    Database(OpenError),
    /// A query failed (wrapped in the storage's error, so the binary names no sqlx type).
    Query(storage::Error),
}

impl CheckError {
    /// The failure as the terminal prints it.
    fn refused(&self) -> Refused {
        match self {
            Self::Database(error) => error.refused(Doing::Check),
            Self::Query(source) => Refused::new(
                Doing::Check.not_done(),
                format!("Reading the database failed ({}).", system_words(source)),
                format!("Run nage check again; {IF_IT_FAILS_AGAIN}"),
            ),
        }
    }
}

impl From<OpenError> for CheckError {
    fn from(error: OpenError) -> Self {
        Self::Database(error)
    }
}
