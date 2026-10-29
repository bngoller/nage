//! The committed fixture `fixtures/books/v0.1.0.sqlite` (PLAN 1.3 item 5, decision 58): a
//! freshly migrated, empty book produced by `cargo xtask seed`, opened only as a copy.

use std::fs;
use std::path::Path;

use nage_core::storage::{MigrationReport, Storage, StorageOptions};
use nage_core::test_support::{assert_no_side_files, fixture_copy};

/// The committed fixture, relative to this crate's manifest directory.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/books/v0.1.0.sqlite"
);

/// A copy of the committed fixture, made by `fixture_copy("0.1.0")`, is at the binary's
/// schema: `migrate` applies nothing and reports schema version 1, and `count_books` is
/// zero. The copy lies inside the returned directory, not at the committed path; while it is
/// open its own `-wal` file exists and none exists beside the committed file (a helper that
/// handed out the committed path would create them there); afterwards the committed file's
/// bytes are unchanged.
///
/// Proves: PLAN 1.3 item 5, decisions 58 and 59.
#[tokio::test]
async fn committed_fixture_is_at_the_current_schema() {
    let fixture = fs::canonicalize(FIXTURE)
        .unwrap_or_else(|error| panic!("cannot resolve the fixture {FIXTURE}: {error}"));
    let committed = fs::read(&fixture)
        .unwrap_or_else(|error| panic!("cannot read the fixture {}: {error}", fixture.display()));

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
            applied: 0,
            schema_version: Some(1),
        },
        "migrate on a copy of the committed fixture"
    );
    let count = storage
        .count_books()
        .await
        .unwrap_or_else(|error| panic!("count_books on the fixture copy: {error}"));
    assert_eq!(count, 0, "book rows in the committed fixture");

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
