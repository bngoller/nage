//! Tests of money: the display and input forms, the checked arithmetic and their properties
//! (SPEC 6.10, INV-3, decisions 85 and 86).

use proptest::prelude::*;

use super::{MinorDigits, Money, ParseMoneyError};

/// The minor digits 0 to 3.
const DIGITS: &[MinorDigits] = &[
    MinorDigits::ZERO,
    MinorDigits::ONE,
    MinorDigits::TWO,
    MinorDigits::THREE,
];

/// Zero, one, five, some powers of ten and the nines below them, both 64-bit extremes and their
/// neighbours: amounts whose display or arithmetic sits on a boundary.
const EDGE_AMOUNTS: &[i64] = &[
    0,
    1,
    -1,
    5,
    -5,
    9,
    10,
    99,
    100,
    999,
    1_000,
    -1_000,
    99_999,
    100_000,
    999_999_999_999_999_999,
    1_000_000_000_000_000_000,
    -1_000_000_000_000_000_000,
    i64::MAX,
    i64::MAX - 1,
    i64::MIN,
    i64::MIN + 1,
];

/// `minor` displayed with `digits`.
fn shown(minor: i64, digits: MinorDigits) -> String {
    Money::from_minor(minor).display(digits).to_string()
}

/// `text` read with `digits`, as minor units.
fn parsed(text: &str, digits: MinorDigits) -> Result<i64, ParseMoneyError> {
    Money::parse(text, digits).map(Money::minor)
}

/// One of the minor digits 0 to 3.
fn any_digits() -> impl Strategy<Value = MinorDigits> {
    prop::sample::select(DIGITS)
}

/// An amount with the edges weighted in: an edge amount, a money-sized amount, or any 64-bit
/// integer (uniform over the bit patterns, so alone it almost never meets an edge).
fn any_amount() -> impl Strategy<Value = i64> {
    prop_oneof![
        3 => prop::sample::select(EDGE_AMOUNTS),
        4 => -1_000_000_000_000_i64..=1_000_000_000_000,
        3 => any::<i64>(),
    ]
}

/// A strategy for the strings `pattern` matches, built once.
fn regex(pattern: &str) -> impl Strategy<Value = String> {
    prop::string::string_regex(pattern).unwrap_or_else(|error| panic!("{pattern}: {error}"))
}

/// Text for the robustness property: any string, any text, a hostile mix of an amount's
/// characters and their look-alikes (a no-break and a narrow no-break space, the minus sign, an
/// en dash, Arabic-Indic and fullwidth digits, a fullwidth comma), and amount-shaped texts of up
/// to 24 whole and 4 decimal digits.
fn hostile_text() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        regex("(?s).{0,40}"),
        regex(
            "[-+$.,0-9 \\t\u{a0}\u{202f}\u{2212}\u{2013}\u{661}\u{662}\u{ff11}\u{ff0c}_eE]{0,24}"
        ),
        regex("-?\\$?-?[0-9]{1,3}(,[0-9]{3}){0,7}(\\.[0-9]{0,4})?"),
    ]
}

/// Minor digits are 0 to 3, the range the `book` table's CHECK allows; 4 and more are refused.
///
/// Proves: decision 85.
#[test]
fn minor_digits_are_zero_to_three() {
    for digits in 0..=3 {
        assert_eq!(MinorDigits::new(digits).map(MinorDigits::get), Some(digits));
    }
    for digits in [4, 5, 9, u8::MAX] {
        assert_eq!(MinorDigits::new(digits), None, "{digits} digits");
    }
}

/// Amounts display with thousands commas, a point and exactly the currency's digits, a hyphen
/// before a negative amount and no symbol: at 0, 1, 2 and 3 digits, for zero, amounts below one
/// unit, the turn of a thousand and both 64-bit extremes at every digit count.
///
/// Proves: SPEC 6.10, decision 86.
#[test]
fn amounts_display_as_specified() {
    for (minor, digits, expected) in [
        (0, MinorDigits::TWO, "0.00"),
        (5, MinorDigits::TWO, "0.05"),
        (-5, MinorDigits::TWO, "-0.05"),
        (100, MinorDigits::TWO, "1.00"),
        (99_999, MinorDigits::TWO, "999.99"),
        (100_000, MinorDigits::TWO, "1,000.00"),
        (123_456, MinorDigits::TWO, "1,234.56"),
        (-123_456, MinorDigits::TWO, "-1,234.56"),
        (123_456_789, MinorDigits::TWO, "1,234,567.89"),
        (0, MinorDigits::ZERO, "0"),
        (999, MinorDigits::ZERO, "999"),
        (1_234, MinorDigits::ZERO, "1,234"),
        (-1_234_567, MinorDigits::ZERO, "-1,234,567"),
        (15, MinorDigits::ONE, "1.5"),
        (1, MinorDigits::THREE, "0.001"),
        (-1, MinorDigits::THREE, "-0.001"),
        (1_234_567, MinorDigits::THREE, "1,234.567"),
        (i64::MAX, MinorDigits::TWO, "92,233,720,368,547,758.07"),
        (i64::MIN, MinorDigits::TWO, "-92,233,720,368,547,758.08"),
        (i64::MIN, MinorDigits::ZERO, "-9,223,372,036,854,775,808"),
        (i64::MIN, MinorDigits::THREE, "-9,223,372,036,854,775.808"),
        (i64::MAX, MinorDigits::ZERO, "9,223,372,036,854,775,807"),
        (i64::MAX, MinorDigits::ONE, "922,337,203,685,477,580.7"),
        (i64::MIN, MinorDigits::ONE, "-922,337,203,685,477,580.8"),
        (i64::MAX, MinorDigits::THREE, "9,223,372,036,854,775.807"),
    ] {
        assert_eq!(
            shown(minor, digits),
            expected,
            "{minor} at {} digits",
            digits.get()
        );
    }
}

/// Typed amounts read as their minor units in every accepted form: with and without thousands
/// commas; a leading `$`, a leading minus, or both in either order; white space around (a tab, a
/// no-break space); fewer decimals than the currency has, none, and an empty side of the point;
/// leading zeros without commas; a negative zero; both 64-bit extremes at 0, 1, 2 and 3 digits.
///
/// Proves: SPEC 6.10, decision 86.
#[test]
fn typed_amounts_parse_in_every_accepted_form() {
    for (text, digits, expected) in [
        ("1,234.56", MinorDigits::TWO, 123_456),
        ("1234.56", MinorDigits::TWO, 123_456),
        ("$1,234.56", MinorDigits::TWO, 123_456),
        ("-1,234.56", MinorDigits::TWO, -123_456),
        ("-$1,234.56", MinorDigits::TWO, -123_456),
        ("$-1,234.56", MinorDigits::TWO, -123_456),
        ("  1,234.56\t", MinorDigits::TWO, 123_456),
        ("\u{a0}5\u{a0}", MinorDigits::TWO, 500),
        ("1.5", MinorDigits::TWO, 150),
        ("5", MinorDigits::TWO, 500),
        ("5.", MinorDigits::TWO, 500),
        (".5", MinorDigits::TWO, 50),
        ("-.05", MinorDigits::TWO, -5),
        ("0", MinorDigits::TWO, 0),
        ("-0", MinorDigits::TWO, 0),
        ("-0.00", MinorDigits::TWO, 0),
        ("007.50", MinorDigits::TWO, 750),
        ("1,000,000", MinorDigits::TWO, 100_000_000),
        ("92,233,720,368,547,758.07", MinorDigits::TWO, i64::MAX),
        ("-92,233,720,368,547,758.08", MinorDigits::TWO, i64::MIN),
        ("1,234", MinorDigits::ZERO, 1_234),
        ("1.", MinorDigits::ZERO, 1),
        ("$-1,234", MinorDigits::ZERO, -1_234),
        ("1.234", MinorDigits::THREE, 1_234),
        ("1,234.5", MinorDigits::THREE, 1_234_500),
        ("-9,223,372,036,854,775.808", MinorDigits::THREE, i64::MIN),
        ("922,337,203,685,477,580.7", MinorDigits::ONE, i64::MAX),
        ("-922,337,203,685,477,580.8", MinorDigits::ONE, i64::MIN),
        ("9,223,372,036,854,775,807", MinorDigits::ZERO, i64::MAX),
        ("-9,223,372,036,854,775,808", MinorDigits::ZERO, i64::MIN),
        ("9,223,372,036,854,775.807", MinorDigits::THREE, i64::MAX),
    ] {
        assert_eq!(
            parsed(text, digits),
            Ok(expected),
            "{text:?} at {} digits",
            digits.get()
        );
    }
}

/// Text outside the accepted forms is refused with its reason, in the order the parser checks
/// (the character check before the point and comma checks, the decimals check before the
/// overflow): no amount at all; an unexpected character (a `+`, a trailing, doubled or inner
/// sign, a doubled or trailing `$`, inner white space (no-break spaces included), letters, an
/// exponent, an underscore, Arabic-Indic or fullwidth digits, a fullwidth comma, the minus sign
/// U+2212 or an en dash, and the import notations of SPEC W3: parentheses, another currency's
/// symbol, a CR suffix); a misplaced decimal point (a second one, even after a misplaced comma; a
/// lone one); a misplaced comma (groups not of three, an empty group, a first group starting with
/// 0, a comma after the point); more decimals than the currency has, zeros included; and an
/// amount one minor unit past the largest (at 0, 1 and 2 digits) or the most negative (at 2), at
/// 2^64 minor units (where an unsigned 64-bit count overflows), or far past.
///
/// Proves: SPEC 6.10, decision 86.
#[test]
fn other_text_is_refused_with_its_reason() {
    use ParseMoneyError::{
        Empty, MisplacedComma, MisplacedPoint, Overflow, TooManyDecimals, UnexpectedCharacter,
    };
    for (text, digits, expected) in [
        ("", MinorDigits::TWO, Empty),
        ("   ", MinorDigits::TWO, Empty),
        ("-", MinorDigits::TWO, Empty),
        ("$", MinorDigits::TWO, Empty),
        ("-$", MinorDigits::TWO, Empty),
        ("$-", MinorDigits::TWO, Empty),
        ("+5", MinorDigits::TWO, UnexpectedCharacter),
        ("5-", MinorDigits::TWO, UnexpectedCharacter),
        ("--5", MinorDigits::TWO, UnexpectedCharacter),
        ("-$-5", MinorDigits::TWO, UnexpectedCharacter),
        ("$$5", MinorDigits::TWO, UnexpectedCharacter),
        ("-$$5", MinorDigits::TWO, UnexpectedCharacter),
        ("$--5", MinorDigits::TWO, UnexpectedCharacter),
        ("$-$5", MinorDigits::TWO, UnexpectedCharacter),
        ("5$", MinorDigits::TWO, UnexpectedCharacter),
        ("1 234", MinorDigits::TWO, UnexpectedCharacter),
        ("1\u{a0}234", MinorDigits::TWO, UnexpectedCharacter),
        ("1\u{202f}234", MinorDigits::TWO, UnexpectedCharacter),
        ("1\u{ff0c}234", MinorDigits::TWO, UnexpectedCharacter),
        ("$ 5", MinorDigits::TWO, UnexpectedCharacter),
        ("- 5", MinorDigits::TWO, UnexpectedCharacter),
        ("1e3", MinorDigits::TWO, UnexpectedCharacter),
        ("1_000", MinorDigits::TWO, UnexpectedCharacter),
        ("CAD 5", MinorDigits::TWO, UnexpectedCharacter),
        ("\u{661}\u{662}", MinorDigits::TWO, UnexpectedCharacter),
        ("\u{ff11}\u{ff12}", MinorDigits::TWO, UnexpectedCharacter),
        ("\u{2212}5", MinorDigits::TWO, UnexpectedCharacter),
        ("\u{2013}5", MinorDigits::TWO, UnexpectedCharacter),
        ("(5)", MinorDigits::TWO, UnexpectedCharacter),
        ("\u{20ac}5", MinorDigits::TWO, UnexpectedCharacter),
        ("5 CR", MinorDigits::TWO, UnexpectedCharacter),
        ("1x.2.3", MinorDigits::TWO, UnexpectedCharacter),
        ("1.2.3", MinorDigits::TWO, MisplacedPoint),
        ("1..2", MinorDigits::TWO, MisplacedPoint),
        ("1,2.3.4", MinorDigits::TWO, MisplacedPoint),
        (".", MinorDigits::TWO, MisplacedPoint),
        ("-.", MinorDigits::TWO, MisplacedPoint),
        ("12,34", MinorDigits::TWO, MisplacedComma),
        ("1,2345", MinorDigits::TWO, MisplacedComma),
        ("1234,567", MinorDigits::TWO, MisplacedComma),
        (",123", MinorDigits::TWO, MisplacedComma),
        ("1,,234", MinorDigits::TWO, MisplacedComma),
        ("1,234,", MinorDigits::TWO, MisplacedComma),
        ("0,123", MinorDigits::TWO, MisplacedComma),
        ("01,234", MinorDigits::TWO, MisplacedComma),
        ("00,500", MinorDigits::TWO, MisplacedComma),
        ("1.2,3", MinorDigits::TWO, MisplacedComma),
        ("1.234", MinorDigits::TWO, TooManyDecimals { allowed: 2 }),
        ("1.500", MinorDigits::TWO, TooManyDecimals { allowed: 2 }),
        ("1.0", MinorDigits::ZERO, TooManyDecimals { allowed: 0 }),
        ("1.2345", MinorDigits::THREE, TooManyDecimals { allowed: 3 }),
        (
            "99999999999999999999999.999",
            MinorDigits::TWO,
            TooManyDecimals { allowed: 2 },
        ),
        ("92,233,720,368,547,758.08", MinorDigits::TWO, Overflow),
        ("-92,233,720,368,547,758.09", MinorDigits::TWO, Overflow),
        ("9,223,372,036,854,775,808", MinorDigits::ZERO, Overflow),
        ("99999999999999999999999", MinorDigits::TWO, Overflow),
        ("922,337,203,685,477,580.8", MinorDigits::ONE, Overflow),
        ("18446744073709551616", MinorDigits::ZERO, Overflow),
        ("-18,446,744,073,709,551,616", MinorDigits::ZERO, Overflow),
        ("184,467,440,737,095,516.16", MinorDigits::TWO, Overflow),
    ] {
        assert_eq!(
            parsed(text, digits),
            Err(expected),
            "{text:?} at {} digits",
            digits.get()
        );
    }
}

/// Each refusal explains itself in words a form can show beside the field.
///
/// Proves: decision 86.
#[test]
fn refusals_explain_themselves() {
    for (error, expected) in [
        (ParseMoneyError::Empty, "no amount"),
        (
            ParseMoneyError::UnexpectedCharacter,
            "an amount holds only digits, commas, a decimal point and a leading minus",
        ),
        (
            ParseMoneyError::MisplacedComma,
            "commas separate groups of three digits, as in 1,234,567.89",
        ),
        (
            ParseMoneyError::MisplacedPoint,
            "one decimal point, with a digit on at least one side",
        ),
        (
            ParseMoneyError::TooManyDecimals { allowed: 2 },
            "too many decimals: at most 2",
        ),
        (
            ParseMoneyError::TooManyDecimals { allowed: 3 },
            "too many decimals: at most 3",
        ),
        (
            ParseMoneyError::TooManyDecimals { allowed: 0 },
            "this currency has no decimals",
        ),
        (ParseMoneyError::Overflow, "the amount is too large"),
    ] {
        assert_eq!(error.to_string(), expected);
    }
}

/// Each operation refuses what does not fit in 64 bits instead of wrapping: one past either
/// extreme, the negation of the most negative amount, a sum past the extreme; a sum does not
/// depend on the order of its terms, so a total that fits is found even where a running 64-bit
/// total would overflow on the way; an empty sum is zero.
///
/// Proves: INV-3 (core half), SPEC 6.10, decision 85.
#[test]
fn checked_arithmetic_refuses_what_does_not_fit() {
    let max = Money::from_minor(i64::MAX);
    let min = Money::from_minor(i64::MIN);
    let one = Money::from_minor(1);
    assert_eq!(max.checked_add(one), None);
    assert_eq!(min.checked_sub(one), None);
    assert_eq!(min.checked_neg(), None);
    assert_eq!(max.checked_neg(), Some(Money::from_minor(-i64::MAX)));
    assert_eq!(
        Money::from_minor(5).checked_add(Money::from_minor(-7)),
        Some(Money::from_minor(-2))
    );
    assert_eq!(
        Money::from_minor(5).checked_sub(Money::from_minor(7)),
        Some(Money::from_minor(-2))
    );
    assert_eq!(Money::checked_sum([max, one]), None);
    assert_eq!(
        Money::checked_sum([max, one, Money::from_minor(-1)]),
        Some(max)
    );
    assert_eq!(
        Money::checked_sum([min, min, max, max, Money::from_minor(2)]),
        Some(Money::ZERO)
    );
    assert_eq!(Money::checked_sum(std::iter::empty()), Some(Money::ZERO));
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// Every amount, at every number of digits, reads back from its display as itself: the
    /// display is exact and the parser accepts every text the display writes.
    ///
    /// Proves: SPEC 6.10, decision 86.
    #[test]
    fn displayed_amounts_parse_back(minor in any_amount(), digits in any_digits()) {
        let text = shown(minor, digits);
        prop_assert_eq!(parsed(&text, digits), Ok(minor), "{:?} at {} digits", text, digits.get());
    }

    /// Every amount reads back from each accepted variation of its display: without its
    /// thousands commas, with a `$` before or after its minus, with white space around it (a
    /// no-break space included) and with its trailing zero decimals dropped.
    ///
    /// Proves: SPEC 6.10, decision 86.
    #[test]
    fn every_accepted_form_of_an_amount_parses_to_it(minor in any_amount(), digits in any_digits()) {
        let text = shown(minor, digits);
        let sign = if minor < 0 { "-" } else { "" };
        let unsigned = text.strip_prefix('-').unwrap_or(&text);
        let short = if unsigned.contains('.') { unsigned.trim_end_matches('0') } else { unsigned };
        for variant in [
            format!("{sign}{}", unsigned.replace(',', "")),
            format!("{sign}${unsigned}"),
            format!("${sign}{unsigned}"),
            format!(" \t{text}\u{a0}"),
            format!("{sign}{short}"),
        ] {
            prop_assert_eq!(parsed(&variant, digits), Ok(minor), "{:?} at {} digits", variant, digits.get());
        }
    }

    /// Checked arithmetic agrees with 128-bit arithmetic wherever the result fits in 64 bits and
    /// refuses everywhere else, and a sum of several amounts does too, in either order.
    ///
    /// Proves: INV-3 (core half), SPEC 6.10, decision 85.
    #[test]
    fn checked_arithmetic_matches_wide_arithmetic(
        a in any_amount(),
        b in any_amount(),
        more in prop::collection::vec(any_amount(), 0..8),
    ) {
        let fitting = |wide: i128| i64::try_from(wide).ok().map(Money::from_minor);
        let (x, y) = (Money::from_minor(a), Money::from_minor(b));
        prop_assert_eq!(x.checked_add(y), fitting(i128::from(a) + i128::from(b)));
        prop_assert_eq!(x.checked_sub(y), fitting(i128::from(a) - i128::from(b)));
        prop_assert_eq!(x.checked_neg(), fitting(-i128::from(a)));
        let terms: Vec<i64> = [a, b].into_iter().chain(more).collect();
        let total: i128 = terms.iter().map(|&term| i128::from(term)).sum();
        prop_assert_eq!(Money::checked_sum(terms.iter().map(|&term| Money::from_minor(term))), fitting(total));
        prop_assert_eq!(Money::checked_sum(terms.iter().rev().map(|&term| Money::from_minor(term))), fitting(total));
    }

    /// An amount beyond 64 bits of minor units is refused as an overflow, never wrapped: from one
    /// minor unit past either extreme to far past it (small excesses weighted in), at every number
    /// of digits, written by this test without commas (not by the display under test).
    ///
    /// Proves: INV-3 (core half), SPEC 6.10.
    #[test]
    fn amounts_past_64_bits_are_refused(
        excess in prop_oneof![1 => 1_u64..=1_000, 3 => 1_u64..=u64::MAX],
        negative in any::<bool>(),
        digits in any_digits(),
    ) {
        let limit: u128 = if negative { 1 << 63 } else { (1 << 63) - 1 };
        let magnitude = limit + u128::from(excess);
        let scale = 10_u128.pow(u32::from(digits.get()));
        let sign = if negative { "-" } else { "" };
        let whole = magnitude / scale;
        let text = if digits.get() == 0 {
            format!("{sign}{whole}")
        } else {
            let fraction = magnitude % scale;
            let width = usize::from(digits.get());
            format!("{sign}{whole}.{fraction:0width$}")
        };
        prop_assert_eq!(parsed(&text, digits), Err(ParseMoneyError::Overflow), "{}", text);
    }

    /// Parsing never panics and never rounds: any text, a hostile mix of an amount's characters
    /// and their look-alikes, or an amount-shaped text of up to 28 digits is an amount or a
    /// refusal; an accepted text has at most the currency's decimals, and its amount's display
    /// reads back as the same amount.
    ///
    /// Proves: SPEC 6.10, decision 86.
    #[test]
    fn parsing_never_panics_and_never_rounds(text in hostile_text(), digits in any_digits()) {
        if let Ok(money) = Money::parse(&text, digits) {
            let decimals = text.trim().split_once('.').map_or(0, |(_, fraction)| fraction.len());
            prop_assert!(decimals <= usize::from(digits.get()), "{:?} at {} digits", text, digits.get());
            prop_assert_eq!(Money::parse(&money.display(digits).to_string(), digits), Ok(money));
        }
    }
}
