//! The committed book of the current milestone against a fresh seed (assumption A15 of PLAN M2
//! slice 8, decision 161): `cargo xtask seed` run into a temporary folder writes the same rows as
//! `fixtures/books/v0.2.0.sqlite` holds, bookkeeping times, salts and hashes left out, so a seed
//! changed without its book regenerated fails here. The committed book is read through a copy only.

use std::path::Path;
use std::process::Command;

use nage_core::password::{self, Verification};
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files, book_rows, fixture_copy};
use nage_core::users;

/// The fixtures' README, relative to this crate's manifest directory.
const README: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/README.md");

/// The committed book of milestone 2, relative to this crate's manifest directory.
const BOOK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/books/v0.2.0.sqlite"
);

/// Runs `xtask seed --out <out>` and asserts that it exited 0 and left the file.
#[track_caller]
fn seed_into(out: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("seed")
        .arg("--out")
        .arg(out)
        .output()
        .unwrap_or_else(|err| panic!("cannot run xtask seed --out {}: {err}", out.display()));
    assert_eq!(
        output.status.code(),
        Some(0),
        "exit code of `xtask seed --out {}` ({}); stderr:\n{}",
        out.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.is_file(),
        "{} does not exist after xtask seed exited 0",
        out.display()
    );
}

/// The test password `fixtures/README.md` prints: the text between the backticks after "the test
/// password".
fn documented_password() -> String {
    let readme =
        std::fs::read_to_string(README).unwrap_or_else(|err| panic!("cannot read {README}: {err}"));
    let after = readme
        .split_once("the test password `")
        .map(|(_, after)| after)
        .unwrap_or_else(|| panic!("{README} prints no test password"));
    let (password, _) = after
        .split_once('`')
        .unwrap_or_else(|| panic!("{README}'s test password has no closing backtick"));
    password.to_owned()
}

/// A fresh seed and a copy of the committed `fixtures/books/v0.2.0.sqlite` hold the same rows, as
/// `test_support::book_rows` reads them (sqlx's bookkeeping times and the password hashes with
/// their salts left out), table for table and row for row; the committed file gets no side file.
///
/// Proves: decision 161 (assumption A15: a stale committed book fails)
#[tokio::test]
async fn the_committed_v0_2_0_book_equals_a_fresh_seed() {
    let dir = TestDir::new();
    let fresh = dir.path().join("fresh.sqlite");
    seed_into(&fresh);
    let (_copy_dir, copy) = fixture_copy("0.2.0");

    let seeded = book_rows(&fresh).await;
    let committed = book_rows(&copy).await;
    assert!(
        committed.iter().any(|line| line.starts_with("postings ")),
        "the committed book holds no posting: {committed:#?}"
    );
    assert_eq!(
        committed, seeded,
        "the rows of the committed v0.2.0 book against a fresh seed; write the book again with the seed"
    );
    assert_no_side_files(
        Path::new(BOOK),
        "beside the committed book after its copy was read",
    );
}

/// Each user of a copy of the committed `fixtures/books/v0.2.0.sqlite` (sam and alex) signs in with
/// the test password `fixtures/README.md` prints, and each stored hash names SPEC 5.3's parameters
/// (m=19456, t=2, p=1), so a copied book carries no weak hash and no hash of another password.
///
/// Proves: decision 161, ruling P10 (each user of v0.2.0 signs in with the password fixtures/README.md prints; each hash carries m=19456, t=2, p=1)
#[tokio::test]
async fn the_committed_books_users_sign_in_with_the_documented_password() {
    let password = documented_password();
    assert!(
        !password.is_empty(),
        "{README} prints an empty test password"
    );
    let (_copy_dir, copy) = fixture_copy("0.2.0");
    let storage = Storage::open(&copy, &StorageOptions::default())
        .await
        .unwrap_or_else(|err| panic!("cannot open the copy {}: {err}", copy.display()));
    let mut conn = storage
        .reader()
        .acquire()
        .await
        .unwrap_or_else(|err| panic!("cannot acquire a reader: {err}"));
    for username in ["sam", "alex"] {
        let credentials = users::user_by_username(&mut conn, username)
            .await
            .unwrap_or_else(|err| panic!("cannot look {username} up: {err}"))
            .unwrap_or_else(|| panic!("the committed book has no user {username}"));
        assert!(
            credentials
                .password_hash
                .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{username}'s stored hash does not carry SPEC 5.3's parameters"
        );
        assert_eq!(
            password::verify(&password, &credentials.password_hash),
            Verification::Verified,
            "{username} does not sign in with the password {README} prints"
        );
        assert_eq!(
            password::verify("another long password", &credentials.password_hash),
            Verification::WrongPassword,
            "{username} signs in with another password"
        );
    }
    drop(conn);
    storage.close().await;
    assert_no_side_files(
        Path::new(BOOK),
        "beside the committed book after its copy was opened",
    );
}
