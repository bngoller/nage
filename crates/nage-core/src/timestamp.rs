//! Whole-second UTC instants (SPEC 5.4, decisions 56, 89 and 98): the who/when stamps every row
//! carries, written and read as exactly `YYYY-MM-DDTHH:MM:SSZ`, from 2020-01-01T00:00:00Z (no
//! stamp can legitimately be earlier, so a clock reset to 1970 or 2000 refuses to stamp instead
//! of recording a wrong time) to 2199-12-31T23:59:59Z (the last year of the dates, decision 88).
//! The instant comes from the caller: only the binary reads the clock (decision 88), the core
//! never does.
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use std::fmt;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// A whole-second UTC instant from [`Timestamp::MIN`] to [`Timestamp::MAX`], a newtype over
/// jiff's instant with its sub-second part cut off; its text is exactly `YYYY-MM-DDTHH:MM:SSZ`,
/// which decision 56's CHECK accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(jiff::Timestamp);

impl Timestamp {
    /// The earliest instant, 2020-01-01T00:00:00Z: no stamp can legitimately be earlier.
    pub const MIN: Timestamp = Timestamp(jiff::Timestamp::constant(1_577_836_800, 0));
    /// The latest instant, 2199-12-31T23:59:59Z.
    pub const MAX: Timestamp = Timestamp(jiff::Timestamp::constant(7_258_118_399, 0));

    /// The instant `second` seconds after 1970-01-01T00:00:00Z, or `OutOfRange` outside
    /// [`Timestamp::MIN`] to [`Timestamp::MAX`].
    pub fn from_second(second: i64) -> Result<Timestamp, TimestampError> {
        if !(Timestamp::MIN.as_second()..=Timestamp::MAX.as_second()).contains(&second) {
            return Err(TimestampError::OutOfRange);
        }
        jiff::Timestamp::from_second(second)
            .map(Timestamp)
            .map_err(|_| TimestampError::OutOfRange)
    }

    /// `instant` cut to the whole second (the sub-second part dropped, never rounded up), or
    /// `OutOfRange` outside [`Timestamp::MIN`] to [`Timestamp::MAX`] (a clock reset to 1970 or
    /// 2000 reads as an instant before `MIN`). The binary reads `jiff::Timestamp::now()` and
    /// passes it here; the core never reads the clock.
    pub fn from_jiff(instant: jiff::Timestamp) -> Result<Timestamp, TimestampError> {
        // `as_second` truncates toward zero, which is the floor for every instant after the
        // epoch; an instant before the epoch lies outside the range whatever its fraction.
        Timestamp::from_second(instant.as_second())
    }

    /// The seconds since 1970-01-01T00:00:00Z.
    pub fn as_second(self) -> i64 {
        self.0.as_second()
    }

    /// The same instant as jiff's, for zone conversions (`Date::today`).
    pub fn to_jiff(self) -> jiff::Timestamp {
        self.0
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    /// Exactly `YYYY-MM-DDTHH:MM:SSZ`: four ASCII digits, a hyphen, two, a hyphen, two, a `T`,
    /// two, a colon, two, a colon, two and a `Z`, nothing before or after; `Format` otherwise,
    /// `Invalid` for a month, day, hour, minute or second that does not exist (a 13th month, a
    /// 30th of February, a 60th second), `OutOfRange` outside the range. jiff's own lenient
    /// parser is not used.
    fn from_str(text: &str) -> Result<Timestamp, TimestampError> {
        let shaped = text.len() == 20
            && text.bytes().enumerate().all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                19 => byte == b'Z',
                _ => byte.is_ascii_digit(),
            });
        if !shaped {
            return Err(TimestampError::Format);
        }
        let datetime = jiff::civil::DateTime::new(
            number(text, 0..4)?,
            number(text, 5..7)?,
            number(text, 8..10)?,
            number(text, 11..13)?,
            number(text, 14..16)?,
            number(text, 17..19)?,
            0,
        )
        .map_err(|_| TimestampError::Invalid)?;
        let instant = jiff::tz::Offset::UTC
            .to_timestamp(datetime)
            .map_err(|_| TimestampError::OutOfRange)?;
        Timestamp::from_second(instant.as_second())
    }
}

impl fmt::Display for Timestamp {
    /// `YYYY-MM-DDTHH:MM:SSZ`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let civil = jiff::tz::Offset::UTC.to_datetime(self.0);
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            civil.year(),
            civil.month(),
            civil.day(),
            civil.hour(),
            civil.minute(),
            civil.second()
        )
    }
}

/// Why a timestamp was refused (decision 98).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampError {
    /// Not exactly `YYYY-MM-DDTHH:MM:SSZ`.
    Format,
    /// No such instant: a month, day, hour, minute or second that does not exist.
    Invalid,
    /// Outside 2020-01-01T00:00:00Z to 2199-12-31T23:59:59Z.
    OutOfRange,
}

impl fmt::Display for TimestampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Format => "not a timestamp of the form YYYY-MM-DDTHH:MM:SSZ",
            Self::Invalid => "no such instant",
            Self::OutOfRange => "outside 2020-01-01T00:00:00Z to 2199-12-31T23:59:59Z",
        })
    }
}

impl std::error::Error for TimestampError {}

/// The number in `text[range]`, which the caller has checked holds only ASCII digits.
fn number<T: FromStr>(text: &str, range: std::ops::Range<usize>) -> Result<T, TimestampError> {
    text.get(range)
        .and_then(|digits| digits.parse().ok())
        .ok_or(TimestampError::Format)
}
