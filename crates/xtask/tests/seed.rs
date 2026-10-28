//! `cargo xtask seed` observed from outside the process: the file it writes is a migrated,
//! empty book at the binary's schema with no SQLite side files beside it, and an existing
//! file is refused (milestone 1 slice 3).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nage_core::storage::{Storage, StorageOptions};

/// Runs `xtask seed --out <out>` to completion and returns what the process produced.
fn run_seed(out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("seed")
        .arg("--out")
        .arg(out)
        .output()
        .unwrap_or_else(|err| panic!("cannot run xtask seed --out {}: {err}", out.display()))
}

/// The SQLite side file `<database><suffix>`, for `-wal` and `-shm`.
fn side_file(database: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(database.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// Asserts that neither `-wal` nor `-shm` exists beside `database`; `when` names the moment
/// in the message.
#[track_caller]
fn assert_no_side_files(database: &Path, when: &str) {
    for suffix in ["-wal", "-shm"] {
        let side = side_file(database, suffix);
        assert!(
            !side.exists(),
            "{} exists {when}; xtask seed must close the database cleanly",
            side.display()
        );
    }
}

/// `xtask seed --out <path>` exits 0 and writes a database that `Storage` opens with
/// nothing left to migrate and no book row, leaves no `-wal` or `-shm` beside it, and a
/// second run on the same path refuses to overwrite the file: exit 1, a message on stderr,
/// the file's bytes unchanged.
///
/// Proves: Decision 58.
#[test]
fn seed_writes_a_migrated_empty_book_and_no_wal_files() {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    let out = dir.path().join("seed.sqlite");

    let first = run_seed(&out);
    assert_eq!(
        first.status.code(),
        Some(0),
        "exit code of `xtask seed --out {}` ({}); stderr:\n{}",
        out.display(),
        first.status,
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        out.is_file(),
        "{} does not exist after xtask seed exited 0",
        out.display()
    );
    assert_no_side_files(&out, "after the first xtask seed run");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a current-thread tokio runtime");
    runtime.block_on(async {
        let storage = Storage::open(&out, &StorageOptions::default())
            .await
            .unwrap_or_else(|err| panic!("cannot open the seeded {}: {err}", out.display()));
        let report = storage
            .migrate()
            .await
            .unwrap_or_else(|err| panic!("cannot migrate the seeded {}: {err}", out.display()));
        assert_eq!(
            report.applied,
            0,
            "migrations applied on opening the seeded {}: it is not at the binary's schema",
            out.display()
        );
        let count = storage.count_books().await.unwrap_or_else(|err| {
            panic!("cannot count books in the seeded {}: {err}", out.display())
        });
        assert_eq!(count, 0, "book rows in the seeded {}", out.display());
        storage.close().await;
    });

    let before = fs::read(&out)
        .unwrap_or_else(|err| panic!("cannot read the seeded {}: {err}", out.display()));
    let second = run_seed(&out);
    assert_eq!(
        second.status.code(),
        Some(1),
        "exit code of a second `xtask seed --out {}` on an existing file ({}); stderr:\n{}",
        out.display(),
        second.status,
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !second.stderr.iter().all(u8::is_ascii_whitespace),
        "the second `xtask seed --out {}` exited 1 without a message on stderr",
        out.display()
    );
    let after = fs::read(&out)
        .unwrap_or_else(|err| panic!("cannot read the seeded {}: {err}", out.display()));
    assert!(
        before == after,
        "the second `xtask seed --out {}` changed the existing file's bytes",
        out.display()
    );
}
