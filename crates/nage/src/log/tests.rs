//! The log's stamp (decision 160).

use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime as _;

use super::Stamp;

/// Whether `text` is `YYYY-MM-DDTHH:MM:SSZ`: digits where the digits go, nothing else.
fn is_whole_second_utc(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:ddZ";
    text.len() == shape.len()
        && text
            .chars()
            .zip(shape.chars())
            .all(|(found, want)| match want {
                'd' => found.is_ascii_digit(),
                other => found == other,
            })
}

/// The stamp the log's timer writes is the instant it is asked at, in UTC, cut to the whole
/// second: `YYYY-MM-DDTHH:MM:SSZ`, with no zone name, no offset and no fraction of a second.
///
/// Proves: decision 160 (YYYY-MM-DDTHH:MM:SSZ, no zone, offset or fraction)
#[test]
fn the_stamp_is_a_whole_second_in_utc() {
    let before = jiff::Timestamp::now().as_second();
    let mut text = String::new();
    Stamp
        .format_time(&mut Writer::new(&mut text))
        .unwrap_or_else(|error| panic!("the stamp could not be written: {error}"));
    let after = jiff::Timestamp::now().as_second();

    assert!(
        is_whole_second_utc(&text),
        "not a whole-second UTC stamp: {text:?}"
    );
    let stamped: jiff::Timestamp = text
        .parse()
        .unwrap_or_else(|error| panic!("{text:?} is not an instant: {error}"));
    assert_eq!(stamped.subsec_nanosecond(), 0, "{text:?}");
    assert!(
        (before..=after).contains(&stamped.as_second()),
        "{text:?} is not the instant it was asked at ({before} to {after})"
    );

    for shape in ["2027-03-14T02:00:00Z", "1970-01-01T00:00:00Z"] {
        assert!(is_whole_second_utc(shape), "{shape}");
    }
    for not in [
        "2027-03-14T02:00:00.5Z",
        "2027-03-14T02:00:00+00:00",
        "2027-03-14T02:00:00Z[UTC]",
        "2027-03-14 02:00:00Z",
    ] {
        assert!(!is_whole_second_utc(not), "{not}");
    }
}
