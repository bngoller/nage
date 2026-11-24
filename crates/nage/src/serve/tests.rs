//! The start failures and update failures no process test can reach (a backup that fails its
//! check, an older copy that cannot be removed, the clock, the signal handlers, the hashing, the
//! log, a listener that fails for another reason than a taken address, the listening address and
//! a server that stops serving), each pinned as a snapshot of its three rendered lines (ruling P9
//! of PLAN M2 slice 8's pause).

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

use nage_core::password::{self, HashParams};
use nage_core::timestamp::Timestamp;
use nage_core::update::{UpdateError, Written};

use super::ServeError;
use crate::shape::{Doing, Paint};

/// The backup a failed update names, as a data folder at `/srv/nage/data` would hold it.
const BACKUP: &str = "/srv/nage/data/backups/2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite";
/// An older copy of the same step.
const OLDER: &str = "/srv/nage/data/backups/2027-03-13T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite";

/// A real operating system error by its number, whose words are the same in every C library:
/// `Operation not permitted` (1), `No such file or directory` (2), `Permission denied` (13), `Bad
/// file descriptor` (9).
fn os_error(code: i32) -> io::Error {
    io::Error::from_raw_os_error(code)
}

/// The reason SQLite gives for a truncated backup: a freshly migrated database cut to its first
/// page, checked by the core's own `verify_backup` as the update checks a backup it wrote.
async fn truncated_backup_reason() -> String {
    let (dir, storage) = nage_core::test_support::open_migrated().await;
    storage.close().await;
    let bytes = std::fs::read(dir.db_path())
        .unwrap_or_else(|error| panic!("cannot read the migrated database: {error}"));
    let cut = dir.path().join("cut.sqlite");
    std::fs::write(&cut, &bytes[..4096])
        .unwrap_or_else(|error| panic!("cannot write the truncated copy: {error}"));
    match nage_core::update::verify_backup(&cut, 2).await {
        Ok(()) => panic!("a truncated copy passed the backup's check"),
        Err(reason) => reason,
    }
}

/// The reason SQLite gives for a database whose header counts one free page and lists none: a
/// freshly migrated database with that count planted, checked by the core's own `verify_backup`.
/// SQLite's own rows for it hold a line break, which the reason must not.
async fn free_page_reason() -> String {
    use std::io::{Seek as _, SeekFrom, Write as _};
    let (dir, storage) = nage_core::test_support::open_migrated().await;
    storage.close().await;
    let path = dir.db_path();
    let mut file = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot open the migrated database: {error}"));
    file.seek(SeekFrom::Start(36))
        .and_then(|_| file.write_all(&1_u32.to_be_bytes()))
        .unwrap_or_else(|error| panic!("cannot write the free page count: {error}"));
    drop(file);
    match nage_core::update::verify_backup(&path, 2).await {
        Ok(()) => panic!("a database counting a free page it does not list passed the check"),
        Err(reason) => reason,
    }
}

/// Each failure no process test reaches reads as its three lines, each snapshotted under its own
/// name, every system error a real one: a backup that failed its check (the reason SQLite gives
/// for a truncated copy), an older copy of the same step that cannot be removed (`Permission
/// denied`), a clock before the range a stamp can carry, signal handlers that cannot be installed
/// (`Operation not permitted`), a hashing that cannot be prepared (the reason argon2 gives for a
/// memory cost below its minimum), a log that cannot be started (tracing's words when a global
/// subscriber is set already), a listener refused for another reason than a taken address
/// (`Permission denied`), a listening address that cannot be read (`Bad file descriptor`), a
/// server that stops serving (`Bad file descriptor`), a relative data folder that cannot be made
/// absolute (`No such file or directory`, the working directory gone) and an update that failed
/// without sqlx naming its
/// change (the writer busy past its queue), which `nage migrate` prints in its own lines too.
/// The backup and the copies removed are what a start logs before its lines when its change
/// failed or an older copy could not be removed, and nothing for every other failure.
///
/// Proves: decision 159, ruling P9 (a backup that failed its check, a copy that cannot be removed, the clock, the signal handlers, the hashing, the log, the listener, the listening address and serving, as rendered lines)
#[tokio::test]
async fn the_failures_no_process_reaches_read_as_their_lines() {
    let clock = Timestamp::from_second(0)
        .err()
        .unwrap_or_else(|| panic!("the clock at 1970-01-01T00:00:00Z gave a stamp"));
    let hashing = password::hash(
        HashParams {
            memory_kib: 1,
            passes: 1,
            lanes: 1,
        },
        "correct horse 42",
    )
    .err()
    .unwrap_or_else(|| panic!("argon2 hashed with a memory cost of 1 KiB"));
    let addr: SocketAddr = "192.0.2.1:8787"
        .parse()
        .unwrap_or_else(|error| panic!("the test address: {error}"));
    let cases: [(&str, ServeError); 11] = [
        (
            "data_folder_not_absolute",
            ServeError::DataDir {
                path: PathBuf::from("data"),
                source: os_error(2),
            },
        ),
        ("update_failed_without_a_change", update_failed()),
        (
            "backup_failed_its_check",
            ServeError::Update(UpdateError::BackupUnverified {
                path: PathBuf::from(BACKUP),
                reason: truncated_backup_reason().await,
            }),
        ),
        (
            "older_copy_cannot_be_removed",
            ServeError::Update(UpdateError::RemoveOlder {
                path: PathBuf::from(OLDER),
                source: os_error(13),
                written: Box::new(Written {
                    backup: Some(PathBuf::from(BACKUP)),
                    removed: Vec::new(),
                }),
            }),
        ),
        ("clock", ServeError::Clock(clock)),
        ("signal_handlers", ServeError::Signal(os_error(1))),
        ("hashing", ServeError::Hashing(hashing)),
        (
            "log",
            ServeError::Logging(String::from(
                "a global default trace dispatcher has already been set",
            )),
        ),
        (
            "listen_failed",
            ServeError::Listen {
                addr,
                source: os_error(13),
            },
        ),
        ("listening_address", ServeError::LocalAddr(os_error(9))),
        ("serving_failed", ServeError::Serve(os_error(9))),
    ];
    for (name, error) in cases {
        let lines = error.refused().render(Paint::PLAIN);
        assert_eq!(
            lines.lines().count(),
            3,
            "the lines of {name} are not three: {lines}"
        );
        // What a failed start logs before its lines: the backup and the copies removed of a
        // change that failed or of a copy that could not be removed, nothing for the others.
        let written = match &error {
            ServeError::Update(update) => super::written_before(update).cloned(),
            _ => None,
        };
        let expected = match name {
            "older_copy_cannot_be_removed" | "update_failed_without_a_change" => Some(Written {
                backup: Some(PathBuf::from(BACKUP)),
                removed: Vec::new(),
            }),
            _ => None,
        };
        assert_eq!(written, expected, "what {name} logs before its lines");
        insta::assert_snapshot!(name, lines);
    }

    // A backup that failed its check on a header counting a free page it does not list: SQLite's
    // own rows of that check hold a line break, and the lines are still exactly three.
    let free_page = ServeError::Update(UpdateError::BackupUnverified {
        path: PathBuf::from(BACKUP),
        reason: free_page_reason().await,
    });
    let lines = free_page.refused().render(Paint::PLAIN);
    assert_eq!(
        lines.lines().count(),
        3,
        "the lines of a backup failing on a free page are not three: {lines}"
    );
    for doing in [Doing::Migrate, Doing::DryRun] {
        let ServeError::Update(unverified) = &free_page else {
            panic!("the free page's failure is not an update's failure");
        };
        let lines = crate::migrate::update_refused(doing, "0.3.0", unverified).render(Paint::PLAIN);
        assert_eq!(
            lines.lines().count(),
            3,
            "the lines of a backup failing on a free page are not three for {doing:?}: {lines}"
        );
    }

    // The same failed update as `nage migrate` prints it, which names no change either.
    let ServeError::Update(failed) = update_failed() else {
        panic!("the failed update is not an update's failure");
    };
    let lines =
        crate::migrate::update_refused(Doing::Migrate, "0.3.0", &failed).render(Paint::PLAIN);
    insta::assert_snapshot!("update_failed_without_a_change_migrate", lines);
}

/// An update that failed without sqlx naming the change, after its backup was written: the
/// writer stayed busy past its queue.
fn update_failed() -> ServeError {
    ServeError::Update(UpdateError::Update {
        path: PathBuf::from("/srv/nage/data/nage.sqlite"),
        version: None,
        source: nage_core::storage::Error::Busy(nage_core::storage::BusyCause::WriteQueue),
        written: Box::new(Written {
            backup: Some(PathBuf::from(BACKUP)),
            removed: Vec::new(),
        }),
    })
}
