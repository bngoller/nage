//! The committed fixture `fixtures/books/v0.1.0.sqlite` (PLAN 1.3 item 5, decision 58): a
//! freshly migrated, empty book produced by `cargo xtask seed`, opened only as a copy.

mod common;

use std::fs;
use std::path::Path;

use nage_core::storage::{MigrationReport, Storage, StorageOptions};

use common::{db_path, temp_dir};

/// The committed fixture, relative to this crate's manifest directory.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/books/v0.1.0.sqlite"
);

/// A copy of the committed fixture is at the binary's schema: `migrate` applies nothing and
/// reports schema version 1, and `count_books` is zero.
///
/// Cites: PLAN 1.3 item 5, decision 58.
#[tokio::test]
async fn committed_fixture_is_at_the_current_schema() {
    let fixture = Path::new(FIXTURE);
    let dir = temp_dir();
    let path = db_path(&dir);
    fs::copy(fixture, &path).unwrap_or_else(|error| {
        panic!(
            "cannot copy the fixture {} to {}: {error}",
            fixture.display(),
            path.display()
        )
    });

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

    storage.close().await;
}
