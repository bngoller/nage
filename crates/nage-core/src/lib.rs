//! Money, dates, chart of accounts, ledger operations, storage, import engine,
//! reconciliation, reports and integrity checks. No HTTP, no HTML, no floats.
#![forbid(clippy::float_arithmetic)]

#[cfg(test)]
mod tests {
    #[test]
    fn test_runner_reaches_core() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("nage"), "unexpected crate name {name}");
    }
}
