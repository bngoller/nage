//! The pause after wrong passwords (SPEC 5.3's login rate limit as decision 111 amends it):
//! kept in memory, so a restart forgets it, keyed by the fold of the typed username whether
//! or not the user exists, and timed by the instants its caller hands in. A failure is a checked
//! attempt that was refused: a password that did not match, an unknown username (checked against
//! the dummy hash) or a deactivated user whatever the password (decision 111). For each username
//! the count of failures since the last success is kept; it is forgotten once [`MEMORY_SECONDS`]
//! (an hour) have passed since that username's last failure, and a success forgets it at once. The
//! first four failures start no pause; the fifth starts a pause of 60 seconds, the sixth of 120,
//! the seventh of 300, the eighth and every later one of 600 ([`pause_seconds`]). An attempt that
//! comes before a pause has passed is refused, without checking the password and without counting
//! as a failure, and a pause lasts its whole length: the callers ask for a pause before they
//! verify the password ([`Lockout::check`]) and again when the verification ends, under the same
//! lock as the count ([`Lockout::settle`]), so an attempt let through before a pause for its
//! username began is answered as one during that pause, whatever its password, and is neither
//! counted nor forgets the count: attempts sent together get no more checked answers than
//! attempts sent one after another. A typed username whose stored form
//! (`nage_core::fold::normalize`) is longer than 64 characters (`nage_core::fold::MAX_NAME_CHARS`)
//! is never remembered when it finds no user; one that finds a user, as a letter typed as a base
//! and a combining mark can, counts under its fold like any other spelling. A username is dropped
//! once its failures are forgotten: at the next failure of any username an hour after its own
//! last one, or at its success.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nage_core::timestamp::Timestamp;

#[cfg(test)]
mod tests;

/// The failure that starts the first pause (decision 111: so the sixth attempt is refused when it
/// comes within a minute of the fifth failure): the first four start none.
pub(crate) const FAILURES_BEFORE_PAUSE: usize = 5;

/// How long a username's count of failures is remembered, in seconds (decision 111: an hour):
/// it is forgotten once this many seconds have passed since that username's last failure.
pub(crate) const MEMORY_SECONDS: i64 = 3_600;

/// The pause the fifth, sixth and seventh failures start, in seconds (decision 111).
pub(crate) const PAUSE_SECONDS: [u64; 3] = [60, 120, 300];

/// The pause the eighth and every later failure starts, in seconds: the longest (decision
/// 111).
pub(crate) const MAX_PAUSE_SECONDS: u64 = 600;

/// What is remembered of one username: its failures since the last success, and the instant
/// of the last of them, at which the pause it started began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Failures {
    pub(crate) count: usize,
    pub(crate) last: Timestamp,
}

/// What the verification of an attempt's password found, as [`Lockout::settle`] takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// The password matched an active user's: a success.
    Succeeded,
    /// A failure (decision 111); `user_found` tells whether the typed username found a user, a
    /// deactivated one included.
    Failed { user_found: bool },
}

/// The remembered failures of every username, keyed by its fold. Cloning shares them.
#[derive(Clone)]
pub(crate) struct Lockout {
    failures: Arc<Mutex<HashMap<String, Failures>>>,
}

impl Lockout {
    /// No failure remembered.
    pub(crate) fn new() -> Self {
        Self {
            failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Whether an attempt for `username` (as typed; the key is its fold) may be checked at
    /// `now`: `Ok(())`, or `Err(seconds)` during a pause, the seconds left until an attempt
    /// is checked again, at least 1. The pause is the one the username's last failure started:
    /// it lasts [`pause_seconds`] of the count of failures at that failure, itself included,
    /// from that failure's instant. Nothing is counted here.
    pub(crate) fn check(&self, username: &str, now: Timestamp) -> Result<(), u64> {
        Self::pause_left(&self.remembered_failures(), username, now)
    }

    /// What [`Lockout::check`] answers for `username` at `now`, read from `remembered`.
    fn pause_left(
        remembered: &HashMap<String, Failures>,
        username: &str,
        now: Timestamp,
    ) -> Result<(), u64> {
        let Some(failures) = remembered.get(&fold(username)).copied() else {
            return Ok(());
        };
        // A clock that went back counts as no time passed: the pause is never longer than
        // its length.
        let passed =
            u64::try_from(now.as_second().saturating_sub(failures.last.as_second())).unwrap_or(0);
        match pause_seconds(failures.count).checked_sub(passed) {
            Some(left) if left > 0 => Err(left),
            _ => Ok(()),
        }
    }

    /// Decides an attempt for `username` again when the verification of its password has
    /// ended, under the one lock (decision 111): [`Lockout::check`] let the attempt through at
    /// `now`, and `attempt` is what the verification found. When a pause for the username has
    /// begun since, the answer is `Err(seconds)` with the seconds left, as [`Lockout::check`]
    /// gives them, and the attempt is neither counted as a failure nor forgets the failures,
    /// whatever it found. Otherwise the answer is `Ok(())`: a success forgets the username's
    /// failures and drops it, and a failure is remembered as `remember_failure` says.
    pub(crate) fn settle(
        &self,
        username: &str,
        now: Timestamp,
        attempt: Attempt,
    ) -> Result<(), u64> {
        let mut remembered = self.remembered_failures();
        Self::pause_left(&remembered, username, now)?;
        match attempt {
            Attempt::Succeeded => Self::forget_failures(&mut remembered, username),
            Attempt::Failed { user_found } => {
                Self::remember_failure(&mut remembered, username, now, user_found);
            }
        }
        Ok(())
    }

    /// Remembers in `remembered` that an attempt for `username` was checked at `now` and
    /// failed: the count of its failures grows by one and `now` becomes its last failure.
    /// First, every username whose last failure is [`MEMORY_SECONDS`] old or older at `now` is
    /// forgotten and dropped, this one included, so that its count starts again at one. A
    /// username whose stored form (`nage_core::fold::normalize`) is longer than
    /// `nage_core::fold::MAX_NAME_CHARS` (64) characters is not remembered at all when it found
    /// no user (`user_found` false); one that found a user is remembered under its fold, however
    /// long its typed spelling.
    fn remember_failure(
        remembered: &mut HashMap<String, Failures>,
        username: &str,
        now: Timestamp,
        user_found: bool,
    ) {
        remembered.retain(|_, failures| {
            now.as_second().saturating_sub(failures.last.as_second()) < MEMORY_SECONDS
        });
        if !user_found && normalize(username).chars().count() > MAX_NAME_CHARS {
            return;
        }
        let failures = remembered.entry(fold(username)).or_insert(Failures {
            count: 0,
            last: now,
        });
        failures.count = failures.count.saturating_add(1);
        failures.last = now;
    }

    /// Forgets in `remembered` the failures of `username` and drops it: its password was
    /// checked and matched.
    fn forget_failures(remembered: &mut HashMap<String, Failures>, username: &str) {
        remembered.remove(&fold(username));
    }

    /// Remembers a failure for `username` at `now` as [`Lockout::settle`] does for a typed
    /// username that found no user, without its second look at the pause: the tests count
    /// failures this way, as many as they like at one instant.
    #[cfg(test)]
    pub(crate) fn failed(&self, username: &str, now: Timestamp) {
        Self::remember_failure(&mut self.remembered_failures(), username, now, false);
    }

    /// Forgets the failures of `username` and drops it as [`Lockout::settle`] does for a
    /// success, without its second look at the pause.
    #[cfg(test)]
    pub(crate) fn succeeded(&self, username: &str) {
        Self::forget_failures(&mut self.remembered_failures(), username);
    }

    /// How many usernames are remembered: those with a failure in the last hour, as of the
    /// last failure remembered.
    #[cfg(test)]
    pub(crate) fn remembered(&self) -> usize {
        self.failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// How long the pause the `failures`-th failure since the last success starts lasts, in
/// seconds: none below [`FAILURES_BEFORE_PAUSE`], then 60, 120 and 300 ([`PAUSE_SECONDS`]),
/// and [`MAX_PAUSE_SECONDS`] from the eighth on, however many failures there are.
pub(crate) fn pause_seconds(failures: usize) -> u64 {
    match failures.checked_sub(FAILURES_BEFORE_PAUSE) {
        None => 0,
        Some(beyond) => PAUSE_SECONDS
            .get(beyond)
            .copied()
            .unwrap_or(MAX_PAUSE_SECONDS),
    }
}

use std::sync::{MutexGuard, PoisonError};

use nage_core::fold::{MAX_NAME_CHARS, fold, normalize};

impl Lockout {
    /// The remembered failures, locked. A poisoned lock is used as it is: every change made
    /// under it leaves the map whole.
    fn remembered_failures(&self) -> MutexGuard<'_, HashMap<String, Failures>> {
        self.failures.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
