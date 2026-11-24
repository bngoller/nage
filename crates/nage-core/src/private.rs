//! What Nage creates in the data folder, created so that only the account Nage runs as can open it
//! (SPEC 5.2, decision 154): a folder with mode 0700, a file with mode 0600, from the moment it
//! exists, whatever the umask. What exists already is left as it is, its mode included.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::Path;

#[cfg(test)]
mod tests;

/// The mode of a folder Nage creates: only its own account can open it.
const FOLDER_MODE: u32 = 0o700;
/// The mode of a file Nage creates: only its own account can read and write it.
const FILE_MODE: u32 = 0o600;

/// Creates the folder `path` and every missing folder above it with mode 0700; a folder that
/// exists keeps its mode. Fails as `std::fs::create_dir_all` fails.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    // The mode is given to mkdir itself, so the umask can only narrow it: a folder is never
    // wider than 0700, not even for a moment.
    DirBuilder::new()
        .recursive(true)
        .mode(FOLDER_MODE)
        .create(path)
}

/// Creates the file `path`, which must not exist (an existing file, whatever it holds, is
/// `AlreadyExists` and is left as it is), with mode 0600, open for writing.
pub fn create_new_file(path: &Path) -> io::Result<File> {
    // `create_new` is O_CREAT | O_EXCL: a file, an empty one or a symbolic link (dangling or
    // not) at the name fails the call, and nothing is followed or written over.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)
}

/// Opens the file `path` for reading and writing, creating it with mode 0600 when it is missing;
/// a file that exists keeps its mode and its bytes.
pub fn open_or_create_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(FILE_MODE)
        .open(path)
}
