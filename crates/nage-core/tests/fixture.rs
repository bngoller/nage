//! The committed fixture `fixtures/books/v0.1.0.sqlite` (PLAN 1.3 item 5, decisions 58 and 94):
//! milestone 1's empty book at schema version 1, frozen with `0001_init.sql`, opened only as a
//! copy, which migrates to the current schema: the migration test of SPEC 10.1.

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
