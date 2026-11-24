//! The answers of `nage migrate` no process test can reach (the check after the update that could
//! not run, and a clock Nage cannot use), each pinned as a snapshot of its three rendered lines
//! (ruling P9 of PLAN M2 slice 8's pause: every text has a picture), as `serve/tests.rs` pins the
//! server's.

use std::io;
use std::path::PathBuf;

use nage_core::timestamp::Timestamp;
use nage_core::update::UpdateReport;

use super::{check_failed, clock_refused};
use crate::shape::Paint;

/// What `nage migrate` did before its check, on a database at `/srv/nage/data`: the changes
/// `applied`.
fn report(applied: Vec<i64>) -> UpdateReport {
    UpdateReport {
        database: PathBuf::from("/srv/nage/data/nage.sqlite"),
        created: false,
        backup: None,
        removed: Vec::new(),
        applied,
        schema_version: Some(2),
    }
}

/// The check after the update could not run, after an update and with nothing waiting, and the
/// clock gave a time Nage cannot use: each reads as its three lines, snapshotted under its own
/// name. The reading failed with a real operating system error (`Input/output error`, 5), and the
/// clock is one before the range a stamp can carry.
///
/// Proves: ruling P9 (every text has a picture: check_failed after an update and with nothing waiting, and the clock's refusal, as rendered lines)
#[test]
fn the_answers_no_process_reaches_read_as_their_lines() {
    let reading = nage_core::storage::Error::Sqlx(sqlx::Error::Io(io::Error::from_raw_os_error(5)));
    let clock = Timestamp::from_second(0)
        .err()
        .unwrap_or_else(|| panic!("the clock at 1970-01-01T00:00:00Z gave a stamp"));
    let cases = [
        (
            "check_failed_after_an_update",
            check_failed(&report(vec![2]), &reading),
        ),
        (
            "check_failed_with_nothing_waiting",
            check_failed(&report(Vec::new()), &reading),
        ),
        ("clock", clock_refused(&clock)),
    ];
    for (name, refused) in cases {
        let lines = refused.render(Paint::PLAIN);
        assert_eq!(
            lines.lines().count(),
            3,
            "the lines of {name} are not three: {lines}"
        );
        insta::assert_snapshot!(name, lines);
    }
}
