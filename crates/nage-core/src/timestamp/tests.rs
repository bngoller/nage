//! Tests of the whole-second UTC timestamp type (SPEC 5.4, decisions 56, 89 and 98).

use proptest::prelude::*;

use super::{Timestamp, TimestampError};

/// The seconds of the range's first and last instants.
const MIN_SECOND: i64 = 1_577_836_800;
const MAX_SECOND: i64 = 7_258_118_399;

/// The instant `second` seconds after the epoch, which must lie in the range.
fn stamp(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap_or_else(|error| panic!("second {second}: {error}"))
}

/// jiff's instant `second` seconds and `nanosecond` nanoseconds after the epoch.
fn jiff_instant(second: i64, nanosecond: i32) -> jiff::Timestamp {
    jiff::Timestamp::new(second, nanosecond)
        .unwrap_or_else(|error| panic!("jiff instant {second}.{nanosecond}: {error}"))
}

/// `MIN` and `MAX` are 2020-01-01T00:00:00Z and 2199-12-31T23:59:59Z, `from_second` reads a
/// second count back through `as_second`, the text of a whole-second instant is jiff's RFC 3339
/// form with `Z` and no fraction, and a second count outside the range is refused on both
/// sides: the ends of `i64`, the epoch and 2000-01-01 (the readings of a reset clock) and 1900
/// (the floor of the dates) among them.
///
/// Proves: SPEC 5.4, decision 98.
#[test]
fn range_ends_second_counts_and_text_form() {
    assert_eq!(Timestamp::MIN.to_string(), "2020-01-01T00:00:00Z");
    assert_eq!(Timestamp::MAX.to_string(), "2199-12-31T23:59:59Z");
    assert_eq!(Timestamp::MIN.as_second(), MIN_SECOND);
    assert_eq!(Timestamp::MAX.as_second(), MAX_SECOND);
    assert_eq!(stamp(MIN_SECOND), Timestamp::MIN);
    assert_eq!(stamp(MAX_SECOND), Timestamp::MAX);
    assert_eq!(stamp(1_600_000_000).to_string(), "2020-09-13T12:26:40Z");
    assert_eq!(stamp(1_700_000_000).to_string(), "2023-11-14T22:13:20Z");
    assert_eq!(stamp(1_794_225_600).to_string(), "2026-11-09T12:00:00Z");
    assert_eq!(stamp(1_700_000_000).as_second(), 1_700_000_000);
    for second in [
        MIN_SECOND - 1,
        MAX_SECOND + 1,
        0,
        946_684_800,
        -2_208_988_800,
        i64::MIN,
        i64::MAX,
        -62_135_596_800,
        253_402_300_799,
    ] {
        assert_eq!(
            Timestamp::from_second(second),
            Err(TimestampError::OutOfRange),
            "from_second({second})"
        );
    }
}

/// `from_jiff` drops the sub-second part (never rounding up), refuses an instant before the
/// first second (the epoch, a reset clock's reading, among them) or after the last one, and
/// `to_jiff` gives the same instant back.
///
/// Proves: decision 98.
#[test]
fn from_jiff_cuts_the_sub_second_part_and_keeps_the_range() {
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(1_700_000_000, 999_999_999)),
        Ok(stamp(1_700_000_000))
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(1_700_000_000, 0)),
        Ok(stamp(1_700_000_000))
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(MAX_SECOND, 999_999_999)),
        Ok(Timestamp::MAX)
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(MIN_SECOND, 0)),
        Ok(Timestamp::MIN)
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(MIN_SECOND - 1, 500_000_000)),
        Err(TimestampError::OutOfRange)
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(MAX_SECOND + 1, 0)),
        Err(TimestampError::OutOfRange)
    );
    assert_eq!(
        Timestamp::from_jiff(jiff::Timestamp::UNIX_EPOCH),
        Err(TimestampError::OutOfRange)
    );
    assert_eq!(
        Timestamp::from_jiff(jiff_instant(946_684_800, 0)),
        Err(TimestampError::OutOfRange)
    );
    assert_eq!(
        stamp(1_600_000_005).to_jiff(),
        jiff_instant(1_600_000_005, 0)
    );
    assert_eq!(
        Timestamp::from_jiff(stamp(1_600_000_077).to_jiff()),
        Ok(stamp(1_600_000_077))
    );
}

/// Exactly `YYYY-MM-DDTHH:MM:SSZ` reads back as itself; another shape is `Format` (a missing
/// `Z`, a space or a lowercase `t` as the separator, a lowercase `z`, an offset, a fraction,
/// missing seconds, the basic format, surrounding white space, an empty text, a letter in a
/// digit's place, a sign in front, a digit after the `Z`, a sign in a digit's place, hyphens in
/// the colons' places); a day, month, hour, minute or second that does not exist is
/// `Invalid` (a 30th of February, a 13th month, a zero month or day, hour 24, minute 60, the
/// leap second 60, which jiff's own parser would clamp); a real instant outside the range is
/// `OutOfRange` (1970 and 2000, the readings of a reset clock, and 1900, the floor of the
/// dates, among them). Each reason has its own message.
///
/// Proves: SPEC 5.4, decision 98.
#[test]
fn text_reads_back_and_other_text_is_refused_with_its_reason() {
    for text in [
        "2020-01-01T00:00:00Z",
        "2199-12-31T23:59:59Z",
        "2026-11-09T12:00:00Z",
        "2024-02-29T23:59:59Z",
        "2028-02-29T00:00:00Z",
    ] {
        let instant: Timestamp = text
            .parse()
            .unwrap_or_else(|error| panic!("{text:?}: {error}"));
        assert_eq!(instant.to_string(), text, "the text of {text:?} read back");
    }
    for text in [
        "2026-11-09T12:00:00",
        "2026-11-09 12:00:00Z",
        "2026-11-09t12:00:00Z",
        "2026-11-09T12:00:00z",
        "2026-11-09T12:00:00+00:00",
        "2026-11-09T12:00:00.0Z",
        "2026-11-09T12:00:00.000Z",
        "2026-11-09T12:00Z",
        "20261109T120000Z",
        " 2026-11-09T12:00:00Z",
        "2026-11-09T12:00:00Z ",
        "2026-11-09T12:00:00ZZ",
        "",
        "2026-11-09T12:00:0OZ",
        "+2026-11-09T12:00:00Z",
        "2026/11/09T12:00:00Z",
        "2026-11-09T12:00:00Z0",
        "2026-11-09T12:00:+0Z",
        "2026-11-09T12-00-00Z",
    ] {
        assert_eq!(
            text.parse::<Timestamp>(),
            Err(TimestampError::Format),
            "parse({text:?})"
        );
    }
    for text in [
        "2026-02-30T00:00:00Z",
        "2023-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-00-10T00:00:00Z",
        "2026-11-00T00:00:00Z",
        "2026-11-31T00:00:00Z",
        "2026-11-09T24:00:00Z",
        "2026-11-09T12:60:00Z",
        "2026-11-09T12:00:60Z",
    ] {
        assert_eq!(
            text.parse::<Timestamp>(),
            Err(TimestampError::Invalid),
            "parse({text:?})"
        );
    }
    for text in [
        "2019-12-31T23:59:59Z",
        "2200-01-01T00:00:00Z",
        "1970-01-01T00:00:00Z",
        "2000-01-01T00:00:00Z",
        "1900-01-01T00:00:00Z",
        "0001-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
    ] {
        assert_eq!(
            text.parse::<Timestamp>(),
            Err(TimestampError::OutOfRange),
            "parse({text:?})"
        );
    }
    let messages: Vec<String> = [
        TimestampError::Format,
        TimestampError::Invalid,
        TimestampError::OutOfRange,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "not a timestamp of the form YYYY-MM-DDTHH:MM:SSZ",
            "no such instant",
            "outside 2020-01-01T00:00:00Z to 2199-12-31T23:59:59Z",
        ]
    );
}

/// Instants order by time, and equal seconds are equal instants.
///
/// Proves: decision 98.
#[test]
fn instants_order_by_time() {
    assert!(Timestamp::MIN < stamp(1_600_000_000));
    assert!(stamp(1_600_000_000) < stamp(1_600_000_001));
    assert!(stamp(1_600_000_001) < Timestamp::MAX);
    assert_eq!(stamp(1_600_000_042), stamp(1_600_000_042));
    assert_eq!(Timestamp::MIN.max(Timestamp::MAX), Timestamp::MAX);
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// Every second of the range round-trips through its text: the text is twenty bytes in the
    /// shape `YYYY-MM-DDTHH:MM:SSZ`, parses back to the same instant with the same second
    /// count, and equals what jiff prints for the same whole second.
    ///
    /// Proves: SPEC 5.4, decision 98.
    #[test]
    fn every_second_in_the_range_round_trips(second in MIN_SECOND..=MAX_SECOND) {
        let instant = stamp(second);
        let text = instant.to_string();
        prop_assert_eq!(text.len(), 20);
        for (index, byte) in text.bytes().enumerate() {
            let expected = match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                19 => byte == b'Z',
                _ => byte.is_ascii_digit(),
            };
            prop_assert!(expected, "byte {} of {:?}", index, text);
        }
        let back: Timestamp = text.parse().unwrap_or_else(|error| panic!("{text:?}: {error}"));
        prop_assert_eq!(back, instant);
        prop_assert_eq!(back.as_second(), second);
        prop_assert_eq!(&text, &jiff_instant(second, 0).to_string());
    }

    /// A second count outside the range is refused as `OutOfRange`.
    ///
    /// Proves: decision 98.
    #[test]
    fn seconds_outside_the_range_are_refused(
        second in prop_oneof![i64::MIN..MIN_SECOND, (MAX_SECOND + 1)..=i64::MAX]
    ) {
        prop_assert_eq!(Timestamp::from_second(second), Err(TimestampError::OutOfRange));
    }
}
