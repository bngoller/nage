//! Tests of the currency table (SPEC 6.10, decision 87).

use super::{CURRENCIES, Currency, PUBLISHED};
use crate::money::MinorDigits;

/// The codes of the table's rows with `digits` minor digits, in table order.
fn codes_with(digits: u8) -> Vec<&'static str> {
    CURRENCIES
        .iter()
        .filter(|currency| currency.minor_digits().get() == digits)
        .map(|currency| currency.code())
        .collect()
}

/// Whether the table has a row for `code` with `digits`.
fn holds(code: &str, digits: MinorDigits) -> bool {
    CURRENCIES
        .iter()
        .any(|currency| currency.code() == code && currency.minor_digits() == digits)
}

/// The table is ISO 4217 list one of 2026-09-17 as decision 87 reads it: 155 codes, 132 with two
/// minor digits, the sixteen with none and the seven with three named here, CAD, USD and EUR
/// with two, and none of the entries left out (the precious metals, special drawing rights,
/// bond-market and other units of account, test and no-currency codes, whose minor units are N.A.,
/// and the funds). A refresh of the table updates this test with it.
///
/// Proves: SPEC 6.10, decision 87.
#[test]
fn the_table_is_iso_4217_list_one() {
    assert_eq!(PUBLISHED, "2026-09-17");
    assert_eq!(CURRENCIES.len(), 155);
    assert_eq!(
        codes_with(0),
        [
            "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "VND",
            "VUV", "XAF", "XOF", "XPF",
        ]
    );
    assert!(codes_with(1).is_empty());
    assert_eq!(codes_with(2).len(), 132);
    assert_eq!(
        codes_with(3),
        ["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"]
    );
    for code in ["CAD", "USD", "EUR"] {
        assert!(holds(code, MinorDigits::TWO), "{code} with two digits");
    }
    for code in [
        "XAG", "XAU", "XBA", "XBB", "XBC", "XBD", "XDR", "XPD", "XPT", "XSU", "XTS", "XUA", "XXX",
        "BOV", "CHE", "CHW", "CLF", "COU", "MXV", "USN", "UYI", "UYW", "XAD",
    ] {
        assert!(
            CURRENCIES.iter().all(|currency| currency.code() != code),
            "{code} is left out of the table"
        );
    }
}

/// Every row's code is three capital ASCII letters, as the `book` table's CHECK requires
/// (decision 56), and the rows are sorted by code without repeats, an order a setup select can
/// show.
///
/// Proves: decisions 56 and 87.
#[test]
fn every_row_fits_the_book_table_and_the_rows_are_sorted() {
    for currency in CURRENCIES {
        let code = currency.code();
        assert!(
            code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()),
            "{code:?} is not three capital letters"
        );
    }
    for pair in CURRENCIES.windows(2) {
        assert!(
            pair[0].code() < pair[1].code(),
            "{} is not before {}",
            pair[0].code(),
            pair[1].code()
        );
    }
}

/// A code is found only when it is exactly a row's code, with that row's digits: CAD two, JPY
/// none, KWD three, and every row's code finds that row; `cad`, ` CAD`, `CAD `, `CA`, `CADX`, a
/// left-out code (XAU) and the empty string find nothing.
///
/// Proves: SPEC 6.10, decision 87.
#[test]
fn codes_are_looked_up_exactly() {
    for (code, digits) in [
        ("CAD", MinorDigits::TWO),
        ("JPY", MinorDigits::ZERO),
        ("KWD", MinorDigits::THREE),
    ] {
        let found = Currency::from_code(code);
        assert_eq!(found.map(Currency::code), Some(code));
        assert_eq!(found.map(Currency::minor_digits), Some(digits), "{code}");
    }
    for currency in CURRENCIES {
        assert_eq!(Currency::from_code(currency.code()), Some(*currency));
    }
    for code in ["cad", " CAD", "CAD ", "CA", "CADX", "XAU", ""] {
        assert_eq!(Currency::from_code(code), None, "{code:?}");
    }
}
