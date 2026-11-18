//! `database::create`, the one function of the commands that creates a database (decision
//! 113).

use std::fs;

use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files, fixture_copy};

use super::{OpenError, create, open_existing, path};

/// `create` makes the data directory with its `backups/` folder and a database at this
/// binary's schema version, which `open_existing` then opens; closed again, the database
/// leaves no `-wal` or `-shm` file.
///
/// Proves: decision 113 (`init` creates the data directory with `backups/` and the database
/// when they are missing and migrates a new database), decision 53 (a clean close).
#[tokio::test]
async fn create_makes_the_data_directory_and_a_migrated_database() {
    let dir = TestDir::new();
    let data = dir.path().join("books").join("data");
    let storage = create(&data)
        .await
        .unwrap_or_else(|error| panic!("cannot create the database: {error}"));
    assert_eq!(
        storage
            .schema_version()
            .await
            .unwrap_or_else(|error| panic!("cannot read the schema version: {error}")),
        Some(Storage::latest_schema_version()),
        "the schema version of the database create made"
    );
    storage.close().await;
    assert!(
        data.join("backups").is_dir(),
        "create did not make the backups folder"
    );
    assert_no_side_files(&path(&data), "after create and a close");
    let again = open_existing(&data)
        .await
        .unwrap_or_else(|error| panic!("cannot open the database create made: {error}"));
    again.close().await;
}

/// A data directory that cannot be made, here because a file stands where one of its parents
/// should be, fails as the data directory's failure, naming the directory, and leaves the
/// file as it was.
///
/// Proves: decision 113 (`init` creates the data directory; one that cannot be made is the data
/// directory's own failure, `OpenError::DataDir`, naming it).
#[tokio::test]
async fn create_fails_when_the_data_directory_cannot_be_made() {
    let dir = TestDir::new();
    let file = dir.path().join("a-file");
    fs::write(&file, b"not a directory")
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", file.display()));
    let data = file.join("data");
    let created = create(&data).await;
    assert!(
        matches!(&created, Err(OpenError::DataDir { path, .. }) if *path == data),
        "create under a file: {:?}",
        created.as_ref().err()
    );
    if let Ok(storage) = created {
        storage.close().await;
    }
    assert_eq!(
        fs::read(&file).unwrap_or_default(),
        b"not a directory",
        "the file that stood in the way"
    );
}

/// `create` never touches a database file that is already there: with a copy of the 0.1.0
/// fixture at the path, which is at schema version 1, it fails as unable to create the
/// database, and the file is still at schema version 1 afterwards. So `nage init` cannot
/// migrate a database behind this binary, even one that appears between its look and its
/// write.
///
/// Proves: decision 113 (`init` migrates only a database it created, so that the
/// pre-migration backup is never skipped).
#[tokio::test]
async fn create_leaves_a_database_that_is_already_there_as_it_is() {
    let (fixture_dir, copy) = fixture_copy("0.1.0");
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let database = path(&data);
    fs::copy(copy, &database)
        .unwrap_or_else(|error| panic!("cannot copy the fixture copy: {error}"));
    drop(fixture_dir);

    let created = create(&data).await;
    assert!(
        matches!(&created, Err(OpenError::Create { path, .. }) if *path == database),
        "create on a database that is already there: {:?}",
        created.as_ref().err()
    );
    if let Ok(storage) = created {
        storage.close().await;
    }
    let storage = Storage::open(&database, &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", database.display()));
    let version = storage
        .schema_version()
        .await
        .unwrap_or_else(|error| panic!("cannot read the schema version: {error}"));
    storage.close().await;
    assert_eq!(
        version,
        Some(1),
        "create migrated a database it did not create"
    );
}
