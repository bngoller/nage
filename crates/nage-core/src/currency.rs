//! The built-in currency table (SPEC 6.10, decision 87): every current ISO 4217 currency code with
//! 0 to 3 minor digits (no fund code) and those digits, and nothing else (SPEC 4: no names, no
//! symbols). The table is written by `cargo xtask currencies` from ISO 4217 list one; a book keeps
//! the digits of the currency chosen at setup (INV-5), so the table is read only at setup.
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use crate::money::MinorDigits;

mod table;
#[cfg(test)]
mod tests;

pub use table::{CURRENCIES, PUBLISHED};

/// A currency of the table: its ISO 4217 code and its minor digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Currency {
    code: &'static str,
    minor_digits: MinorDigits,
}

impl Currency {
    /// The three-letter ISO 4217 code, such as `CAD`.
    pub const fn code(self) -> &'static str {
        self.code
    }

    /// The currency's minor digits in ISO 4217.
    pub const fn minor_digits(self) -> MinorDigits {
        self.minor_digits
    }

    /// The table's currency whose code is exactly `code` (three capital letters), if any.
    pub fn from_code(code: &str) -> Option<Currency> {
        CURRENCIES
            .iter()
            .find(|currency| currency.code == code)
            .copied()
    }
}

/// A row of the table, for the generated `table` module.
const fn row(code: &'static str, minor_digits: MinorDigits) -> Currency {
    Currency { code, minor_digits }
}
