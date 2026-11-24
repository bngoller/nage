//! The data folder's database as the commands open it (SPEC 5.2, decisions 97, 113 and 155).
//! `check`, `user add` and `user reset-password` open the database that is there and never
//! create or update one ([`open_existing`]); `init` does the same when the file is there, creates
//! and updates it when it is missing ([`create`]), since a first update needs no backup (SPEC
//! 5.5), and finishes an empty one ([`finish`]). Whether a database is empty is the core's to
//! say (`nage_core::update::is_empty`, ruling P6 of the pause): a file that holds no table, a
//! zero-byte file included, asked without writing a byte to it. `serve` and `nage migrate` open
//! the database through `nage_core::update::open_and_update`, which alone updates a database that
//! is behind, so that the pre-migration backup is never skipped by a command. The rules are one;
//! the words are `check`'s here and the commands' own in `messages`.

use std::io;
use std::path::{Path, PathBuf};

use nage_core::storage::{self, Storage, StorageOptions};

use crate::config;
use crate::shape::{Doing, IF_IT_FAILS_AGAIN, Refused, shown, system_words};

#[cfg(test)]
mod tests;

/// The database file of `data_dir`, as an absolute path: the one the messages name.
pub(crate) fn path(data_dir: &Path) -> PathBuf {
    let given = data_dir.join(config::DATABASE_FILE);
    std::path::absolute(&given).unwrap_or(given)
}

/// Why, where a command meets a data folder without a database (the maintainer's answer "One
/// sentence per situation"): the same sentence in `nage check`, the user commands, `nage migrate`
/// and its dry run.
pub(crate) fn no_database(path: &Path) -> String {
    format!("There is no database at {}.", shown(path))
}

/// What now, where `nage check` or a user command meets a data folder without a database.
pub(crate) const NO_DATABASE_NOW: &str =
    "Check the folder given with --data-dir or NAGE_DATA_DIR; a new book is set up with nage init.";

/// Why, where a command meets a database a newer Nage wrote: the same sentence in `nage serve`,
/// `nage migrate`, its dry run, `nage check`, `nage init` and the user commands. `carried` is the
/// newest change the database carries, `latest` the newest this version carries, as the core
/// reads it (`Storage::latest_schema_version`).
pub(crate) fn newer(carried: i64, latest: i64) -> String {
    format!(
        "A newer Nage wrote this database: it carries the change {carried:04}, and this version knows up to {latest:04}."
    )
}

/// What now, where `nage check`, `nage init` or a user command meets a database a newer Nage
/// wrote.
pub(crate) const NEWER_NOW: &str = "Run the command with the newer Nage.";

/// Why, where `nage check`, `nage init` or a user command meets a database behind this version:
/// the sentence `nage check` prints.
pub(crate) fn behind(path: &Path, version: i64, expects: i64) -> String {
    format!(
        "The database at {} was left by an older version of Nage: it is at version {version}, and this Nage expects {expects}.",
        shown(path)
    )
}

/// Why, where a command cannot open the database: the sentence `nage check` prints, in every
/// command, the system's own words in brackets at its end.
pub(crate) fn not_opened(path: &Path, source: &(dyn std::error::Error + 'static)) -> String {
    format!(
        "The database at {} could not be opened ({}).",
        shown(path),
        system_words(source)
    )
}

/// Opens the database of `data_dir` by `check`'s rules (decision 97): the file must exist
/// (`Missing`; nothing is created, not even the folder), must not be empty (`Empty`, as the core
/// decides it, without a byte written) and must be at this binary's schema version (`Behind` or
/// `Newer`; nothing is updated). A file without sqlx's bookkeeping table fails as `Open`, since no
/// version can be read from it; a bookkeeping table without rows is `Behind`, at version 0. A
/// database that is refused is closed again, so no `-wal` or `-shm` file stays behind. The caller
/// closes the storage it is handed.
pub(crate) async fn open_existing(data_dir: &Path) -> Result<Storage, OpenError> {
    let path = path(data_dir);
    if !path.is_file() {
        return Err(OpenError::Missing(path));
    }
    // An empty database, as a cut-off `nage init` leaves it, is told apart before it is opened
    // for writing (decision 155): `init` finishes it, the other commands refuse it.
    match nage_core::update::is_empty(&path).await {
        Ok(true) => return Err(OpenError::Empty(path)),
        Ok(false) => {}
        Err(source) => return Err(OpenError::Open { path, source }),
    }
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .map_err(|source| OpenError::Open {
            path: path.clone(),
            source,
        })?;
    match at_this_version(&storage, &path).await {
        Ok(()) => Ok(storage),
        Err(error) => {
            storage.close().await;
            Err(error)
        }
    }
}

/// The schema version must be this binary's.
async fn at_this_version(storage: &Storage, path: &Path) -> Result<(), OpenError> {
    let version = storage
        .schema_version()
        .await
        .map_err(|source| OpenError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    let expects = Storage::latest_schema_version();
    match version {
        Some(version) if version > expects => Err(OpenError::Newer {
            path: path.to_path_buf(),
            version,
        }),
        Some(version) if version == expects => Ok(()),
        other => Err(OpenError::Behind {
            path: path.to_path_buf(),
            version: other.unwrap_or(0),
            expects,
        }),
    }
}

/// Creates the database of `data_dir` for `nage init` (decision 113), once [`open_existing`]
/// has answered `Missing`: the data folder with its `backups/` folder, as `serve` creates them
/// (`DataDir`), then the database file, which must not exist (`Create`: a file that appeared in
/// the meantime is left exactly as it is, so that this function never updates a database it did
/// not create), then every change through `Storage::migrate`, the one update path (`Migrate`).
/// The caller closes the storage it is handed; a failure after the file was created closes it
/// first.
pub(crate) async fn create(data_dir: &Path) -> Result<Storage, OpenError> {
    // Only the account Nage runs as can open what it creates (decision 154).
    nage_core::private::create_dir_all(&data_dir.join("backups")).map_err(|source| {
        OpenError::DataDir {
            path: data_dir.to_path_buf(),
            source,
        }
    })?;
    let path = path(data_dir);
    // `create_new_file` fails on a file that is there, whatever it holds, and creates the empty
    // file SQLite takes for an empty database otherwise; the handle closes at once.
    nage_core::private::create_new_file(&path).map_err(|source| OpenError::Create {
        path: path.clone(),
        source,
    })?;
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .map_err(|source| OpenError::Open {
            path: path.clone(),
            source,
        })?;
    match storage.migrate().await {
        Ok(_) => Ok(storage),
        Err(source) => {
            storage.close().await;
            Err(OpenError::Migrate { path, source })
        }
    }
}

/// Finishes the empty database a cut-off `nage init` left in `data_dir` (decision 155): a
/// zero-byte file, or one of a page without tables, as an init that failed after the open leaves
/// it. The file is opened as it is, never created or replaced, and updated like a new one through
/// `Storage::migrate`; the data folder's modes are left as they are. A file that holds a table by
/// the time this function has it (one that appeared or was written meanwhile) is never updated:
/// the core's own answer to whether it is empty is asked again before the open and once the file
/// is open, and a file that is not empty is refused exactly as [`create`] refuses a file that
/// appeared meanwhile (`Create`, a file already there), nothing written to it, so that the
/// pre-migration backup is never skipped (decision 113). The caller closes the storage it is
/// handed.
pub(crate) async fn finish(data_dir: &Path) -> Result<Storage, OpenError> {
    let path = path(data_dir);
    // The file must still be there: this function never creates one.
    if !path.is_file() {
        return Err(OpenError::Missing(path));
    }
    still_empty(&path).await?;
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .map_err(|source| OpenError::Open {
            path: path.clone(),
            source,
        })?;
    if let Err(error) = still_empty(&path).await {
        storage.close().await;
        return Err(error);
    }
    match storage.migrate().await {
        Ok(_) => Ok(storage),
        Err(source) => {
            storage.close().await;
            Err(OpenError::Migrate { path, source })
        }
    }
}

/// The operating system's number for a file already there (`EEXIST`, 17 on Linux and the BSDs), the
/// error `create` meets for a database file that appeared meanwhile.
const FILE_EXISTS: i32 = 17;

/// Whether the database file at `path` is still empty, as the core answers it (decision 155): a
/// file that holds a table is refused as [`create`] refuses a file that appeared meanwhile
/// (`Create`, the system's words for a file already there), and nothing is written to it.
async fn still_empty(path: &Path) -> Result<(), OpenError> {
    match nage_core::update::is_empty(path).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(OpenError::Create {
            path: path.to_path_buf(),
            source: io::Error::from_raw_os_error(FILE_EXISTS),
        }),
        Err(source) => Err(OpenError::Open {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Why the database of the data folder cannot be used (decisions 97 and 113). The three commands
/// word a missing database and one behind or ahead of this binary themselves, in three parts
/// (`messages::Stop::database`); `check` and the failures of the three print [`OpenError::refused`].
#[derive(Debug)]
pub(crate) enum OpenError {
    /// No file at the path; only `init` goes on to create one.
    Missing(PathBuf),
    /// An empty database at the path (no table, a zero-byte file included), as a cut-off `nage
    /// init` leaves it; only `init` goes on to finish it (decision 155).
    Empty(PathBuf),
    /// The storage could not be opened or its version read.
    Open {
        path: PathBuf,
        source: storage::Error,
    },
    /// The database is behind this binary; `nage migrate` and the server's start update it. A
    /// bookkeeping table without rows is behind at version 0; a file without that table is
    /// `Open`, not `Behind`.
    Behind {
        path: PathBuf,
        version: i64,
        expects: i64,
    },
    /// The database carries a change this binary does not know (decision 54).
    Newer { path: PathBuf, version: i64 },
    /// `init` could not create the data folder or its `backups/` folder.
    DataDir { path: PathBuf, source: io::Error },
    /// `init` could not create the database file, or found one there after all.
    Create { path: PathBuf, source: io::Error },
    /// `init` could not update the database it created or finished.
    Migrate {
        path: PathBuf,
        source: storage::Error,
    },
}

impl OpenError {
    /// Why the database of the data folder cannot be used, as the terminal prints it for
    /// `doing` (decisions 157 and 159): `check`'s words for every reason, and the words of
    /// `init`, `user add` and `user reset-password` for the failures (the refusals of those three,
    /// a missing database and one behind or ahead of this version, are their own `Refusal`s; an
    /// empty one is `check`'s words for the user commands too, ruling P6).
    pub(crate) fn refused(&self, doing: Doing) -> Refused {
        let again = match doing {
            Doing::Check => "then run nage check again",
            _ => "then run the command again",
        };
        match self {
            Self::Missing(path) => {
                Refused::new(doing.not_done(), no_database(path), NO_DATABASE_NOW)
            }
            Self::Empty(path) => Refused::new(
                doing.not_done(),
                format!(
                    "The database at {} is empty: setting Nage up was cut off before it finished.",
                    shown(path)
                ),
                "Run nage init again, which finishes it, or start the server.",
            ),
            Self::Open { path, source } => Refused::new(
                doing.not_done(),
                not_opened(path, source),
                format!(
                    "Check that the file is a Nage database and that this account can read and write it, {again}."
                ),
            ),
            Self::Behind {
                path,
                version,
                expects,
            } => Refused::new(
                doing.not_done(),
                behind(path, *version, *expects),
                format!(
                    "Update it with nage migrate, or start the server, which updates it itself, {again}."
                ),
            ),
            Self::Newer { version, .. } => Refused::new(
                doing.not_done(),
                newer(*version, Storage::latest_schema_version()),
                NEWER_NOW,
            ),
            Self::DataDir { path, source } => Refused::new(
                doing.not_done(),
                format!(
                    "The data folder {} could not be created ({}).",
                    shown(path),
                    system_words(source)
                ),
                format!(
                    "Check that this account may create it, or name another folder with --data-dir, {again}."
                ),
            ),
            Self::Create { path, source } => Refused::new(
                doing.not_done(),
                format!(
                    "The database file {} could not be created ({}).",
                    shown(path),
                    system_words(source)
                ),
                format!(
                    "If another nage init is running on this folder, wait for it to end; otherwise check the folder's permissions, {again}."
                ),
            ),
            Self::Migrate { path, source } => Refused::new(
                doing.not_done(),
                format!(
                    "The new database at {} could not be prepared ({}).",
                    shown(path),
                    system_words(source)
                ),
                format!("Run the command again, which finishes it; {IF_IT_FAILS_AGAIN}"),
            ),
        }
    }
}
