//! The pre-migration backup's file name and the copy an older Nage points to (SPEC 5.5, decisions
//! 151 and 152). A backup is named `<stamp>-pre-migrate-<step>-to-<version>.sqlite`: the stamp is
//! the UTC instant the update began, written `YYYY-MM-DDTHH-MM-SSZ` (an RFC 3339 instant with
//! hyphens for its colons, which no file system refuses); the step is the last change the database
//! carried before the update, four digits as `migrations/` numbers it; the version is the version
//! of the Nage that made the copy, which may hold hyphens of its own (`0.3.0-rc.1`). A name is read
//! back by its fixed-length stamp, then `-pre-migrate-`, the number up to `-to-` (compared as a
//! number), and the version up to `.sqlite`; anything else in the backups folder is not a backup of
//! this kind and is left out. Copies are ordered by the stamp in their names, never by a file's
//! time, which a copy or a restore changes.

use std::path::{Path, PathBuf};

use crate::timestamp::Timestamp;

#[cfg(test)]
mod tests;

/// What stands between the stamp and the step.
pub const PRE_MIGRATE: &str = "-pre-migrate-";
/// What stands between the step and the version.
pub const TO: &str = "-to-";
/// The end of every backup's name.
pub const EXTENSION: &str = ".sqlite";

/// What a backup's name says (decision 151).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupName {
    /// When the update that made the copy began.
    pub stamp: Timestamp,
    /// The last change the copied database carried: the schema version the copy is at.
    pub step: i64,
    /// The version of the Nage that made the copy.
    pub version: String,
}

/// The file name of the backup made at `stamp` of a database at `step` by Nage `version`, such as
/// `2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite` (decision 151).
pub fn name(stamp: Timestamp, step: i64, version: &str) -> String {
    let stamp = stamp.to_string().replace(':', "-");
    format!("{stamp}{PRE_MIGRATE}{step:04}{TO}{version}{EXTENSION}")
}

/// The length of a stamp as a name writes it: `YYYY-MM-DDTHH-MM-SSZ`.
const STAMP_LEN: usize = 20;

/// What the file name `name` says, or `None` when it is not a backup's name: a stamp that is not
/// a whole-second UTC instant, a step that is not a number, an empty version or another ending
/// (decision 151).
pub fn parse(name: &str) -> Option<BackupName> {
    let stamp_text = name.get(..STAMP_LEN)?;
    let rest = name.get(STAMP_LEN..)?;
    let stamp = parse_stamp(stamp_text)?;
    let rest = rest.strip_prefix(PRE_MIGRATE)?;
    let rest = rest.strip_suffix(EXTENSION)?;
    let (step_text, version) = rest.split_once(TO)?;
    if step_text.is_empty() || !step_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let step: i64 = step_text.parse().ok()?;
    if version.is_empty() {
        return None;
    }
    Some(BackupName {
        stamp,
        step,
        version: version.to_owned(),
    })
}

/// The instant a name's stamp `text` writes, `YYYY-MM-DDTHH-MM-SSZ`: the hyphens that stand for
/// the time's colons are put back and the text is read as an instant exactly.
fn parse_stamp(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    if bytes.get(13) != Some(&b'-') || bytes.get(16) != Some(&b'-') {
        return None;
    }
    let instant = format!(
        "{}:{}:{}",
        text.get(..13)?,
        text.get(14..16)?,
        text.get(17..)?
    );
    instant.parse().ok()
}

/// The copy an older Nage, which knows the changes up to `latest`, can open: of the copies whose
/// step is not above `latest`, the newest by its stamp among those whose step equals `latest`, or
/// else the newest of them all; `None` when no copy qualifies (decision 152).
pub fn choose(names: &[BackupName], latest: i64) -> Option<&BackupName> {
    let newest_at = |wanted: &dyn Fn(i64) -> bool| {
        names
            .iter()
            .filter(|name| wanted(name.step))
            .max_by_key(|name| name.stamp)
    };
    newest_at(&|step| step == latest).or_else(|| newest_at(&|step| step <= latest))
}

/// The path of the copy [`choose`] picks among the files of the folder `backups`, or `None` when
/// the folder holds none or cannot be read (decision 152).
pub fn newest_for(backups: &Path, latest: i64) -> Option<PathBuf> {
    let copies = copies(backups);
    let names: Vec<BackupName> = copies.iter().map(|(_, name)| name.clone()).collect();
    let chosen = choose(&names, latest)?;
    copies
        .into_iter()
        .find(|(_, name)| name == chosen)
        .map(|(path, _)| path)
}

/// The other copies of the step `step` made by Nage `version` among the files of the folder
/// `backups`, `written` (the copy just made) left out: every one, whatever its stamp, since a
/// clock set back may leave a later stamp on an older copy (ruling P4 of the pause). Copies of
/// other steps and files that are not backups are never named.
pub fn same_step(backups: &Path, step: i64, version: &str, written: &Path) -> Vec<PathBuf> {
    copies(backups)
        .into_iter()
        .filter(|(path, name)| name.step == step && name.version == version && path != written)
        .map(|(path, _)| path)
        .collect()
}

/// The backups in the folder `backups`: each regular file whose name [`parse`] reads, with what
/// its name says, ordered by the stamp in its name; empty when the folder cannot be read.
fn copies(backups: &Path) -> Vec<(PathBuf, BackupName)> {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return Vec::new();
    };
    let mut copies: Vec<(PathBuf, BackupName)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = parse(entry.file_name().to_str()?)?;
            Some((entry.path(), name))
        })
        .collect();
    copies.sort_by_key(|(_, name)| name.stamp);
    copies
}
