//! The SQLite storage (SPEC 5.4 and 5.5, decisions 53, 54, 57 and 58): a writer pool of
//! exactly one connection whose transactions begin with `BEGIN IMMEDIATE`, a pool of
//! read-only readers, the PRAGMAs every connection carries, the migrations embedded from
//! `migrations/` and the busy classification behind the 503 page. Every ledger operation
//! takes a [`WriteTransaction`] from here (SPEC 11); the web layer never opens a connection
//! of its own.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use sqlx::migrate::{Migrate as _, MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// The migrations under `migrations/` at the workspace root, embedded at compile time
/// (SPEC 5.5, decision 57). sqlx checksums each file's exact bytes, so a released file is
/// frozen; the crate's `build.rs` re-runs the build when the directory changes.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// `SQLITE_BUSY`, the primary result code SQLite answers when it cannot take a lock within
/// its busy timeout (SPEC 5.4). Extended codes such as `SQLITE_BUSY_SNAPSHOT` (517) carry
/// it in their low byte.
const SQLITE_BUSY: i32 = 5;

/// A write transaction on the one writer connection, begun with `BEGIN IMMEDIATE` so the
/// write lock is taken at `BEGIN` (SPEC 5.4, decision 53). Dropping it without `commit`
/// rolls back (INV-28).
pub type WriteTransaction = Transaction<'static, Sqlite>;

/// How long storage waits and how many readers it opens (decision 53). The fields are
/// public so integration tests can shorten the waits; `serve` uses the [`Default`].
#[derive(Clone, Debug)]
pub struct StorageOptions {
    /// SQLite's own busy timeout, set on every connection, for lockers outside this process
    /// (SPEC 5.4: five seconds).
    pub busy_timeout: Duration,
    /// How long a request waits for the writer connection (decision 53: 30 seconds).
    pub write_queue: Duration,
    /// How long a request waits for a reader connection (decision 53: 5 seconds).
    pub read_queue: Duration,
    /// How many read-only connections the read pool holds (decision 53: four).
    pub readers: u32,
}

impl Default for StorageOptions {
    /// The values of decision 53: busy 5 s, write queue 30 s, read queue 5 s, four readers.
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_millis(5000),
            write_queue: Duration::from_secs(30),
            read_queue: Duration::from_secs(5),
            readers: 4,
        }
    }
}

/// The two pools over one `nage.sqlite` (SPEC 5.4, decision 53). Cloning shares the pools.
#[derive(Clone, Debug)]
pub struct Storage {
    writer: SqlitePool,
    reader: SqlitePool,
}

impl Storage {
    /// Opens the writer first (creating the file if missing and setting `journal_mode=WAL`,
    /// `synchronous=NORMAL`, `foreign_keys=ON` and the busy timeout), then the read-only
    /// readers (`foreign_keys=ON` and the busy timeout). Nothing is migrated: `serve` calls
    /// [`Storage::migrate`] before answering any request (decision 54).
    ///
    /// The writer pool holds exactly one connection, kept open (`min_connections` 1) so the
    /// idle reaper never empties it; its eager connect creates the file and switches it to
    /// WAL before any reader opens. The file's parent directory must already exist.
    pub async fn open(path: &Path, options: &StorageOptions) -> Result<Storage, Error> {
        let writer_options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(options.busy_timeout);
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .acquire_timeout(options.write_queue)
            .connect_with(writer_options)
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))?;

        let reader_options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .foreign_keys(true)
            .busy_timeout(options.busy_timeout);
        let reader = SqlitePoolOptions::new()
            .max_connections(options.readers)
            .acquire_timeout(options.read_queue)
            .connect_with(reader_options)
            .await;
        match reader {
            Ok(reader) => Ok(Storage { writer, reader }),
            Err(err) => {
                // The writer is the only connection and read-write, so closing it leaves no
                // `-wal` or `-shm` file behind a failed open.
                writer.close().await;
                Err(classify(err, BusyCause::ReadQueue))
            }
        }
    }

    /// Applies the pending migrations on one acquired writer connection and reports how many
    /// ran and the schema version afterwards (SPEC 5.5, decision 54). A database carrying a
    /// migration this binary does not know is [`Error::NewerDatabase`]; a migration whose
    /// file changed since it was applied is [`Error::ModifiedMigration`].
    pub async fn migrate(&self) -> Result<MigrationReport, Error> {
        // One connection for the whole call: the writer pool holds exactly one, so running
        // the migrator against the pool while holding a connection would wait out the write
        // queue and fail.
        let mut conn = self
            .writer
            .acquire()
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))?;
        let table = MIGRATOR.table_name.as_ref();
        conn.ensure_migrations_table(table)
            .await
            .map_err(migrate_error)?;
        let before = conn
            .list_applied_migrations(table)
            .await
            .map_err(migrate_error)?
            .len();
        MIGRATOR.run(&mut *conn).await.map_err(migrate_error)?;
        let after = conn
            .list_applied_migrations(table)
            .await
            .map_err(migrate_error)?;
        Ok(MigrationReport {
            applied: after.len().saturating_sub(before),
            schema_version: after.iter().map(|migration| migration.version).max(),
        })
    }

    /// The only way to write: `BEGIN IMMEDIATE` on the writer connection (SPEC 5.4,
    /// decision 53). A write-queue wait that expires is [`Error::Busy`] with
    /// [`BusyCause::WriteQueue`]; SQLite refusing the lock after its busy timeout is
    /// [`BusyCause::Sqlite`].
    pub async fn begin_write(&self) -> Result<WriteTransaction, Error> {
        self.writer
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))
    }

    /// `SELECT COUNT(*) FROM book` on the read pool: the `/health` query (SPEC 5.2,
    /// decision 55). Fails before [`Storage::migrate`] has created the table; a read-queue
    /// wait that expires is [`Error::Busy`] with [`BusyCause::ReadQueue`].
    pub async fn count_books(&self) -> Result<i64, Error> {
        sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM book"#)
            .fetch_one(&self.reader)
            .await
            .map_err(|err| classify(err, BusyCause::ReadQueue))
    }

    /// Closes the read pool first and the writer last, so the last connection to close is
    /// read-write and can checkpoint and remove the `-wal` and `-shm` files (decisions 53
    /// and 58).
    ///
    /// The writer connection is held while the readers close: a read-only connection that
    /// happened to close last could not checkpoint and would leave the side files behind.
    /// When it cannot be acquired (a write still in flight, or the pool already closed) the
    /// pools still close in order.
    pub async fn close(&self) {
        let held = self.writer.acquire().await.ok();
        self.reader.close().await;
        drop(held);
        self.writer.close().await;
    }

    /// The read pool, for read-only queries and for tests inspecting PRAGMAs. There is no
    /// writer accessor: [`Storage::begin_write`] is the only way to write (decision 53).
    pub fn reader(&self) -> &SqlitePool {
        &self.reader
    }
}

/// What [`Storage::migrate`] did (decision 54).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// The number of migrations this call applied.
    pub applied: usize,
    /// The latest applied migration version afterwards; `None` on a database with none.
    pub schema_version: Option<i64>,
}

/// A storage failure (decision 53). `Busy` is the one a request answers with 503 and
/// `Retry-After`; everything else is a 500 after an `error` log.
#[derive(Debug)]
pub enum Error {
    /// The database is in use: a queue wait expired or SQLite answered SQLITE_BUSY after its
    /// own timeout (SPEC 5.4, decision 53).
    Busy(BusyCause),
    /// The database carries a migration this binary does not know: a newer Nage wrote it
    /// (SPEC 5.5, decision 54).
    NewerDatabase { version: i64 },
    /// An applied migration's checksum differs from the embedded file (SPEC 5.5).
    ModifiedMigration { version: i64 },
    /// Any other migration failure.
    Migrate(MigrateError),
    /// Any other sqlx failure.
    Sqlx(sqlx::Error),
}

impl Error {
    /// Whether this is [`Error::Busy`], whatever its cause (decision 53).
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Busy(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(cause) => write!(f, "the database is busy: {cause}"),
            Self::NewerDatabase { version } => write!(
                f,
                "the database was written by a newer Nage: it carries migration {version}, which this version does not know"
            ),
            Self::ModifiedMigration { version } => write!(
                f,
                "migration {version} was applied from a file that differs from the one this version carries"
            ),
            Self::Migrate(source) => write!(f, "cannot migrate: {source}"),
            Self::Sqlx(source) => write!(f, "database error: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Migrate(source) => Some(source),
            Self::Sqlx(source) => Some(source),
            Self::Busy(_) | Self::NewerDatabase { .. } | Self::ModifiedMigration { .. } => None,
        }
    }
}

/// Why the database was busy (decision 53): which wait expired, or the extended SQLite
/// result code when SQLite itself answered busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyCause {
    /// The wait for the writer connection expired (30 seconds by default).
    WriteQueue,
    /// The wait for a reader connection expired (5 seconds by default).
    ReadQueue,
    /// SQLite answered SQLITE_BUSY (primary code 5) after its own busy timeout; `code` is
    /// the extended result code.
    Sqlite { code: i32 },
}

impl fmt::Display for BusyCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteQueue => f.write_str("the write queue timed out"),
            Self::ReadQueue => f.write_str("the read queue timed out"),
            Self::Sqlite { code } => write!(f, "SQLite answered busy (code {code})"),
        }
    }
}

/// Maps a sqlx error to [`Error`] (decision 53): a pool acquire timeout to `Busy(queue)`, a
/// database error whose primary result code (extended code & 0xFF) is 5 to
/// `Busy(BusyCause::Sqlite { code })`, anything else to `Sqlx`.
fn classify(err: sqlx::Error, queue: BusyCause) -> Error {
    if matches!(err, sqlx::Error::PoolTimedOut) {
        return Error::Busy(queue);
    }
    match busy_code(&err) {
        Some(code) => Error::Busy(BusyCause::Sqlite { code }),
        None => Error::Sqlx(err),
    }
}

/// The extended result code of a database error whose primary code is SQLITE_BUSY; `None`
/// for any other error. sqlx reports SQLite's extended code as a decimal string.
fn busy_code(err: &sqlx::Error) -> Option<i32> {
    let code: i32 = err.as_database_error()?.code()?.parse().ok()?;
    ((code & 0xFF) == SQLITE_BUSY).then_some(code)
}

/// Maps a migrator failure to [`Error`] (SPEC 5.5, decision 54): an applied version this
/// binary does not carry means a newer Nage wrote the file; a checksum that differs means
/// the migration file changed after it was applied.
fn migrate_error(err: MigrateError) -> Error {
    match err {
        MigrateError::VersionMissing(version) => Error::NewerDatabase { version },
        MigrateError::VersionMismatch(version) => Error::ModifiedMigration { version },
        other => Error::Migrate(other),
    }
}
