//! The server's log line (SPEC 5.2, decision 160, amending 47): one event per line, a whole-second
//! UTC stamp `YYYY-MM-DDTHH:MM:SSZ`, the level, the message, then the fields as key=value, with no
//! module name. The stamp is a small timer of Nage's own over jiff, which the binary already
//! carries; no zone, offset or local time is ever written.

use std::fmt;

use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

#[cfg(test)]
mod tests;

/// The log's timer: the system clock's instant cut to the whole second, in UTC (decision 160).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Stamp;

impl FormatTime for Stamp {
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        let second = jiff::Timestamp::now().as_second();
        // The instant cut to the whole second is always an instant jiff can hold; the epoch is
        // written should it ever not be, rather than no stamp at all.
        let whole = jiff::Timestamp::from_second(second).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        write!(writer, "{}", whole.strftime("%Y-%m-%dT%H:%M:%SZ"))
    }
}
