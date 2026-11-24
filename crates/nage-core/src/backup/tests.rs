//! The backup's name, written and read back, and the copy an older Nage points to (decisions 151
//! and 152).

use std::path::PathBuf;

use proptest::prelude::*;

use crate::test_support::TestDir;
use crate::timestamp::Timestamp;

use super::{BackupName, choose, name, newest_for, parse, same_step};

/// The instant written `text`, which must be one.
fn at(text: &str) -> Timestamp {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is not an instant: {error:?}"))
}

/// The parts of a backup's name.
fn backup(stamp: &str, step: i64, version: &str) -> BackupName {
    BackupName {
        stamp: at(stamp),
        step,
        version: version.to_owned(),
    }
}

/// The name begins with the UTC instant, its colons written as hyphens, then `-pre-migrate-`,
/// the step in four digits (more when it needs more), `-to-`, the version and `.sqlite`: the
/// example of decision 151 and its edges.
///
/// Proves: decision 151 (the backup's name, the maintainer's pick Date first)
#[test]
fn a_name_is_written_date_first_in_utc() {
    assert_eq!(
        name(at("2027-03-14T02:00:00Z"), 2, "0.3.0"),
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "decision 151's example"
    );
    assert_eq!(
        name(at("2026-11-24T23:59:09Z"), 12, "0.2.0"),
        "2026-11-24T23-59-09Z-pre-migrate-0012-to-0.2.0.sqlite",
        "a two-digit step and the last second of a minute"
    );
    assert_eq!(
        name(at("2030-01-02T03:04:05Z"), 12345, "1.0.0-rc.1"),
        "2030-01-02T03-04-05Z-pre-migrate-12345-to-1.0.0-rc.1.sqlite",
        "a step of five digits and a version with a hyphen"
    );
}

/// A name is read by its fixed-length stamp, `-pre-migrate-`, the number up to `-to-` (compared
/// as a number, so leading zeros and a fifth digit change nothing) and the version up to
/// `.sqlite`, whatever hyphens the version holds.
///
/// Proves: decision 151 (a name read by its fixed-length stamp, the number compared as a number)
#[test]
fn a_name_is_read_back_whatever_hyphens_the_version_holds() {
    let cases = [
        (
            "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
            backup("2027-03-14T02:00:00Z", 2, "0.3.0"),
        ),
        (
            "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0-rc.1.sqlite",
            backup("2027-03-14T02:00:00Z", 2, "0.3.0-rc.1"),
        ),
        (
            "2027-03-14T02-00-00Z-pre-migrate-0010-to-1.0.0-to-be-named.sqlite",
            backup("2027-03-14T02:00:00Z", 10, "1.0.0-to-be-named"),
        ),
        (
            "2026-12-31T23-59-59Z-pre-migrate-12345-to-0.9.0.sqlite",
            backup("2026-12-31T23:59:59Z", 12345, "0.9.0"),
        ),
        (
            "2026-12-31T23-59-59Z-pre-migrate-2-to-0.9.0.sqlite",
            backup("2026-12-31T23:59:59Z", 2, "0.9.0"),
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(parse(text), Some(expected), "the parts of {text}");
    }
}

/// What the backups folder may hold besides Nage's copies is no backup: a stray file, the
/// database's own name, a side file, a partial copy, a name with colons, a stamp that is no
/// instant, a step that is no number, an empty version, another ending.
///
/// Proves: decision 151 (stray and partial files are not backups)
#[test]
fn a_name_that_is_not_a_backups_is_left_out() {
    for text in [
        "",
        "notes.txt",
        "nage.sqlite",
        ".sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite-journal",
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite.partial",
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0",
        "2027-03-14T02:00:00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "2027-03-14T02-00-00z-pre-migrate-0002-to-0.3.0.sqlite",
        "2027-02-30T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "1999-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-00x2-to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate--to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-+2-to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate--2-to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-0002.sqlite",
        "2027-03-14T02-00-00Z-post-migrate-0002-to-0.3.0.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-99999999999999999999-to-0.3.0.sqlite",
        "x2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
    ] {
        assert_eq!(parse(text), None, "{text:?} read as a backup's name");
    }
}

/// Of the copies whose step is not above the binary's last change, one whose step equals it is
/// preferred, the newest by its stamp; without one, the newest of the lower steps; with none, no
/// copy. A file's own time plays no part: only the stamps in the names.
///
/// Proves: decision 152 (the step not above the binary's, the equal one preferred, the newest by its stamp)
#[test]
fn the_newest_copy_an_older_nage_can_open_is_chosen() {
    let names = vec![
        backup("2027-01-01T00:00:00Z", 2, "0.3.0"),
        backup("2027-02-01T00:00:00Z", 2, "0.3.0"),
        backup("2027-03-01T00:00:00Z", 1, "0.2.0"),
        backup("2027-04-01T00:00:00Z", 3, "0.4.0"),
        backup("2026-12-01T00:00:00Z", 1, "0.2.0"),
    ];
    assert_eq!(
        choose(&names, 2),
        Some(&names[1]),
        "the newest copy at the binary's own step, although a lower step is newer"
    );
    assert_eq!(
        choose(&names, 1),
        Some(&names[2]),
        "the newest copy at step 1"
    );
    assert_eq!(
        choose(&names[2..], 2),
        Some(&names[2]),
        "without a copy at step 2, the newest of the lower steps"
    );
    assert_eq!(
        choose(&names, 5),
        Some(&names[3]),
        "without a copy at step 5, the newest of the lower steps"
    );
    assert_eq!(
        choose(&names[3..4], 2),
        None,
        "a copy above the binary's step only"
    );
    assert_eq!(choose(&[], 2), None, "no copy at all");
}

/// In a backups folder holding the copy just written, an older and a later copy of the same
/// step and version, copies of another step or another version and files that are not backups,
/// the other copies of the same step are exactly the older and the later one; a symbolic link and
/// a folder that carry the names of newer copies of the step are neither found nor named as the
/// newest backup, since only regular files are backups.
///
/// Proves: decision 151, ruling P4 (a later stamp included; other steps, other versions and files that are not backups never named)
#[test]
fn every_other_copy_of_the_same_step_is_found_whatever_its_stamp() {
    let dir = TestDir::new();
    let folder = dir.path();
    let written = folder.join("2027-02-01T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite");
    let older = folder.join("2027-01-01T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite");
    let later = folder.join("2027-03-01T00-00-00Z-pre-migrate-2-to-0.3.0.sqlite");
    let others = [
        folder.join("2027-01-15T00-00-00Z-pre-migrate-0001-to-0.3.0.sqlite"),
        folder.join("2027-01-16T00-00-00Z-pre-migrate-0002-to-0.2.0.sqlite"),
        folder.join("2027-01-17T00-00-00Z-pre-migrate-0002-to-0.3.0-rc.1.sqlite"),
        folder.join("2027-01-18T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite-journal"),
        folder.join("notes.txt"),
        folder.join("nage.sqlite"),
    ];
    for path in [&written, &older, &later].into_iter().chain(others.iter()) {
        std::fs::write(path, b"")
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", path.display()));
    }
    // A symbolic link and a folder that carry the names of newer copies of the same step: only
    // regular files are backups.
    let link = folder.join("2027-04-01T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite");
    std::os::unix::fs::symlink(folder.join("notes.txt"), &link)
        .unwrap_or_else(|error| panic!("cannot link {}: {error}", link.display()));
    let inner = folder.join("2027-04-02T00-00-00Z-pre-migrate-0002-to-0.3.0.sqlite");
    std::fs::create_dir(&inner)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", inner.display()));

    let mut found = same_step(folder, 2, "0.3.0", &written);
    found.sort();
    let mut expected: Vec<PathBuf> = vec![older, later.clone()];
    expected.sort();
    assert_eq!(found, expected, "the other copies of step 2 by 0.3.0");
    assert_eq!(
        newest_for(folder, 2),
        Some(later),
        "the newest copy at step 2, the link and the folder left out"
    );

    assert_eq!(
        same_step(&folder.join("missing"), 2, "0.3.0", &written),
        Vec::<PathBuf>::new(),
        "a folder that does not exist"
    );
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// Every stamp of the range, every step from 0 to 99,999 and every version made of digits,
    /// dots and a hyphenated suffix survive the name: what `parse` reads back is what `name`
    /// was given.
    ///
    /// Proves: decision 151 (property: every stamp, step and version survive the name)
    #[test]
    fn a_written_name_reads_back_to_its_parts(
        second in Timestamp::MIN.as_second()..=Timestamp::MAX.as_second(),
        step in 0_i64..100_000,
        version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}(-[a-z0-9.-]{1,12})?",
    ) {
        let stamp = Timestamp::from_second(second)
            .unwrap_or_else(|error| panic!("second {second}: {error:?}"));
        let written = name(stamp, step, &version);
        prop_assert_eq!(
            parse(&written),
            Some(BackupName { stamp, step, version: version.clone() }),
            "the parts of {}", written
        );
    }
}
