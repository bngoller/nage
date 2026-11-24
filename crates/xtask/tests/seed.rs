//! `cargo xtask seed` observed from outside the process: the file it writes holds milestone 2's
//! made-up household, written through nage-core's operations at fixed instants, at the binary's
//! schema with no SQLite side files beside it; two runs write the same rows but for the hashes'
//! salts and sqlx's bookkeeping times; the book passes `nage check`; and an existing file is
//! refused (decisions 58 and 161).
//!
//! The expected household is written out here as literals, never read from the seed's own
//! constants, so that a change to the seed shows as a failing test.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use nage_core::accounts::{self, Account, Kind};
use nage_core::book;
use nage_core::check::{self, Outcome};
use nage_core::money::Money;
use nage_core::password::{self, Verification};
use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files, book_rows};
use nage_core::transactions;
use nage_core::users;

/// The one instant every row of the household is stamped with.
const NOW: &str = "2026-01-02T12:00:00Z";
/// The test password of both users, as `fixtures/README.md` writes it down.
const PASSWORD: &str = "household test password";

/// Runs `xtask seed --out <out>` to completion and returns what the process produced.
fn run_seed(out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("seed")
        .arg("--out")
        .arg(out)
        .output()
        .unwrap_or_else(|err| panic!("cannot run xtask seed --out {}: {err}", out.display()))
}

/// How many further seeds [`seed_into`] runs into fresh files of its own, each to be closed
/// cleanly. A seed that never closes its database leaves the `-wal` and `-shm` files only when its
/// process exits before the writer connection, closing last, has checkpointed; the process wins
/// or loses that race by chance, so one run alone cannot show a missing close, and several in a
/// row all but always do.
const CLOSE_RUNS: usize = 4;

/// Runs the seed into `out` and asserts that it exited 0, printed the path it wrote, left the
/// file and no side file; then runs it [`CLOSE_RUNS`] more times into fresh files of a folder of
/// its own and asserts that each exited 0 and left no side file.
#[track_caller]
fn seed_into(out: &Path) {
    let output = run_seed(out);
    assert_eq!(
        output.status.code(),
        Some(0),
        "exit code of `xtask seed --out {}` ({}); stderr:\n{}",
        out.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", out.display()),
        "what `xtask seed --out {}` printed: the path it wrote",
        out.display()
    );
    assert!(
        out.is_file(),
        "{} does not exist after xtask seed exited 0",
        out.display()
    );
    assert_no_side_files(
        out,
        "after an xtask seed run; xtask seed must close the database cleanly",
    );

    let spare = TestDir::new();
    for run in 1..=CLOSE_RUNS {
        let again = spare.path().join(format!("close-{run}.sqlite"));
        let output = run_seed(&again);
        assert_eq!(
            output.status.code(),
            Some(0),
            "exit code of `xtask seed --out {}` ({}); stderr:\n{}",
            again.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_side_files(
            &again,
            "after a further xtask seed run; xtask seed must close the database cleanly every time",
        );
    }
}

/// Opens the seeded file through nage-core's storage.
async fn open(out: &Path) -> Storage {
    Storage::open(out, &StorageOptions::default())
        .await
        .unwrap_or_else(|err| panic!("cannot open the seeded {}: {err}", out.display()))
}

/// `xtask seed --out <path>` creates the missing folders of the path, exits 0, prints the path
/// and writes a database that `Storage` opens with nothing left to migrate, holding the
/// household's book ("Household" in CAD) and its two users, leaves no `-wal` or `-shm` beside
/// it, and a second run on the same path refuses to overwrite the file: exit 1, the refusal
/// naming the existing file on stderr, the file's bytes unchanged. A dangling symbolic link and a
/// folder at the path are refused the same way, nothing written through the link; a parent folder
/// that cannot be created and a path under a file each fail with their own line, pinned whole.
///
/// Proves: decisions 58 and 161 (the household of v0.2.0, no side files, an existing file refused)
#[test]
fn seed_writes_the_household_through_core_operations_and_no_wal_files() {
    let dir = TestDir::new();
    // Two folders that do not exist yet: the seed creates the whole parent path.
    let out = dir.path().join("books").join("m2").join("seed.sqlite");

    seed_into(&out);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a current-thread tokio runtime");
    runtime.block_on(async {
        let storage = open(&out).await;
        let report = storage
            .migrate()
            .await
            .unwrap_or_else(|err| panic!("cannot migrate the seeded {}: {err}", out.display()));
        assert_eq!(
            report.applied,
            0,
            "changes applied on opening the seeded {}: it is not at the binary's schema",
            out.display()
        );
        let count = storage.count_books().await.unwrap_or_else(|err| {
            panic!("cannot count books in the seeded {}: {err}", out.display())
        });
        assert_eq!(count, 1, "book rows in the seeded {}", out.display());
        let mut conn = storage
            .reader()
            .acquire()
            .await
            .unwrap_or_else(|err| panic!("cannot acquire a reader: {err}"));
        let found = book::book(&mut conn)
            .await
            .unwrap_or_else(|err| panic!("cannot read the book: {err}"))
            .unwrap_or_else(|| panic!("the seeded {} holds no book", out.display()));
        assert_eq!(found.name, "Household", "the seeded book's name");
        assert_eq!(found.currency_code, "CAD", "the seeded book's currency");
        for username in ["sam", "alex"] {
            assert!(
                users::user_by_username(&mut conn, username)
                    .await
                    .unwrap_or_else(|err| panic!("cannot look {username} up: {err}"))
                    .is_some(),
                "the seeded book has no user {username}"
            );
        }
        drop(conn);
        storage.close().await;
    });
    assert_no_side_files(&out, "after the seeded file was opened and closed again");

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
    let refusal = String::from_utf8_lossy(&second.stderr);
    assert_eq!(
        refusal,
        format!(
            "xtask seed: {} already exists; seed never overwrites, delete it first\n",
            out.display()
        ),
        "the second `xtask seed --out {}` was not refused for the existing file",
        out.display()
    );
    let after = fs::read(&out)
        .unwrap_or_else(|err| panic!("cannot read the seeded {}: {err}", out.display()));
    assert!(
        before == after,
        "the second `xtask seed --out {}` changed the existing file's bytes",
        out.display()
    );

    // A dangling symbolic link at the path is refused as existing, never followed: nothing is
    // created at its target.
    let link = dir.path().join("link.sqlite");
    let target = dir.path().join("target.sqlite");
    std::os::unix::fs::symlink(&target, &link)
        .unwrap_or_else(|err| panic!("cannot link {}: {err}", link.display()));
    assert_eq!(
        refused(&link),
        format!(
            "xtask seed: {} already exists; seed never overwrites, delete it first\n",
            link.display()
        ),
        "the refusal of a dangling link at the path"
    );
    assert!(
        fs::symlink_metadata(&target).is_err(),
        "the seed wrote through the dangling link to {}",
        target.display()
    );

    // A folder at the path is refused as existing.
    let folder = dir.path().join("folder.sqlite");
    fs::create_dir(&folder)
        .unwrap_or_else(|err| panic!("cannot create {}: {err}", folder.display()));
    assert_eq!(
        refused(&folder),
        format!(
            "xtask seed: {} already exists; seed never overwrites, delete it first\n",
            folder.display()
        ),
        "the refusal of a folder at the path"
    );

    // A parent folder that cannot be created: its own parent may be read but not written.
    let closed = dir.path().join("closed");
    fs::create_dir(&closed)
        .unwrap_or_else(|err| panic!("cannot create {}: {err}", closed.display()));
    set_mode(&closed, 0o500);
    let inside = closed.join("books").join("seed.sqlite");
    let message = refused(&inside);
    set_mode(&closed, 0o700);
    assert_eq!(
        message,
        format!(
            "xtask seed: cannot create directory {}: Permission denied (os error 13)\n",
            closed.join("books").display()
        ),
        "the failure of a parent folder that cannot be created"
    );

    // A path whose parent is a file cannot be inspected.
    let file = dir.path().join("notes.txt");
    fs::write(&file, b"notes")
        .unwrap_or_else(|err| panic!("cannot write {}: {err}", file.display()));
    let under_a_file = file.join("seed.sqlite");
    assert_eq!(
        refused(&under_a_file),
        format!(
            "xtask seed: cannot inspect {}: Not a directory (os error 20)\n",
            under_a_file.display()
        ),
        "the failure of a path under a file"
    );
}

/// Sets the permission bits of `path` to `bits`.
fn set_mode(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(bits))
        .unwrap_or_else(|err| panic!("cannot set the mode of {}: {err}", path.display()));
}

/// Runs the seed into `out`, asserts that it exited 1 and printed nothing on standard output, and
/// returns what it printed on standard error.
#[track_caller]
fn refused(out: &Path) -> String {
    let output = run_seed(out);
    assert_eq!(
        output.status.code(),
        Some(1),
        "exit code of `xtask seed --out {}` ({}); stderr:\n{}",
        out.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "`xtask seed --out {}` printed on stdout:\n{}",
        out.display(),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Two seeds into two fresh files give the same rows, as `test_support::book_rows` reads them
/// (the password hashes and sqlx's bookkeeping times left out): every row is stamped at fixed
/// instants, never from the clock, so every created, updated and archived time of the book, the
/// users, the accounts and the transactions is the one instant 2026-01-02T12:00:00Z, and every
/// opening balance is dated 2026-01-01.
///
/// Proves: decision 161 (fixed instants, the clock never read; hashes and bookkeeping times left out)
#[tokio::test]
async fn two_seeds_write_the_same_rows() {
    let dir = TestDir::new();
    let first = dir.path().join("first.sqlite");
    let second = dir.path().join("second.sqlite");
    seed_into(&first);
    seed_into(&second);

    let first_rows = book_rows(&first).await;
    let second_rows = book_rows(&second).await;
    assert_eq!(
        first_rows, second_rows,
        "the rows of two seeds of the household"
    );
    assert!(
        first_rows.iter().any(|line| line.starts_with("postings ")),
        "the seeded book holds no posting: {first_rows:#?}"
    );

    let mut stamps = 0_usize;
    let mut dates = 0_usize;
    for line in first_rows
        .iter()
        .filter(|line| !line.starts_with("_sqlx_migrations "))
    {
        for field in line.split(' ') {
            // A nullable value is shown quoted (its `Debug` text), a required one bare.
            let Some((name, value)) = field
                .split_once('=')
                .map(|(name, value)| (name, value.trim_matches('"')))
            else {
                continue;
            };
            if name.ends_with("_at") {
                assert!(
                    value == "NULL" || value == NOW,
                    "{name}={value} is not the seed's fixed instant {NOW}: {line}"
                );
                if value == NOW {
                    stamps = stamps.saturating_add(1);
                }
            }
            if name == "date" {
                assert_eq!(
                    value, "2026-01-01",
                    "a transaction not dated as the opening balances are: {line}"
                );
                dates = dates.saturating_add(1);
            }
        }
    }
    assert!(stamps > 0, "no stamp was read in {first_rows:#?}");
    assert_eq!(
        dates, 6,
        "transactions in the seeded book (one per opening balance): {first_rows:#?}"
    );
}

/// An account the household must hold: its full name, its kind, its owner's username (`None`:
/// joint), its opening balance in natural sign in cents (`None`: none entered) and whether it
/// is archived.
type Expected = (&'static str, Kind, Option<&'static str>, Option<i64>, bool);

/// The seeded book passes every rule of `nage check` (checked, not "nothing to check") and holds
/// the household with its awkward corners: sam the admin and alex added by sam, each signing in
/// with the test password, hashed with SPEC 5.3's parameters (m=19456, t=2, p=1); the renamed
/// Chequing, Wallet and Credit card (its capitals only); the accounts made, with their owners
/// (none meaning joint); the opening balances in natural sign, Overdrawn account below zero and
/// Wallet without one; Old account and the category Gifts archived; Pets added, Dining renamed
/// Eating out, and one category whose name has 64 characters.
///
/// Proves: decision 161 (the awkward corners, every scenario a nage check subject; SPEC 10.1)
#[tokio::test]
async fn the_seeded_book_passes_the_check_and_holds_the_household() {
    let dir = TestDir::new();
    let out = dir.path().join("household.sqlite");
    seed_into(&out);

    let storage = open(&out).await;
    let mut conn = storage
        .reader()
        .acquire()
        .await
        .unwrap_or_else(|err| panic!("cannot acquire a reader: {err}"));

    let report = check::run(&mut conn)
        .await
        .unwrap_or_else(|err| panic!("the check could not run: {err}"));
    match &report.outcome {
        Outcome::NotInitialized => panic!("the check found no book in the seeded file"),
        Outcome::Checked { violations } => assert!(
            violations.is_empty(),
            "the seeded book breaks rules:\n{}",
            violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }

    let sam = users::user_by_username(&mut conn, "sam")
        .await
        .unwrap_or_else(|err| panic!("cannot look sam up: {err}"))
        .unwrap_or_else(|| panic!("the seeded book has no user sam"));
    let alex = users::user_by_username(&mut conn, "alex")
        .await
        .unwrap_or_else(|err| panic!("cannot look alex up: {err}"))
        .unwrap_or_else(|| panic!("the seeded book has no user alex"));
    assert_eq!(sam.user.display_name, "Sam", "sam's display name");
    assert!(sam.user.is_admin, "sam is not an admin");
    assert_eq!(
        sam.user.created_by, None,
        "sam, the first admin, has a creator"
    );
    assert_eq!(alex.user.display_name, "Alex", "alex's display name");
    assert!(!alex.user.is_admin, "alex is an admin");
    assert_eq!(
        alex.user.created_by,
        Some(sam.user.id),
        "alex was not added by sam"
    );
    for (name, credentials) in [("sam", &sam), ("alex", &alex)] {
        assert!(
            credentials
                .password_hash
                .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{name}'s hash is not made with SPEC 5.3's parameters"
        );
        assert_eq!(
            password::verify(PASSWORD, &credentials.password_hash),
            Verification::Verified,
            "{name} does not sign in with the test password"
        );
    }

    let expected: [Expected; 15] = [
        ("Assets:Chequing", Kind::Asset, None, Some(245_000), false),
        ("Assets:Savings", Kind::Asset, None, None, false),
        (
            "Assets:Savings:Holiday fund",
            Kind::Asset,
            None,
            Some(120_000),
            false,
        ),
        (
            "Assets:Savings:Emergency fund",
            Kind::Asset,
            Some("sam"),
            Some(800_000),
            false,
        ),
        ("Assets:Wallet", Kind::Asset, Some("alex"), None, false),
        (
            "Assets:Overdrawn account",
            Kind::Asset,
            Some("sam"),
            Some(-31_040),
            false,
        ),
        ("Assets:Old account", Kind::Asset, Some("alex"), None, true),
        (
            "Liabilities:Credit card",
            Kind::Liability,
            Some("alex"),
            Some(64_025),
            false,
        ),
        (
            "Liabilities:Car loan",
            Kind::Liability,
            None,
            Some(1_200_000),
            false,
        ),
        ("Expenses:Pets", Kind::Expense, None, None, false),
        ("Expenses:Eating out", Kind::Expense, None, None, false),
        ("Expenses:Gifts", Kind::Expense, None, None, true),
        ("Expenses:Groceries", Kind::Expense, None, None, false),
        ("Income:Salary", Kind::Income, None, None, false),
        (
            "Expenses:Repairs to the old farmhouse roof, gutters, porch and back fence",
            Kind::Expense,
            None,
            None,
            false,
        ),
    ];
    for (full_name, kind, owner, opening, archived) in expected {
        let account: Account = accounts::resolve(&mut conn, full_name)
            .await
            .unwrap_or_else(|err| panic!("cannot resolve {full_name}: {err}"))
            .unwrap_or_else(|| panic!("the seeded book has no account {full_name}"));
        let own_name = full_name.rsplit(':').next().unwrap_or(full_name);
        assert_eq!(
            account.name, own_name,
            "the name of {full_name} as stored, its capitals included"
        );
        assert_eq!(account.kind, kind, "the kind of {full_name}");
        let owner_id = owner.map(|name| {
            if name == "sam" {
                sam.user.id
            } else {
                alex.user.id
            }
        });
        assert_eq!(account.owner_user_id, owner_id, "the owner of {full_name}");
        assert_eq!(
            account.is_archived(),
            archived,
            "whether {full_name} is archived"
        );
        let entered = transactions::opening_balance(&mut conn, account.id)
            .await
            .unwrap_or_else(|err| panic!("cannot read the opening balance of {full_name}: {err}"));
        match (opening, entered) {
            (None, None) => {}
            (Some(cents), Some(entry)) => {
                assert_eq!(
                    entry.amount,
                    Money::from_minor(cents),
                    "the opening balance of {full_name}"
                );
                assert_eq!(
                    entry.date.to_string(),
                    "2026-01-01",
                    "the date of {full_name}'s opening balance"
                );
            }
            (want, got) => panic!("the opening balance of {full_name}: want {want:?}, got {got:?}"),
        }
    }

    let long = "Repairs to the old farmhouse roof, gutters, porch and back fence";
    assert_eq!(long.chars().count(), 64, "the long category's name length");
    for gone in ["Assets:Checking", "Assets:Cash", "Expenses:Dining"] {
        assert!(
            accounts::resolve(&mut conn, gone)
                .await
                .unwrap_or_else(|err| panic!("cannot resolve {gone}: {err}"))
                .is_none(),
            "{gone} is still in the seeded book; it was to be renamed"
        );
    }

    drop(conn);
    storage.close().await;
}
