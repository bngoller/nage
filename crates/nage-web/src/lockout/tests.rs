//! The pause's lengths and memory, without a server (decision 111).

use nage_core::timestamp::Timestamp;

use super::{
    Attempt, FAILURES_BEFORE_PAUSE, Lockout, MAX_PAUSE_SECONDS, MEMORY_SECONDS, PAUSE_SECONDS,
    pause_seconds,
};

/// The instant `seconds` after 2027-01-15T08:00:00Z.
fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds)
        .unwrap_or_else(|error| panic!("{seconds} is no instant of the tests: {error}"))
}

/// `count` failures for `username`, all at second `second`.
fn fail(lockout: &Lockout, username: &str, count: usize, second: i64) {
    for _ in 0..count {
        lockout.failed(username, at(second));
    }
}

/// The pause the k-th failure since the last success starts: none for the first four, then
/// one minute, two, five, and ten minutes from the eighth on, however many failures there
/// are, without overflowing.
///
/// Proves: decision 111 (the lengths that amend SPEC 5.3's rate limit: 60, 120, 300 and 600
/// seconds; five free tries).
#[test]
fn the_pause_lasts_one_two_five_then_ten_minutes() {
    assert_eq!(FAILURES_BEFORE_PAUSE, 5, "the failure that starts a pause");
    assert_eq!(PAUSE_SECONDS, [60, 120, 300], "the first three pauses");
    assert_eq!(MAX_PAUSE_SECONDS, 600, "the longest pause");
    assert_eq!(MEMORY_SECONDS, 3_600, "how long a count is remembered");
    for (failures, seconds) in [
        (0, 0),
        (1, 0),
        (4, 0),
        (5, 60),
        (6, 120),
        (7, 300),
        (8, 600),
        (9, 600),
        (10, 600),
        (64, 600),
        (1_000_000, 600),
        (usize::MAX, 600),
    ] {
        assert_eq!(
            pause_seconds(failures),
            seconds,
            "the pause after {failures} failures"
        );
    }
}

/// Each failure starts its pause at its own instant: four failures pause nothing; the fifth
/// pauses the username for 60 seconds, in any spelling its fold makes equal, and leaves other
/// usernames alone; an attempt during the pause is told the seconds left, however often it
/// comes, and changes nothing; once the pause has passed the next failure is the sixth and
/// pauses for 120 seconds, the seventh for 300, the eighth for 600, and the tenth for 600
/// again.
///
/// Proves: decision 111 (keyed by the fold of the typed username; the count of failures since
/// the last success; the lengths; a refused attempt is not counted).
#[test]
fn each_failure_starts_the_pause_of_its_count() {
    let lockout = Lockout::new();
    for second in 0..4 {
        lockout.failed("sam", at(second));
        assert_eq!(
            lockout.check("sam", at(second)),
            Ok(()),
            "an attempt after {} failures",
            second + 1
        );
    }
    lockout.failed("Sam", at(10));
    assert_eq!(
        lockout.check("sam", at(10)),
        Err(60),
        "an attempt at the fifth failure's instant"
    );
    assert_eq!(
        lockout.check(" SAM ", at(10)),
        Err(60),
        "an attempt in another spelling of the username"
    );
    assert_eq!(
        lockout.check("alex", at(10)),
        Ok(()),
        "an attempt for another username"
    );
    assert_eq!(
        lockout.check("sam", at(40)),
        Err(30),
        "an attempt half-way through the pause"
    );
    assert_eq!(
        lockout.check("sam", at(69)),
        Err(1),
        "an attempt a second before the pause ends"
    );
    assert_eq!(
        lockout.check("sam", at(70)),
        Ok(()),
        "an attempt when the pause has passed"
    );

    let mut second = 70;
    for (failure, pause) in [(6, 120), (7, 300), (8, 600), (9, 600), (10, 600)] {
        lockout.failed("sam", at(second));
        assert_eq!(
            lockout.check("sam", at(second)),
            Err(pause),
            "an attempt at failure {failure}'s instant"
        );
        let length = i64::try_from(pause).unwrap_or(i64::MAX);
        assert_eq!(
            lockout.check("sam", at(second + length - 1)),
            Err(1),
            "an attempt a second before the pause after failure {failure} ends"
        );
        second += length;
        assert_eq!(
            lockout.check("sam", at(second)),
            Ok(()),
            "an attempt when the pause after failure {failure} has passed"
        );
    }
}

/// The count is forgotten an hour after the username's last failure, to the second, and not
/// before: a failure 3,599 seconds after the fifth is the sixth and pauses for 120 seconds;
/// a failure 3,600 seconds after the last one is the first again and pauses nothing. A
/// success forgets the count at once: four more failures after it pause nothing.
///
/// Proves: decision 111 (the count is forgotten when 3,600 seconds have passed since the
/// last failure; a success forgets it).
#[test]
fn the_count_is_forgotten_an_hour_after_the_last_failure_or_at_a_success() {
    let lockout = Lockout::new();
    fail(&lockout, "sam", 5, 0);
    assert_eq!(lockout.check("sam", at(0)), Err(60), "after five failures");
    lockout.failed("sam", at(MEMORY_SECONDS - 1));
    assert_eq!(
        lockout.check("sam", at(MEMORY_SECONDS - 1)),
        Err(120),
        "after a sixth failure 3,599 seconds after the fifth"
    );

    let later = MEMORY_SECONDS - 1 + MEMORY_SECONDS;
    lockout.failed("sam", at(later));
    assert_eq!(
        lockout.check("sam", at(later)),
        Ok(()),
        "after a failure an hour after the last one, the first again"
    );
    fail(&lockout, "sam", 3, later);
    assert_eq!(
        lockout.check("sam", at(later)),
        Ok(()),
        "after four failures since the count was forgotten"
    );
    lockout.failed("sam", at(later));
    assert_eq!(
        lockout.check("sam", at(later)),
        Err(60),
        "after five failures since the count was forgotten"
    );

    let after_pause = later + 60;
    lockout.succeeded("SAM");
    fail(&lockout, "sam", 4, after_pause);
    assert_eq!(
        lockout.check("sam", at(after_pause)),
        Ok(()),
        "after a success and four more failures"
    );
    lockout.failed("sam", at(after_pause));
    assert_eq!(
        lockout.check("sam", at(after_pause)),
        Err(60),
        "after a success and five more failures"
    );
}

/// A pause lasts its whole length, which it announces when it starts: after the eighth
/// failure an attempt is told to wait 600 seconds, then the seconds left down to the last
/// one, however often it asks, and at the 600th second it is checked.
///
/// Proves: decision 111 (a pause lasts its whole length; a refused attempt does not lengthen
/// it).
#[test]
fn a_pause_lasts_its_whole_length() {
    let lockout = Lockout::new();
    fail(&lockout, "pat", 8, 0);
    for (second, left) in [(0, 600), (1, 599), (300, 300), (599, 1)] {
        assert_eq!(
            lockout.check("pat", at(second)),
            Err(left),
            "an attempt at second {second} of the pause after the eighth failure"
        );
    }
    assert_eq!(
        lockout.check("pat", at(-60)),
        Err(600),
        "an attempt after the clock went back a minute"
    );
    assert_eq!(
        lockout.check("pat", at(600)),
        Ok(()),
        "an attempt when the pause has passed"
    );
}

/// Only usernames with recent failures are remembered, and of a typed username longer than 64
/// characters only one that found a user: a username of 64 characters of two bytes each is
/// remembered, the spaces typed around it not being part of it, and one of 65 characters
/// that found no user never is; a username is dropped once an hour has passed since its last
/// failure, at the next failure of any username, and at once when it succeeds. A failure in a
/// spelling of 66 characters that found a user (33 letters, each followed by its combining
/// mark) is remembered, while one of 65 characters that found no user still is not.
///
/// Proves: decision 111 (a typed username that 95's normalization leaves longer than 64
/// characters is not remembered when it finds no user and is remembered when it finds one; the
/// 64 are characters, not bytes; a username is dropped once its failures are forgotten).
#[test]
fn only_usernames_with_recent_failures_are_remembered() {
    let lockout = Lockout::new();
    assert_eq!(lockout.remembered(), 0, "usernames remembered at first");
    lockout.failed("sam", at(0));
    lockout.failed("alex", at(10));
    let longest = "\u{e9}".repeat(64);
    lockout.failed(&format!("  {longest}  "), at(20));
    assert_eq!(
        lockout.remembered(),
        3,
        "usernames remembered after failures for sam, alex and one of 64 characters"
    );
    let too_long = "y".repeat(65);
    fail(&lockout, &too_long, 5, 30);
    assert_eq!(
        lockout.remembered(),
        3,
        "usernames remembered after five failures for one of 65 characters"
    );
    assert_eq!(
        lockout.check(&too_long, at(30)),
        Ok(()),
        "an attempt for a username of 65 characters after five failures"
    );

    lockout.failed("pat", at(MEMORY_SECONDS));
    assert_eq!(
        lockout.remembered(),
        3,
        "usernames remembered an hour after sam's failure: alex, the long one and pat"
    );
    lockout.succeeded("ALEX");
    assert_eq!(
        lockout.remembered(),
        2,
        "usernames remembered after alex's success"
    );
    lockout.failed("pat", at(MEMORY_SECONDS + 20));
    assert_eq!(
        lockout.remembered(),
        1,
        "usernames remembered an hour after the long one's failure: pat"
    );

    let spelled_out = "e\u{301}".repeat(33);
    assert_eq!(
        spelled_out.chars().count(),
        66,
        "characters of the long spelling"
    );
    let found = Attempt::Failed { user_found: true };
    assert_eq!(
        lockout.settle(&spelled_out, at(MEMORY_SECONDS + 30), found),
        Ok(()),
        "a failure in a spelling of 66 characters that found a user"
    );
    assert_eq!(
        lockout.remembered(),
        2,
        "usernames remembered after a failure of 66 characters that found a user"
    );
    let nobody = Attempt::Failed { user_found: false };
    assert_eq!(
        lockout.settle(&too_long, at(MEMORY_SECONDS + 30), nobody),
        Ok(()),
        "a failure of 65 characters that found no user"
    );
    assert_eq!(
        lockout.remembered(),
        2,
        "usernames remembered after a failure of 65 characters that found no user"
    );
}
