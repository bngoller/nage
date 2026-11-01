//! Helpers shared by the tests of every workspace crate (decision 59): temporary
//! directories, storage opened in them, copies of the committed fixtures, the side-file
//! assertion and the property-test configuration.
//!
//! The module exists only with the `test-support` feature, and only dev-dependencies enable
//! that feature, so a production build (`cargo build -p nage`, the Dockerfile) never carries
//! it. Other crates reach it as `nage_core::test_support`; nage-core's own in-crate unit tests
//! reach it as `crate::test_support`, never through the extern name, because the self
//! dev-dependency that turns the feature on links a second copy of the library whose types
//! do not match this one's.

use std::path::{Component, Path, PathBuf};

use proptest::test_runner::FileFailurePersistence;
use tempfile::TempDir;

use crate::storage::{MigrationReport, Storage, StorageOptions};

/// The file name `serve` gives the database (SPEC 5.2).
pub const DB_FILE_NAME: &str = "nage.sqlite";

/// A fresh directory under the system temporary directory, named `nage-test-*`, with its
/// path canonicalised at creation so it compares equal to what a child process reports as
/// its working directory. Removed on drop, except when the thread is panicking: a test that
/// panics while it holds the directory leaves it behind for inspection.
#[derive(Debug)]
pub struct TestDir {
    dir: TempDir,
    path: PathBuf,
}

impl TestDir {
    /// Creates the directory, panicking if it cannot be created or resolved.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("nage-test-")
            .tempdir()
            .unwrap_or_else(|error| panic!("cannot create a temporary directory: {error}"));
        let path = std::fs::canonicalize(dir.path())
            .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", dir.path().display()));
        Self { dir, path }
    }

    /// The canonical path of the directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database path inside the directory: [`DB_FILE_NAME`].
    pub fn db_path(&self) -> PathBuf {
        self.path.join(DB_FILE_NAME)
    }
}

impl Default for TestDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.dir.disable_cleanup(true);
        }
    }
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with `options`; nothing is migrated.
/// Panics if the database cannot be opened.
pub async fn open_with(options: &StorageOptions) -> (TestDir, Storage) {
    let dir = TestDir::new();
    let path = dir.db_path();
    let storage = Storage::open(&path, options)
        .await
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
    (dir, storage)
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with the default options; nothing is
/// migrated.
pub async fn open_fresh() -> (TestDir, Storage) {
    open_with(&StorageOptions::default()).await
}

/// Opens [`DB_FILE_NAME`] in a fresh [`TestDir`] with the default options and applies the
/// migrations.
pub async fn open_migrated() -> (TestDir, Storage) {
    let (dir, storage) = open_fresh().await;
    migrate(&storage).await;
    (dir, storage)
}

/// Applies the migrations, panicking on failure.
pub async fn migrate(storage: &Storage) -> MigrationReport {
    storage
        .migrate()
        .await
        .unwrap_or_else(|error| panic!("cannot migrate: {error}"))
}

/// Copies the committed fixture `fixtures/books/v<version>.sqlite` (the version without a
/// leading `v`, for example `"0.1.0"`; never a path), resolved from nage-core's
/// `CARGO_MANIFEST_DIR`, into a fresh [`TestDir`] and returns the directory and the copy's
/// path. The copy lies at the directory's [`TestDir::db_path`], the name `serve` gives the
/// database. The committed file is never opened in place, only read as bytes by
/// `std::fs::copy` (decision 58). Panics, naming the path, if the fixture is missing or
/// cannot be copied.
pub fn fixture_copy(version: &str) -> (TestDir, PathBuf) {
    let fixture = workspace_root()
        .join("fixtures/books")
        .join(format!("v{version}.sqlite"));
    assert!(
        fixture.is_file(),
        "the fixture {} does not exist",
        fixture.display()
    );
    let dir = TestDir::new();
    let copy = dir.db_path();
    std::fs::copy(&fixture, &copy).unwrap_or_else(|error| {
        panic!(
            "cannot copy the fixture {} to {}: {error}",
            fixture.display(),
            copy.display()
        )
    });
    (dir, copy)
}

/// The workspace root: the grandparent of nage-core's `CARGO_MANIFEST_DIR`, fixed at compile
/// time, so it does not depend on the working directory.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("{} has no grandparent", manifest.display()))
        .to_path_buf()
}

/// Asserts that neither `<db>-wal` nor `<db>-shm` exists next to the database file `db`: a
/// clean close checkpoints the WAL and removes both (decision 53). `when` names the moment
/// in the message.
#[track_caller]
pub fn assert_no_side_files(db: &Path, when: &str) {
    for suffix in ["-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        let side = PathBuf::from(name);
        assert!(
            !side.exists(),
            "the SQLite side file {} exists {when}",
            side.display()
        );
    }
}

/// The proptest regression file for the workspace-relative source path `file` (what
/// `file!()` gives under cargo): `<workspace root>/<crate dir>/proptest-regressions/<src or
/// tests>/<rest>.txt`. The workspace root is the grandparent of nage-core's
/// `CARGO_MANIFEST_DIR`, a compile-time constant, so the result is absolute and does not
/// depend on the working directory; `<crate dir>` is the leading `crates/<name>` of `file`,
/// and the rest is the path from its `src/` or `tests/` component with the extension
/// replaced by `.txt`. A path without those components maps to
/// `proptest-regressions/<file name>.txt` under the crate directory (decision 61). Panics if
/// `file` does not start with `crates/<name>/` or has no file name.
pub fn regression_path(file: &str) -> PathBuf {
    let path = Path::new(file);
    let mut components = path.components();
    let crate_dir = match (components.next(), components.next()) {
        (Some(Component::Normal(first)), Some(Component::Normal(name))) if first == "crates" => {
            Path::new(first).join(name)
        }
        _ => panic!("regression_path({file}): the path does not start with crates/<name>/"),
    };
    let inner = components.as_path();
    let start = inner
        .components()
        .position(|component| {
            matches!(component, Component::Normal(name) if name == "src" || name == "tests")
        });
    let rest = match start {
        Some(start) => inner.components().skip(start).collect::<PathBuf>(),
        None => PathBuf::from(
            inner
                .file_name()
                .unwrap_or_else(|| panic!("regression_path({file}): the path has no file name")),
        ),
    };
    workspace_root()
        .join(crate_dir)
        .join("proptest-regressions")
        .join(rest)
        .with_extension("txt")
}

/// The configuration every property declares through
/// `#![proptest_config(test_support::proptest_config(file!()))]`: `Config::default()` (256
/// cases unless `PROPTEST_CASES` overrides it) with failure persistence set to
/// `FileFailurePersistence::Direct` of [`regression_path`] for `file`. `Direct` takes a
/// `&'static str`, so each call leaks one path string; that is one small string per
/// property per test run (decision 61). `Config::default()` is proptest's contextualised
/// default, so `PROPTEST_CASES` and the other `PROPTEST_*` variables still apply.
pub fn proptest_config(file: &'static str) -> proptest::test_runner::Config {
    let path = regression_path(file)
        .into_os_string()
        .into_string()
        .unwrap_or_else(|path| panic!("the regression path {path:?} is not UTF-8"));
    let leaked: &'static str = Box::leak(path.into_boxed_str());
    proptest::test_runner::Config {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(leaked))),
        ..proptest::test_runner::Config::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use proptest::test_runner::FileFailurePersistence;

    use super::{TestDir, assert_no_side_files, proptest_config, regression_path};

    /// The workspace root, computed here from nage-core's manifest directory and anchored
    /// against the tree: it must hold the workspace `Cargo.toml` and nage-core's manifest, so
    /// the mapping below is checked against the real root, not against the helper's formula.
    fn workspace_root() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| panic!("{} has no grandparent", manifest.display()))
            .to_path_buf();
        assert!(
            root.join("Cargo.toml").is_file() && root.join("crates/nage-core/Cargo.toml").is_file(),
            "{} is not the workspace root",
            root.display()
        );
        root
    }

    /// Asserts that `file` maps to the absolute path `expected` under the workspace root.
    #[track_caller]
    fn assert_maps(file: &str, expected: &str) {
        let path = regression_path(file);
        assert!(
            path.is_absolute(),
            "regression_path({file}) is not absolute: {}",
            path.display()
        );
        let root = workspace_root();
        let suffix = path.strip_prefix(&root).unwrap_or_else(|_| {
            panic!(
                "regression_path({file}) = {} lies outside the workspace root {}",
                path.display(),
                root.display()
            )
        });
        assert_eq!(
            suffix,
            Path::new(expected),
            "regression_path({file}) under the workspace root"
        );
    }

    /// `regression_path` mirrors the source tree under each crate's `proptest-regressions/`,
    /// absolute under the workspace root: a `src/` file, a `tests/` file and a nested file of
    /// another crate. `proptest_config(file!())` persists failures directly to this file's
    /// regression path and runs `PROPTEST_CASES` cases when that variable is set and parses
    /// (the test reads it and never sets it), else 256.
    ///
    /// Proves: decision 61.
    #[test]
    fn regression_path_mirrors_the_source_tree() {
        assert_maps(
            "crates/nage-core/src/storage.rs",
            "crates/nage-core/proptest-regressions/src/storage.txt",
        );
        assert_maps(
            "crates/nage-core/tests/props.rs",
            "crates/nage-core/proptest-regressions/tests/props.txt",
        );
        assert_maps(
            "crates/nage-web/src/import/fingerprint.rs",
            "crates/nage-web/proptest-regressions/src/import/fingerprint.txt",
        );

        let config = proptest_config(file!());
        let expected =
            workspace_root().join("crates/nage-core/proptest-regressions/src/test_support.txt");
        let persistence = config
            .failure_persistence
            .as_ref()
            .unwrap_or_else(|| panic!("proptest_config(file!()) sets no failure persistence"));
        match persistence
            .as_any()
            .downcast_ref::<FileFailurePersistence>()
        {
            Some(FileFailurePersistence::Direct(path)) => assert_eq!(
                Path::new(path),
                expected.as_path(),
                "the Direct failure persistence of proptest_config(file!())"
            ),
            other => panic!(
                "proptest_config(file!()) persists failures through {other:?}, not FileFailurePersistence::Direct"
            ),
        }

        let cases = std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(256);
        assert_eq!(
            config.cases, cases,
            "proptest_config(file!()).cases (PROPTEST_CASES or 256)"
        );
    }

    /// `assert_no_side_files` fails when `<db>-wal` or `<db>-shm` exists beside the database
    /// path, naming that file, and returns when neither does: the callers that assert a clean
    /// close left no side files rely on it being able to fail. Each file is created empty and
    /// removed again before the next case.
    ///
    /// Proves: decision 59.
    #[test]
    fn assert_no_side_files_fails_when_a_side_file_exists() {
        let dir = TestDir::new();
        let db = dir.db_path();
        for suffix in ["-wal", "-shm"] {
            let side = dir.path().join(format!("nage.sqlite{suffix}"));
            std::fs::write(&side, b"")
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", side.display()));
            let outcome =
                std::panic::catch_unwind(|| assert_no_side_files(&db, "in the self-test"));
            std::fs::remove_file(&side)
                .unwrap_or_else(|error| panic!("cannot remove {}: {error}", side.display()));
            let payload = match outcome {
                Ok(()) => panic!(
                    "assert_no_side_files returned although {} exists",
                    side.display()
                ),
                Err(payload) => payload,
            };
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or_else(|| panic!("assert_no_side_files panicked without a message"));
            let expected = format!(
                "the SQLite side file {} exists in the self-test",
                side.display()
            );
            assert_eq!(
                message, expected,
                "the message of assert_no_side_files with {suffix} present"
            );
        }
        assert_no_side_files(&db, "in the self-test with neither side file present");
    }

    /// A `TestDir` exists at its canonical path and its `db_path` is `nage.sqlite` inside it;
    /// dropping it removes the directory, but a thread that panics while holding one leaves
    /// the directory behind for inspection (the test removes it afterwards).
    ///
    /// Proves: decision 59.
    #[test]
    fn test_dir_is_canonical_removed_on_drop_and_kept_on_panic() {
        let dir = TestDir::new();
        let path = dir.path().to_path_buf();
        assert!(path.is_dir(), "{} does not exist", path.display());
        let canonical = std::fs::canonicalize(&path)
            .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", path.display()));
        assert_eq!(path, canonical, "the TestDir path is not canonical");
        assert_eq!(
            dir.db_path(),
            path.join("nage.sqlite"),
            "the TestDir database path"
        );
        drop(dir);
        assert!(
            !path.exists(),
            "{} still exists after the TestDir was dropped",
            path.display()
        );

        let (sender, receiver) = mpsc::channel();
        let joined = std::thread::spawn(move || {
            let dir = TestDir::new();
            sender
                .send(dir.path().to_path_buf())
                .unwrap_or_else(|error| panic!("cannot send the TestDir path: {error}"));
            panic!("deliberate panic while holding a TestDir");
        })
        .join();
        let kept = receiver
            .recv()
            .unwrap_or_else(|error| panic!("the panicking thread sent no path: {error}"));
        assert!(
            joined.is_err(),
            "the thread holding the TestDir did not panic"
        );
        let survived = kept.is_dir();
        if survived {
            std::fs::remove_dir_all(&kept)
                .unwrap_or_else(|error| panic!("cannot remove {}: {error}", kept.display()));
        }
        assert!(
            survived,
            "{} was removed although its thread panicked",
            kept.display()
        );
    }
}
