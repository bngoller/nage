//! The storage layer of SPEC 5.4 and 5.5: the two pools, their PRAGMAs, the write lock, the
//! busy classification, the migrations and the clean close (decisions 53, 54, 55 and 58).

mod common;

use std::time::{Duration, Instant};

use nage_core::storage::{BusyCause, Error, MigrationReport, Storage, StorageOptions};
use nage_core::test_support::{
    DB_FILE_NAME, assert_no_side_files, migrate, open_fresh, open_migrated, open_with,
};

use common::{SHORT_WAIT, SOON, pragma_int, pragma_text, primary_code, sqlite_code, write};

/// `SQLITE_BUSY`, the primary result code SQLite answers when it cannot take a lock within
/// the busy timeout.
const SQLITE_BUSY: i32 = 5;

/// `SQLITE_READONLY`, the primary result code of a write on a read-only connection.
const SQLITE_READONLY: i32 = 8;

/// Opening a missing path creates the file and puts it in WAL mode: `PRAGMA journal_mode`
/// answers `wal` inside a write transaction and through the read pool.
///
/// Proves: SPEC 5.4, decision 53.
#[tokio::test]
async fn open_creates_the_file_in_wal_mode() {
    let (dir, storage) = open_fresh().await;
    let path = dir.db_path();
    assert!(
        path.is_file(),
        "Storage::open did not create {}",
        path.display()
    );

    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction: {error}"));
    let journal_mode = pragma_text(&mut *tx, "journal_mode").await;
    assert_eq!(
        journal_mode, "wal",
        "PRAGMA journal_mode inside a write transaction"
    );
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back: {error}"));

    let journal_mode = pragma_text(storage.reader(), "journal_mode").await;
    assert_eq!(
        journal_mode, "wal",
        "PRAGMA journal_mode through the reader"
    );

    storage.close().await;
}

/// Inside a write transaction `synchronous` is NORMAL (1), `foreign_keys` is on and the busy
/// timeout is the configured one: 5000 ms by default and 1234 ms when `StorageOptions` says
/// so (the non-default value proves Nage sets it rather than inheriting sqlx's default).
///
/// Proves: SPEC 5.4, decision 53.
#[tokio::test]
async fn writer_connections_carry_the_pragmas() {
    let (_dir, storage) = open_fresh().await;
    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction: {error}"));
    assert_eq!(
        pragma_int(&mut *tx, "synchronous").await,
        1,
        "PRAGMA synchronous on the writer (1 is NORMAL)"
    );
    assert_eq!(
        pragma_int(&mut *tx, "foreign_keys").await,
        1,
        "PRAGMA foreign_keys on the writer"
    );
    assert_eq!(
        pragma_int(&mut *tx, "busy_timeout").await,
        5000,
        "PRAGMA busy_timeout on the writer with the default options"
    );
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back: {error}"));
    storage.close().await;

    let options = StorageOptions {
        busy_timeout: Duration::from_millis(1234),
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    let mut tx = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin a write transaction: {error}"));
    assert_eq!(
        pragma_int(&mut *tx, "busy_timeout").await,
        1234,
        "PRAGMA busy_timeout on the writer with busy_timeout = 1234 ms"
    );
    tx.rollback()
        .await
        .unwrap_or_else(|error| panic!("cannot roll back: {error}"));
    storage.close().await;
}

/// On a migrated database the readers carry `foreign_keys` on (also sqlx's default) and the
/// configured busy timeout: 5000 ms by default and 1234 ms when `StorageOptions` says so (the
/// non-default value proves Nage sets it on the readers rather than inheriting sqlx's
/// default). An INSERT into `book` through the read pool fails with SQLITE_READONLY, not with
/// "no such table".
///
/// Proves: decision 53.
#[tokio::test]
async fn reader_connections_are_read_only_with_the_pragmas() {
    let (_dir, storage) = open_migrated().await;
    assert_eq!(
        pragma_int(storage.reader(), "busy_timeout").await,
        5000,
        "PRAGMA busy_timeout on a reader with the default options"
    );
    storage.close().await;

    let options = StorageOptions {
        busy_timeout: Duration::from_millis(1234),
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    migrate(&storage).await;
    assert_eq!(
        pragma_int(storage.reader(), "foreign_keys").await,
        1,
        "PRAGMA foreign_keys on a reader"
    );
    assert_eq!(
        pragma_int(storage.reader(), "busy_timeout").await,
        1234,
        "PRAGMA busy_timeout on a reader with busy_timeout = 1234 ms"
    );

    let result = sqlx::query(
        "INSERT INTO book (id, name, currency, minor_digits, created_at) \
         VALUES (1, 'Home', 'EUR', 2, '2026-10-28T12:00:00Z')",
    )
    .execute(storage.reader())
    .await;
    let error = match result {
        Ok(done) => panic!(
            "an INSERT through the reader succeeded ({} rows affected)",
            done.rows_affected()
        ),
        Err(error) => error,
    };
    let code = sqlite_code(&error).unwrap_or_else(|| {
        panic!("an INSERT through the reader failed without a SQLite code: {error}")
    });
    assert_eq!(
        primary_code(code),
        SQLITE_READONLY,
        "primary result code of an INSERT through the reader (extended {code}): {error}"
    );

    storage.close().await;
}

/// The writer pool holds exactly one connection: while one write transaction is open, a
/// second `begin_write` on the same `Storage` with a 100 ms write queue is `Busy` with the
/// write-queue cause and returns well under a second (a second connection would instead sit
/// in SQLite's five-second busy timeout).
///
/// Proves: SPEC 5.4, decision 53.
#[tokio::test]
async fn writer_pool_has_exactly_one_connection() {
    let options = StorageOptions {
        write_queue: SHORT_WAIT,
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    let first = storage
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin the first write transaction: {error}"));

    let started = Instant::now();
    let second = storage.begin_write().await;
    let elapsed = started.elapsed();
    match second {
        Err(Error::Busy(cause)) => assert_eq!(
            cause,
            BusyCause::WriteQueue,
            "cause of the second begin_write while the first is open"
        ),
        Err(other) => panic!("the second begin_write failed with {other:?}, not Busy"),
        Ok(_) => panic!("the second begin_write succeeded while the first is open"),
    }
    assert!(
        elapsed < SOON,
        "the second begin_write took {elapsed:?} to expire a {SHORT_WAIT:?} write queue"
    );

    drop(first);
    storage.close().await;
}

/// `begin_write` takes the write lock at `BEGIN`: with two `Storage` handles on one file,
/// each with a 100 ms SQLite busy timeout, a write transaction held open on A without
/// writing makes B's `begin_write` fail with `Busy` and a SQLite cause whose primary code is
/// SQLITE_BUSY (a deferred transaction would have begun and failed only at the first write).
///
/// Proves: SPEC 5.4, decision 53.
#[tokio::test]
async fn begin_write_takes_the_lock_at_begin() {
    let options = StorageOptions {
        busy_timeout: SHORT_WAIT,
        ..StorageOptions::default()
    };
    let (dir, a) = open_with(&options).await;
    let path = dir.db_path();
    let b = Storage::open(&path, &options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {} a second time: {error}", path.display()));

    let held = a
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin the write transaction on A: {error}"));

    let started = Instant::now();
    let blocked = b.begin_write().await;
    let elapsed = started.elapsed();
    match blocked {
        Err(Error::Busy(BusyCause::Sqlite { code })) => assert_eq!(
            primary_code(code),
            SQLITE_BUSY,
            "primary result code of B's begin_write while A holds the lock (extended {code})"
        ),
        Err(Error::Busy(cause)) => {
            panic!("B's begin_write was busy for {cause:?}, not for a SQLite code")
        }
        Err(other) => panic!("B's begin_write failed with {other:?}, not Busy"),
        Ok(_) => panic!("B's begin_write succeeded while A holds the write lock"),
    }
    assert!(
        elapsed < Duration::from_secs(2),
        "B's begin_write took {elapsed:?} to give up with a {SHORT_WAIT:?} busy timeout"
    );

    drop(held);
    b.close().await;
    a.close().await;
}

/// Both kinds of busy error, an expired write queue and SQLite answering busy, report
/// `is_busy()` and name their cause in the message: the queue by name, SQLite by its result
/// code. A plain sqlx error and a migration refusal are not busy.
///
/// Proves: decision 53.
#[tokio::test]
async fn busy_errors_report_their_cause() {
    let options = StorageOptions {
        busy_timeout: SHORT_WAIT,
        write_queue: SHORT_WAIT,
        ..StorageOptions::default()
    };
    let (dir, a) = open_with(&options).await;
    let path = dir.db_path();
    let b = Storage::open(&path, &options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {} a second time: {error}", path.display()));
    let held = a
        .begin_write()
        .await
        .unwrap_or_else(|error| panic!("cannot begin the write transaction on A: {error}"));

    let queue_error = match a.begin_write().await {
        Err(error) => error,
        Ok(_) => panic!("a second begin_write on A succeeded while the first is open"),
    };
    assert!(
        queue_error.is_busy(),
        "an expired write queue is not busy: {queue_error:?}"
    );
    let text = queue_error.to_string();
    assert!(
        text.contains("busy") && text.contains("write queue"),
        "the write-queue message names neither busy nor the queue: {text}"
    );

    let sqlite_error = match b.begin_write().await {
        Err(error) => error,
        Ok(_) => panic!("B's begin_write succeeded while A holds the write lock"),
    };
    assert!(
        sqlite_error.is_busy(),
        "SQLite answering busy is not busy: {sqlite_error:?}"
    );
    let code = match &sqlite_error {
        Error::Busy(BusyCause::Sqlite { code }) => *code,
        other => panic!("B's begin_write failed with {other:?}, not a SQLite busy cause"),
    };
    let text = sqlite_error.to_string();
    assert!(
        text.contains("busy") && text.contains(&code.to_string()),
        "the SQLite busy message names neither busy nor code {code}: {text}"
    );

    let plain = Error::Sqlx(sqlx::Error::RowNotFound);
    assert!(!plain.is_busy(), "a plain sqlx error is busy: {plain:?}");
    let newer = Error::NewerDatabase { version: 2 };
    assert!(
        !newer.is_busy(),
        "a newer-database refusal is busy: {newer:?}"
    );

    drop(held);
    b.close().await;
    a.close().await;
}

/// The first `migrate` on a fresh file applies migration 1 and reports schema version 1; a
/// second applies nothing and reports the same version.
///
/// Proves: SPEC 5.5, decision 54.
#[tokio::test]
async fn migrate_applies_0001_once() {
    let (_dir, storage) = open_fresh().await;
    let first = migrate(&storage).await;
    assert_eq!(
        first,
        MigrationReport {
            applied: 1,
            schema_version: Some(1),
        },
        "the first migrate on a fresh database"
    );
    let second = migrate(&storage).await;
    assert_eq!(
        second,
        MigrationReport {
            applied: 0,
            schema_version: Some(1),
        },
        "the second migrate on the same database"
    );
    storage.close().await;
}

/// A database recording a migration this binary does not carry (version 9999, inserted into
/// sqlx's bookkeeping table) is refused as written by a newer Nage, naming the version.
///
/// Proves: SPEC 5.5, decision 54.
#[tokio::test]
async fn migrate_refuses_a_newer_database() {
    let (_dir, storage) = open_migrated().await;
    write(
        &storage,
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'future', 1, x'00', 0)",
    )
    .await;

    let result = storage.migrate().await;
    match result {
        Err(Error::NewerDatabase { version }) => {
            assert_eq!(version, 9999, "the unknown migration version reported");
        }
        Err(other) => panic!("migrate failed with {other:?}, not NewerDatabase"),
        Ok(report) => panic!("migrate accepted a database with migration 9999: {report:?}"),
    }

    storage.close().await;
}

/// A database whose record of migration 1 carries a different checksum than the embedded
/// file is refused as a modified migration, naming the version.
///
/// Proves: SPEC 5.5.
#[tokio::test]
async fn migrate_refuses_a_modified_migration() {
    let (_dir, storage) = open_migrated().await;
    write(
        &storage,
        "UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 1",
    )
    .await;

    let result = storage.migrate().await;
    match result {
        Err(Error::ModifiedMigration { version }) => {
            assert_eq!(version, 1, "the modified migration version reported");
        }
        Err(other) => panic!("migrate failed with {other:?}, not ModifiedMigration"),
        Ok(report) => panic!("migrate accepted a database with a changed checksum: {report:?}"),
    }

    storage.close().await;
}

/// `count_books` fails with a plain storage error, not a busy one, before the migrations
/// have created `book`, and answers zero on a freshly migrated database.
///
/// Proves: decision 55.
#[tokio::test]
async fn count_books_fails_before_migrating_and_is_zero_after() {
    let (_dir, storage) = open_fresh().await;
    match storage.count_books().await {
        Err(error) => {
            assert!(
                matches!(error, Error::Sqlx(_)),
                "count_books before migrating failed with {error:?}, not a plain sqlx error"
            );
            assert!(
                !error.is_busy(),
                "count_books before migrating reports busy: {error:?}"
            );
        }
        Ok(count) => panic!("count_books answered {count} before the migrations ran"),
    }

    migrate(&storage).await;
    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books after migrating: {error}"));
    assert_eq!(count, 0, "count_books on a freshly migrated database");

    storage.close().await;
}

/// While the storage is open the `-wal` and `-shm` side files exist; after `close` neither
/// does and the database file remains, so a closed database is a single file. A reader has
/// read first, so a read-only connection closing last would leave the files behind: the
/// assertions hold only when the readers close first and the writer last (decision 53's
/// order).
///
/// Proves: decisions 53 and 58.
#[tokio::test]
async fn close_leaves_no_wal_or_shm_files() {
    let (dir, storage) = open_migrated().await;
    let path = dir.db_path();
    let wal = dir.path().join(format!("{DB_FILE_NAME}-wal"));
    let shm = dir.path().join(format!("{DB_FILE_NAME}-shm"));
    assert!(
        wal.exists(),
        "{} does not exist while the storage is open",
        wal.display()
    );
    assert!(
        shm.exists(),
        "{} does not exist while the storage is open",
        shm.display()
    );

    storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books before close: {error}"));
    storage.close().await;

    assert_no_side_files(&path, "after close");
    assert!(path.is_file(), "{} is gone after close", path.display());
}

/// The default options are the recorded ones: a 5 s SQLite busy timeout, a 30 s write
/// queue, a 5 s read queue and four readers.
///
/// Proves: decision 53.
#[test]
fn default_options_match_the_record() {
    let options = StorageOptions::default();
    assert_eq!(
        options.busy_timeout,
        Duration::from_millis(5000),
        "StorageOptions::default().busy_timeout"
    );
    assert_eq!(
        options.write_queue,
        Duration::from_secs(30),
        "StorageOptions::default().write_queue"
    );
    assert_eq!(
        options.read_queue,
        Duration::from_secs(5),
        "StorageOptions::default().read_queue"
    );
    assert_eq!(options.readers, 4, "StorageOptions::default().readers");
}

/// With one reader and a 100 ms read queue, `count_books` while the only reader connection
/// is held is `Busy` with the read-queue cause within a second; once the connection is
/// released it answers zero, so the queue was the cause.
///
/// Proves: decision 53.
#[tokio::test]
async fn read_queue_timeout_is_busy() {
    let options = StorageOptions {
        readers: 1,
        read_queue: SHORT_WAIT,
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    migrate(&storage).await;

    let held = storage
        .reader()
        .acquire()
        .await
        .unwrap_or_else(|error| panic!("cannot acquire the only reader: {error}"));
    let started = Instant::now();
    let result = storage.count_books().await;
    let elapsed = started.elapsed();
    match result {
        Err(Error::Busy(cause)) => assert_eq!(
            cause,
            BusyCause::ReadQueue,
            "cause of count_books while the only reader is held"
        ),
        Err(other) => panic!("count_books while the only reader is held failed with {other:?}"),
        Ok(count) => panic!("count_books answered {count} while the only reader is held"),
    }
    assert!(
        elapsed < SOON,
        "count_books took {elapsed:?} to expire a {SHORT_WAIT:?} read queue"
    );

    drop(held);
    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books after releasing the reader: {error}"));
    assert_eq!(count, 0, "count_books after releasing the reader");

    storage.close().await;
}

/// With the default four readers and a 100 ms read queue, four reader connections are held
/// at once, each acquired within a second; a fifth use through `count_books` is `Busy` with
/// the read-queue cause within a second; once one connection is released `count_books`
/// answers zero, so the pool holds exactly four (one reader fewer fails an acquire, one more
/// lets the fifth use through).
///
/// Proves: decision 53.
#[tokio::test]
async fn read_pool_holds_four_readers() {
    let options = StorageOptions {
        read_queue: SHORT_WAIT,
        ..StorageOptions::default()
    };
    let (_dir, storage) = open_with(&options).await;
    migrate(&storage).await;

    let mut held = Vec::new();
    for index in 1..=4 {
        let started = Instant::now();
        let connection = storage
            .reader()
            .acquire()
            .await
            .unwrap_or_else(|error| panic!("cannot acquire reader {index} of 4: {error}"));
        let elapsed = started.elapsed();
        assert!(
            elapsed < SOON,
            "acquiring reader {index} of 4 took {elapsed:?}"
        );
        held.push(connection);
    }

    let started = Instant::now();
    let result = storage.count_books().await;
    let elapsed = started.elapsed();
    match result {
        Err(Error::Busy(cause)) => assert_eq!(
            cause,
            BusyCause::ReadQueue,
            "cause of count_books while four readers are held"
        ),
        Err(other) => panic!("count_books while four readers are held failed with {other:?}"),
        Ok(count) => panic!("count_books answered {count} while four readers are held"),
    }
    assert!(
        elapsed < SOON,
        "count_books took {elapsed:?} to expire a {SHORT_WAIT:?} read queue"
    );

    drop(held.pop());
    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books after releasing one reader: {error}"));
    assert_eq!(count, 0, "count_books after releasing one reader");

    drop(held);
    storage.close().await;
}
