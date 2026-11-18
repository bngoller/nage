//! Calendar dates and half-open periods (SPEC 6.4, INV-6, decision 88) on jiff's civil dates,
//! and "today" in the configured time zone. A date has no time zone and its text is exactly
//! `YYYY-MM-DD`; a period holds its first date and not its end. The zone and the instant come
//! from the caller: only the binary reads `NAGE_TZ` (decisions 42 and 43), and no crate asks jiff
//! for the system's zone (the workspace's `clippy.toml` refuses jiff's three named system-zone
//! methods).
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::civil;
use jiff::tz::TimeZone;

#[cfg(test)]
mod tests;

/// A calendar date without a time zone from 1900-01-01 to 2199-12-31 (decision 88), read and
/// written as exactly `YYYY-MM-DD` (INV-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(civil::Date);

impl Date {
    /// The earliest date, 1900-01-01.
    pub const MIN: Date = Date(civil::date(1900, 1, 1));
    /// The latest date, 2199-12-31.
    pub const MAX: Date = Date(civil::date(2199, 12, 31));

    /// The date `year-month-day`: `OutOfRange` for a year outside 1900 to 2199, `Invalid` when
    /// that month or day does not exist (2026-02-29, month 13).
    pub fn new(year: i16, month: i8, day: i8) -> Result<Date, DateError> {
        if !(Date::MIN.year()..=Date::MAX.year()).contains(&year) {
            return Err(DateError::OutOfRange);
        }
        civil::Date::new(year, month, day)
            .map(Date)
            .map_err(|_| DateError::Invalid)
    }

    /// The year, 1900 to 2199.
    pub fn year(self) -> i16 {
        self.0.year()
    }

    /// The month, 1 to 12.
    pub fn month(self) -> i8 {
        self.0.month()
    }

    /// The day of the month, 1 to 31.
    pub fn day(self) -> i8 {
        self.0.day()
    }

    /// The next day, or `None` after [`Date::MAX`].
    pub fn tomorrow(self) -> Option<Date> {
        self.0
            .tomorrow()
            .ok()
            .and_then(|date| Date::try_from(date).ok())
    }

    /// The previous day, or `None` before [`Date::MIN`].
    pub fn yesterday(self) -> Option<Date> {
        self.0
            .yesterday()
            .ok()
            .and_then(|date| Date::try_from(date).ok())
    }

    /// Today: the date in `zone` at the instant `now` (SPEC 6.4, "the current date in the
    /// configured time zone"). The binary resolves the zone from `--tz`, `NAGE_TZ` or `TZ`
    /// (decision 43) and the caller reads the clock, so this is pure; `OutOfRange` when the date
    /// in `zone` falls outside 1900-01-01 to 2199-12-31.
    pub fn today(zone: &TimeZone, now: Timestamp) -> Result<Date, DateError> {
        Date::try_from(zone.to_datetime(now).date())
    }
}

impl TryFrom<civil::Date> for Date {
    type Error = DateError;

    /// `date` when it lies in 1900-01-01 to 2199-12-31, else `OutOfRange`.
    fn try_from(date: civil::Date) -> Result<Date, DateError> {
        if (Date::MIN.0..=Date::MAX.0).contains(&date) {
            Ok(Date(date))
        } else {
            Err(DateError::OutOfRange)
        }
    }
}

impl From<Date> for civil::Date {
    fn from(date: Date) -> civil::Date {
        date.0
    }
}

impl FromStr for Date {
    type Err = DateError;

    /// Exactly `YYYY-MM-DD`: four ASCII digits, a hyphen, two, a hyphen and two, nothing before
    /// or after (a form trims its field first); `Format` otherwise, then [`Date::new`]'s refusals.
    fn from_str(text: &str) -> Result<Date, DateError> {
        let shaped = text.len() == 10
            && text.bytes().enumerate().all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        if !shaped {
            return Err(DateError::Format);
        }
        Date::new(
            number(text, 0..4)?,
            number(text, 5..7)?,
            number(text, 8..10)?,
        )
    }
}

impl fmt::Display for Date {
    /// `YYYY-MM-DD`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = (self.0.year(), self.0.month(), self.0.day());
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// A half-open period of dates, `[start, end)`: it holds `start` and each later date before
/// `end`, never `end` itself, and is never empty (SPEC 6.4: a month is `[first day, first day of
/// next month)`; INV-6). No period holds 2199-12-31, whose end would be 2200-01-01, past
/// [`Date::MAX`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Period {
    start: Date,
    end: Date,
}

impl Period {
    /// `[start, end)`, or `EmptyPeriod` unless `start` is before `end`.
    pub fn new(start: Date, end: Date) -> Result<Period, DateError> {
        if start < end {
            Ok(Period { start, end })
        } else {
            Err(DateError::EmptyPeriod)
        }
    }

    /// The dates `first` to `last`, both included, as `[first, the day after last)`:
    /// `EmptyPeriod` when `last` is before `first`, `OutOfRange` when `last` is [`Date::MAX`].
    pub fn from_inclusive(first: Date, last: Date) -> Result<Period, DateError> {
        Period::new(first, last.tomorrow().ok_or(DateError::OutOfRange)?)
    }

    /// The month that holds `date`: `[its first day, the first day of the next month)`;
    /// `OutOfRange` in December 2199, whose end lies after [`Date::MAX`].
    pub fn month_of(date: Date) -> Result<Period, DateError> {
        let start = Date(date.0.first_of_month());
        let end = Date(date.0.last_of_month())
            .tomorrow()
            .ok_or(DateError::OutOfRange)?;
        Period::new(start, end)
    }

    /// The month `month` of `year`.
    pub fn month(year: i16, month: i8) -> Result<Period, DateError> {
        Period::month_of(Date::new(year, month, 1)?)
    }

    /// The year: `[January 1, January 1 of the next year)`; `OutOfRange` for 2199.
    pub fn year(year: i16) -> Result<Period, DateError> {
        let start = Date::new(year, 1, 1)?;
        let end = Date(start.0.last_of_year())
            .tomorrow()
            .ok_or(DateError::OutOfRange)?;
        Period::new(start, end)
    }

    /// The first date of the period.
    pub fn start(self) -> Date {
        self.start
    }

    /// The first date after the period, which it does not hold.
    pub fn end(self) -> Date {
        self.end
    }

    /// Whether `date` lies in the period: `start <= date < end`.
    pub fn contains(self, date: Date) -> bool {
        (self.start..self.end).contains(&date)
    }
}

/// Why a date or a period was refused (INV-6, decision 88).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateError {
    /// Not exactly `YYYY-MM-DD`.
    Format,
    /// No such month or day, such as 2026-02-29 or month 13.
    Invalid,
    /// Outside 1900-01-01 to 2199-12-31, or a period that would end after 2199-12-31.
    OutOfRange,
    /// A period whose end is not after its start.
    EmptyPeriod,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Format => "not a date of the form YYYY-MM-DD",
            Self::Invalid => "no such date",
            Self::OutOfRange => "outside 1900-01-01 to 2199-12-31",
            Self::EmptyPeriod => "an empty period: its end is not after its start",
        })
    }
}

impl std::error::Error for DateError {}

/// The number in `text[range]`, which the caller has checked holds only ASCII digits.
fn number<T: FromStr>(text: &str, range: std::ops::Range<usize>) -> Result<T, DateError> {
    text.get(range)
        .and_then(|digits| digits.parse().ok())
        .ok_or(DateError::Format)
}
