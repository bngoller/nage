//! `nage migrate` observed from outside the process (decision 156): every state pinned as a text
//! snapshot of its transcript (decision 157), the refusal while a server runs on the data
//! folder, the dry run beside a running server, a missing database refused with nothing created, an
//! empty database file finished, and exit 1 when the check after the update finds broken rules. The
//! data folder of a test is shown as `/srv/nage/data`, the version as `[version]`, the stamp of a
//! backup the binary wrote as `[STAMP]`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nage_core::test_support::TestDir;
use nage_core::timestamp::Timestamp;

use common::commands::{self, block_on, hold_mark, plant};
use common::transcript::{new_stamps, transcript};
use common::web::PASSWORD;
use common::{Finished, ServeProcess, VERSION, nage, run};

/// The stamp of the older copy the tests plant: a copy of the same step, written long before.
const OLDER_STAMP: &str = "2026-01-01T00-00-00Z";

/// Asserts that `out` ended with the exit code `code`, showing what it printed otherwise (a stub's
/// panic shows only on standard error).
#[track_caller]
fn assert_exit(out: &Finished, code: i32) {
    assert_eq!(
        out.code(),
        Some(code),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
        out.stderr
    );
}

/// Asserts that a refused or failed command printed nothing on standard output, and a finished one
/// nothing on standard error.
#[track_caller]
fn assert_streams(out: &Finished) {
    if out.code() == Some(0) {
        assert!(
            out.stderr.is_empty(),
            "stderr of `{}`:\n{}",
            out.what,
            out.stderr
        );
    } else {
        assert!(
            out.stdout.is_empty(),
            "stdout of `{}`:\n{}",
            out.what,
            out.stdout
        );
    }
}

/// The data folder of the working directory `dir`.
fn data(dir: &Path) -> PathBuf {
    dir.join("data")
}

/// Runs `nage migrate --data-dir <dir>/data` (with `--dry-run` when `dry_run`) in `dir`: the process
/// and its transcript, the working directory shown as `/srv/nage` and each backup stamp the process
/// printed, but the older copy's, as `[STAMP]`.
fn migrate(dir: &Path, dry_run: bool) -> (Finished, String) {
    let mut command = nage();
    command
        .current_dir(dir)
        .arg("migrate")
        .arg("--data-dir")
        .arg(data(dir));
    if dry_run {
        command.arg("--dry-run");
    }
    let shown = transcript(&command).dir(dir);
    let out = run(&mut command);
    let printed = format!("{}{}", out.stdout, out.stderr);
    let transcript = shown.stamps(&new_stamps(&printed, &[OLDER_STAMP])).of(&out);
    (out, transcript)
}

/// The names of the files in the backups folder of `dir`, sorted; none when it is missing.
fn backups(dir: &Path) -> Vec<String> {
    let folder = data(dir).join("backups");
    let Ok(entries) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", folder.display()))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The name of the older copy of the step 0001 the tests plant beside the backups.
fn older_copy() -> String {
    format!("{OLDER_STAMP}-pre-migrate-0001-to-{VERSION}.sqlite")
}

/// Plants the older copy of the step 0001 in the backups folder of `dir`.
fn plant_older_copy(dir: &Path) {
    let folder = data(dir).join("backups");
    fs::create_dir_all(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    fs::write(folder.join(older_copy()), b"an older copy")
        .unwrap_or_else(|error| panic!("cannot plant the older copy: {error}"));
}

/// The file names of the backups `nage migrate` would write of the step 0001 from `before` seconds
/// before now to `after` seconds after it: the names a test plants to take the backup's name.
fn names_around_now(before: i64, after: i64) -> Vec<String> {
    let now = jiff::Timestamp::now().as_second();
    (now - before..=now + after)
        .map(|second| {
            let stamp = Timestamp::from_second(second)
                .unwrap_or_else(|error| panic!("{second} is no instant of Nage: {error}"));
            nage_core::backup::name(stamp, 1, VERSION)
        })
        .collect()
}

/// Sets the mode of `path` to `mode`.
fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("cannot change the mode of {}: {error}", path.display()));
}

/// Every state of `nage migrate`, run as a process and pinned as the text of its transcript: a
/// database one change behind updated after its backup (exit 0), the mark's file left holding
/// `migrate`; the same update whose backup replaced the older copy of its step, which is named
/// (exit 0); one already up to date whose book
/// passes (exit 0, nothing changed); and every refusal and failure, exit 1, with nothing changed: no
/// database, the mark held by `nage migrate` and by a holder whose word cannot be read, a file at
/// the backup's name, a backups folder Nage may not write in, a change that fails after the backup
/// and the removal of the older copy of its step, a changed record of an applied change, a file
/// that holds tables but is not Nage's, and a database a newer Nage wrote (with change 9999, and
/// with change 3, named 0003). A data folder given relative to the working directory is updated
/// and named by its absolute path.
///
/// Proves: decisions 151 to 157 (every state of nage migrate pinned as a text snapshot, the holders of the mark, a file at the backup's name, a changed record, a file that is not Nage's)
#[test]
fn migrate_reads_as_its_transcripts() {
    // A database one change behind: updated, after its backup.
    let dir = commands::behind();
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(2));
    assert_eq!(backups(dir.path()).len(), 1, "the backups after the update");
    // The mark's file stays, holding the word of its last holder.
    assert_eq!(
        fs::read_to_string(data(dir.path()).join("nage.lock")).unwrap_or_default(),
        "migrate",
        "the mark's file after the update"
    );
    insta::assert_snapshot!("migrate_updated", transcript);

    // An update whose backup replaced the older copy of its step: the removed copy is named.
    let dir = commands::behind();
    plant_older_copy(dir.path());
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(2));
    let kept = backups(dir.path());
    assert!(
        kept.len() == 1 && !kept.contains(&older_copy()),
        "the backups after the update: {kept:?}"
    );
    insta::assert_snapshot!("migrate_updated_replacing_an_older_copy", transcript);

    // Up to date, the book passing: nothing changed.
    let (dir, _) = commands::set_up();
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    insta::assert_snapshot!("migrate_up_to_date", transcript);

    // No database at all.
    let dir = TestDir::new();
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    insta::assert_snapshot!("migrate_no_database", transcript);

    // The mark held by another nage migrate, and by a holder whose word cannot be read.
    for (word, name) in [
        ("migrate", "migrate_migrate_running"),
        ("", "migrate_folder_in_use"),
    ] {
        let dir = commands::behind();
        let mark = hold_mark(&data(dir.path()), word);
        let (out, transcript) = migrate(dir.path(), false);
        drop(mark);
        assert_exit(&out, 1);
        assert_streams(&out);
        assert_eq!(commands::schema_version(dir.path()), Some(1), "{name}");
        assert!(
            backups(dir.path()).is_empty(),
            "{name}: a backup was written"
        );
        insta::assert_snapshot!(name, transcript);
    }

    // A file at the backup's name: every name of the seconds around the run is taken.
    let dir = commands::behind();
    let folder = data(dir.path()).join("backups");
    fs::create_dir_all(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    let taken = names_around_now(2, 8);
    for name in &taken {
        fs::write(folder.join(name), b"")
            .unwrap_or_else(|error| panic!("cannot plant {name}: {error}"));
    }
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    assert_eq!(backups(dir.path()).len(), taken.len(), "the backups folder");
    insta::assert_snapshot!("migrate_backup_name_taken", transcript);

    // A backups folder Nage may not write in.
    let dir = commands::behind();
    let folder = data(dir.path()).join("backups");
    fs::create_dir_all(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    set_mode(&folder, 0o500);
    let (out, transcript) = migrate(dir.path(), false);
    set_mode(&folder, 0o700);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    insta::assert_snapshot!("migrate_backup_failed", transcript);

    // A change that fails after the backup and the removal of the older copy of its step: the
    // database stays exactly as it was.
    let dir = commands::behind();
    plant(
        &commands::database(dir.path()),
        &["CREATE TABLE accounts (planted INTEGER)"],
    );
    plant_older_copy(dir.path());
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    let kept = backups(dir.path());
    assert!(
        kept.len() == 1 && !kept.contains(&older_copy()),
        "the backups after the failed change: {kept:?}"
    );
    insta::assert_snapshot!("migrate_change_failed", transcript);

    // A changed record of an applied change.
    let dir = commands::migrated();
    plant(
        &commands::database(dir.path()),
        &["UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 1"],
    );
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    insta::assert_snapshot!("migrate_changed_record", transcript);

    // A file that holds tables but is not Nage's.
    let dir = TestDir::new();
    fs::create_dir_all(data(dir.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    plant(
        &commands::database(dir.path()),
        &["CREATE TABLE shopping (item TEXT NOT NULL)"],
    );
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    insta::assert_snapshot!("migrate_not_a_nage_database", transcript);

    // A database a newer Nage wrote.
    let dir = commands::ahead();
    let before = fs::read(commands::database(dir.path()))
        .unwrap_or_else(|error| panic!("cannot read the database: {error}"));
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(
        fs::read(commands::database(dir.path()))
            .unwrap_or_else(|error| panic!("cannot read the database: {error}")),
        before,
        "the newer database was written"
    );
    insta::assert_snapshot!("migrate_database_newer", transcript);

    // A database the next version of Nage wrote: its change 3 named in four digits, as this
    // version's own 0002 is on the same line.
    let dir = commands::ahead_by_one();
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    insta::assert_snapshot!("migrate_database_newer_by_one", transcript);

    // A data folder given relative to the working directory: the database, the backup and the
    // folder are named by their absolute paths, as the server and the dry run name them.
    let dir = commands::behind();
    let (out, transcript) = migrate_relative(dir.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert_eq!(commands::schema_version(dir.path()), Some(2));
    insta::assert_snapshot!("migrate_updated_relative", transcript);
}

/// Runs `nage migrate --data-dir data` (with `--dry-run` when `dry_run`) in `dir`, the data folder
/// given relative to the working directory: the process and its transcript, as [`migrate`] shows
/// them.
fn migrate_relative(dir: &Path, dry_run: bool) -> (Finished, String) {
    let mut command = nage();
    command
        .current_dir(dir)
        .args(["migrate", "--data-dir", "data"]);
    if dry_run {
        command.arg("--dry-run");
    }
    let shown = transcript(&command).dir(dir);
    let out = run(&mut command);
    let printed = format!("{}{}", out.stdout, out.stderr);
    let transcript = shown.stamps(&new_stamps(&printed, &[OLDER_STAMP])).of(&out);
    (out, transcript)
}

/// `nage migrate --dry-run` lists what waits and changes nothing, exit 0: the change a database one
/// behind waits for, with the version and where a backup would be written; nothing waiting on a
/// database at this version; and every change on an empty database (a one-page file without
/// tables), which needs no backup. The files stay byte for byte as they were, and no mark is taken.
/// A data folder given relative to the working directory is named by its absolute path.
///
/// Proves: decision 156 (the dry run lists what waits, an empty database included, and changes nothing)
#[test]
fn migrate_dry_run_reads_as_its_transcripts() {
    let behind = commands::behind();
    let migrated = commands::migrated();
    let empty = TestDir::new();
    fs::create_dir_all(data(empty.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    commands::one_page_without_tables(&commands::database(empty.path()));
    for (name, dir) in [
        ("migrate_dry_run_waiting", &behind),
        ("migrate_dry_run_nothing_waiting", &migrated),
        ("migrate_dry_run_empty", &empty),
    ] {
        let database = commands::database(dir.path());
        let before = fs::read(&database)
            .unwrap_or_else(|error| panic!("{name}: cannot read the database: {error}"));
        let (out, transcript) = migrate(dir.path(), true);
        assert_exit(&out, 0);
        assert_streams(&out);
        assert_eq!(
            fs::read(&database)
                .unwrap_or_else(|error| panic!("{name}: cannot read the database: {error}")),
            before,
            "{name}: the dry run changed the database"
        );
        assert!(
            !data(dir.path()).join("nage.lock").exists(),
            "{name}: the dry run took the mark"
        );
        assert!(
            backups(dir.path()).is_empty(),
            "{name}: the dry run wrote a backup"
        );
        insta::assert_snapshot!(name, transcript);
    }

    // A data folder given relative to the working directory: where a backup would go is named by
    // its absolute path.
    let behind = commands::behind();
    let (out, transcript) = migrate_relative(behind.path(), true);
    assert_exit(&out, 0);
    assert_streams(&out);
    insta::assert_snapshot!("migrate_dry_run_waiting_relative", transcript);
}

/// A dry run that could not look has a first line of its own, "The waiting changes could not be
/// listed.", and exits 1: no database, one a newer Nage wrote, a file that cannot be opened as
/// a database (left exactly as it was), and a database whose record of change 2 differs, refused
/// as the real run refuses it (left exactly as it was, no mark taken, no backup written).
///
/// Proves: decision 156, ruling P7 (no database, a newer one, one that cannot be opened: a first line of their own, exit 1)
#[test]
fn migrate_dry_run_refusals_read_as_their_transcripts() {
    let none = TestDir::new();
    let ahead = commands::ahead();
    let garbage = TestDir::new();
    fs::create_dir_all(data(garbage.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    let bytes = "This is a shopping list, not a database.\n".repeat(64);
    fs::write(commands::database(garbage.path()), &bytes)
        .unwrap_or_else(|error| panic!("cannot write the stray file: {error}"));
    // A database at this version whose record of change 2 differs: refused as the real run
    // refuses it, also when nothing waits (the maintainer's answer "Refuse, like the real run").
    let changed = commands::migrated();
    plant(
        &commands::database(changed.path()),
        &["UPDATE _sqlx_migrations SET checksum = x'00' WHERE version = 2"],
    );
    let changed_bytes = fs::read(commands::database(changed.path()))
        .unwrap_or_else(|error| panic!("cannot read the changed database: {error}"));
    for (name, dir) in [
        ("migrate_dry_run_no_database", &none),
        ("migrate_dry_run_database_newer", &ahead),
        ("migrate_dry_run_cannot_be_opened", &garbage),
        ("migrate_dry_run_changed_record", &changed),
    ] {
        let (out, transcript) = migrate(dir.path(), true);
        assert_exit(&out, 1);
        assert_streams(&out);
        assert!(
            out.stderr
                .starts_with("Error: The waiting changes could not be listed.\n"),
            "{name}: the first line:\n{}",
            out.stderr
        );
        insta::assert_snapshot!(name, transcript);
    }
    assert!(
        !data(none.path()).exists(),
        "the dry run created the data folder"
    );
    assert_eq!(
        fs::read(commands::database(garbage.path()))
            .unwrap_or_else(|error| panic!("cannot read the stray file: {error}")),
        bytes.as_bytes(),
        "the dry run changed the file that is not a database"
    );
    assert_eq!(
        fs::read(commands::database(changed.path()))
            .unwrap_or_else(|error| panic!("cannot read the changed database: {error}")),
        changed_bytes,
        "the dry run changed the database whose record differs"
    );
    assert!(
        !data(changed.path()).join("nage.lock").exists() && backups(changed.path()).is_empty(),
        "the dry run took the mark or wrote a backup beside the database whose record differs"
    );
}

/// On a database at this version whose book breaks rules, `nage migrate` changes nothing and
/// exits 1: "The database is up to date, but the book did not pass every check", the real lines
/// nage-core writes for the planted corruption under a bare `Why:`, never "The database was
/// updated", and no backup.
///
/// Proves: decision 156, ruling P7 (The database is up to date, but the book did not pass every check; never The database was updated)
#[test]
fn migrate_with_nothing_waiting_exits_1_on_a_book_that_breaks_rules() {
    let dir = TestDir::new();
    commands::broken_book_in(dir.path());
    let before = fs::read(commands::database(dir.path()))
        .unwrap_or_else(|error| panic!("cannot read the database: {error}"));
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(
        !out.stderr.contains("The database was updated"),
        "nothing waited, yet:\n{}",
        out.stderr
    );
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    assert_eq!(
        fs::read(commands::database(dir.path()))
            .unwrap_or_else(|error| panic!("cannot read the database: {error}")),
        before,
        "the database was changed"
    );
    insta::assert_snapshot!("migrate_up_to_date_broken", transcript);
}

/// `nage migrate` beside a running server is refused, exit 1, naming the server, and changes
/// nothing: no backup, the database still at its version, the server still answering.
///
/// Proves: decision 153 (nage migrate refuses beside a running server and changes nothing)
#[test]
fn migrate_refuses_while_a_server_runs_on_the_folder() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let dir = server.dir().to_path_buf();
    let (out, transcript) = migrate(&dir, false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(backups(&dir).is_empty(), "a backup was written");
    let health = common::http_get(addr, "/health", &[]);
    assert_eq!(health.status, 200, "GET /health after the refusal");
    let status = server.stop();
    assert!(status.success(), "the server stopped with {status}");
    assert_eq!(commands::schema_version(&dir), Some(2));
    insta::assert_snapshot!("migrate_server_running", transcript);
}

/// `nage migrate --dry-run` takes no mark: beside a running server it lists what waits, exit 0.
///
/// Proves: decisions 153 and 156 (the dry run takes no mark)
#[test]
fn migrate_dry_run_works_beside_a_running_server() {
    let mut server = ServeProcess::spawn();
    server.wait_listening();
    let dir = server.dir().to_path_buf();
    let (out, transcript) = migrate(&dir, true);
    assert_exit(&out, 0);
    assert_streams(&out);
    let status = server.stop();
    assert!(status.success(), "the server stopped with {status}");
    insta::assert_snapshot!("migrate_dry_run_nothing_waiting", transcript);
}

/// `nage migrate` refuses a folder without a database before anything else and creates nothing:
/// not the data folder when it is missing, and nothing in it, not even the mark's file, when it is
/// there and empty.
///
/// Proves: decision 156, ruling P5 (assumption A11: nothing created, not even the mark's file)
#[test]
fn migrate_refuses_a_missing_database_and_creates_nothing() {
    let dir = TestDir::new();
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert!(
        !data(dir.path()).exists(),
        "the refusal created the data folder"
    );
    insta::assert_snapshot!("migrate_no_database", transcript);

    let dir = TestDir::new();
    fs::create_dir_all(data(dir.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    let (out, again) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(again, transcript, "the refusal in an empty data folder");
    let left: Vec<_> = fs::read_dir(data(dir.path()))
        .unwrap_or_else(|error| panic!("cannot read the data folder: {error}"))
        .collect();
    assert!(
        left.is_empty(),
        "the refusal created {} file(s) in the data folder",
        left.len()
    );
}

/// An empty database a cut-off `nage init` left is finished by `nage migrate` like a new one, with
/// no backup: a zero-byte file and the one-page file without tables alike, each brought to this
/// version and found to hold no book.
///
/// Proves: decision 155, ruling P6 (an empty file finished by nage migrate)
#[test]
fn migrate_finishes_an_empty_database_file() {
    let zero = TestDir::new();
    fs::create_dir_all(data(zero.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    fs::write(commands::database(zero.path()), b"")
        .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
    let (out, transcript) = migrate(zero.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert_eq!(commands::schema_version(zero.path()), Some(2));
    assert!(backups(zero.path()).is_empty(), "a backup was written");
    insta::assert_snapshot!("migrate_finished_empty", transcript);

    let page = TestDir::new();
    fs::create_dir_all(data(page.path()))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    commands::one_page_without_tables(&commands::database(page.path()));
    let (out, again) = migrate(page.path(), false);
    assert_exit(&out, 0);
    assert_streams(&out);
    assert_eq!(commands::schema_version(page.path()), Some(2));
    assert!(backups(page.path()).is_empty(), "a backup was written");
    assert_eq!(again, transcript, "the one-page file finished");
}

/// A copy of the 0.1.0 book given a book row and no accounts is updated, and the check after the
/// update finds the fourteen broken rules nage-core writes for it (the five roots and the eight
/// system accounts missing): exit 1, the lines under a bare `Why:`, the backup named, and the
/// update stays.
///
/// Proves: decision 156 (assumption A11: exit 1, the update stays; a v0.1.0 copy given a book row and no accounts: the fourteen real INV-8 and INV-11 lines)
#[test]
fn migrate_exits_1_when_the_check_after_the_update_finds_broken_rules() {
    let dir = commands::behind();
    plant(
        &commands::database(dir.path()),
        &[
            "INSERT INTO book (id, name, currency, minor_digits, created_at) VALUES (1, 'Household', 'CAD', 2, '2026-01-01T00:00:00Z')",
        ],
    );
    let (out, transcript) = migrate(dir.path(), false);
    assert_exit(&out, 1);
    assert_streams(&out);
    assert_eq!(
        commands::schema_version(dir.path()),
        Some(2),
        "the update did not stay"
    );
    assert_eq!(backups(dir.path()).len(), 1, "the backups after the update");
    insta::assert_snapshot!("migrate_finds_broken_rules", transcript);
}

/// Running `nage init` again just works on what a cut-off init left: on a zero-byte database file
/// and on the one-page file without tables an init that failed after the open leaves, the book is
/// set up, its admin can be found, and the book passes the check.
///
/// Proves: decision 155, ruling P6 (running init again just works, on a zero-byte file and on the one-page file an init that failed after the open leaves)
#[test]
fn init_finishes_an_empty_database_file_a_cut_off_init_left() {
    for one_page in [false, true] {
        let dir = TestDir::new();
        fs::create_dir_all(data(dir.path()).join("backups"))
            .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
        let database = commands::database(dir.path());
        if one_page {
            commands::one_page_without_tables(&database);
        } else {
            fs::write(&database, b"")
                .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
        }
        assert!(
            block_on(nage_core::update::is_empty(&database))
                .unwrap_or_else(|error| panic!("cannot ask whether it is empty: {error}")),
            "the planted file is not empty"
        );
        let out = commands::command(
            dir.path(),
            &[
                "init",
                "--data-dir",
                "data",
                "--book-name",
                "Household",
                "--currency",
                "CAD",
                "--username",
                "sam",
                "--display-name",
                "Sam",
            ],
            Some(format!("{PASSWORD}\n").as_bytes()),
        );
        assert_exit(&out, 0);
        assert_streams(&out);
        let holds = commands::holds(dir.path());
        assert!(
            holds.book.is_some() && holds.users == 1 && holds.schema_version == Some(2),
            "one page {one_page}: the database holds {holds:?}"
        );
        commands::assert_check_ok(dir.path());
    }
}
