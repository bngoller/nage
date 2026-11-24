//! `database::create`, the one function of the commands that creates a database (decision
//! 113).

use std::fs;

use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files, fixture_copy};

use super::{OpenError, create, finish, open_existing, path};
use crate::shape::Doing;

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
        .unwrap_or_else(|error| panic!("cannot create the database: {error:?}"));
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
        .unwrap_or_else(|error| panic!("cannot open the database create made: {error:?}"));
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

/// Writes at `path` the one page an init cut off after the open leaves: SQLite's header, written
/// by the switch to WAL, and no table (decision 155).
async fn one_page_without_tables(path: &std::path::Path) {
    use sqlx::Connection as _;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
    let conn = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal),
    )
    .await
    .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    conn.close()
        .await
        .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
}

/// `finish` updates only a database that is still empty when it has it: handed a copy of the
/// committed 0.1.0 book, which holds tables and is at schema version 1, it refuses with the error
/// and the words `create` gives for a file that appeared meanwhile, writes not a byte to it, leaves
/// it at version 1 and writes no backup; a zero-byte file and a one-page file without tables are
/// still finished, to this binary's schema version.
///
/// Proves: decision 113 (the pre-migration backup is never skipped: finish never updates a database that holds something), decision 155 (an empty database still finished)
#[tokio::test]
async fn finish_updates_only_a_database_that_is_still_empty() {
    let (fixture_dir, copy) = fixture_copy("0.1.0");
    let dir = TestDir::new();
    let data = dir.path().join("data");
    fs::create_dir_all(data.join("backups"))
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let database = path(&data);
    fs::copy(copy, &database).unwrap_or_else(|error| panic!("cannot copy the fixture: {error}"));
    drop(fixture_dir);
    let before =
        fs::read(&database).unwrap_or_else(|error| panic!("cannot read the database: {error}"));

    let finished = finish(&data).await;
    let refused = match finished {
        Err(error @ OpenError::Create { .. }) => error,
        Err(other) => panic!("finish on a database that holds tables: {other:?}"),
        Ok(storage) => {
            storage.close().await;
            panic!("finish updated a database that holds tables");
        }
    };
    assert!(
        matches!(&refused, OpenError::Create { path, source }
            if *path == database && source.kind() == std::io::ErrorKind::AlreadyExists),
        "the refusal: {refused:?}"
    );
    assert_eq!(
        fs::read(&database).unwrap_or_else(|error| panic!("cannot read the database: {error}")),
        before,
        "finish wrote to a database that holds tables"
    );
    // The same error and the same words as `create`'s for a file that is already there.
    let created = match create(&data).await {
        Err(error) => error,
        Ok(storage) => {
            storage.close().await;
            panic!("create made a database over one that is there");
        }
    };
    assert_eq!(
        refused.refused(Doing::Init),
        created.refused(Doing::Init),
        "finish's words against create's"
    );
    let entries: Vec<_> = fs::read_dir(data.join("backups"))
        .unwrap_or_else(|error| panic!("cannot read the backups folder: {error}"))
        .collect();
    assert!(entries.is_empty(), "a backup was written");
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
        "finish updated a database that holds tables"
    );

    for one_page in [false, true] {
        let dir = TestDir::new();
        let data = dir.path().join("data");
        fs::create_dir_all(data.join("backups"))
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
        let database = path(&data);
        if one_page {
            one_page_without_tables(&database).await;
        } else {
            fs::write(&database, b"")
                .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
        }
        let storage = finish(&data)
            .await
            .unwrap_or_else(|error| panic!("one page {one_page}: finish refused: {error:?}"));
        let version = storage
            .schema_version()
            .await
            .unwrap_or_else(|error| panic!("cannot read the schema version: {error}"));
        storage.close().await;
        assert_eq!(
            version,
            Some(Storage::latest_schema_version()),
            "one page {one_page}: the version after finish"
        );
    }
}
