//! Today, read from a clock the test sets in a zone the test names (decision 120).

use jiff::tz::TimeZone;
use nage_core::date::Date;
use nage_core::timestamp::{Timestamp, TimestampError};

use super::{Clock, today};
use crate::AppError;

/// A clock that stands at the instant written `instant`.
fn clock_at(instant: &str) -> Clock {
    let stamp: Timestamp = instant
        .parse()
        .unwrap_or_else(|error| panic!("{instant:?} is no instant: {error}"));
    Clock::new(move || Ok(stamp))
}

/// The zone called `name`.
fn zone(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap_or_else(|error| panic!("{name} is no time zone: {error}"))
}

/// The date written `text`.
fn date(text: &str) -> Date {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is no date: {error}"))
}

/// Today is the date in the configured zone at the clock's instant, so the day turns at
/// midnight there and not at midnight UTC: one second before midnight in Toronto it is still
/// the day before, in winter (five hours behind UTC) and in summer (four); far east of UTC it
/// is already the next day; and in UTC the day is the instant's own date.
///
/// Proves: decision 120 (each request that needs today takes it as the date in the configured
/// zone at the clock's instant, so the day turns at midnight in that zone), SPEC 6.4, decision
/// 88.
#[test]
fn today_is_the_date_in_the_configured_zone_at_the_clocks_instant() {
    for (instant, name, expected) in [
        ("2027-01-15T04:59:59Z", "America/Toronto", "2027-01-14"),
        ("2027-01-15T05:00:00Z", "America/Toronto", "2027-01-15"),
        ("2027-07-01T03:59:59Z", "America/Toronto", "2027-06-30"),
        ("2027-07-01T04:00:00Z", "America/Toronto", "2027-07-01"),
        ("2027-01-15T09:59:59Z", "Pacific/Kiritimati", "2027-01-15"),
        ("2027-01-15T10:00:00Z", "Pacific/Kiritimati", "2027-01-16"),
        ("2027-01-15T04:59:59Z", "UTC", "2027-01-15"),
        ("2027-01-15T23:59:59Z", "UTC", "2027-01-15"),
    ] {
        let found = today(&clock_at(instant), &zone(name))
            .unwrap_or_else(|error| panic!("no today at {instant} in {name}: {error:?}"));
        assert_eq!(found, date(expected), "today at {instant} in {name}");
    }
}

/// An instant whose date in the zone lies after 2199-12-31 has no today, and neither has a
/// clock that shows no instant of Nage: both are a failure with its message, never a date.
///
/// Proves: decision 120 (an instant whose date falls outside decision 88's range answers
/// 500, a failure and not a refusal), decision 98 (a clock outside the years a stamp holds).
#[test]
fn a_day_outside_the_range_is_a_failure() {
    let late = Clock::new(|| Ok(Timestamp::MAX));
    assert_eq!(
        today(&late, &zone("UTC")).ok(),
        Some(date("2199-12-31")),
        "today at the last instant in UTC"
    );
    let beyond = today(&late, &zone("Pacific/Kiritimati"));
    assert!(
        matches!(beyond, Err(AppError::Internal(_))),
        "today at the last instant, fourteen hours east of UTC: {beyond:?}"
    );
    let broken = Clock::new(|| Err(TimestampError::OutOfRange));
    let none = today(&broken, &zone("UTC"));
    assert!(
        matches!(none, Err(AppError::Internal(_))),
        "today by a clock outside the years a stamp holds: {none:?}"
    );
}
