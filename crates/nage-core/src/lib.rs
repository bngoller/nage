//! Money, dates, chart of accounts, ledger operations, storage, import engine,
//! reconciliation, reports and integrity checks. No HTTP, no HTML, no floats.
#![forbid(clippy::float_arithmetic)]

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
}
