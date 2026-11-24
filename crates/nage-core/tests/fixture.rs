//! The committed books under `fixtures/books` (PLAN 1.3 item 5, decisions 58, 94 and 161), each
//! opened only as a copy: `v0.1.0.sqlite`, milestone 1's empty book at schema version 1, frozen
//! with `0001_init.sql`, migrates to the current schema; and every committed book, `v0.2.0.sqlite`
//! included, is brought up to date through the one update operation, backed up exactly when a
//! change waits, and passes the check or holds no book: the migration test of SPEC 10.1.

use std::fs;
use std::path::Path;

use nage_core::storage::{MigrationReport, Storage, StorageOptions};
use nage_core::test_support::{assert_no_side_files, fixture_copy, open_migrated};
use sqlx::Row as _;

/// The committed fixture, relative to this crate's manifest directory.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/books/v0.1.0.sqlite"
);

/// Every row of `sqlite_schema` as `type name table: sql`, in type and name order, read
/// through the read pool of `storage`.
async fn schema(storage: &Storage) -> Vec<String> {
    sqlx::query(
        "SELECT type || ' ' || name || ' ' || tbl_name || ': ' || coalesce(sql, '') \
         FROM sqlite_schema ORDER BY type, name",
    )
    .fetch_all(storage.reader())
    .await
    .unwrap_or_else(|error| panic!("cannot read sqlite_schema: {error}"))
    .iter()
    .map(|row| {
        row.try_get_unchecked::<String, _>(0)
            .unwrap_or_else(|error| panic!("a sqlite_schema row: {error}"))
    })
    .collect()
}

/// The committed books, relative to this crate's manifest directory.
const BOOKS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/books");

/// The versions of every committed book under `fixtures/books`, from their names
/// `v<version>.sqlite`, in name order.
fn committed_books() -> Vec<String> {
    let mut versions: Vec<String> = fs::read_dir(BOOKS)
        .unwrap_or_else(|error| panic!("cannot list the books in {BOOKS}: {error}"))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("cannot read an entry of {BOOKS}: {error}"))
                .file_name()
                .into_string()
                .unwrap_or_else(|name| panic!("a book's name is not UTF-8: {name:?}"))
        })
        .filter_map(|name| {
            name.strip_prefix('v')
                .and_then(|rest| rest.strip_suffix(".sqlite"))
                .map(str::to_owned)
        })
        .collect();
    versions.sort();
    versions
}

/// The changes the bookkeeping of the database at `path` records, in order, read on a read-only,
/// immutable connection of the test's own, so that nothing is written beside the file (migration
/// bookkeeping read with unchecked SQL, as decision 57 allows).
async fn recorded_changes(path: &Path) -> Vec<i64> {
    use sqlx::Connection as _;

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .immutable(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {} read-only: {error}", path.display()));
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut conn)
            .await
            .unwrap_or_else(|error| {
                panic!("cannot read the bookkeeping of {}: {error}", path.display())
            });
    conn.close()
        .await
        .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
    versions
}

/// Every committed book under `fixtures/books` (the v0.1.0 and v0.2.0 books among them, and any
/// later one by itself) is brought up to date through the one update operation, on a copy in a
/// data folder of its own: a pre-migration backup is written into `backups/` exactly when a change
/// waits (v0.1.0's change 0002), and none when the book is up to date (v0.2.0); the changes
/// applied are exactly those the book did not record; the schema ends equal to a freshly migrated
/// database's, row for row; and the check passes, or finds no book set up where the book holds
/// none (v0.1.0). The committed files keep their bytes and get no side file.
///
/// Proves: SPEC 10.1 Migration row, 10.3 Migrations row, decisions 94 and 161 (each committed book migrates, is backed up exactly when a change waits and passes check)
#[tokio::test]
async fn every_committed_book_updates_backs_up_and_passes_the_check() {
    let books = committed_books();
    for version in ["0.1.0", "0.2.0"] {
        assert!(
            books.iter().any(|book| book == version),
            "the book v{version} is not among the committed books {books:?}"
        );
    }

    let (_fresh_dir, fresh) = open_migrated().await;
    let expected = schema(&fresh).await;
    fresh.close().await;
    let latest = Storage::latest_schema_version();
    let now = nage_core::timestamp::Timestamp::from_second(1_794_225_600)
        .unwrap_or_else(|error| panic!("the test instant: {error}"));

    for version in &books {
        let committed = Path::new(BOOKS).join(format!("v{version}.sqlite"));
        let bytes = fs::read(&committed)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", committed.display()));
        let (dir, path) = fixture_copy(version);
        let recorded = recorded_changes(&path).await;
        let waits = recorded.iter().copied().max() != Some(latest);

        let updated = nage_core::update::open_and_update(nage_core::update::Update {
            data_dir: dir.path(),
            now,
            version: env!("CARGO_PKG_VERSION"),
            missing: nage_core::update::Missing::Refuse,
            holder: nage_core::update::Holder::Migrate,
            options: &StorageOptions::default(),
        })
        .await
        .unwrap_or_else(|error| panic!("the book v{version} was not brought up to date: {error}"));
        let report = &updated.report;
        assert_eq!(
            report.backup.is_some(),
            waits,
            "a backup of the book v{version} (changes recorded {recorded:?}, latest {latest}): {report:?}"
        );
        if let Some(backup) = &report.backup {
            assert!(
                backup.starts_with(dir.path().join("backups")) && backup.is_file(),
                "the backup of the book v{version} is not a file in its backups folder: {}",
                backup.display()
            );
            let step = recorded.iter().copied().max().unwrap_or_default();
            nage_core::update::verify_backup(backup, step)
                .await
                .unwrap_or_else(|reason| {
                    panic!("the backup of the book v{version} does not pass: {reason}")
                });
        } else {
            let backups = dir.path().join("backups");
            let written = fs::read_dir(&backups)
                .map(|entries| entries.count())
                .unwrap_or(0);
            assert_eq!(
                written, 0,
                "files in the backups folder of the book v{version}"
            );
        }
        let waiting: Vec<i64> = (1..=latest)
            .filter(|change| !recorded.contains(change))
            .collect();
        assert_eq!(
            report.applied, waiting,
            "the changes applied to the book v{version}"
        );
        assert_eq!(
            report.schema_version,
            Some(latest),
            "the schema version of the book v{version} afterwards"
        );
        assert_eq!(
            schema(&updated.storage).await,
            expected,
            "the sqlite_schema rows of the book v{version} against a fresh database's"
        );

        let books_held = updated
            .storage
            .count_books()
            .await
            .unwrap_or_else(|error| panic!("count_books on the book v{version}: {error}"));
        let mut tx =
            updated.storage.begin_read().await.unwrap_or_else(|error| {
                panic!("cannot begin a read on the book v{version}: {error}")
            });
        let check = nage_core::check::run(&mut tx)
            .await
            .unwrap_or_else(|error| panic!("the check of the book v{version} failed: {error}"));
        tx.rollback()
            .await
            .unwrap_or_else(|error| panic!("cannot end the read on the book v{version}: {error}"));
        if books_held == 0 {
            assert_eq!(
                check.outcome,
                nage_core::check::Outcome::NotInitialized,
                "the check of the book v{version}, which holds no book row"
            );
        } else {
            assert_eq!(
                check.outcome,
                nage_core::check::Outcome::Checked {
                    violations: Vec::new()
                },
                "the check of the book v{version}"
            );
        }

        let nage_core::update::Updated { storage, mark, .. } = updated;
        storage.close().await;
        drop(mark);
        assert_no_side_files(
            &committed,
            "beside a committed book after its copy was updated",
        );
        let after = fs::read(&committed)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", committed.display()));
        assert!(
            after == bytes,
            "the committed book {} changed while its copy was used",
            committed.display()
        );
    }
}

/// A copy of the committed fixture, made by `fixture_copy("0.1.0")`, is one migration behind
/// the binary: `migrate` applies exactly one and reports schema version 2, the book still has
/// no row, and the copy's `sqlite_schema` then equals a freshly migrated database's row for
/// row (the same tables, indexes and triggers with the same SQL, sqlx's bookkeeping table and
/// `sqlite_sequence` included). The copy lies inside the returned directory, not at the
/// committed path; while it is open its own `-wal` file exists and none exists beside the
/// committed file (a helper that handed out the committed path would create them there);
/// afterwards the committed file's bytes are unchanged.
///
/// Proves: SPEC 10.1 (the Migration row), decisions 58, 59 and 94.
#[tokio::test]
async fn committed_fixture_migrates_to_the_current_schema() {
    let fixture = fs::canonicalize(FIXTURE)
        .unwrap_or_else(|error| panic!("cannot resolve the fixture {FIXTURE}: {error}"));
    let committed = fs::read(&fixture)
        .unwrap_or_else(|error| panic!("cannot read the fixture {}: {error}", fixture.display()));

    let (_fresh_dir, fresh) = open_migrated().await;
    let expected = schema(&fresh).await;
    fresh.close().await;

    let (dir, path) = fixture_copy("0.1.0");
    assert!(
        path.starts_with(dir.path()),
        "the fixture copy {} lies outside its directory {}",
        path.display(),
        dir.path().display()
    );
    assert_ne!(
        path, fixture,
        "fixture_copy returned the committed fixture's path"
    );
    assert!(
        path.is_file(),
        "the fixture copy {} is not a file",
        path.display()
    );

    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open the fixture copy {}: {error}", path.display()));
    let report = storage
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("cannot migrate the fixture copy: {error}"));
    assert_eq!(
        report,
        MigrationReport {
            applied: 1,
            schema_version: Some(2),
        },
        "migrate on a copy of the committed fixture"
    );
    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books on the fixture copy: {error}"));
    assert_eq!(count, 0, "book rows in the committed fixture");
    let migrated = schema(&storage).await;
    assert_eq!(
        migrated, expected,
        "the sqlite_schema rows of the migrated fixture copy against a fresh database's"
    );

    let mut copy_wal = path.clone().into_os_string();
    copy_wal.push("-wal");
    assert!(
        Path::new(&copy_wal).exists(),
        "the fixture copy has no -wal file while it is open"
    );
    assert_no_side_files(
        &fixture,
        "beside the committed fixture while its copy is open",
    );

    storage.close().await;

    assert_no_side_files(
        &fixture,
        "beside the committed fixture after its copy closed",
    );
    let after = fs::read(&fixture)
        .unwrap_or_else(|error| panic!("cannot read the fixture {}: {error}", fixture.display()));
    assert!(
        after == committed,
        "the committed fixture {} changed while its copy was used",
        fixture.display()
    );
}
