//! Money, dates, chart of accounts, ledger operations, storage, import engine,
//! reconciliation, reports and integrity checks. No HTTP, no HTML, no floats.
#![forbid(clippy::float_arithmetic)]
#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

pub mod currency;
pub mod date;
pub mod money;
pub mod storage;
#[cfg(feature = "test-support")]
pub mod test_support;

// The unit tests reach the helpers through the self dev-dependency in Cargo.toml, which turns
// the feature on for the lib-test unit too; this guard fails the build if it ever does not
// (decision 59).
#[cfg(all(test, not(feature = "test-support")))]
compile_error!("nage-core's unit tests need the test-support feature (decision 59)");

#[cfg(test)]
mod tests {
    /// The test runner reaches the core crate.
    ///
    /// Proves: PLAN M1 slice 1.
    #[test]
    fn test_runner_reaches_core() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("nage"), "unexpected crate name {name}");
    }

    /// The crate root forbids floating-point arithmetic, so no `#[allow]` inside nage-core can
    /// reopen it. Read from the source, since a lint level is not observable at run time; the
    /// slice's recorded mutants showed clippy refusing a float operator and such an allow.
    ///
    /// Proves: INV-3 (lint half), decision 39.
    #[test]
    fn core_forbids_float_arithmetic() {
        assert!(
            include_str!("lib.rs")
                .lines()
                .any(|line| line == "#![forbid(clippy::float_arithmetic)]"),
            "lib.rs lacks its float forbid"
        );
    }

    /// nage-core denies integer arithmetic operators (`clippy::arithmetic_side_effects`) outside
    /// its tests, so a later module is covered without a line of its own and an exception is a
    /// visible `#[allow]`; the money, currency and date modules forbid it, so no `#[allow]`
    /// reopens it there. Read from the source and from the workspace's `clippy.toml`, which must
    /// not narrow the lint; the slice's recorded mutants showed clippy refusing an unchecked `+`
    /// in storage and in money, and an allow of the lint in money (E0453).
    ///
    /// Proves: SPEC 6.10, decision 85.
    #[test]
    fn the_core_refuses_unchecked_arithmetic() {
        const DENY: &str = "#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]";
        const FORBID: &str = "#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]";
        assert!(
            include_str!("lib.rs").lines().any(|line| line == DENY),
            "lib.rs lacks its arithmetic deny"
        );
        for (file, text) in [
            ("money.rs", include_str!("money.rs")),
            ("currency.rs", include_str!("currency.rs")),
            ("date.rs", include_str!("date.rs")),
        ] {
            assert!(
                text.lines().any(|line| line == FORBID),
                "{file} lacks its arithmetic forbid"
            );
        }
        assert!(
            !include_str!("../../../clippy.toml").contains("arithmetic-side-effects"),
            "clippy.toml narrows the arithmetic lint"
        );
    }

    /// The workspace's clippy configuration refuses by name, in every crate, the three jiff
    /// methods that read the system's zone (the `TZ` variable and `/etc/localtime`):
    /// `TimeZone::system`, `TimeZone::try_system` and `Zoned::now`; `Zoned::try_from(SystemTime)`
    /// reads it too and is left to review. Read from the file's `disallowed-methods` list, every
    /// line of which up to its own `]` line is an entry or a comment, so no entry sits under
    /// another key; the slice's recorded mutants showed clippy refusing each of the three calls in
    /// nage-core and `Zoned::now` in the binary.
    ///
    /// Proves: decision 88.
    #[test]
    fn no_crate_asks_jiff_for_the_system_zone() {
        let list: Vec<&str> = include_str!("../../../clippy.toml")
            .lines()
            .skip_while(|line| *line != "disallowed-methods = [")
            .take_while(|line| *line != "]")
            .collect();
        assert!(
            list.iter().skip(1).all(|line| {
                line.starts_with("  { path = ") || line.trim_start().starts_with('#')
            }),
            "a disallowed-methods line of clippy.toml is neither an entry nor a comment"
        );
        for path in [
            "jiff::tz::TimeZone::system",
            "jiff::tz::TimeZone::try_system",
            "jiff::Zoned::now",
        ] {
            assert!(
                list.iter()
                    .any(|line| line.starts_with(&format!("  {{ path = \"{path}\","))),
                "clippy.toml does not disallow {path}"
            );
        }
    }
}
