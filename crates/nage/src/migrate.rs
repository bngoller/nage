//! `nage migrate [--data-dir DIR] [--dry-run]` (SPEC 5.2 and 5.5, decision 156): the update the
//! server makes when it starts, made at the terminal with the server stopped. It refuses a folder
//! without a database before anything else and creates nothing, not even the mark's file; it takes
//! the data folder's in-use mark as `migrate`, so it refuses while a server or another `nage
//! migrate` holds the folder; it writes the backup, applies every waiting change all or nothing
//! and runs the check, through the one operation `serve` calls
//! (`nage_core::update::open_and_update`), then prints what was done. Nothing waiting says so and
//! changes nothing. Exit 0 when the database is up to date and the book passes the check (or no
//! book is set up yet); 1 when it is refused, fails, or the check finds a problem, after an update
//! (the update stays) or with nothing waiting (ruling P7 of the pause). `--dry-run` only reads: it
//! takes no mark, works beside a running server, lists what waits (an empty database included,
//! read without a byte written) and exits 0 whenever it could look and the real run would not
//! refuse the database before its backup; a dry run that could not look or meets what the real
//! run refuses (no database, a newer one, a file that is not Nage's, one that cannot be opened, a
//! changed record of an applied change) has a first line of its own and the real run's reason and
//! next step, exit 1.
//!
//! The words of a refused or failed update live here for `serve` too: the two say the same three
//! parts, each with its own first line and its own way to try again.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nage_core::check::{self, Outcome, Report};
use nage_core::storage::{Storage, StorageOptions};
use nage_core::timestamp::Timestamp;
use nage_core::update::{
    self, Holder, Missing, Update, UpdateError, UpdateReport, Updated, Waiting,
};

use crate::check::problems_found;
use crate::database;
use crate::shape::{Doing, Finished, IF_IT_FAILS_AGAIN, Refused, Screen, shown, system_words};

#[cfg(test)]
mod tests;

/// The workspace version, which a backup's name carries and the refusals name.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs `nage migrate` on `data_dir` and prints what was done (decision 156): the update through
/// the one operation the server's start calls, under the mark `migrate`, then the check of the
/// book in one read transaction. Exit 0 when the database is up to date and the book passes (or no
/// book is set up yet); 1 when the update is refused or fails, or the check finds broken rules or
/// cannot run.
pub(crate) async fn run(data_dir: PathBuf, screen: Screen) -> ExitCode {
    let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);
    let now = match Timestamp::from_jiff(jiff::Timestamp::now()) {
        Ok(now) => now,
        Err(error) => return screen.refused(&clock_refused(&error), 1),
    };
    let outcome = update::open_and_update(Update {
        data_dir: &data_dir,
        now,
        version: VERSION,
        missing: Missing::Refuse,
        holder: Holder::Migrate,
        options: &StorageOptions::default(),
    })
    .await;
    let Updated {
        storage,
        mark,
        report,
    } = match outcome {
        Ok(done) => done,
        Err(error) => return screen.refused(&update_refused(Doing::Migrate, VERSION, &error), 1),
    };
    let checked = check_book(&storage).await;
    // The writer closes last and checkpoints the WAL away (decision 53); the mark goes after it.
    storage.close().await;
    drop(mark);
    let check = match checked {
        Ok(check) => check,
        Err(error) => return screen.refused(&check_failed(&report, &error), 1),
    };
    match (check_fact(&check), report.applied.is_empty()) {
        (Some(fact), true) => screen.finished(&up_to_date(&report, fact)),
        (Some(fact), false) => screen.finished(&updated(&report, fact)),
        (None, true) => screen.refused(&broken_up_to_date(&check), 1),
        (None, false) => screen.refused(&broken_after_update(&report, &check), 1),
    }
}

/// What `nage migrate` prints when the system clock gives a time Nage cannot use: exit 1, nothing
/// done.
fn clock_refused(error: &(dyn std::error::Error + 'static)) -> Refused {
    Refused::new(
        Doing::Migrate.not_done(),
        format!(
            "The system clock gave a time Nage cannot use ({}).",
            system_words(error)
        ),
        "Set the computer's clock right, then run nage migrate again.",
    )
}

/// Runs the check of the book on `storage` in one read transaction (decision 97).
async fn check_book(storage: &Storage) -> Result<Report, nage_core::storage::Error> {
    let mut tx = storage.begin_read().await?;
    let report = check::run(&mut tx)
        .await
        .map_err(nage_core::storage::Error::Sqlx)?;
    tx.rollback()
        .await
        .map_err(nage_core::storage::Error::Sqlx)?;
    Ok(report)
}

/// What `nage migrate` prints when the check after the update (or with nothing waiting) could not
/// run: exit 1, the update staying.
fn check_failed(report: &UpdateReport, error: &nage_core::storage::Error) -> Refused {
    let what = if report.applied.is_empty() {
        "The database is up to date, but the book could not be checked."
    } else {
        "The database was updated, but the book could not be checked."
    };
    Refused::new(
        what,
        format!("Reading the database failed ({}).", system_words(error)),
        format!("Run nage check; {IF_IT_FAILS_AGAIN}"),
    )
}

/// Runs `nage migrate --dry-run` on `data_dir` and prints what waits (decision 156): read
/// without a byte written and without the mark, so it works beside a running server; exit 0
/// whenever it could look and the real run would not refuse the database, 1 with a first line of
/// its own when it could not or the real run would (a changed record included).
pub(crate) async fn dry_run(data_dir: PathBuf, screen: Screen) -> ExitCode {
    let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);
    match update::waiting(&data_dir).await {
        Ok(found) => screen.finished(&waiting(&found)),
        Err(error) => screen.refused(&update_refused(Doing::DryRun, VERSION, &error), 1),
    }
}

/// The changes `versions` as the terminal names them: `0003`, `0002, 0003`.
pub(crate) fn changes(versions: &[i64]) -> String {
    versions
        .iter()
        .map(|version| format!("{version:04}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The check's result as a fact of `nage migrate`'s report, when it found nothing broken.
pub(crate) fn check_fact(report: &Report) -> Option<&'static str> {
    match &report.outcome {
        Outcome::NotInitialized => Some("nothing to check: no book is set up yet"),
        Outcome::Checked { violations } if violations.is_empty() => {
            Some("the book passed every check")
        }
        Outcome::Checked { .. } => None,
    }
}

/// What `nage migrate` prints when it applied changes and the book passed the check (decision
/// 156).
pub(crate) fn updated(report: &UpdateReport, check: &'static str) -> Finished {
    let mut finished = Finished::new("The database was updated.").fact(
        "Changes applied",
        format!("{} ({})", report.applied.len(), changes(&report.applied)),
    );
    if let Some(version) = report.schema_version {
        finished = finished.fact("Database version", version.to_string());
    }
    if let Some(backup) = &report.backup {
        finished = finished.fact("Backup", shown(backup));
    }
    for removed in &report.removed {
        finished = finished.fact("Older copy removed", shown(removed));
    }
    finished.fact("Check", check).then("Start the server.")
}

/// What `nage migrate` prints when nothing waited and the book passed the check (decision 156).
pub(crate) fn up_to_date(report: &UpdateReport, check: &'static str) -> Finished {
    let mut finished = Finished::new("The database is already up to date; nothing was changed.");
    if let Some(version) = report.schema_version {
        finished = finished.fact("Database version", version.to_string());
    }
    finished.fact("Check", check)
}

/// What `nage migrate` prints when the check after the update found broken rules: exit 1, the
/// update stays (assumption A11 of PLAN M2 slice 8).
pub(crate) fn broken_after_update(report: &UpdateReport, check: &Report) -> Refused {
    let backup = match &report.backup {
        Some(backup) => format!(
            " The backup from before the update is at {}.",
            shown(backup)
        ),
        None => String::new(),
    };
    Refused::listing(
        format!(
            "The database was updated, but {}",
            first_lower(&problems_found(check.violations().len()))
        ),
        check.violations().iter().map(ToString::to_string).collect(),
        format!(
            "Keep a copy of the data folder as it is, and keep the lines above: the deployment guide says where to report them.{backup}"
        ),
    )
}

/// What `nage migrate` prints when nothing waited and the book did not pass the check: exit 1,
/// the database unchanged (ruling P7 of the pause). Never "The database was updated".
pub(crate) fn broken_up_to_date(check: &Report) -> Refused {
    Refused::listing(
        format!(
            "The database is up to date, but {}",
            first_lower(&problems_found(check.violations().len()))
        ),
        check.violations().iter().map(ToString::to_string).collect(),
        crate::check::KEEP_A_COPY,
    )
}

/// What `nage migrate --dry-run` prints (decision 156): what waits, the version, where a backup
/// would go; that the database is empty and every change waits; or that nothing waits.
pub(crate) fn waiting(waiting: &Waiting) -> Finished {
    let version = waiting
        .schema_version
        .map_or_else(|| String::from("none"), |version| version.to_string());
    if waiting.waiting.is_empty() {
        return Finished::new("Nothing is waiting; nothing was changed.")
            .fact("Database version", version);
    }
    let count = waiting.waiting.len();
    let sentence = if count == 1 {
        String::from("1 change is waiting; nothing was changed.")
    } else {
        format!("{count} changes are waiting; nothing was changed.")
    };
    let finished = Finished::new(sentence).fact("Waiting", changes(&waiting.waiting));
    if waiting.empty {
        return finished
            .fact(
                "Database",
                "empty: setting Nage up was cut off before it finished",
            )
            .fact("Backup", "none needed, since the database holds nothing yet")
            .then(
                "Run nage migrate without --dry-run, start the server, or run nage init again, which finishes it.",
            );
    }
    let mut finished = finished.fact(
        "Database version",
        format!("{version} (this Nage expects {})", waiting.expects),
    );
    if let Some(backups) = &waiting.backup_to {
        finished = finished.fact("Backup", format!("would be written to {}/", shown(backups)));
    }
    finished.then("Run nage migrate without --dry-run, or start the server.")
}

/// A refused or failed update as the terminal prints it for `doing` (`serve`, `migrate` or the
/// dry run), by the Nage of `version` (decisions 151 to 156 and 159). Every failure but a newer
/// database says that nothing was changed; a failed change names the backup written before it.
pub(crate) fn update_refused(doing: Doing, version: &str, error: &UpdateError) -> Refused {
    let (again, try_again) = match doing {
        Doing::Serve => ("then start Nage again", "Start Nage again"),
        // A dry run only looks: it is pointed back at itself, never at the update.
        Doing::DryRun => (
            "then run nage migrate --dry-run again",
            "Run nage migrate --dry-run again",
        ),
        _ => ("then run nage migrate again", "Run nage migrate again"),
    };
    let not_done = doing.not_done();
    match error {
        UpdateError::InUse { data_dir, holder } => {
            let folder = shown(data_dir);
            let (why, now) = match (holder, doing) {
                (Some(Holder::Serve), Doing::Serve) => (
                    format!("A Nage server is running on this data folder ({folder})."),
                    "Stop the other server first, or give this one another data folder with --data-dir or NAGE_DATA_DIR.",
                ),
                (Some(Holder::Serve), _) => (
                    format!("A Nage server is running on this data folder ({folder})."),
                    "Stop the server, then run nage migrate again. Or just start the new version: the server updates the database itself when it starts.",
                ),
                (Some(Holder::Migrate), Doing::Serve) => (
                    format!("nage migrate is working on this data folder ({folder})."),
                    "Wait for it to end, then start Nage again.",
                ),
                (Some(Holder::Migrate), _) => (
                    format!("nage migrate is working on this data folder ({folder})."),
                    "Wait for it to end, then run nage migrate again.",
                ),
                (None, Doing::Serve) => (
                    format!("Another Nage is using this data folder ({folder})."),
                    "Stop the other Nage first, or give this one another data folder with --data-dir or NAGE_DATA_DIR.",
                ),
                (None, _) => (
                    format!("Another Nage is using this data folder ({folder})."),
                    "Stop the other Nage, then run nage migrate again.",
                ),
            };
            Refused::new(not_done, why, now)
        }
        UpdateError::Mark { path, source } => Refused::new(
            not_done,
            format!(
                "The in-use mark {} could not be taken ({}).",
                shown(path),
                system_words(source)
            ),
            format!("Check that the account Nage runs as can write in the data folder, {again}."),
        ),
        UpdateError::DataDir { path, source } => Refused::new(
            not_done,
            format!(
                "The folder {} could not be created ({}).",
                shown(path),
                system_words(source)
            ),
            "Check that the account Nage runs as may create it, or give another data folder with --data-dir or NAGE_DATA_DIR.",
        ),
        // A missing database, one that cannot be opened and a newer one are said in the sentences
        // every command shares (the maintainer's answer "One sentence per situation").
        UpdateError::Missing { path } => Refused::new(
            not_done,
            database::no_database(path),
            "Check the folder given with --data-dir or NAGE_DATA_DIR; a new book is set up with nage init or by starting the server.",
        ),
        UpdateError::Open { path, source } => Refused::new(
            not_done,
            database::not_opened(path, source),
            format!(
                "Check that the file is a Nage database and that the account Nage runs as can read and write it, {again}."
            ),
        ),
        UpdateError::Foreign { path } => Refused::new(
            not_done,
            format!(
                "The file {} holds tables but is not a Nage database.",
                shown(path)
            ),
            "Move the file out of the data folder, or give another data folder with --data-dir or NAGE_DATA_DIR.",
        ),
        UpdateError::Newer {
            path,
            version: carried,
            latest,
            backup,
        } => {
            let what = match doing {
                Doing::Serve => format!("Nage {version} did not start."),
                _ => String::from(not_done),
            };
            let now = match backup {
                Some(backup) => format!(
                    "Start the newer Nage again. To stay on {version}, restore the backup {} as the deployment guide shows; entries made since it was written are not in it.",
                    shown(backup)
                ),
                None => format!(
                    "Start the newer Nage again. No backup that {version} can open was found in {}.",
                    shown(&backups_of(path))
                ),
            };
            Refused::new(what, database::newer(*carried, *latest), now)
        }
        UpdateError::Modified { version, .. } => Refused::new(
            not_done,
            format!(
                "The change {version:04} recorded in the database differs from the one this version carries. Nothing was changed."
            ),
            "Start the version of Nage that wrote the database, and keep the lines above: the deployment guide says where to report them.",
        ),
        UpdateError::BackupTaken { path } => Refused::new(
            not_done,
            format!(
                "A file already carries the backup's name {}, and Nage never writes over a file. Nothing was changed.",
                shown(path)
            ),
            format!(
                "{try_again} in a moment, since the name carries the time to the second; move that file out of the backups folder only if Nage did not write it."
            ),
        ),
        UpdateError::BackupCreate { path, source } => Refused::new(
            not_done,
            format!(
                "The backup's file {} could not be created ({}). Nothing was changed.",
                shown(path),
                system_words(source)
            ),
            format!(
                "Let the account Nage runs as write in the backups folder, or free some space, {again}."
            ),
        ),
        UpdateError::Backup { path, source } => Refused::new(
            not_done,
            format!(
                "The backup {} could not be written ({}); it was removed, and nothing was changed.",
                shown(path),
                system_words(source)
            ),
            format!("Free some space, {again}."),
        ),
        UpdateError::BackupUnverified { path, reason } => Refused::new(
            not_done,
            format!(
                "The backup {} was written but did not pass its check ({reason}); it was removed, and nothing was changed.",
                shown(path)
            ),
            format!("{try_again}; {IF_IT_FAILS_AGAIN}"),
        ),
        UpdateError::RemoveOlder {
            path,
            source,
            written,
        } => Refused::new(
            not_done,
            format!(
                "The older copy {} of the same step could not be removed ({}). Nothing was updated; the new backup {} was kept.",
                shown(path),
                system_words(source),
                written.backup.as_deref().map(shown).unwrap_or_default()
            ),
            format!(
                "Remove that copy by hand, or let the account Nage runs as change the backups folder, {again}."
            ),
        ),
        UpdateError::Update {
            version: failed,
            source,
            written,
            ..
        } => {
            let since = match doing {
                Doing::Serve => "this start",
                _ => "this update",
            };
            let change = match failed {
                Some(failed) => format!("The change {failed:04} failed"),
                None => String::from("The update failed"),
            };
            let mut now = String::from(
                "Start the previous version of Nage again, and keep the lines above: the deployment guide says where to report them.",
            );
            if let Some(backup) = &written.backup {
                now.push_str(&format!(
                    " A backup from before the update is at {}.",
                    shown(backup)
                ));
            }
            // The server logs the copies it removed; `nage migrate` has no log, so it names them.
            if doing != Doing::Serve {
                for removed in &written.removed {
                    now.push_str(&format!(
                        " The older copy {} of the same step was removed.",
                        shown(removed)
                    ));
                }
            }
            Refused::new(
                not_done,
                format!(
                    "{change} ({}). Nothing was changed: the database is exactly as it was before {since}.",
                    system_words(source)
                ),
                now,
            )
        }
    }
}

/// The backups folder beside the database `database`.
fn backups_of(database: &Path) -> PathBuf {
    database
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
        .join(nage_core::update::BACKUPS_FOLDER)
}

/// `text` with its first letter made small, to continue a sentence.
fn first_lower(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}
