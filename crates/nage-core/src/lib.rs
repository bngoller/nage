//! Money, dates, chart of accounts, ledger operations, storage, import engine,
//! reconciliation, reports and integrity checks. No HTTP, no HTML, no floats.
#![forbid(clippy::float_arithmetic)]

pub mod storage;

#[cfg(test)]
mod tests {
    /// The test runner reaches the core crate.
    ///
    /// Cites: PLAN M1 slice 1.
    #[test]
    fn test_runner_reaches_core() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("nage"), "unexpected crate name {name}");
    }
}
