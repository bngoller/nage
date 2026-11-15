//! The clock the application is handed (decisions 88, 108 and 113): nage-web never reads the
//! system's clock. `serve` hands one that reads it; tests hand one they set, so that expiry,
//! the hourly touch and the pause after wrong passwords are tested without waiting.

use std::sync::Arc;

use nage_core::timestamp::{Timestamp, TimestampError};

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
