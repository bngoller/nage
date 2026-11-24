//! The modes of what Nage creates in the data folder, and of what it finds there (decision 154).

use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use crate::test_support::TestDir;

use super::{create_dir_all, create_new_file, open_or_create_file};

/// The permission bits of `path`.
#[track_caller]
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
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

/// Under the runner's umask, a folder created with every missing folder above it is 0700 at
/// each level, a new file is 0600 whether created new or opened-or-created, and what exists
/// keeps its mode and its bytes: a folder at 0755, a file at 0644 that `create_new_file` refuses
/// as `AlreadyExists` and `open_or_create_file` opens as it is. The modes are read once the
/// helpers return, so the test claims only the final modes under this runner's umask; the
/// binary's tests start the commands under a loose umask through a shell.
///
/// Proves: decision 154 (folders 0700, files 0600 under the runner's umask; existing modes kept)
#[test]
fn what_is_created_is_private_and_what_exists_keeps_its_mode() {
    let dir = TestDir::new();
    let top = dir.path().join("data");
    let nested = top.join("backups");
    create_dir_all(&nested)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", nested.display()));
    assert_eq!(mode(&top), 0o700, "the mode of the data folder created");
    assert_eq!(
        mode(&nested),
        0o700,
        "the mode of the backups folder created"
    );
    create_dir_all(&nested)
        .unwrap_or_else(|error| panic!("cannot create {} again: {error}", nested.display()));

    let existing = dir.path().join("existing");
    std::fs::create_dir(&existing)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", existing.display()));
    set_mode(&existing, 0o755);
    let inner = existing.join("inner");
    create_dir_all(&inner)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", inner.display()));
    assert_eq!(mode(&existing), 0o755, "the mode of a folder that existed");
    assert_eq!(
        mode(&inner),
        0o700,
        "the mode of the folder created inside it"
    );

    let file = nested.join("copy.sqlite");
    let mut created = create_new_file(&file)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", file.display()));
    created
        .write_all(b"copy")
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", file.display()));
    drop(created);
    assert_eq!(mode(&file), 0o600, "the mode of a file created new");
    match create_new_file(&file) {
        Err(error) => assert_eq!(
            error.kind(),
            ErrorKind::AlreadyExists,
            "creating {} a second time",
            file.display()
        ),
        Ok(_) => panic!("{} was created a second time", file.display()),
    }
    assert_eq!(
        std::fs::read(&file).unwrap_or_default(),
        b"copy",
        "the bytes of a file that a second creation met"
    );

    let mark = top.join("nage.lock");
    drop(
        open_or_create_file(&mark)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", mark.display())),
    );
    assert_eq!(mode(&mark), 0o600, "the mode of a file opened or created");

    let kept = dir.path().join("kept.lock");
    std::fs::write(&kept, b"serve")
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", kept.display()));
    set_mode(&kept, 0o644);
    let mut opened = open_or_create_file(&kept)
        .unwrap_or_else(|error| panic!("cannot open {}: {error}", kept.display()));
    let mut text = String::new();
    opened
        .read_to_string(&mut text)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", kept.display()));
    assert_eq!(text, "serve", "the bytes of a file that existed");
    assert_eq!(mode(&kept), 0o644, "the mode of a file that existed");
}
