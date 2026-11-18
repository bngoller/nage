//! The clock the application is handed (decisions 88, 108 and 113): nage-web never reads the
//! system's clock. `serve` hands one that reads it; tests hand one they set, so that expiry,
//! the hourly touch, the pause after wrong passwords and the turn of the day are tested
//! without waiting. Today is read from it with the configured zone (decision 120).

use std::sync::Arc;

use jiff::tz::TimeZone;
use nage_core::date::Date;
use nage_core::timestamp::{Timestamp, TimestampError};

use crate::AppError;

#[cfg(test)]
mod tests;

type NowFn = dyn Fn() -> Result<Timestamp, TimestampError> + Send + Sync;

/// An injectable source of the current instant, whole seconds in UTC. Cloning shares it.
#[derive(Clone)]
pub struct Clock(Arc<NowFn>);

impl Clock {
    /// A clock that asks `now` at every reading. `serve` passes the system clock cut to the
    /// second (`Timestamp::from_jiff(jiff::Timestamp::now())`); an instant outside the range a
    /// [`Timestamp`] holds is that conversion's error, and the request that needed the instant
    /// answers 500 instead of stamping a wrong time.
    pub fn new<F>(now: F) -> Self
    where
        F: Fn() -> Result<Timestamp, TimestampError> + Send + Sync + 'static,
    {
        Self(Arc::new(now))
    }

    /// The current instant.
    pub fn now(&self) -> Result<Timestamp, TimestampError> {
        (self.0)()
    }
}

/// Today (SPEC 6.4, decision 120): the date in `zone` at the instant `clock` shows, through
/// `nage_core::date::Date::today`. Each request that needs today asks for it, so the day
/// turns at midnight in the configured zone, whatever the server's own zone is. A clock
/// outside the years a stamp holds, and an instant whose date in `zone` lies outside
/// 1900-01-01 to 2199-12-31, are `AppError::Internal`: the request answers 500, a failure and
/// not a refusal.
pub(crate) fn today(clock: &Clock, zone: &TimeZone) -> Result<Date, AppError> {
    let now = clock.now()?;
    Date::today(zone, now.to_jiff()).map_err(|error| {
        AppError::Internal(format!(
            "today in the configured zone at {now} is outside the dates Nage holds: {error}"
        ))
    })
}
