//! The one operation that brings a data folder's database up to this version, observed on real
//! files (SPEC 5.2 and 5.5, decisions 151 to 156): the in-use mark, the backup written exactly when
//! a change waits, its check, the older copy of the same step replaced, the update all or nothing,
//! the private modes of what Nage creates, an empty database finished and a newer one refused.
//! Databases at an older schema are copies of the committed books (`fixture_copy`); a failing
//! change is planted by unchecked SQL, as a corruption test may (decision 57).

mod common;

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, fixture_copy};
use nage_core::timestamp::Timestamp;
use nage_core::update::{
    self, Holder, Mark, Missing, Update, UpdateError, Updated, Waiting, Written,
};
use sqlx::migrate::Migrate as _;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};

use common::{corrupt, exec};

/// The version of Nage these tests hand to the update.
const VERSION: &str = "0.2.0";
/// The instant the update begins.
const NOW: &str = "2026-11-24T04:00:00Z";
/// The name of the backup an update at [`NOW`] by [`VERSION`] writes of a database at step 1.
const BACKUP_1: &str = "2026-11-24T04-00-00Z-pre-migrate-0001-to-0.2.0.sqlite";
/// An older copy of the same step.
const OLDER_1: &str = "2026-11-20T00-00-00Z-pre-migrate-0001-to-0.2.0.sqlite";
/// A copy of the same step whose stamp is later than [`NOW`]: a clock set back.
const LATER_1: &str = "2026-12-01T00-00-00Z-pre-migrate-0001-to-0.2.0.sqlite";

/// The instant [`NOW`].
fn now() -> Timestamp {
    NOW.parse()
        .unwrap_or_else(|error| panic!("{NOW}: {error:?}"))
}

/// Runs the update on `data_dir` with `missing` and `holder`.
async fn run(data_dir: &Path, missing: Missing, holder: Holder) -> Result<Updated, UpdateError> {
    let options = StorageOptions::default();
    update::open_and_update(Update {
        data_dir,
        now: now(),
        version: VERSION,
        missing,
        holder,
        options: &options,
    })
    .await
}

/// Runs the update as `serve` on `data_dir`, which must succeed; closes the storage and drops
/// the mark, and returns what was done.
async fn updated(data_dir: &Path) -> update::UpdateReport {
    let done = run(data_dir, Missing::Create, Holder::Serve)
        .await
        .unwrap_or_else(|error| panic!("the update of {} failed: {error}", data_dir.display()));
    done.storage.close().await;
    drop(done.mark);
    done.report
}

/// The refusal of an update as `serve` on `data_dir`.
async fn refused(data_dir: &Path, missing: Missing) -> UpdateError {
    match run(data_dir, missing, Holder::Serve).await {
        Ok(done) => {
            done.storage.close().await;
            panic!(
                "the update of {} was accepted: {:?}",
                data_dir.display(),
                done.report
            )
        }
        Err(error) => error,
    }
}

/// The names of the files in the folder `folder`, sorted; empty when it does not exist.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("cannot read {}: {error}", folder.display()))
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// The backups folder of `data_dir`.
fn backups(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// The permission bits of `path`.
#[track_caller]
fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("cannot read the mode of {}: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// Sets the permission bits of `path` to `bits`.
#[track_caller]
fn set_mode(path: &Path, bits: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
        .unwrap_or_else(|error| panic!("cannot set the mode of {}: {error}", path.display()));
}

/// Writes `bytes` to the new file `path`.
#[track_caller]
fn plant(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

/// The bytes of `path`.
#[track_caller]
fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// A read-only connection of the test's own to the database at `path`.
async fn read_only(path: &Path) -> SqliteConnection {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {} read-only: {error}", path.display()))
}

/// The tables of the database at `path`, sorted: a schema question in unchecked SQL, a literal
/// (decision 57).
async fn tables(path: &Path) -> Vec<String> {
    let mut conn = read_only(path).await;
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .fetch_all(&mut conn)
            .await
            .unwrap_or_else(|error| {
                panic!("cannot list the tables of {}: {error}", path.display())
            });
    conn.close().await.unwrap_or_default();
    tables
}

/// The versions sqlx's bookkeeping table records in the database at `path`, in order: migration
/// bookkeeping in unchecked SQL, a literal (decision 57).
async fn versions(path: &Path) -> Vec<i64> {
    let mut conn = read_only(path).await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut conn)
            .await
            .unwrap_or_else(|error| {
                panic!("cannot read the bookkeeping of {}: {error}", path.display())
            });
    conn.close().await.unwrap_or_default();
    versions
}

/// A data folder holding a copy of the committed v0.1.0 book as its database: a database at step
/// 1 with one change waiting and a row in the bookkeeping table.
fn at_step_one() -> (TestDir, PathBuf) {
    fixture_copy("0.1.0")
}

/// Writes, at `path`, a database holding sqlx's bookkeeping table without a row (created by
/// sqlx's own code) and a planted table named `accounts`: the first waiting change applies and
/// the second fails.
async fn two_changes_the_second_failing(path: &Path) {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
    conn.ensure_migrations_table("_sqlx_migrations")
        .await
        .unwrap_or_else(|error| panic!("cannot create the bookkeeping table: {error}"));
    exec(&mut conn, &["CREATE TABLE accounts (id INTEGER)"]).await;
    conn.close()
        .await
        .unwrap_or_else(|error| panic!("cannot close {}: {error}", path.display()));
}

/// Asserts that the database at `path` is exactly as [`two_changes_the_second_failing`] wrote
/// it: the book table of the first change and its bookkeeping row are gone too.
async fn assert_as_planted(path: &Path) {
    assert_eq!(
        tables(path).await,
        vec![String::from("_sqlx_migrations"), String::from("accounts")],
        "the tables after the failed update"
    );
    assert_eq!(
        versions(path).await,
        Vec::<i64>::new(),
        "the bookkeeping rows after the failed update"
    );
}

/// `Storage::migrate`, the one update path, on a database with an empty bookkeeping table and a
/// planted `accounts` table: change 1 applies, change 2 fails on the planted table, and the
/// failure rolls both back, so the book table and bookkeeping row 1 are gone too.
///
/// Proves: decision 155, ruling P2 (Storage::migrate, the one update path: an empty bookkeeping table and a planted accounts table; the book table and bookkeeping row 1 gone afterwards)
#[tokio::test]
async fn storage_migrate_rolls_two_waiting_changes_back_when_the_second_fails() {
    let dir = TestDir::new();
    let path = dir.db_path();
    two_changes_the_second_failing(&path).await;
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    let result = storage.migrate().await;
    storage.close().await;
    match result {
        Err(error) => assert!(
            error.to_string().contains("accounts"),
            "the failure does not name the planted table: {error}"
        ),
        Ok(report) => panic!("the update was accepted: {report:?}"),
    }
    assert_as_planted(&path).await;
}

/// A backup is written for a database at step 1, whose bookkeeping table holds a row and to which
/// a change waits, and for no other: not for a new database, not for one at this version, not for
/// one whose bookkeeping table holds no row.
///
/// Proves: decision 151, SPEC 10.3 Migrations row (the backup only when migrations are pending and the bookkeeping table holds a row)
#[tokio::test]
async fn a_backup_is_written_exactly_when_a_change_waits_and_the_table_holds_a_row() {
    let (dir, path) = at_step_one();
    let report = updated(dir.path()).await;
    let expected = backups(dir.path()).join(BACKUP_1);
    assert_eq!(
        report.backup,
        Some(expected.clone()),
        "the backup at step 1"
    );
    assert_eq!(report.applied, vec![2], "the changes applied at step 1");
    assert_eq!(report.schema_version, Some(2), "the version afterwards");
    assert!(
        !report.created,
        "a database that existed reported as created"
    );
    assert_eq!(names(&backups(dir.path())), vec![BACKUP_1], "the backups");
    assert_eq!(
        versions(&expected).await,
        vec![1],
        "the backup's bookkeeping"
    );
    assert_eq!(
        versions(&path).await,
        vec![1, 2],
        "the database's bookkeeping"
    );

    let report = updated(dir.path()).await;
    assert_eq!(
        report.backup, None,
        "a backup of a database at this version"
    );
    assert!(report.applied.is_empty(), "changes applied twice");
    assert_eq!(names(&backups(dir.path())), vec![BACKUP_1], "the backups");

    let fresh = TestDir::new();
    let report = updated(fresh.path()).await;
    assert_eq!(report.backup, None, "a backup of a new database");
    assert!(report.created, "a new database not reported as created");
    assert_eq!(
        report.applied,
        vec![1, 2],
        "the changes applied to a new one"
    );
    assert!(
        names(&backups(fresh.path())).is_empty(),
        "a backup of a new database"
    );

    let rowless = TestDir::new();
    let rowless_path = rowless.db_path();
    let options = SqliteConnectOptions::new()
        .filename(&rowless_path)
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", rowless_path.display()));
    conn.ensure_migrations_table("_sqlx_migrations")
        .await
        .unwrap_or_else(|error| panic!("cannot create the bookkeeping table: {error}"));
    conn.close().await.unwrap_or_default();
    let report = updated(rowless.path()).await;
    assert_eq!(
        report.backup, None,
        "a backup with an empty bookkeeping table"
    );
    assert_eq!(report.applied, vec![1, 2], "the changes applied");
    assert!(
        names(&backups(rowless.path())).is_empty(),
        "a backup with an empty bookkeeping table"
    );
}

/// A database whose `book` table holds a NULL name under its NOT NULL (planted by editing the
/// schema for a moment): `VACUUM INTO` copies the row as it is, the copy fails SQLite's integrity
/// check, and so the copy is removed, the older copy of its step planted beside it stays, and
/// nothing is updated. Without the check the update would go on, since the second change does not
/// touch the book. And the copy is what is checked: a database whose own check fails on a free
/// page its header counts, which the copy does not carry, has its backup pass and kept; the reason
/// that database's own check gives is one line, without a line break or another control character.
///
/// Proves: decision 151 (open_and_update calls the backup's check before the update)
#[tokio::test]
async fn the_backup_is_verified_before_anything_is_updated() {
    let (dir, path) = at_step_one();
    corrupt(
        &path,
        false,
        &[
            "PRAGMA writable_schema = ON",
            "UPDATE sqlite_schema SET sql = replace(sql, 'name TEXT NOT NULL', 'name TEXT') WHERE name = 'book'",
            "PRAGMA writable_schema = RESET",
            "INSERT INTO book (id, name, currency, minor_digits, created_at) VALUES (1, NULL, 'CAD', 2, '2026-11-09T12:00:00Z')",
            "PRAGMA writable_schema = ON",
            "UPDATE sqlite_schema SET sql = replace(sql, 'name TEXT,', 'name TEXT NOT NULL,') WHERE name = 'book'",
            "PRAGMA writable_schema = RESET",
        ],
    )
    .await;
    assert!(
        update::verify_backup(&path, 1).await.is_err(),
        "the planted NULL does not fail SQLite's integrity check"
    );
    // An older copy of the same step: it goes only once a new backup passed, so it stays.
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    plant(&folder.join(OLDER_1), b"an older copy");
    let error = refused(dir.path(), Missing::Create).await;
    match &error {
        UpdateError::BackupUnverified { path: copy, reason } => {
            assert_eq!(copy, &folder.join(BACKUP_1), "the copy named");
            assert!(!reason.is_empty(), "the reason is empty");
        }
        other => panic!("the update failed with {other:?}, not BackupUnverified"),
    }
    assert_eq!(
        names(&folder),
        vec![OLDER_1],
        "the backups after a copy that did not pass"
    );
    assert_eq!(
        bytes(&folder.join(OLDER_1)),
        b"an older copy",
        "the older copy's bytes"
    );
    assert_eq!(versions(&path).await, vec![1], "the database was updated");
    assert!(
        !tables(&path).await.contains(&String::from("users")),
        "a change was applied"
    );

    // The copy is what is checked, not the database: a database whose header counts one free page
    // and lists none fails SQLite's integrity check, while its copy, which `VACUUM INTO` writes
    // without free pages, passes; the backup is kept, and the update goes on to its change, which
    // SQLite then refuses on the database's own header (the page it would take is page 0).
    let (dir, path) = at_step_one();
    let mut file = File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    file.seek(SeekFrom::Start(36))
        .and_then(|_| file.write_all(&1_u32.to_be_bytes()))
        .unwrap_or_else(|error| panic!("cannot write the free page count: {error}"));
    drop(file);
    // The reason is one line: SQLite's rows joined, a line break inside a row made a space.
    match update::verify_backup(&path, 1).await {
        Err(reason) => assert!(
            !reason.is_empty() && !reason.chars().any(char::is_control),
            "the reason is empty or holds a control character: {reason:?}"
        ),
        Ok(()) => panic!("the planted free page count does not fail SQLite's integrity check"),
    }
    let copy = backups(dir.path()).join(BACKUP_1);
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Update { written, .. } => {
            assert_eq!(written.backup, Some(copy.clone()), "the backup kept");
        }
        other => panic!("the update failed with {other:?}, not at its change"),
    }
    update::verify_backup(&copy, 1)
        .await
        .unwrap_or_else(|reason| panic!("the copy did not pass its check: {reason}"));
    assert_eq!(versions(&path).await, vec![1], "the database was updated");
}

/// The committed v0.1.0 book passes the backup's check at step 1; a copy cut to half its length
/// does not.
///
/// Proves: decision 151, ruling P3 (verify_backup: SQLite's integrity check)
#[tokio::test]
async fn a_truncated_backup_does_not_pass_its_check() {
    let (_dir, path) = at_step_one();
    update::verify_backup(&path, 1)
        .await
        .unwrap_or_else(|reason| panic!("the committed book did not pass its check: {reason}"));

    let (_cut_dir, cut) = at_step_one();
    let length = std::fs::metadata(&cut)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", cut.display()))
        .len();
    let file = File::options()
        .write(true)
        .open(&cut)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", cut.display()));
    file.set_len(length / 2)
        .unwrap_or_else(|error| panic!("cannot cut {}: {error}", cut.display()));
    drop(file);
    match update::verify_backup(&cut, 1).await {
        Err(reason) => assert!(!reason.is_empty(), "the reason is empty"),
        Ok(()) => panic!("a copy cut to {} of {length} bytes passed", length / 2),
    }
}

/// A copy at step 1 checked as a copy of step 2 does not pass, and the reason names both.
///
/// Proves: decision 151, ruling P3 (verify_backup: the version the name carries)
#[tokio::test]
async fn a_backup_at_another_version_does_not_pass_its_check() {
    let (_dir, path) = at_step_one();
    match update::verify_backup(&path, 2).await {
        Err(reason) => assert!(
            reason.contains('1') && reason.contains('2'),
            "the reason names neither version: {reason}"
        ),
        Ok(()) => panic!("a copy at step 1 passed as a copy of step 2"),
    }
    update::verify_backup(&path, 1)
        .await
        .unwrap_or_else(|reason| panic!("the copy did not pass at its own step: {reason}"));
}

/// A database whose `book` table's root page is overwritten: it opens and its bookkeeping reads,
/// but `VACUUM INTO` cannot read the book, so the backup fails; its file is removed and nothing is
/// updated.
///
/// Proves: decision 151, SPEC 5.5 (a failed backup aborts the start without migrating)
#[tokio::test]
async fn a_failed_backup_updates_nothing_and_leaves_no_file() {
    let (dir, path) = at_step_one();
    let (page_size, root): (i64, i64) = {
        let mut conn = read_only(&path).await;
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&mut conn)
            .await
            .unwrap_or_else(|error| panic!("cannot read the page size: {error}"));
        let root: i64 =
            sqlx::query_scalar("SELECT rootpage FROM sqlite_schema WHERE name = 'book'")
                .fetch_one(&mut conn)
                .await
                .unwrap_or_else(|error| panic!("cannot read the book's root page: {error}"));
        conn.close().await.unwrap_or_default();
        (page_size, root)
    };
    let mut file = File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    let offset = u64::try_from((root - 1) * page_size)
        .unwrap_or_else(|error| panic!("the book's page offset: {error}"));
    file.seek(SeekFrom::Start(offset))
        .unwrap_or_else(|error| panic!("cannot seek in {}: {error}", path.display()));
    file.write_all(&[0xFF; 64])
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    drop(file);

    let error = refused(dir.path(), Missing::Create).await;
    match &error {
        UpdateError::Backup { path: copy, .. } => {
            assert_eq!(copy, &backups(dir.path()).join(BACKUP_1), "the copy named");
        }
        other => panic!("the update failed with {other:?}, not Backup"),
    }
    assert!(
        names(&backups(dir.path())).is_empty(),
        "the copy's file was left"
    );
    assert_eq!(versions(&path).await, vec![1], "the database was updated");
}

/// A backups folder the account cannot write in: the backup's file cannot be created, which is
/// an input-output error of its own naming the file, and nothing is updated.
///
/// Proves: decision 151, ruling P3 (an input-output error of its own, BackupCreate; nothing updated)
#[tokio::test]
async fn a_backup_file_that_cannot_be_created_updates_nothing() {
    let (dir, path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    set_mode(&folder, 0o500);
    let error = refused(dir.path(), Missing::Create).await;
    set_mode(&folder, 0o700);
    match &error {
        UpdateError::BackupCreate { path: copy, source } => {
            assert_eq!(copy, &folder.join(BACKUP_1), "the file named");
            assert_eq!(
                source.kind(),
                std::io::ErrorKind::PermissionDenied,
                "the system's error"
            );
        }
        other => panic!("the update failed with {other:?}, not BackupCreate"),
    }
    assert!(
        names(&folder).is_empty(),
        "a file was left in the backups folder"
    );
    assert_eq!(versions(&path).await, vec![1], "the database was updated");
}

/// An empty file at the backup's name, then a dangling symbolic link there, then a file at that
/// name with `-journal`, with `-wal` and with `-shm` after it (which SQLite would clear beside the
/// new backup):
/// each stops the update as `BackupTaken` naming it, the file stays empty, the link stays dangling
/// (nothing was written through it), each side file keeps its bytes, and nothing is updated.
///
/// Proves: decision 151, ruling P3 (an empty file and a dangling link planted at the name: BackupTaken, nothing written at either)
#[tokio::test]
async fn a_file_at_the_backup_name_stops_the_update_and_is_left_as_it_is() {
    let (dir, path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    let at_name = folder.join(BACKUP_1);
    plant(&at_name, b"");
    match refused(dir.path(), Missing::Create).await {
        UpdateError::BackupTaken { path: taken } => assert_eq!(taken, at_name, "the name"),
        other => panic!("an empty file at the name gave {other:?}, not BackupTaken"),
    }
    assert!(
        bytes(&at_name).is_empty(),
        "something was written at the name"
    );
    assert_eq!(versions(&path).await, vec![1], "the database was updated");

    std::fs::remove_file(&at_name)
        .unwrap_or_else(|error| panic!("cannot remove {}: {error}", at_name.display()));
    let target = dir.path().join("elsewhere.sqlite");
    std::os::unix::fs::symlink(&target, &at_name)
        .unwrap_or_else(|error| panic!("cannot link {}: {error}", at_name.display()));
    match refused(dir.path(), Missing::Create).await {
        UpdateError::BackupTaken { path: taken } => assert_eq!(taken, at_name, "the name"),
        other => panic!("a dangling link at the name gave {other:?}, not BackupTaken"),
    }
    assert!(
        std::fs::symlink_metadata(&at_name)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false),
        "the link at the name is gone"
    );
    assert!(!target.exists(), "something was written through the link");
    assert_eq!(versions(&path).await, vec![1], "the database was updated");

    // A file at the name with SQLite's -journal, -wal or -shm after it, which SQLite would clear
    // beside the new, empty backup: the update stops before the backup's file is created, and the
    // side file keeps its bytes.
    std::fs::remove_file(&at_name)
        .unwrap_or_else(|error| panic!("cannot remove {}: {error}", at_name.display()));
    for suffix in ["-journal", "-wal", "-shm"] {
        let side = folder.join(format!("{BACKUP_1}{suffix}"));
        plant(&side, b"a file a test planted beside the name");
        match refused(dir.path(), Missing::Create).await {
            UpdateError::BackupTaken { path: taken } => assert_eq!(taken, side, "the file named"),
            other => panic!("a file at the name{suffix} gave {other:?}, not BackupTaken"),
        }
        assert_eq!(
            bytes(&side),
            b"a file a test planted beside the name",
            "the bytes of the file at the name{suffix}"
        );
        assert!(
            std::fs::symlink_metadata(&at_name).is_err(),
            "the backup's file was created beside the file at the name{suffix}"
        );
        assert_eq!(versions(&path).await, vec![1], "the database was updated");
        std::fs::remove_file(&side)
            .unwrap_or_else(|error| panic!("cannot remove {}: {error}", side.display()));
    }
}

/// A data folder whose name holds an apostrophe: the backup is written at its name in that
/// folder and passes its check, and the database is updated, since the path is bound as a
/// parameter of `VACUUM INTO`, never assembled into the statement.
///
/// Proves: decision 151, ruling P3 (the path bound as a parameter of VACUUM INTO)
#[tokio::test]
async fn the_backup_path_is_bound_so_an_apostrophe_in_the_data_folder_changes_nothing() {
    let (dir, path) = at_step_one();
    let data = dir.path().join("Sam's data");
    std::fs::create_dir(&data)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", data.display()));
    let db = data.join("nage.sqlite");
    std::fs::copy(&path, &db)
        .unwrap_or_else(|error| panic!("cannot copy to {}: {error}", db.display()));
    let report = updated(&data).await;
    let expected = backups(&data).join(BACKUP_1);
    assert_eq!(report.backup, Some(expected.clone()), "the backup");
    assert_eq!(
        versions(&expected).await,
        vec![1],
        "the backup's bookkeeping"
    );
    assert_eq!(
        versions(&db).await,
        vec![1, 2],
        "the database's bookkeeping"
    );
}

/// An older copy of the same step is removed once the new one passed its check, and the report
/// names it.
///
/// Proves: decision 151 (one copy per update step, the others removed only after the new one passed)
#[tokio::test]
async fn the_newest_copy_replaces_the_last_of_the_same_step() {
    let (dir, _path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    plant(&folder.join(OLDER_1), b"an older copy");
    let report = updated(dir.path()).await;
    assert_eq!(
        report.removed,
        vec![folder.join(OLDER_1)],
        "the copies removed"
    );
    assert_eq!(names(&folder), vec![BACKUP_1], "the backups afterwards");
}

/// A copy of the same step whose stamp is later than the update's (a clock set back) is removed
/// too.
///
/// Proves: decision 151, ruling P4 (a clock set back: every other copy of the same step goes, whatever its stamp)
#[tokio::test]
async fn a_same_step_copy_with_a_later_stamp_is_removed_too() {
    let (dir, _path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    plant(&folder.join(LATER_1), b"a copy from a clock set forward");
    let report = updated(dir.path()).await;
    assert_eq!(
        report.removed,
        vec![folder.join(LATER_1)],
        "the copies removed"
    );
    assert_eq!(names(&folder), vec![BACKUP_1], "the backups afterwards");
}

/// Copies of another step, copies made by another version and files that are not backups stay,
/// their bytes as they were.
///
/// Proves: decision 151 (copies of other steps and files that are not backups stay)
#[tokio::test]
async fn copies_of_other_steps_are_never_touched() {
    let (dir, _path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    let others = [
        "2026-11-20T00-00-00Z-pre-migrate-0002-to-0.2.0.sqlite",
        "2026-11-20T00-00-00Z-pre-migrate-0001-to-0.1.9.sqlite",
        "2026-11-20T00-00-00Z-pre-migrate-0001-to-0.2.0-rc.1.sqlite",
        "2026-11-20T00-00-00Z-pre-migrate-0001-to-0.2.0.sqlite-journal",
        "notes.txt",
    ];
    for name in others {
        plant(&folder.join(name), name.as_bytes());
    }
    let report = updated(dir.path()).await;
    assert!(
        report.removed.is_empty(),
        "copies removed: {:?}",
        report.removed
    );
    for name in others {
        assert_eq!(
            bytes(&folder.join(name)),
            name.as_bytes(),
            "the bytes of {name}"
        );
    }
    assert_eq!(
        names(&folder).len(),
        others.len() + 1,
        "the files of the folder"
    );
}

/// Through `open_and_update`: two waiting changes, the second failing on a planted `accounts`
/// table, leave the database exactly as it was, the book table and bookkeeping row 1 gone too;
/// the error names the change that failed.
///
/// Proves: decision 155, ruling P2 (through open_and_update, two waiting changes and the second failing: the book table and bookkeeping row 1 gone too)
#[tokio::test]
async fn an_update_is_all_or_nothing() {
    let dir = TestDir::new();
    let path = dir.db_path();
    two_changes_the_second_failing(&path).await;
    match refused(dir.path(), Missing::Refuse).await {
        UpdateError::Update { version, .. } => {
            assert_eq!(version, Some(2), "the change named as failing");
        }
        other => panic!("the update failed with {other:?}, not Update"),
    }
    assert_as_planted(&path).await;
}

/// A database at step 1 whose second change fails on a planted `accounts` table, with an older
/// copy of the same step: the error carries the backup written and the copy removed, so that the
/// server can log both; the backup stays and the database is at step 1.
///
/// Proves: decision 160, ruling P4 (UpdateError::Update carries the backup written and the copies removed)
#[tokio::test]
async fn a_failed_change_names_the_backup_and_the_copies_removed() {
    let (dir, path) = at_step_one();
    corrupt(&path, false, &["CREATE TABLE accounts (id INTEGER)"]).await;
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    plant(&folder.join(OLDER_1), b"an older copy");
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Update {
            version, written, ..
        } => {
            assert_eq!(version, Some(2), "the change named as failing");
            assert_eq!(
                *written,
                Written {
                    backup: Some(folder.join(BACKUP_1)),
                    removed: vec![folder.join(OLDER_1)],
                },
                "what the failure carries"
            );
        }
        other => panic!("the update failed with {other:?}, not Update"),
    }
    assert_eq!(names(&folder), vec![BACKUP_1], "the backups afterwards");
    assert_eq!(versions(&path).await, vec![1], "the database's bookkeeping");
}

/// Asserts that the dry run's look (`update::waiting`) at the data folder `data_dir`, whose
/// database `path` records the change `version` with another checksum, refuses it as `Modified`
/// naming that change, as the real update does, without a byte written to the database and
/// without a file created in the data folder.
async fn assert_waiting_refuses_the_changed_record(data_dir: &Path, path: &Path, version: i64) {
    let before = bytes(path);
    let files = names(data_dir);
    match update::waiting(data_dir).await {
        Err(UpdateError::Modified {
            version: named,
            path: database,
        }) => {
            assert_eq!(named, version, "the change the dry run named");
            assert_eq!(database, path, "the database the dry run named");
        }
        other => panic!("the dry run answered {other:?}, not Modified"),
    }
    assert_eq!(bytes(path), before, "the dry run wrote to the database");
    assert_eq!(names(data_dir), files, "the dry run created a file");
}

/// A database at step 1 whose record of change 1 carries another checksum is refused as
/// modified, naming the change, and no backup is written: not even the backups folder is created.
/// The dry run's look refuses it too, as the real run would, writing and creating nothing.
///
/// Proves: SPEC 5.5, ruling P2 (Modified through open_and_update; no backup written)
#[tokio::test]
async fn a_changed_record_is_refused_before_any_backup_when_a_change_waits() {
    let (dir, path) = at_step_one();
    corrupt(
        &path,
        false,
        &["UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 1"],
    )
    .await;
    assert_waiting_refuses_the_changed_record(dir.path(), &path, 1).await;
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Modified { version, .. } => assert_eq!(version, 1, "the change named"),
        other => panic!("the update failed with {other:?}, not Modified"),
    }
    assert!(
        !backups(dir.path()).exists(),
        "a backups folder was created, or a backup written"
    );
    assert_eq!(versions(&path).await, vec![1], "the database was updated");
}

/// A database at this version whose record of change 2 carries another checksum is refused as
/// modified, although nothing waits; the dry run's look refuses it too, writing and creating
/// nothing.
///
/// Proves: SPEC 5.5, ruling P2 (Modified through open_and_update on a database at this version)
#[tokio::test]
async fn a_changed_record_is_refused_when_nothing_waits() {
    let dir = TestDir::new();
    updated(dir.path()).await;
    corrupt(
        &dir.db_path(),
        false,
        &["UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 2"],
    )
    .await;
    assert_waiting_refuses_the_changed_record(dir.path(), &dir.db_path(), 2).await;
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Modified { version, .. } => assert_eq!(version, 2, "the change named"),
        other => panic!("the update failed with {other:?}, not Modified"),
    }
    assert!(
        names(&backups(dir.path())).is_empty(),
        "a backup was written"
    );
}

/// A new data folder: the folder, the backups folder, the database and the mark's file are
/// created so that only the account Nage runs as can open them (0700 and 0600); a backup is
/// created 0600 too.
///
/// Proves: decision 154 (the data folder, backups, the database, each backup and the mark file)
#[tokio::test]
async fn what_nage_creates_in_the_data_folder_only_its_own_account_can_open() {
    let dir = TestDir::new();
    let data = dir.path().join("data");
    updated(&data).await;
    assert_eq!(mode(&data), 0o700, "the data folder");
    assert_eq!(mode(&backups(&data)), 0o700, "the backups folder");
    assert_eq!(mode(&data.join("nage.sqlite")), 0o600, "the database");
    assert_eq!(mode(&data.join("nage.lock")), 0o600, "the mark's file");

    let (old, _path) = at_step_one();
    updated(old.path()).await;
    assert_eq!(
        mode(&backups(old.path()).join(BACKUP_1)),
        0o600,
        "the backup"
    );
}

/// A data folder that exists with its own modes: the folder, the backups folder, the database and
/// the mark's file keep theirs; only the backup, which Nage creates, is 0600.
///
/// Proves: decision 154 (what exists is left as it is)
#[tokio::test]
async fn what_exists_in_the_data_folder_keeps_its_modes() {
    let (dir, path) = at_step_one();
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    let mark = dir.path().join("nage.lock");
    plant(&mark, b"");
    set_mode(&folder, 0o750);
    set_mode(&path, 0o640);
    set_mode(&mark, 0o644);
    updated(dir.path()).await;
    assert_eq!(mode(&folder), 0o750, "the backups folder");
    assert_eq!(mode(&path), 0o640, "the database");
    assert_eq!(mode(&mark), 0o644, "the mark's file");
    assert_eq!(mode(&folder.join(BACKUP_1)), 0o600, "the backup");
}

/// While one mark is held on a data folder, a second is refused, whoever asks.
///
/// Proves: decision 153 (one holder of the in-use mark)
#[tokio::test]
async fn a_second_mark_on_one_data_folder_is_refused() {
    let dir = TestDir::new();
    let held = Mark::take(dir.path(), Holder::Serve)
        .unwrap_or_else(|error| panic!("cannot take the mark: {error}"));
    for holder in [Holder::Serve, Holder::Migrate] {
        match Mark::take(dir.path(), holder) {
            Err(UpdateError::InUse { data_dir, .. }) => {
                assert_eq!(data_dir, dir.path(), "the data folder named");
            }
            Err(other) => panic!("a second mark failed with {other:?}, not InUse"),
            Ok(_) => panic!("a second mark was taken for {holder:?}"),
        }
    }
    match refused(dir.path(), Missing::Create).await {
        UpdateError::InUse { .. } => {}
        other => panic!("an update beside the mark failed with {other:?}, not InUse"),
    }
    drop(held);
}

/// The mark's file holds its holder's word alone, and a refused mark names that holder: serve,
/// migrate, or none when the file holds no holder's word.
///
/// Proves: decision 153, ruling P5 (the word serve or migrate in the mark's file; none when it cannot be read)
#[tokio::test]
async fn a_refused_mark_names_its_holder() {
    let dir = TestDir::new();
    let file = dir.path().join("nage.lock");
    plant(&file, b"migrate and more words from an earlier holder");
    // The words are decision 153's, held as literal texts: a Nage of another version reads them.
    for (holder, word) in [(Holder::Serve, "serve"), (Holder::Migrate, "migrate")] {
        let held = Mark::take(dir.path(), holder)
            .unwrap_or_else(|error| panic!("cannot take the mark for {holder:?}: {error}"));
        assert_eq!(bytes(&file), word.as_bytes(), "the mark's file");
        match Mark::take(dir.path(), Holder::Migrate) {
            Err(UpdateError::InUse { holder: named, .. }) => {
                assert_eq!(named, Some(holder), "the holder named");
            }
            Err(other) => panic!("a second mark failed with {other:?}, not InUse"),
            Ok(_) => panic!("a second mark was taken"),
        }
        drop(held);
    }

    let mut foreign = File::options()
        .read(true)
        .write(true)
        .open(&file)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", file.display()));
    foreign
        .try_lock()
        .unwrap_or_else(|error| panic!("cannot lock {}: {error:?}", file.display()));
    foreign
        .set_len(0)
        .and_then(|()| foreign.write_all(b"\xFFsomething"))
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", file.display()));
    match Mark::take(dir.path(), Holder::Serve) {
        Err(UpdateError::InUse { holder, .. }) => {
            assert_eq!(holder, None, "a holder named from an unreadable word");
        }
        Err(other) => panic!("a second mark failed with {other:?}, not InUse"),
        Ok(_) => panic!("a second mark was taken"),
    }
    let mut text = Vec::new();
    foreign
        .seek(SeekFrom::Start(0))
        .and_then(|_| foreign.read_to_end(&mut text))
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()));
    assert_eq!(text, b"\xFFsomething", "the refused mark wrote in the file");
}

/// A mark dropped is released, so the next one is taken, and its file stays.
///
/// Proves: decision 153 (no stale mark; the file stays)
#[tokio::test]
async fn the_mark_is_released_with_its_holder() {
    let dir = TestDir::new();
    let first = Mark::take(dir.path(), Holder::Migrate)
        .unwrap_or_else(|error| panic!("cannot take the mark: {error}"));
    drop(first);
    assert!(
        dir.path().join("nage.lock").is_file(),
        "the mark's file is gone"
    );
    let second = Mark::take(dir.path(), Holder::Serve)
        .unwrap_or_else(|error| panic!("the released mark cannot be taken again: {error}"));
    drop(second);
    let after_update = updated(dir.path()).await;
    assert!(after_update.created, "the database was not created");
    let third = Mark::take(dir.path(), Holder::Serve)
        .unwrap_or_else(|error| panic!("the mark of a finished update was not released: {error}"));
    drop(third);
}

/// The dry run's read: a database at step 1 lists change 2, its version and the backups folder,
/// an empty database lists every change and no backup, a database whose bookkeeping table holds
/// no row lists every change and no backup either, a database at this version lists nothing;
/// none of the files changes by a byte, no side file appears beside a database, and no mark's
/// file is created.
///
/// Proves: decision 156 (the dry run only reads, an empty database included, not a byte written)
#[tokio::test]
async fn what_waits_is_listed_and_nothing_changes() {
    let (dir, path) = at_step_one();
    let before = bytes(&path);
    let files_before = names(dir.path());
    let waiting = update::waiting(dir.path())
        .await
        .unwrap_or_else(|error| panic!("cannot list what waits: {error}"));
    assert_eq!(
        waiting,
        Waiting {
            database: path.clone(),
            empty: false,
            schema_version: Some(1),
            expects: 2,
            waiting: vec![2],
            backup_to: Some(backups(dir.path())),
        },
        "what waits at step 1"
    );
    assert_eq!(bytes(&path), before, "the database's bytes");
    assert_eq!(
        names(dir.path()),
        files_before,
        "the files of the data folder, side files included"
    );
    assert!(
        !dir.path().join("nage.lock").exists(),
        "a mark's file was created"
    );
    assert!(
        !backups(dir.path()).exists(),
        "a backups folder was created"
    );

    let empty = TestDir::new();
    plant(&empty.db_path(), b"");
    let waiting = update::waiting(empty.path())
        .await
        .unwrap_or_else(|error| panic!("cannot list what waits: {error}"));
    assert_eq!(
        waiting,
        Waiting {
            database: empty.db_path(),
            empty: true,
            schema_version: None,
            expects: 2,
            waiting: vec![1, 2],
            backup_to: None,
        },
        "what waits in an empty database"
    );
    assert!(
        bytes(&empty.db_path()).is_empty(),
        "the empty file was written"
    );

    let rowless = TestDir::new();
    let rowless_path = rowless.db_path();
    let options = SqliteConnectOptions::new()
        .filename(&rowless_path)
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", rowless_path.display()));
    conn.ensure_migrations_table("_sqlx_migrations")
        .await
        .unwrap_or_else(|error| panic!("cannot create the bookkeeping table: {error}"));
    conn.close().await.unwrap_or_default();
    let before = bytes(&rowless_path);
    let waiting = update::waiting(rowless.path())
        .await
        .unwrap_or_else(|error| panic!("cannot list what waits: {error}"));
    assert_eq!(
        waiting,
        Waiting {
            database: rowless_path.clone(),
            empty: false,
            schema_version: None,
            expects: 2,
            waiting: vec![1, 2],
            backup_to: None,
        },
        "what waits with a bookkeeping table without a row"
    );
    assert_eq!(bytes(&rowless_path), before, "the rowless database's bytes");

    let current = TestDir::new();
    updated(current.path()).await;
    let before = bytes(&current.db_path());
    let files_before = names(current.path());
    let waiting = update::waiting(current.path())
        .await
        .unwrap_or_else(|error| panic!("cannot list what waits: {error}"));
    assert_eq!(
        waiting.waiting,
        Vec::<i64>::new(),
        "what waits at this version"
    );
    assert_eq!(waiting.backup_to, None, "a backup with nothing waiting");
    assert_eq!(bytes(&current.db_path()), before, "the database's bytes");
    assert_eq!(
        names(current.path()),
        files_before,
        "the files of the data folder, side files included"
    );

    let missing = TestDir::new();
    match update::waiting(missing.path()).await {
        Err(UpdateError::Missing { path }) => assert_eq!(path, missing.db_path(), "the path"),
        other => panic!("a missing database gave {other:?}, not Missing"),
    }
}

/// A zero-byte file and a one-page file without tables are empty; the committed book is not;
/// neither file changes by a byte. A database whose tables still sit in its write-ahead log beside
/// a storage that holds it open is not empty either, and nothing waits in it.
///
/// Proves: decision 155, ruling P6 (a zero-byte file and a one-page file without tables are empty; a file with a table is not; neither file changes)
#[tokio::test]
async fn an_empty_database_is_told_apart_without_a_byte_written() {
    let zero = TestDir::new();
    plant(&zero.db_path(), b"");
    assert!(
        update::is_empty(&zero.db_path())
            .await
            .unwrap_or_else(|error| panic!("cannot tell a zero-byte file: {error}")),
        "a zero-byte file is not empty"
    );
    assert!(
        bytes(&zero.db_path()).is_empty(),
        "the zero-byte file was written"
    );

    let page = TestDir::new();
    let storage = Storage::open(&page.db_path(), &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open a new database: {error}"));
    storage.close().await;
    let before = bytes(&page.db_path());
    let files_before = names(page.path());
    assert!(!before.is_empty(), "the one-page file has no page");
    assert!(
        update::is_empty(&page.db_path())
            .await
            .unwrap_or_else(|error| panic!("cannot tell a one-page file: {error}")),
        "a one-page file without tables is not empty"
    );
    assert_eq!(bytes(&page.db_path()), before, "the one-page file's bytes");
    assert_eq!(
        names(page.path()),
        files_before,
        "the files beside the one-page file"
    );

    let (dir, path) = at_step_one();
    let before = bytes(&path);
    let files_before = names(dir.path());
    assert!(
        !update::is_empty(&path)
            .await
            .unwrap_or_else(|error| panic!("cannot tell the committed book: {error}")),
        "a file with tables is empty"
    );
    assert_eq!(bytes(&path), before, "the book's bytes");
    assert_eq!(names(dir.path()), files_before, "the files beside the book");

    // A database whose tables still sit in its write-ahead log, beside a storage that holds it
    // open (a running server, or a crash): read with what the log holds, it is not empty, and
    // what waits is nothing.
    let live = TestDir::new();
    let storage = Storage::open(&live.db_path(), &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open a new database: {error}"));
    storage
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("cannot update the new database: {error}"));
    let mut wal = live.db_path().into_os_string();
    wal.push("-wal");
    let wal_there = Path::new(&wal).exists();
    let empty = update::is_empty(&live.db_path()).await;
    let waiting = update::waiting(live.path()).await;
    storage.close().await;
    assert!(wal_there, "no write-ahead log beside the open database");
    assert!(
        !empty.unwrap_or_else(|error| panic!("cannot tell the open database: {error}")),
        "a database whose tables sit in its write-ahead log is empty"
    );
    assert_eq!(
        waiting
            .unwrap_or_else(|error| panic!("cannot list what waits: {error}"))
            .waiting,
        Vec::<i64>::new(),
        "what waits in a database whose changes sit in its write-ahead log"
    );
}

/// A zero-byte file and a one-page file without tables are finished like a new database: every
/// change applied, no backup, reported as created.
///
/// Proves: decision 155, ruling P6 (a zero-byte file and a one-page file without tables, finished like a new one, with no backup)
#[tokio::test]
async fn an_empty_database_file_is_finished() {
    let zero = TestDir::new();
    plant(&zero.db_path(), b"");
    let page = TestDir::new();
    let storage = Storage::open(&page.db_path(), &StorageOptions::default())
        .await
        .unwrap_or_else(|error| panic!("cannot open a new database: {error}"));
    storage.close().await;
    for (label, dir) in [("zero-byte", &zero), ("one-page", &page)] {
        let done = run(dir.path(), Missing::Refuse, Holder::Migrate)
            .await
            .unwrap_or_else(|error| panic!("the {label} file was not finished: {error}"));
        done.storage.close().await;
        drop(done.mark);
        assert!(
            done.report.created,
            "the {label} file not reported as finished"
        );
        assert_eq!(
            done.report.applied,
            vec![1, 2],
            "the changes applied to the {label} file"
        );
        assert_eq!(done.report.backup, None, "a backup of the {label} file");
        assert!(
            names(&backups(dir.path())).is_empty(),
            "a backup of the {label} file"
        );
        assert_eq!(
            versions(&dir.db_path()).await,
            vec![1, 2],
            "the {label} file's bookkeeping"
        );
    }
}

/// `nage migrate` on a data folder without a database, and on a folder that does not exist: the
/// refusal comes before the mark, so nothing is created, not even the mark's file.
///
/// Proves: decision 156, ruling P5 (refused before the mark: nothing created, not even the mark's file)
#[tokio::test]
async fn a_missing_database_is_refused_when_nothing_may_be_created() {
    let dir = TestDir::new();
    match refused(dir.path(), Missing::Refuse).await {
        UpdateError::Missing { path } => assert_eq!(path, dir.db_path(), "the path named"),
        other => panic!("a missing database gave {other:?}, not Missing"),
    }
    assert!(
        names(dir.path()).is_empty(),
        "files created: {:?}",
        names(dir.path())
    );

    let absent = dir.path().join("absent");
    match refused(&absent, Missing::Refuse).await {
        UpdateError::Missing { path } => {
            assert_eq!(path, absent.join("nage.sqlite"), "the path named");
        }
        other => panic!("a missing folder gave {other:?}, not Missing"),
    }
    assert!(!absent.exists(), "the missing data folder was created");
}

/// A database that carries change 9999: refused as newer before the database or a backup is
/// written, the database's bytes unchanged and the mark's file the one file created; the
/// refusal names the newest copy this version can open (step 2 preferred to a newer step 1,
/// step 3 left out); with no backups folder, none is named and none is created.
///
/// Proves: decision 152, SPEC 10.3 Migrations row, ruling P5 (the database untouched, the mark's file the one file that may be created, the newest backup it can open named)
#[tokio::test]
async fn a_newer_database_is_refused_before_anything_is_written() {
    let (dir, path) = at_step_one();
    corrupt(
        &path,
        false,
        &["INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'future', 1, x'00', 0)"],
    )
    .await;
    let folder = backups(dir.path());
    std::fs::create_dir(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    let copies = [
        "2026-10-01T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "2026-11-01T00-00-00Z-pre-migrate-0001-to-0.2.0.sqlite",
        "2026-11-15T00-00-00Z-pre-migrate-0003-to-0.4.0.sqlite",
    ];
    for name in copies {
        plant(&folder.join(name), name.as_bytes());
    }
    let before = bytes(&path);
    let files_before = names(dir.path());
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Newer {
            version,
            latest,
            backup,
            ..
        } => {
            assert_eq!(version, 9999, "the change the database carries");
            assert_eq!(latest, 2, "the last change this version knows");
            assert_eq!(backup, Some(folder.join(copies[0])), "the copy named");
        }
        other => panic!("a newer database gave {other:?}, not Newer"),
    }
    assert_eq!(bytes(&path), before, "the database's bytes");
    let mut expected = files_before;
    expected.push(String::from("nage.lock"));
    expected.sort();
    assert_eq!(names(dir.path()), expected, "the files of the data folder");
    assert_eq!(names(&folder).len(), copies.len(), "the backups");

    let (bare, _bare_path) = at_step_one();
    corrupt(
        &bare.db_path(),
        false,
        &["INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'future', 1, x'00', 0)"],
    )
    .await;
    match refused(bare.path(), Missing::Create).await {
        UpdateError::Newer { backup, .. } => assert_eq!(backup, None, "a copy named"),
        other => panic!("a newer database gave {other:?}, not Newer"),
    }
    assert!(
        !backups(bare.path()).exists(),
        "the refusal created a backups folder"
    );
}

/// A file with a table but no bookkeeping table is not Nage's: refused, no bookkeeping table
/// created, no backup written, not even the backups folder.
///
/// Proves: decision 155 (a file that is not a Nage database is never migrated)
#[tokio::test]
async fn a_file_with_tables_but_no_bookkeeping_is_refused() {
    let dir = TestDir::new();
    let path = dir.db_path();
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
    exec(&mut conn, &["CREATE TABLE notes (body TEXT)"]).await;
    conn.close().await.unwrap_or_default();
    match refused(dir.path(), Missing::Create).await {
        UpdateError::Foreign { path: named } => assert_eq!(named, path, "the file named"),
        other => panic!("a foreign file gave {other:?}, not Foreign"),
    }
    assert_eq!(
        tables(&path).await,
        vec![String::from("notes")],
        "the tables"
    );
    assert!(
        !backups(dir.path()).exists(),
        "a backups folder was created, or a backup written"
    );
}
