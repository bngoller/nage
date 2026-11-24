//! The one operation that brings a data folder's database up to this version (SPEC 5.2 and 5.5,
//! decisions 151 to 155, assumption A7 of PLAN M2 slice 8): `nage serve` and `nage migrate` both
//! call [`open_and_update`], so the mark, the backup and the update are one code path; the binary
//! only prints and logs what it answers. The update itself is [`Storage::migrate`], the one path
//! `nage init`, `cargo xtask seed` and the tests take too (ruling P2 of the pause). In order:
//!
//! 1. A missing database file under [`Missing::Refuse`] (`nage migrate`) is refused before
//!    anything else, so nothing is created, not even the mark's file. Under [`Missing::Create`]
//!    (`serve`) the data folder is created with mode 0700 when it is missing.
//! 2. The in-use mark ([`Mark`]): an exclusive lock on the file `nage.lock` in the data folder,
//!    which holds one word naming its holder (`serve` or `migrate`), held by `serve` for its life
//!    and by `migrate` while it works; a second holder is refused ([`UpdateError::InUse`], naming
//!    the holder the file names). The lock goes with the process, so a crash leaves no stale mark;
//!    the file stays. The mark binds only a Nage that takes it: a server older than PLAN M2 slice
//!    8 takes none, it may do nothing on some network drives, and the file must not be removed
//!    while a server runs (decision 153).
//! 3. The database, opened through [`Storage::open`]. An empty database ([`is_empty`]: a file
//!    that holds no table, a zero-byte file included), as a cut-off `nage init` leaves it, is
//!    finished like a new one (decision 155). A file with tables but no bookkeeping table is not
//!    Nage's and is refused. A database that carries a change this version does not know is
//!    refused before the database is written, naming the newest backup this version can open (the
//!    mark's file may have been created by then). A changed record of an applied change is refused
//!    before any backup is written, also when nothing waits.
//! 4. The backup, exactly when a change waits and the bookkeeping table holds a row. Nage creates
//!    the backup's file itself, new and private (`private::create_new_file`, mode 0600), under the
//!    name of [`crate::backup::name`]; a file already at the name, an empty one or a dangling
//!    symbolic link included, fails that creation and stops the update with nothing written
//!    ([`UpdateError::BackupTaken`]), and so does a file at the name with SQLite's `-journal`,
//!    `-wal` or `-shm` after it, which SQLite would clear beside the empty file before writing the
//!    copy: it is left as it is. Only then does `VACUUM INTO` write into the empty file, on
//!    the writer connection outside any transaction, the path bound as a parameter. The copy is
//!    then checked by [`verify_backup`] (SQLite's integrity check and its schema version); a
//!    backup that fails or does not pass is removed, and nothing is updated. Once it passed, every
//!    other copy of the same step (the same last change, the same app version) is removed,
//!    whatever its stamp, since a clock set back may leave a later stamp on an older copy; copies
//!    of other steps and files that are not backups are never touched.
//! 5. The update, all or nothing ([`Storage::migrate`]): every waiting change in one outer
//!    `BEGIN IMMEDIATE`, sqlx's migrator inside it by savepoints, so a failure leaves the database
//!    exactly as it was. A failure names the backup written and the copies removed before it, so
//!    that the server can log them.

use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use sqlx::migrate::{Migrate as _, MigrateError};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};

use crate::backup;
use crate::private;
use crate::storage::{self, Bookkeeping, Storage, StorageOptions};
use crate::timestamp::Timestamp;

/// The database file in the data folder (SPEC 5.2).
pub const DATABASE_FILE: &str = "nage.sqlite";
/// The backups folder in the data folder (SPEC 5.2 and 5.5).
pub const BACKUPS_FOLDER: &str = "backups";
/// The in-use mark's file in the data folder (decision 153).
pub const MARK_FILE: &str = "nage.lock";

/// What [`open_and_update`] does with a data folder that holds no database file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// `serve`: the data folder, its backups folder and the database are created.
    Create,
    /// `migrate`: refused as [`UpdateError::Missing`] before the mark is taken; nothing is
    /// created.
    Refuse,
}

/// Who holds a data folder's mark: the one word the mark's file holds (ruling P5 of the pause).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// `nage serve`, for its whole life: the word `serve`.
    Serve,
    /// `nage migrate`, while it works: the word `migrate`.
    Migrate,
}

impl Holder {
    /// The word the mark's file holds.
    pub fn word(self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::Migrate => "migrate",
        }
    }
}

/// What [`open_and_update`] is given.
#[derive(Clone, Debug)]
pub struct Update<'a> {
    /// The data folder, as an absolute path.
    pub data_dir: &'a Path,
    /// The instant the update begins, which the backup's name carries; the caller reads the clock.
    pub now: Timestamp,
    /// The version of this Nage, which the backup's name carries.
    pub version: &'a str,
    /// Whether a missing database is created.
    pub missing: Missing,
    /// Who takes the mark.
    pub holder: Holder,
    /// The storage's waits; `serve` and `migrate` hand the default.
    pub options: &'a StorageOptions,
}

/// The in-use mark of a data folder (decision 153): held while the value lives.
#[derive(Debug)]
pub struct Mark {
    /// The locked file; dropping it releases the lock.
    _file: File,
}

impl Mark {
    /// Takes the mark of `data_dir`, which must exist, for `holder`: the file [`MARK_FILE`] is
    /// opened, created with mode 0600 when it is missing, locked exclusively without waiting, and
    /// then made to hold `holder`'s word alone. Another holder is [`UpdateError::InUse`], naming
    /// the holder its file names (`None` when the word cannot be read or is no holder's); a file
    /// that cannot be opened, locked or written for another reason is [`UpdateError::Mark`].
    pub fn take(data_dir: &Path, holder: Holder) -> Result<Mark, UpdateError> {
        let path = data_dir.join(MARK_FILE);
        let failed = |source: io::Error| UpdateError::Mark {
            path: path.clone(),
            source,
        };
        let mut file = private::open_or_create_file(&path).map_err(failed)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(UpdateError::InUse {
                    data_dir: data_dir.to_path_buf(),
                    holder: holder_named(&mut file),
                });
            }
            Err(TryLockError::Error(source)) => return Err(failed(source)),
        }
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| file.write_all(holder.word().as_bytes()))
            .map_err(failed)?;
        Ok(Mark { _file: file })
    }
}

/// The holder the mark's file `file` names: its whole text must be one holder's word; `None`
/// when it cannot be read or names none.
fn holder_named(file: &mut File) -> Option<Holder> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    [Holder::Serve, Holder::Migrate]
        .into_iter()
        .find(|holder| holder.word() == text)
}

/// What [`open_and_update`] hands back: the open storage, the mark, held until the caller drops
/// it, and what was done.
#[derive(Debug)]
pub struct Updated {
    /// The database, open and at this version.
    pub storage: Storage,
    /// The in-use mark of the data folder.
    pub mark: Mark,
    /// What was done.
    pub report: UpdateReport,
}

/// What [`open_and_update`] did (decision 151).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateReport {
    /// The database file, as an absolute path.
    pub database: PathBuf,
    /// Whether this call created the database or finished an empty one.
    pub created: bool,
    /// The backup written, when one was.
    pub backup: Option<PathBuf>,
    /// The other copies of the same step that were removed once the backup passed its check.
    pub removed: Vec<PathBuf>,
    /// The changes applied, by number, in order; empty when nothing waited.
    pub applied: Vec<i64>,
    /// The schema version afterwards; `None` only for a database with no change at all.
    pub schema_version: Option<i64>,
}

/// Opens the database of the data folder and brings it up to this version (the module's steps).
pub async fn open_and_update(update: Update<'_>) -> Result<Updated, UpdateError> {
    let data_dir = update.data_dir;
    let database = database_path(data_dir);
    let backups = data_dir.join(BACKUPS_FOLDER);
    let missing = !database.exists();
    if missing && update.missing == Missing::Refuse {
        return Err(UpdateError::Missing { path: database });
    }
    if update.missing == Missing::Create {
        private::create_dir_all(data_dir).map_err(|source| UpdateError::DataDir {
            path: data_dir.to_path_buf(),
            source,
        })?;
    }
    let mark = Mark::take(data_dir, update.holder)?;
    if missing {
        // Created new and private before SQLite opens it (decision 154): SQLite then finds an
        // empty file and makes it a database, keeping its mode.
        private::open_or_create_file(&database).map_err(|source| UpdateError::Open {
            path: database.clone(),
            source: storage::Error::Sqlx(sqlx::Error::Io(source)),
        })?;
    }

    // Everything that refuses the database is read before it is opened for writing.
    let (applied, created) = match inspect(&database).await? {
        Inspected::Empty => (Vec::new(), true),
        Inspected::Foreign => return Err(UpdateError::Foreign { path: database }),
        Inspected::Applied {
            newer: Some(version),
            ..
        } => {
            let latest = Storage::latest_schema_version();
            return Err(UpdateError::Newer {
                path: database,
                version,
                latest,
                backup: backup::newest_for(&backups, latest),
            });
        }
        Inspected::Applied {
            modified: Some(version),
            ..
        } => {
            return Err(UpdateError::Modified {
                path: database,
                version,
            });
        }
        Inspected::Applied { versions, .. } => (versions, false),
    };
    private::create_dir_all(&backups).map_err(|source| UpdateError::DataDir {
        path: backups.clone(),
        source,
    })?;

    let storage = Storage::open(&database, update.options)
        .await
        .map_err(|source| UpdateError::Open {
            path: database.clone(),
            source,
        })?;
    let waiting = waiting_changes(&applied);
    match bring_up(&storage, &update, &database, &backups, &applied, &waiting).await {
        Ok((written, schema_version)) => Ok(Updated {
            storage,
            mark,
            report: UpdateReport {
                database,
                created,
                backup: written.backup,
                removed: written.removed,
                applied: waiting,
                schema_version,
            },
        }),
        Err(error) => {
            storage.close().await;
            Err(error)
        }
    }
}

/// The database file of `data_dir`, as an absolute path.
fn database_path(data_dir: &Path) -> PathBuf {
    let given = data_dir.join(DATABASE_FILE);
    std::path::absolute(&given).unwrap_or(given)
}

/// The changes this version carries that `applied` does not hold, in order.
fn waiting_changes(applied: &[i64]) -> Vec<i64> {
    let mut waiting: Vec<i64> = storage::MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .filter(|version| !applied.contains(version))
        .collect();
    waiting.sort_unstable();
    waiting.dedup();
    waiting
}

/// Steps 4 and 5 on the open storage: the backup when a change waits and the bookkeeping table
/// holds a row, then the update. Answers what was written and removed, and the schema version.
async fn bring_up(
    storage: &Storage,
    update: &Update<'_>,
    database: &Path,
    backups: &Path,
    applied: &[i64],
    waiting: &[i64],
) -> Result<(Written, Option<i64>), UpdateError> {
    let mut written = Written::default();
    if let (false, Some(step)) = (waiting.is_empty(), applied.iter().copied().max()) {
        let path = backups.join(backup::name(update.now, step, update.version));
        // SQLite clears a `-journal`, `-wal` or `-shm` file it finds beside an empty database it
        // opens, so `VACUUM INTO` would remove such a file at the backup's name before it wrote the
        // copy: one stops the update as a file at the name does, and is left as it is.
        if let Some(side) = side_file(&path) {
            return Err(UpdateError::BackupTaken { path: side });
        }
        match private::create_new_file(&path) {
            Ok(file) => drop(file),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                return Err(UpdateError::BackupTaken { path });
            }
            Err(source) => return Err(UpdateError::BackupCreate { path, source }),
        }
        if let Err(source) = storage.back_up_into(&path).await {
            remove_quietly(&path);
            return Err(UpdateError::Backup { path, source });
        }
        if let Err(reason) = verify_backup(&path, step).await {
            remove_quietly(&path);
            return Err(UpdateError::BackupUnverified { path, reason });
        }
        written.backup = Some(path.clone());
        for other in backup::same_step(backups, step, update.version, &path) {
            if let Err(source) = std::fs::remove_file(&other) {
                return Err(UpdateError::RemoveOlder {
                    path: other,
                    source,
                    written: Box::new(written),
                });
            }
            written.removed.push(other);
        }
    }
    match storage.migrate().await {
        Ok(report) => Ok((written, report.schema_version)),
        Err(source) => {
            let version = match &source {
                storage::Error::Migrate(MigrateError::ExecuteMigration(_, version)) => {
                    Some(*version)
                }
                _ => None,
            };
            Err(UpdateError::Update {
                path: database.to_path_buf(),
                version,
                source,
                written: Box::new(written),
            })
        }
    }
}

/// The first of the side files SQLite keeps beside a database (`-journal`, `-wal`, `-shm`) that
/// stands at the name `path` with its suffix: a file, a folder or a symbolic link, dangling or not.
fn side_file(path: &Path) -> Option<PathBuf> {
    ["-journal", "-wal", "-shm"].into_iter().find_map(|suffix| {
        let mut side = path.as_os_str().to_owned();
        side.push(suffix);
        let side = PathBuf::from(side);
        std::fs::symlink_metadata(&side).is_ok().then_some(side)
    })
}

/// Removes the file `path` Nage has just created, when it can: a backup that failed or did not
/// pass its check.
fn remove_quietly(path: &Path) {
    drop(std::fs::remove_file(path));
}

/// What [`inspect`] read of a database, before it is opened for writing.
enum Inspected {
    /// No table at all: a new database, or one a cut-off `nage init` left.
    Empty,
    /// Tables, but no bookkeeping table.
    Foreign,
    /// The changes the bookkeeping table records, in order; the newest one this version does not
    /// carry; the first one whose record differs from this version's.
    Applied {
        versions: Vec<i64>,
        newer: Option<i64>,
        modified: Option<i64>,
    },
}

/// Reads the bookkeeping of the database at `path` on a read-only connection of its own, so that
/// nothing is written; a zero-byte file is answered without opening it.
async fn inspect(path: &Path) -> Result<Inspected, UpdateError> {
    let open_error = |source: storage::Error| UpdateError::Open {
        path: path.to_path_buf(),
        source,
    };
    if zero_bytes(path).map_err(|error| open_error(storage::Error::Sqlx(sqlx::Error::Io(error))))? {
        return Ok(Inspected::Empty);
    }
    let mut conn = read_only_beside(path)
        .await
        .map_err(|error| open_error(storage::Error::Sqlx(error)))?;
    let inspected = inspect_on(&mut conn).await;
    drop(conn.close().await);
    inspected.map_err(open_error)
}

/// [`inspect`] on the connection `conn`.
async fn inspect_on(conn: &mut SqliteConnection) -> Result<Inspected, storage::Error> {
    match storage::read_bookkeeping(conn)
        .await
        .map_err(storage::Error::Sqlx)?
    {
        Bookkeeping::Empty => Ok(Inspected::Empty),
        Bookkeeping::Foreign => Ok(Inspected::Foreign),
        Bookkeeping::Applied(versions) => {
            let recorded = conn
                .list_applied_migrations(storage::MIGRATOR.table_name.as_ref())
                .await
                .map_err(storage::Error::Migrate)?;
            let carried = |version: i64| {
                storage::MIGRATOR
                    .iter()
                    .find(|migration| migration.version == version)
            };
            let newer = recorded
                .iter()
                .filter(|record| carried(record.version).is_none())
                .map(|record| record.version)
                .max();
            let modified = recorded
                .iter()
                .find(|record| {
                    carried(record.version)
                        .is_some_and(|migration| migration.checksum != record.checksum)
                })
                .map(|record| record.version);
            Ok(Inspected::Applied {
                versions,
                newer,
                modified,
            })
        }
    }
}

/// Whether the file at `path` holds no byte; an error when it cannot be read, a missing file
/// included.
fn zero_bytes(path: &Path) -> io::Result<bool> {
    Ok(std::fs::metadata(path)?.len() == 0)
}

/// A read-only connection of its own to the database at `path` that writes nothing beside it:
/// while no `-wal` file stands beside the database, the WAL holds nothing, and the file is
/// opened immutable, so SQLite creates no `-wal` or `-shm` file either; with one (a server
/// running, or a crash), it is opened read-only and reads what the WAL holds.
async fn read_only_beside(path: &Path) -> Result<SqliteConnection, sqlx::Error> {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let immutable = !Path::new(&wal).exists();
    read_only(path, immutable).await
}

/// A read-only connection of its own to the database at `path`, never creating it; `immutable`
/// also leaves SQLite's locks and side files alone, for a file nothing else writes.
async fn read_only(path: &Path, immutable: bool) -> Result<SqliteConnection, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .immutable(immutable);
    SqliteConnection::connect_with(&options).await
}

/// Whether the database file at `path` is empty (decision 155, ruling P6 of the pause): a file
/// that holds no table, a zero-byte file included, as a cut-off `nage init` leaves it. The binary's
/// commands that open a database themselves (`nage init`, `nage check` and the user commands, by
/// `database::open_existing`) ask here; [`open_and_update`] (`serve` and `nage migrate`) and
/// [`waiting`] (the dry run) tell it in their own read of the bookkeeping, `inspect`. Both read it
/// the same way, through the same zero-byte test, the same read-only connection and the same
/// question of `storage::read_bookkeeping`, so an empty database has one meaning everywhere. The
/// file is read without writing a byte to it (a zero-byte file is answered without opening it, and
/// any other file is opened read-only, so no WAL header is written). A missing file is an error,
/// as SQLite reports it.
pub async fn is_empty(path: &Path) -> Result<bool, storage::Error> {
    if zero_bytes(path).map_err(|error| storage::Error::Sqlx(sqlx::Error::Io(error)))? {
        return Ok(true);
    }
    let mut conn = read_only_beside(path).await.map_err(storage::Error::Sqlx)?;
    let bookkeeping = storage::read_bookkeeping(&mut conn).await;
    drop(conn.close().await);
    Ok(bookkeeping.map_err(storage::Error::Sqlx)? == Bookkeeping::Empty)
}

/// Checks the backup at `path` before anything is updated (decision 151, ruling P3 of the pause):
/// opened read-only, SQLite's integrity check must answer `ok` and its schema version must be
/// `step`, the step its name carries. The reason it did not pass, in the system's own words, is
/// the error: a truncated copy, one at another version, one SQLite cannot open. The reason is one
/// line: the integrity check's rows are joined with a semicolon and a space, and a line break or
/// another control character inside a row becomes a space.
pub async fn verify_backup(path: &Path, step: i64) -> Result<(), String> {
    let mut conn = read_only(path, true)
        .await
        .map_err(|error| system_words(&error))?;
    let verdict = verify_on(&mut conn, step).await;
    drop(conn.close().await);
    verdict
}

/// [`verify_backup`] on the connection `conn`.
async fn verify_on(conn: &mut SqliteConnection, step: i64) -> Result<(), String> {
    // A PRAGMA, which sqlx cannot check: unchecked SQL as decision 57 allows.
    let lines: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut *conn)
        .await
        .map_err(|error| system_words(&error))?;
    if lines != ["ok"] {
        // One line: the rows joined with a semicolon and a space, a line break or any other
        // control character inside a row made a space.
        let joined = lines.join("; ");
        return Err(joined
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect());
    }
    let version =
        sqlx::query_scalar!(r#"SELECT MAX(version) AS "version: i64" FROM _sqlx_migrations"#)
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| system_words(&error))?;
    match version {
        Some(version) if version == step => Ok(()),
        Some(version) => Err(format!(
            "the copy is at the change {version:04}, not at {step:04} as its name says"
        )),
        None => Err(format!(
            "the copy records no change, not {step:04} as its name says"
        )),
    }
}

/// The system's own words for `error`: SQLite's message when the database answered, else the
/// error as sqlx writes it.
fn system_words(error: &sqlx::Error) -> String {
    match error.as_database_error() {
        Some(database) => database.message().to_owned(),
        None => error.to_string(),
    }
}

/// What waits in a data folder's database, read without changing anything (decision 156, `nage
/// migrate --dry-run`): no mark is taken, so a running server does not stop it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// The database file, as an absolute path.
    pub database: PathBuf,
    /// Whether the database is empty ([`is_empty`]): every change waits, and no backup is needed.
    pub empty: bool,
    /// The schema version the database is at; `None` for a database with no change at all.
    pub schema_version: Option<i64>,
    /// The schema version this Nage expects.
    pub expects: i64,
    /// The changes that wait, by number, in order.
    pub waiting: Vec<i64>,
    /// The backups folder, when an update would write a backup there.
    pub backup_to: Option<PathBuf>,
}

/// Reads what waits in the database of `data_dir` (decision 156) without writing a byte to it,
/// refusing what [`open_and_update`] would refuse before its backup. A missing database is
/// [`UpdateError::Missing`]; a newer one is [`UpdateError::Newer`]; one that cannot be opened is
/// [`UpdateError::Open`]; a file that is not Nage's is [`UpdateError::Foreign`]; a changed record
/// of an applied change is [`UpdateError::Modified`], also when nothing waits.
pub async fn waiting(data_dir: &Path) -> Result<Waiting, UpdateError> {
    let database = database_path(data_dir);
    if !database.exists() {
        return Err(UpdateError::Missing { path: database });
    }
    let expects = Storage::latest_schema_version();
    let backups = data_dir.join(BACKUPS_FOLDER);
    let applied = match inspect(&database).await? {
        Inspected::Empty => {
            return Ok(Waiting {
                database,
                empty: true,
                schema_version: None,
                expects,
                waiting: waiting_changes(&[]),
                backup_to: None,
            });
        }
        Inspected::Foreign => return Err(UpdateError::Foreign { path: database }),
        Inspected::Applied {
            newer: Some(version),
            ..
        } => {
            return Err(UpdateError::Newer {
                path: database,
                version,
                latest: expects,
                backup: backup::newest_for(&backups, expects),
            });
        }
        // Refused as `open_and_update` refuses it, also when nothing waits: the dry run says what
        // the real run would do.
        Inspected::Applied {
            modified: Some(version),
            ..
        } => {
            return Err(UpdateError::Modified {
                path: database,
                version,
            });
        }
        Inspected::Applied { versions, .. } => versions,
    };
    let waiting = waiting_changes(&applied);
    let backup_to = (!waiting.is_empty() && !applied.is_empty()).then_some(backups);
    Ok(Waiting {
        database,
        empty: false,
        schema_version: applied.iter().copied().max(),
        expects,
        waiting,
        backup_to,
    })
}

/// Why a data folder's database was not brought up to date (decisions 151 to 155). The binary
/// words each in three parts at the terminal; `Display` is the short line a log or a test reads.
#[derive(Debug)]
pub enum UpdateError {
    /// Another Nage holds the data folder's mark: `holder` is the one its file names, `None` when
    /// the file names none.
    InUse {
        data_dir: PathBuf,
        holder: Option<Holder>,
    },
    /// The mark's file could not be opened, locked or written.
    Mark { path: PathBuf, source: io::Error },
    /// The data folder or its backups folder could not be created.
    DataDir { path: PathBuf, source: io::Error },
    /// No database file, under [`Missing::Refuse`].
    Missing { path: PathBuf },
    /// The database could not be opened or read.
    Open {
        path: PathBuf,
        source: storage::Error,
    },
    /// The file holds tables but no bookkeeping table: not a Nage database.
    Foreign { path: PathBuf },
    /// The database carries a change this version does not know: a newer Nage wrote it. `backup`
    /// is the newest copy this version can open, when the backups folder holds one.
    Newer {
        path: PathBuf,
        version: i64,
        latest: i64,
        backup: Option<PathBuf>,
    },
    /// A change recorded in the database differs from the one this version carries; refused
    /// before any backup is written.
    Modified { path: PathBuf, version: i64 },
    /// A file is already at the backup's name (an empty one or a dangling link included), or at
    /// that name with SQLite's `-journal`, `-wal` or `-shm` after it, which SQLite would clear;
    /// `path` is that file, and nothing was written at it or removed.
    BackupTaken { path: PathBuf },
    /// The backup's file could not be created for another reason than a file at its name.
    BackupCreate { path: PathBuf, source: io::Error },
    /// The backup could not be written into its file; the file was removed.
    Backup {
        path: PathBuf,
        source: storage::Error,
    },
    /// The backup was written but did not pass its check; it was removed.
    BackupUnverified { path: PathBuf, reason: String },
    /// Another copy of the same step could not be removed; nothing was updated. `written` holds
    /// the backup kept and the copies removed before it.
    RemoveOlder {
        path: PathBuf,
        source: io::Error,
        written: Box<Written>,
    },
    /// A change failed; the database is exactly as it was. `version` is the change that failed,
    /// when sqlx names it; `written` the copy written before and the copies of the same step
    /// removed once it passed.
    Update {
        path: PathBuf,
        version: Option<i64>,
        source: storage::Error,
        written: Box<Written>,
    },
}

/// The backup an update wrote and the other copies of the same step it removed before it failed
/// (ruling P4 of the pause), so that the server can log them before its three lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// The backup written, when one was.
    pub backup: Option<PathBuf>,
    /// The other copies of the same step removed once it passed its check.
    pub removed: Vec<PathBuf>,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InUse { data_dir, holder } => write!(
                f,
                "another Nage ({}) is using the data folder {}",
                holder.map_or("unknown", Holder::word),
                data_dir.display()
            ),
            Self::Mark { path, source } => {
                write!(
                    f,
                    "cannot take the in-use mark {}: {source}",
                    path.display()
                )
            }
            Self::DataDir { path, source } => {
                write!(f, "cannot create the folder {}: {source}", path.display())
            }
            Self::Missing { path } => write!(f, "no database at {}", path.display()),
            Self::Open { path, source } => {
                write!(f, "cannot open the database {}: {source}", path.display())
            }
            Self::Foreign { path } => write!(
                f,
                "{} holds tables but is not a Nage database",
                path.display()
            ),
            Self::Newer {
                version, latest, ..
            } => write!(
                f,
                "a newer Nage wrote the database: it carries the change {version:04}, and this version knows up to {latest:04}"
            ),
            Self::Modified { version, .. } => write!(
                f,
                "the change {version:04} recorded in the database differs from the one this version carries"
            ),
            Self::BackupTaken { path } => {
                write!(
                    f,
                    "a file is already at the backup's name {}",
                    path.display()
                )
            }
            Self::BackupCreate { path, source } => {
                write!(
                    f,
                    "cannot create the backup's file {}: {source}",
                    path.display()
                )
            }
            Self::Backup { path, source } => {
                write!(f, "cannot write the backup {}: {source}", path.display())
            }
            Self::BackupUnverified { path, reason } => write!(
                f,
                "the backup {} did not pass its check: {reason}",
                path.display()
            ),
            Self::RemoveOlder { path, source, .. } => write!(
                f,
                "cannot remove the other copy {}: {source}",
                path.display()
            ),
            Self::Update {
                version: Some(version),
                source,
                ..
            } => write!(f, "the change {version:04} failed: {source}"),
            Self::Update {
                version: None,
                source,
                ..
            } => write!(f, "the update failed: {source}"),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mark { source, .. }
            | Self::DataDir { source, .. }
            | Self::BackupCreate { source, .. }
            | Self::RemoveOlder { source, .. } => Some(source),
            Self::Open { source, .. }
            | Self::Backup { source, .. }
            | Self::Update { source, .. } => Some(source),
            Self::InUse { .. }
            | Self::Missing { .. }
            | Self::Foreign { .. }
            | Self::Newer { .. }
            | Self::Modified { .. }
            | Self::BackupTaken { .. }
            | Self::BackupUnverified { .. } => None,
        }
    }
}
