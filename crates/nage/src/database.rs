//! The data directory's database as the commands open it (SPEC 5.2, decisions 97 and 113).
//! `check`, `user add` and `user reset-password` open the database that is there and never
//! create or migrate one ([`open_existing`]); `init` does the same when the file is there and
//! creates and migrates it when it is missing ([`create`]), since a first migration needs no
//! backup (SPEC 5.5). `serve` opens the database its own way: it alone migrates a database
//! that is behind (decision 54), so that the pre-migration backup of milestone 2 slice 8 is
//! never skipped by a command. The rules are one; the words are `check`'s here and the
//! commands' own in `messages`.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use nage_core::storage::{self, Storage, StorageOptions};

use crate::config;

#[cfg(test)]
mod tests;

/// The database file of `data_dir`, as an absolute path: the one the messages name.
pub(crate) fn path(data_dir: &Path) -> PathBuf {
    let given = data_dir.join(config::DATABASE_FILE);
    std::path::absolute(&given).unwrap_or(given)
}

/// Opens the database of `data_dir` by `check`'s rules (decision 97): the file must exist
/// (`Missing`; nothing is created, not even the directory) and must be at this binary's schema
/// version (`Behind` or `Newer`; nothing is migrated). A file without sqlx's bookkeeping table
/// (`_sqlx_migrations`) fails as `Open`, since no version can be read from it; a bookkeeping
/// table without rows is `Behind`, at version 0. A database that is refused is closed again, so
/// no `-wal` or `-shm` file stays behind. The caller closes the storage it is handed.
pub(crate) async fn open_existing(data_dir: &Path) -> Result<Storage, OpenError> {
    let path = path(data_dir);
    if !path.is_file() {
        return Err(OpenError::Missing(path));
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
/// has answered `Missing`: the data directory with its `backups/` folder, as `serve` creates
/// them (`DataDir`), then the database file, which must not exist (`Create`: a file that
/// appeared in the meantime is left exactly as it is, so that this function never migrates a
/// database it did not create), then every migration (`Migrate`). The caller closes the
/// storage it is handed; a failure after the file was created closes it first.
pub(crate) async fn create(data_dir: &Path) -> Result<Storage, OpenError> {
    std::fs::create_dir_all(data_dir.join("backups")).map_err(|source| OpenError::DataDir {
        path: data_dir.to_path_buf(),
        source,
    })?;
    let path = path(data_dir);
    // `create_new` fails on a file that is there, whatever it holds, and creates the empty
    // file SQLite takes for an empty database otherwise; the handle closes at once.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| OpenError::Create {
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

/// Why the database of the data directory cannot be used (decisions 97 and 113). `check`
/// prints these lines as they stand, the first four as it has since milestone 2 slice 3. The
/// three commands word a missing database and one behind or ahead of this binary themselves,
/// in three parts (`messages::Stop::database`), and print the others as they stand: those are
/// failures, not refusals.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// No file at the path; only `init` goes on to create one.
    Missing(PathBuf),
    /// The storage could not be opened or its version read.
    Open {
        path: PathBuf,
        source: storage::Error,
    },
    /// The database is behind this binary; only `serve` migrates. A bookkeeping table without
    /// rows is behind at version 0; a file without that table is `Open`, not `Behind`.
    Behind {
        path: PathBuf,
        version: i64,
        expects: i64,
    },
    /// The database carries a migration this binary does not know (decision 54).
    Newer { path: PathBuf, version: i64 },
    /// `init` could not create the data directory or its `backups/` folder.
    DataDir { path: PathBuf, source: io::Error },
    /// `init` could not create the database file, or found one there after all.
    Create { path: PathBuf, source: io::Error },
    /// `init` could not migrate the database it created.
    Migrate {
        path: PathBuf,
        source: storage::Error,
    },
}

impl fmt::Display for OpenError {
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
            Self::Behind {
                version, expects, ..
            } => write!(
                f,
                "the database is at schema version {version} and this nage expects {expects}: start nage serve to migrate it"
            ),
            Self::Newer { version, .. } => write!(
                f,
                "the database was written by a newer Nage: it carries migration {version}, which this version does not know"
            ),
            Self::DataDir { path, source } => write!(
                f,
                "cannot create data directory {}: {source}",
                path.display()
            ),
            Self::Create { path, source } => {
                write!(f, "cannot create database {}: {source}", path.display())
            }
            Self::Migrate { path, source } => {
                write!(f, "cannot migrate database {}: {source}", path.display())
            }
        }
    }
}
