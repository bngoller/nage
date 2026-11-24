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

/// A read transaction on a read-only connection of the read pool, begun with a deferred `BEGIN`:
/// one snapshot of the database for a run of queries that must agree with each other, such as
/// `nage check` (decision 97). Dropping it ends it.
pub type ReadTransaction = Transaction<'static, Sqlite>;

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

    /// Applies the pending migrations and reports how many ran and the schema version afterwards
    /// (SPEC 5.5, decisions 54 and 155). A database carrying a migration this binary does not
    /// know is [`Error::NewerDatabase`]; a migration whose file changed since it was applied is
    /// [`Error::ModifiedMigration`], also when nothing waits.
    ///
    /// The update is all or nothing (decision 155, ruling P2 of PLAN M2 slice 8's pause): every
    /// pending migration runs inside one outer `BEGIN IMMEDIATE` begun through
    /// [`Storage::begin_write`], sqlx's migrator nesting each migration in a savepoint, so a
    /// failure rolls every one of them and their bookkeeping rows back, the bookkeeping table
    /// included when this call created it. A later migration therefore cannot switch
    /// `PRAGMA foreign_keys`, which has no effect inside a transaction. This is the one update
    /// path: `update::open_and_update` (for `serve` and `nage migrate`), `nage init`, `cargo xtask
    /// seed` and `test_support::migrate` all go through it.
    pub async fn migrate(&self) -> Result<MigrationReport, Error> {
        let mut tx = self.begin_write().await?;
        let before = match read_bookkeeping(&mut tx).await {
            Ok(Bookkeeping::Applied(versions)) => versions,
            Ok(Bookkeeping::Empty | Bookkeeping::Foreign) => Vec::new(),
            Err(error) => {
                drop(tx.rollback().await);
                return Err(classify(error, BusyCause::WriteQueue));
            }
        };
        // The migrator runs on the transaction's connection: sqlx sees the outer transaction and
        // begins each migration as a savepoint inside it, so a failure part way rolls back with
        // the transaction below, the bookkeeping table and its rows included.
        if let Err(error) = MIGRATOR.run(&mut *tx).await {
            drop(tx.rollback().await);
            return Err(migrate_error(error));
        }
        let after = match read_bookkeeping(&mut tx).await {
            Ok(Bookkeeping::Applied(versions)) => versions,
            Ok(Bookkeeping::Empty | Bookkeeping::Foreign) => Vec::new(),
            Err(error) => {
                drop(tx.rollback().await);
                return Err(classify(error, BusyCause::WriteQueue));
            }
        };
        tx.commit()
            .await
            .map_err(|error| classify(error, BusyCause::WriteQueue))?;
        Ok(MigrationReport {
            applied: after
                .iter()
                .filter(|version| !before.contains(version))
                .count(),
            schema_version: after.iter().copied().max(),
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

    /// A deferred `BEGIN` on a reader connection: one snapshot for a run of read queries that
    /// must agree with each other (decision 97). A read-queue wait that expires is
    /// [`Error::Busy`] with [`BusyCause::ReadQueue`].
    pub async fn begin_read(&self) -> Result<ReadTransaction, Error> {
        self.reader
            .begin()
            .await
            .map_err(|err| classify(err, BusyCause::ReadQueue))
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

    /// The schema version the database is at: the largest applied migration version in sqlx's
    /// bookkeeping table, read on a reader connection, or `None` when none is recorded
    /// (decision 97). A database without the table is [`Error::Migrate`] as sqlx reports it.
    pub async fn schema_version(&self) -> Result<Option<i64>, Error> {
        let mut conn = self
            .reader
            .acquire()
            .await
            .map_err(|err| classify(err, BusyCause::ReadQueue))?;
        let applied = conn
            .list_applied_migrations(MIGRATOR.table_name.as_ref())
            .await
            .map_err(migrate_error)?;
        Ok(applied.iter().map(|migration| migration.version).max())
    }

    /// What sqlx's bookkeeping table says, asked by a checked query on `sqlite_schema` and never
    /// through sqlx's `ensure_migrations_table`, which would create the table (decisions 151 and
    /// 155): [`Bookkeeping::Empty`] for a file without any table, [`Bookkeeping::Foreign`] for one
    /// with tables but no bookkeeping table, otherwise the applied migrations by version, in order
    /// (empty for a bookkeeping table without rows). Read on the writer connection, so it sees
    /// what the writer last wrote.
    pub async fn bookkeeping(&self) -> Result<Bookkeeping, Error> {
        let mut conn = self
            .writer
            .acquire()
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))?;
        read_bookkeeping(&mut conn)
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))
    }

    /// Writes a copy of the whole database to `path` with `VACUUM INTO`, on the writer connection
    /// outside any transaction (SPEC 5.5, decision 151). The path is bound as a parameter of the
    /// statement, never assembled into it, so a data folder whose name holds an apostrophe changes
    /// nothing (ruling P3). `path` is the empty file the caller has just created, new and private
    /// (`private::create_new_file`): SQLite writes into an empty file, so Nage creates it itself
    /// and nothing that was there before can be written over.
    pub async fn back_up_into(&self, path: &Path) -> Result<(), Error> {
        let target = path.to_str().ok_or_else(|| {
            Error::Sqlx(sqlx::Error::Protocol(format!(
                "the backup's path {} is not UTF-8 text",
                path.display()
            )))
        })?;
        // Unchecked SQL as decision 57 allows for VACUUM INTO: a literal statement, the path a
        // bound parameter (ruling P3), on the writer pool's one connection in autocommit mode.
        sqlx::query("VACUUM INTO ?1")
            .bind(target)
            .execute(&self.writer)
            .await
            .map_err(|err| classify(err, BusyCause::WriteQueue))?;
        Ok(())
    }

    /// The newest migration this binary carries, the schema version a current database is at
    /// (decision 97): 2 today.
    pub fn latest_schema_version() -> i64 {
        MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap_or(0)
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

/// What sqlx's bookkeeping table says of a database (decisions 151 and 155).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bookkeeping {
    /// The file holds no table at all: a new database, or one a cut-off `nage init` left.
    Empty,
    /// The file holds tables but no bookkeeping table: not a Nage database.
    Foreign,
    /// The versions of the applied migrations, in order; empty for a table without rows.
    Applied(Vec<i64>),
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

/// What sqlx's bookkeeping table says on `conn` (decisions 151 and 155), asked by checked queries
/// and never through sqlx's `ensure_migrations_table`, which would create the table.
pub(crate) async fn read_bookkeeping(
    conn: &mut sqlx::SqliteConnection,
) -> Result<Bookkeeping, sqlx::Error> {
    let tables = sqlx::query_scalar!(
        r#"SELECT name AS "name!: String" FROM sqlite_schema WHERE type = 'table'"#
    )
    .fetch_all(&mut *conn)
    .await?;
    if tables.is_empty() {
        return Ok(Bookkeeping::Empty);
    }
    if !tables
        .iter()
        .any(|table| table == MIGRATOR.table_name.as_ref())
    {
        return Ok(Bookkeeping::Foreign);
    }
    let versions = sqlx::query_scalar!(
        r#"SELECT version AS "version!: i64" FROM _sqlx_migrations ORDER BY version"#
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(Bookkeeping::Applied(versions))
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt;

    use proptest::prelude::*;
    use sqlx::error::{DatabaseError, ErrorKind};

    use super::{BusyCause, Error, busy_code, classify};

    /// A database error that reports `code` as its extended SQLite result code, the way
    /// sqlx's SQLite driver does: a decimal string.
    #[derive(Debug)]
    struct FakeDatabaseError {
        code: i32,
    }

    impl fmt::Display for FakeDatabaseError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "fake database error with code {}", self.code)
        }
    }

    impl std::error::Error for FakeDatabaseError {}

    impl DatabaseError for FakeDatabaseError {
        fn message(&self) -> &str {
            "fake database error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Owned(self.code.to_string()))
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    /// A sqlx database error carrying the extended result code `code`.
    fn fake(code: i32) -> sqlx::Error {
        sqlx::Error::Database(Box::new(FakeDatabaseError { code }))
    }

    /// The enumerated result codes, stated as literals rather than through the mirrored
    /// oracle of the property below: SQLITE_BUSY (5) and its extended forms 261, 517 and
    /// 773 are busy, and `classify` maps them to `Busy(Sqlite { code })`; the neighbouring
    /// primary codes 4, 6, 8 and 19 are not, and `classify` maps them to `Sqlx`.
    ///
    /// Proves: decision 53.
    #[test]
    fn enumerated_codes_classify_as_stated() {
        for (code, expected) in [
            (5, true),
            (261, true),
            (517, true),
            (773, true),
            (4, false),
            (6, false),
            (8, false),
            (19, false),
        ] {
            assert_eq!(
                busy_code(&fake(code)).is_some(),
                expected,
                "busy_code for extended code {code}"
            );
            let classified = classify(fake(code), BusyCause::WriteQueue);
            let as_stated = if expected {
                matches!(classified, Error::Busy(BusyCause::Sqlite { code: reported }) if reported == code)
            } else {
                matches!(classified, Error::Sqlx(_))
            };
            assert!(
                as_stated,
                "extended code {code} (busy: {expected}) classified as {classified:?}"
            );
        }
    }

    proptest! {
        #![proptest_config(crate::test_support::proptest_config(file!()))]

        /// Whether a database error is busy depends only on its primary result code (the low
        /// byte of the extended code): `busy_code` answers the extended code exactly when the
        /// primary code is SQLITE_BUSY (5), and `classify` maps such an error to
        /// `Busy(Sqlite { code })` and every other code to `Sqlx`, over the enumerated
        /// extended codes (5, `SQLITE_BUSY_RECOVERY` 261, `SQLITE_BUSY_SNAPSHOT` 517,
        /// `SQLITE_BUSY_TIMEOUT` 773), neighbouring primary codes and arbitrary integers. A
        /// pool acquire timeout is `Busy` with the queue the caller names. The oracle mirrors
        /// the implementation; the value is the enumerated extended codes and the
        /// `PoolTimedOut` branch.
        ///
        /// Proves: decision 53.
        #[test]
        fn busy_classification_depends_only_on_the_primary_code(
            code in prop_oneof![
                Just(5),
                Just(261),
                Just(517),
                Just(773),
                Just(4),
                Just(6),
                Just(8),
                Just(19),
                any::<i32>(),
            ]
        ) {
            let busy = (code & 0xFF) == 5;
            prop_assert_eq!(busy_code(&fake(code)), busy.then_some(code));

            let classified = classify(fake(code), BusyCause::WriteQueue);
            if busy {
                prop_assert!(
                    matches!(classified, Error::Busy(BusyCause::Sqlite { code: reported }) if reported == code),
                    "code {} classified as {:?}, not Busy(Sqlite {{ code: {} }})",
                    code,
                    classified,
                    code
                );
            } else {
                prop_assert!(
                    matches!(classified, Error::Sqlx(_)),
                    "code {} classified as {:?}, not Sqlx",
                    code,
                    classified
                );
            }

            let timed_out = classify(sqlx::Error::PoolTimedOut, BusyCause::ReadQueue);
            prop_assert!(
                matches!(timed_out, Error::Busy(BusyCause::ReadQueue)),
                "PoolTimedOut classified as {:?}, not Busy(ReadQueue)",
                timed_out
            );
            let timed_out = classify(sqlx::Error::PoolTimedOut, BusyCause::WriteQueue);
            prop_assert!(
                matches!(timed_out, Error::Busy(BusyCause::WriteQueue)),
                "PoolTimedOut classified as {:?}, not Busy(WriteQueue)",
                timed_out
            );
        }
    }
}
