//! Tests of dates, periods and today (SPEC 6.4, INV-6, decision 88).

use jiff::tz::{Offset, TimeZone, TimeZoneDatabase};
use jiff::{Span, Timestamp, civil};
use proptest::prelude::*;

use super::{Date, DateError, Period};

/// The days from 1900-01-01 to 2199-12-31.
const RANGE_DAYS: i32 = 109_572;

/// Edge dates of the range and of the calendar: the ends of the range, the turns of century in
/// it (1900 and 2100 not leap years, 2000 one), leap days, month and year ends.
const EDGE_DATES: &[(i16, i8, i8)] = &[
    (1900, 1, 1),
    (1900, 2, 28),
    (1900, 3, 1),
    (1999, 12, 31),
    (2000, 2, 29),
    (2000, 3, 1),
    (2024, 2, 29),
    (2026, 1, 31),
    (2100, 2, 28),
    (2100, 3, 1),
    (2199, 11, 30),
    (2199, 12, 1),
    (2199, 12, 31),
];

/// The date written `text`, which must be one.
fn date(text: &str) -> Date {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

/// The range's date `date`, which must lie in it.
fn in_range(date: civil::Date) -> Date {
    Date::try_from(date).unwrap_or_else(|error| panic!("{date}: {error}"))
}

/// The zone `name` from jiff's bundled database, so no test depends on the host's zoneinfo.
fn zone(name: &str) -> TimeZone {
    TimeZoneDatabase::bundled()
        .get(name)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The instant `second` seconds after 1970-01-01T00:00:00Z.
fn instant(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap_or_else(|error| panic!("{second}: {error}"))
}

/// A date of the range: an edge date, or any date, uniformly by its day number.
fn any_date() -> impl Strategy<Value = Date> {
    prop_oneof![
        1 => prop::sample::select(EDGE_DATES)
            .prop_map(|(year, month, day)| in_range(civil::date(year, month, day))),
        3 => (0..=RANGE_DAYS).prop_map(|days| {
            let start = civil::Date::from(Date::MIN);
            in_range(start.checked_add(Span::new().days(days)).unwrap_or_else(|error| panic!("{days}: {error}")))
        }),
    ]
}

/// Dates read and write exactly `YYYY-MM-DD`: the range's first and last dates, leap days (2000
/// and 2024), the day before a turn of century, a non-leap February 28 (1900) and an ordinary
/// day, each written back as it was read; `Date::new` builds the same date from its parts.
///
/// Proves: INV-6 (core half).
#[test]
fn dates_read_and_write_iso_text() {
    for text in [
        "1900-01-01",
        "1900-02-28",
        "1999-12-31",
        "2000-02-29",
        "2024-02-29",
        "2031-07-15",
        "2199-12-31",
    ] {
        assert_eq!(date(text).to_string(), text);
    }
    assert_eq!(date("1900-01-01"), Date::MIN);
    assert_eq!(date("2199-12-31"), Date::MAX);
    assert_eq!(Date::new(2031, 7, 15), Ok(date("2031-07-15")));
    let leap = date("2024-02-29");
    assert_eq!((leap.year(), leap.month(), leap.day()), (2024, 2, 29));
}

/// Text that is not exactly a date of the range is refused with its reason: other shapes
/// (missing zero padding, the basic form, slashes or one slash, white space, a time, a zone or
/// extra digits after the date, a sign before the year or in place of a digit, a six-digit year,
/// Arabic-Indic digits) as `Format`; days and months that do not exist, February 29 of 1900 and
/// 2026 included, as `Invalid`; and years outside 1900 to 2199 as `OutOfRange`.
///
/// Proves: INV-6 (core half), decision 88.
#[test]
fn other_text_is_refused_with_its_reason() {
    use DateError::{Format, Invalid, OutOfRange};
    for (text, expected) in [
        ("", Format),
        ("2026-1-05", Format),
        ("2026-01-5", Format),
        ("20260105", Format),
        ("2026/01/05", Format),
        ("2026-01/05", Format),
        (" 2026-01-05", Format),
        ("2026-01-05 ", Format),
        ("2026-01-05T00:00", Format),
        ("2026-01-05Z", Format),
        ("2026-01-05[UTC]", Format),
        ("2026-01-0512", Format),
        ("+2026-01-05", Format),
        ("+002026-01-05", Format),
        ("+026-01-05", Format),
        ("2026-+1-05", Format),
        ("2026--1-05", Format),
        ("\u{662}\u{660}\u{662}\u{666}-01-05", Format),
        ("2026-02-29", Invalid),
        ("1900-02-29", Invalid),
        ("2026-04-31", Invalid),
        ("2026-13-01", Invalid),
        ("2026-00-10", Invalid),
        ("2026-01-00", Invalid),
        ("2026-01-32", Invalid),
        ("1899-12-31", OutOfRange),
        ("2200-01-01", OutOfRange),
        ("0026-01-05", OutOfRange),
        ("9999-12-31", OutOfRange),
    ] {
        assert_eq!(text.parse::<Date>(), Err(expected), "{text:?}");
    }
}

/// A date's neighbours step across month, year and leap-day boundaries (the non-leap Februaries
/// of 1900 and 2100 included), reach the range's last day and stop at its ends; `Date::new`
/// refuses a day that does not exist and a year outside the range.
///
/// Proves: INV-6 (core half), decision 88.
#[test]
fn days_step_across_boundaries_and_stop_at_the_ends() {
    for (day, next) in [
        ("2026-01-31", "2026-02-01"),
        ("2026-02-28", "2026-03-01"),
        ("2024-02-28", "2024-02-29"),
        ("2024-02-29", "2024-03-01"),
        ("1900-02-28", "1900-03-01"),
        ("2100-02-28", "2100-03-01"),
        ("2026-12-31", "2027-01-01"),
        ("2199-12-30", "2199-12-31"),
    ] {
        assert_eq!(date(day).tomorrow(), Some(date(next)), "{day}");
        assert_eq!(date(next).yesterday(), Some(date(day)), "{next}");
    }
    assert_eq!(Date::MAX.tomorrow(), None);
    assert_eq!(Date::MIN.yesterday(), None);
    assert_eq!(Date::new(2026, 2, 29), Err(DateError::Invalid));
    assert_eq!(Date::new(2026, 13, 1), Err(DateError::Invalid));
    assert_eq!(Date::new(1899, 12, 31), Err(DateError::OutOfRange));
    assert_eq!(Date::new(2200, 1, 1), Err(DateError::OutOfRange));
}

/// A jiff date converts to a `Date` exactly when it lies in the range, and back unchanged; the
/// range spans 109,572 days.
///
/// Proves: INV-6 (core half), decision 88.
#[test]
fn jiff_dates_convert_within_the_range() {
    assert_eq!(Date::try_from(civil::date(1900, 1, 1)), Ok(Date::MIN));
    assert_eq!(Date::try_from(civil::date(2199, 12, 31)), Ok(Date::MAX));
    assert_eq!(
        Date::try_from(civil::date(1899, 12, 31)),
        Err(DateError::OutOfRange)
    );
    assert_eq!(
        Date::try_from(civil::date(2200, 1, 1)),
        Err(DateError::OutOfRange)
    );
    assert_eq!(Date::try_from(civil::Date::MIN), Err(DateError::OutOfRange));
    assert_eq!(civil::Date::from(Date::MIN), civil::date(1900, 1, 1));
    let start = civil::Date::from(Date::MIN);
    assert_eq!(
        start.checked_add(Span::new().days(RANGE_DAYS)).ok(),
        Some(civil::Date::from(Date::MAX))
    );
}

/// Periods are half-open: a month runs from its first day to the first day of the next month and
/// holds its first and last days but not that next first day; a year runs from January 1 to the
/// next January 1; the inclusive form ends the day after its last date; an empty or reversed
/// period is refused, and so is a period whose end would fall after 2199-12-31 or whose month or
/// year lies before 1900.
///
/// Proves: INV-6 (core half), SPEC 6.4.
#[test]
fn periods_are_half_open() {
    let february = Period::month(2024, 2).unwrap_or_else(|error| panic!("2024-02: {error}"));
    assert_eq!(
        (february.start(), february.end()),
        (date("2024-02-01"), date("2024-03-01"))
    );
    assert!(february.contains(date("2024-02-01")));
    assert!(february.contains(date("2024-02-29")));
    assert!(!february.contains(date("2024-03-01")));
    assert!(!february.contains(date("2024-01-31")));
    assert_eq!(Period::month_of(date("2031-07-15")), Period::month(2031, 7));
    assert_eq!(
        Period::month(2026, 12).map(Period::end),
        Ok(date("2027-01-01"))
    );
    assert_eq!(
        Period::month(2026, 2).map(Period::end),
        Ok(date("2026-03-01"))
    );
    assert_eq!(
        Period::month(2199, 11).map(Period::end),
        Ok(date("2199-12-01"))
    );
    let year = Period::year(2026).unwrap_or_else(|error| panic!("2026: {error}"));
    assert_eq!(
        (year.start(), year.end()),
        (date("2026-01-01"), date("2027-01-01"))
    );
    assert_eq!(Period::year(2024).map(Period::end), Ok(date("2025-01-01")));
    let day = Period::from_inclusive(date("2031-07-15"), date("2031-07-15"))
        .unwrap_or_else(|error| panic!("one day: {error}"));
    assert_eq!(
        (day.start(), day.end()),
        (date("2031-07-15"), date("2031-07-16"))
    );
    for (start, end) in [("2031-07-15", "2031-07-15"), ("2031-07-16", "2031-07-15")] {
        assert_eq!(
            Period::new(date(start), date(end)),
            Err(DateError::EmptyPeriod)
        );
    }
    assert_eq!(
        Period::from_inclusive(date("2031-07-16"), date("2031-07-15")),
        Err(DateError::EmptyPeriod)
    );
    assert_eq!(Period::month(2199, 12), Err(DateError::OutOfRange));
    assert_eq!(Period::year(2199), Err(DateError::OutOfRange));
    assert_eq!(Period::year(1899), Err(DateError::OutOfRange));
    assert_eq!(
        Period::from_inclusive(date("2031-07-15"), Date::MAX),
        Err(DateError::OutOfRange)
    );
    assert_eq!(Period::month(2026, 13), Err(DateError::Invalid));
    assert_eq!(Period::month(1899, 12), Err(DateError::OutOfRange));
}

/// Today is the date in the configured zone at the given instant: at 2026-01-01T00:00:00Z it is
/// still 2025-12-31 in Pacific/Pago_Pago (UTC-11) and 2026-01-01 in UTC and Pacific/Kiritimati
/// (UTC+14); at 2025-12-31T10:30:00Z those three zones show three dates; Australia/Lord_Howe's
/// half-hour shift shows on both sides of its year (2026-01-01T13:15:00Z is already 2026-01-02 at
/// UTC+11 in its summer, 2026-06-30T13:15:00Z still 2026-06-30 at UTC+10:30 in its winter);
/// Asia/Kathmandu (UTC+5:45) turns to 2026-07-01 at 2026-06-30T18:15:00Z exactly; a fixed offset
/// follows the same rule; an instant whose date in the zone lies outside the range is refused, at
/// both ends and on either side of UTC.
///
/// Proves: SPEC 6.4, decision 88.
#[test]
fn today_is_the_date_in_the_configured_zone() {
    for (name, second, expected) in [
        ("UTC", 1_767_225_600, "2026-01-01"),
        ("Pacific/Pago_Pago", 1_767_225_600, "2025-12-31"),
        ("Pacific/Kiritimati", 1_767_225_600, "2026-01-01"),
        ("Pacific/Pago_Pago", 1_767_177_000, "2025-12-30"),
        ("UTC", 1_767_177_000, "2025-12-31"),
        ("Pacific/Kiritimati", 1_767_177_000, "2026-01-01"),
        ("Australia/Lord_Howe", 1_767_273_300, "2026-01-02"),
        ("Australia/Lord_Howe", 1_782_825_300, "2026-06-30"),
        ("Asia/Kathmandu", 1_782_843_299, "2026-06-30"),
        ("Asia/Kathmandu", 1_782_843_300, "2026-07-01"),
    ] {
        assert_eq!(
            Date::today(&zone(name), instant(second)),
            Ok(date(expected)),
            "{name} at {second}"
        );
    }
    let minus_eleven = TimeZone::fixed(jiff::tz::offset(-11));
    assert_eq!(
        Date::today(&minus_eleven, instant(1_767_225_600)),
        Ok(date("2025-12-31"))
    );
    for (hours, second, expected) in [
        (0, -2_208_988_800, Ok(Date::MIN)),
        (-1, -2_208_988_800, Err(DateError::OutOfRange)),
        (1, -2_208_988_801, Ok(Date::MIN)),
        (0, 7_258_118_399, Ok(Date::MAX)),
        (14, 7_258_068_000, Err(DateError::OutOfRange)),
        (14, 7_258_067_999, Ok(Date::MAX)),
        (-1, 7_258_118_400, Ok(Date::MAX)),
    ] {
        assert_eq!(
            Date::today(&TimeZone::fixed(jiff::tz::offset(hours)), instant(second)),
            expected,
            "UTC{hours:+} at {second}"
        );
    }
    for second in [-2_208_988_801, 7_258_118_400] {
        assert_eq!(
            Date::today(&TimeZone::UTC, instant(second)),
            Err(DateError::OutOfRange),
            "{second}"
        );
    }
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// Every date of the range reads back from its text as itself, and its text is the ten
    /// characters of `YYYY-MM-DD` that the schema's date CHECK takes (decision 56).
    ///
    /// Proves: INV-6 (core half).
    #[test]
    fn dates_read_back_from_their_text(day in any_date()) {
        let text = day.to_string();
        prop_assert_eq!(text.len(), 10);
        prop_assert_eq!(text.parse::<Date>(), Ok(day));
    }

    /// A text shaped `YYYY-MM-DD` is a date exactly when the calendar has that day and its year is
    /// 1900 to 2199: over years around and inside the range and over every year to 9999, every
    /// month from 0 to 13 and every day from 0 to 32, the parser agrees with jiff's calendar.
    ///
    /// Proves: INV-6 (core half), decision 88.
    #[test]
    fn text_is_a_date_exactly_when_the_day_exists(
        year in prop_oneof![1890_i16..=2210, 0_i16..=9999],
        month in 0_i8..=13,
        day in 0_i8..=32,
    ) {
        let text = format!("{year:04}-{month:02}-{day:02}");
        let expected = if (1900..=2199).contains(&year) {
            civil::Date::new(year, month, day).map_err(|_| DateError::Invalid)
        } else {
            Err(DateError::OutOfRange)
        };
        prop_assert_eq!(text.parse::<Date>().map(civil::Date::from), expected, "{}", text);
    }

    /// The month of a date holds it, starts on the first of its month and ends where the next
    /// month starts, so consecutive months tile the calendar without gap or overlap; only
    /// December 2199, whose end lies after the range, is refused.
    ///
    /// Proves: INV-6 (core half), SPEC 6.4.
    #[test]
    fn months_tile_the_calendar(day in any_date()) {
        let calendar = civil::Date::from(day);
        match Period::month_of(day) {
            Err(error) => {
                prop_assert_eq!(error, DateError::OutOfRange);
                prop_assert_eq!((day.year(), day.month()), (2199, 12));
            }
            Ok(month) => {
                prop_assert!(month.contains(day));
                prop_assert!(!month.contains(month.end()));
                prop_assert_eq!(civil::Date::from(month.start()), calendar.first_of_month());
                prop_assert_eq!(month.end().yesterday().map(civil::Date::from), Some(calendar.last_of_month()));
                if let (Ok(next), Some(last)) = (Period::month_of(month.end()), month.end().yesterday()) {
                    prop_assert_eq!(next.start(), month.end());
                    prop_assert!(!next.contains(last));
                }
            }
        }
    }

    /// A period holds a date exactly when the date is on or after its start and before its end,
    /// for any start, any later end and any date; a period whose end is not after its start is
    /// refused, the empty period at every date included; the inclusive form holds its first and
    /// last dates and ends the day after, or is refused when that day lies after the range.
    ///
    /// Proves: INV-6 (core half).
    #[test]
    fn periods_hold_exactly_the_dates_from_start_to_before_end(
        a in any_date(),
        b in any_date(),
        probe in any_date(),
    ) {
        match Period::new(a, b) {
            Err(error) => {
                prop_assert_eq!(error, DateError::EmptyPeriod);
                prop_assert!(b <= a);
            }
            Ok(period) => {
                prop_assert!(a < b);
                prop_assert_eq!(period.contains(probe), a <= probe && probe < b);
                prop_assert!(period.contains(a));
                prop_assert!(!period.contains(b));
            }
        }
        prop_assert_eq!(Period::new(a, a), Err(DateError::EmptyPeriod));
        let (first, last) = (a.min(b), a.max(b));
        match Period::from_inclusive(first, last) {
            Err(error) => {
                prop_assert_eq!(error, DateError::OutOfRange);
                prop_assert_eq!(last, Date::MAX);
            }
            Ok(period) => {
                prop_assert!(period.contains(first));
                prop_assert!(period.contains(last));
                prop_assert_eq!(Some(period.end()), last.tomorrow());
            }
        }
    }

    /// Today in a zone of fixed offset is the date of the instant moved by that offset, computed
    /// here from whole days since 1970 (not by the zone under test), for any instant from a day
    /// before 1900 to a day after 2199 and any offset from UTC-12 to UTC+14 in minutes; a date
    /// outside the range is refused.
    ///
    /// Proves: SPEC 6.4, decision 88.
    #[test]
    fn today_follows_the_zone_offset(
        second in -2_209_075_200_i64..=7_258_204_800,
        minutes in -720_i32..=840,
    ) {
        let offset = Offset::from_seconds(minutes * 60)
            .unwrap_or_else(|error| panic!("{minutes} minutes: {error}"));
        let days = (second + i64::from(minutes) * 60).div_euclid(86_400);
        let local = civil::date(1970, 1, 1)
            .checked_add(Span::new().days(days))
            .unwrap_or_else(|error| panic!("{days} days: {error}"));
        let range = civil::Date::from(Date::MIN)..=civil::Date::from(Date::MAX);
        let expected = if range.contains(&local) { Ok(local) } else { Err(DateError::OutOfRange) };
        prop_assert_eq!(
            Date::today(&TimeZone::fixed(offset), instant(second)).map(civil::Date::from),
            expected
        );
    }
}
