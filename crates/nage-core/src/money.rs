//! Money (SPEC 6.10, INV-3, decisions 85 and 86): an amount of the book currency is a signed
//! 64-bit count of its minor units, combined only by checked methods that refuse an overflow,
//! and read and written in the one decimal form SPEC 6.10 fixes. No floating-point arithmetic
//! (the crate forbids it, decision 39) and, outside the tests, no integer arithmetic operator
//! (the lint below, `clippy::arithmetic_side_effects`, decision 85).
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use std::fmt;

#[cfg(test)]
mod tests;

/// How many minor-unit digits a currency has: 0 to 3, the range the `book` table's CHECK
/// allows (decision 56). The book keeps the digits of the currency chosen at setup (INV-5), and
/// parsing and display take them from the book.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MinorDigits(u8);

impl MinorDigits {
    /// No minor unit, as for JPY.
    pub const ZERO: MinorDigits = MinorDigits(0);
    /// One digit, which no current ISO 4217 currency has.
    pub const ONE: MinorDigits = MinorDigits(1);
    /// Two digits, as for CAD.
    pub const TWO: MinorDigits = MinorDigits(2);
    /// Three digits, as for KWD.
    pub const THREE: MinorDigits = MinorDigits(3);

    /// `digits` when it is 0 to 3, else `None`.
    pub const fn new(digits: u8) -> Option<MinorDigits> {
        if digits <= 3 {
            Some(MinorDigits(digits))
        } else {
            None
        }
    }

    /// The number of digits, 0 to 3.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// An amount of the book currency as a signed count of its minor units, cents for CAD
/// (INV-3). It carries no digits: [`Money::parse`] and [`Money::display`] take the book's. It
/// has no `+` or `-`; the checked methods answer `None` when a result does not fit in 64 bits,
/// so an overflow is refused, never wrapped (SPEC 6.10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money(i64);

impl Money {
    /// No money.
    pub const ZERO: Money = Money(0);

    /// The amount of `minor` minor units.
    pub const fn from_minor(minor: i64) -> Money {
        Money(minor)
    }

    /// The amount in minor units.
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// `self + other`, or `None` when the sum does not fit.
    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    /// `self - other`, or `None` when the difference does not fit.
    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }

    /// `-self`, or `None` for the most negative amount, whose negation does not fit.
    pub fn checked_neg(self) -> Option<Money> {
        self.0.checked_neg().map(Money)
    }

    /// The sum of `amounts`, or `None` when it does not fit. The running total is kept in 128
    /// bits, so the answer does not depend on the order of the amounts.
    pub fn checked_sum<I>(amounts: I) -> Option<Money>
    where
        I: IntoIterator<Item = Money>,
    {
        let total = amounts.into_iter().try_fold(0_i128, |total, amount| {
            total.checked_add(i128::from(amount.0))
        })?;
        i64::try_from(total).ok().map(Money)
    }

    /// Reads an amount typed into a form, with the book's `digits` (SPEC 6.10, decision 86).
    ///
    /// Accepted: white space around the amount; a leading minus, a leading `$` or both, in
    /// either order; ASCII digits with or without thousands commas, where commas follow a first
    /// group of one to three digits that does not start with 0 and separate groups of exactly
    /// three; an optional decimal point with at most `digits` digits after it, either side of the
    /// point allowed to be empty but not both. Nothing is rounded: more decimals than `digits`,
    /// even zeros, are refused, and so is an amount that does not fit in 64 bits.
    pub fn parse(input: &str, digits: MinorDigits) -> Result<Money, ParseMoneyError> {
        let (negative, number) = split_sign(input.trim());
        if number.is_empty() {
            return Err(ParseMoneyError::Empty);
        }
        if !number
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b',' || byte == b'.')
        {
            return Err(ParseMoneyError::UnexpectedCharacter);
        }
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        if fraction.contains('.') || (whole.is_empty() && fraction.is_empty()) {
            return Err(ParseMoneyError::MisplacedPoint);
        }
        if fraction.contains(',') || !well_grouped(whole) {
            return Err(ParseMoneyError::MisplacedComma);
        }
        let allowed = usize::from(digits.get());
        if fraction.len() > allowed {
            return Err(ParseMoneyError::TooManyDecimals {
                allowed: digits.get(),
            });
        }
        let padding = std::iter::repeat_n('0', allowed.saturating_sub(fraction.len()));
        let mut magnitude: u64 = 0;
        for digit in whole
            .chars()
            .chain(fraction.chars())
            .chain(padding)
            .filter_map(|character| character.to_digit(10))
        {
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(digit)))
                .ok_or(ParseMoneyError::Overflow)?;
        }
        let minor = if negative {
            0_i64.checked_sub_unsigned(magnitude)
        } else {
            i64::try_from(magnitude).ok()
        };
        minor.map(Money).ok_or(ParseMoneyError::Overflow)
    }

    /// The largest amount a form takes in a book of `digits` (decision 142): twelve digits
    /// before the point and every decimal the currency has, 999,999,999,999.99 at two digits,
    /// 999,999,999,999 at none and 999,999,999,999.999 at three, so the same count of whole units
    /// whatever the minor digits. A form takes it and its negation and everything between.
    pub fn largest_taken(digits: MinorDigits) -> Money {
        Money(match digits.get() {
            0 => 999_999_999_999,
            1 => 9_999_999_999_999,
            2 => 99_999_999_999_999,
            _ => 999_999_999_999_999,
        })
    }

    /// Whether a form takes this amount in a book of `digits` (decision 142): at most
    /// [`Money::largest_taken`] above or below zero. Every write that takes an amount from a form
    /// holds this rule whatever the form sends, so that no sum of the household's amounts can
    /// pass what the pages can show.
    pub fn is_taken(self, digits: MinorDigits) -> bool {
        self.0.unsigned_abs() <= Money::largest_taken(digits).0.unsigned_abs()
    }

    /// The amount as SPEC 6.10 displays it with the book's `digits`: see [`MoneyDisplay`].
    pub const fn display(self, digits: MinorDigits) -> MoneyDisplay {
        MoneyDisplay {
            money: self,
            digits,
        }
    }
}

/// An amount as SPEC 6.10 displays it: an ASCII hyphen before a negative amount, the whole part
/// with thousands commas, then a decimal point and exactly the currency's minor digits (no point
/// when it has none), no currency symbol: 123456 minor units read `1,234.56` at two digits, -5
/// reads `-0.05`, 1234 reads `1,234` at none and 1234567 reads `1,234.567` at three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoneyDisplay {
    money: Money,
    digits: MinorDigits,
}

impl fmt::Display for MoneyDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = usize::from(self.digits.get());
        let mut text = self.money.0.unsigned_abs().to_string();
        while text.len() <= digits {
            text.insert(0, '0');
        }
        let (whole, fraction) = text.split_at(text.len().saturating_sub(digits));
        if self.money.0 < 0 {
            f.write_str("-")?;
        }
        for (index, group) in whole.as_bytes().rchunks(3).rev().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            f.write_str(std::str::from_utf8(group).map_err(|_| fmt::Error)?)?;
        }
        if !fraction.is_empty() {
            f.write_str(".")?;
            f.write_str(fraction)?;
        }
        Ok(())
    }
}

/// Why a typed amount was refused (SPEC 6.10, decision 86); each reason's message says why and
/// what to do (decisions 102 and 145), written to follow a form's "was not saved because", as
/// the opening balance's sentences do (SPEC W2: every refusal explains itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseMoneyError {
    /// No amount: nothing but white space, a minus or a `$`.
    Empty,
    /// A character other than ASCII digits, commas and decimal points after the leading minus
    /// and `$`: a `+`, inner white space, a letter, a second sign, a non-ASCII digit or minus.
    UnexpectedCharacter,
    /// A comma out of place: commas separate groups of exactly three digits before the decimal
    /// point, after a first group of one to three that does not start with 0.
    MisplacedComma,
    /// A second decimal point, or a lone point, with nothing on either side.
    MisplacedPoint,
    /// More decimals than the currency has (any, when it has none); nothing is rounded.
    TooManyDecimals {
        /// The currency's minor digits.
        allowed: u8,
    },
    /// The amount does not fit in 64 bits of minor units.
    Overflow,
}

impl fmt::Display for ParseMoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => {
                "the amount box holds no amount: Nage never guesses an amount. Type it as 1,234.56, then save again."
            }
            Self::UnexpectedCharacter => {
                "the amount holds a sign an amount cannot have: only digits, commas between thousands, one point, and a minus or $ in front. Type it as 1,234.56, then save again."
            }
            Self::MisplacedComma => {
                "the comma in the amount is out of place: Nage writes amounts as 1,234.56, with commas between thousands and a point before the cents. Type it that way, then save again."
            }
            Self::MisplacedPoint => {
                "the point in the amount is out of place: Nage writes amounts as 1,234.56, with one point, before the cents. Type it that way, then save again."
            }
            Self::TooManyDecimals { allowed: 0 } => {
                "the amount has decimals, and the book's currency has none: Nage never rounds. Type it without decimals, then save again."
            }
            Self::TooManyDecimals { allowed } => {
                return write!(
                    f,
                    "the amount has more decimals than the book's currency, which has {allowed}: Nage never rounds. Type it with at most {allowed}, then save again."
                );
            }
            Self::Overflow => {
                "the amount is more than Nage can count. Check it for a typing mistake, then save again."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for ParseMoneyError {}

impl ParseMoneyError {
    /// The example figure of a book of `digits` (decision 145, the pause's P6): 1,234.56 at two
    /// digits, 1,234 at none, 1,234.5 at one and 1,234.567 at three, so that no sentence tells a
    /// person to type what the same form then refuses.
    pub fn example(digits: MinorDigits) -> MoneyDisplay {
        let minor = match digits.get() {
            0 => 1_234,
            1 => 12_345,
            2 => 123_456,
            _ => 1_234_567,
        };
        Money::from_minor(minor).display(digits)
    }

    /// The reason in the words of a book of `digits`, to follow a form's "was not saved
    /// because" (decision 145, the pause's P6): the example follows the book's digits; in a book
    /// without decimals no sentence speaks of a point or of cents, and at other than two digits
    /// cents reads decimals. At two digits it reads as [`fmt::Display`] does, but for `Empty`,
    /// which a form words itself.
    pub fn because(self, digits: MinorDigits) -> ParseMoneyBecause {
        ParseMoneyBecause {
            error: self,
            digits,
        }
    }
}

/// A [`ParseMoneyError`] in the words of a book's digits: see [`ParseMoneyError::because`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseMoneyBecause {
    error: ParseMoneyError,
    digits: MinorDigits,
}

impl fmt::Display for ParseMoneyBecause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let example = ParseMoneyError::example(self.digits);
        let none = self.digits.get() == 0;
        let after = if self.digits == MinorDigits::TWO {
            "the cents"
        } else {
            "the decimals"
        };
        match self.error {
            ParseMoneyError::Empty => write!(
                f,
                "the amount box holds no amount: Nage never guesses an amount. Type it as {example}, then save again."
            ),
            ParseMoneyError::UnexpectedCharacter if none => write!(
                f,
                "the amount holds a sign an amount cannot have: only digits, commas between thousands, and a minus or $ in front. Type it as {example}, then save again."
            ),
            ParseMoneyError::UnexpectedCharacter => write!(
                f,
                "the amount holds a sign an amount cannot have: only digits, commas between thousands, one point, and a minus or $ in front. Type it as {example}, then save again."
            ),
            ParseMoneyError::MisplacedComma if none => write!(
                f,
                "the comma in the amount is out of place: Nage writes amounts as {example}, with commas between thousands. Type it that way, then save again."
            ),
            ParseMoneyError::MisplacedComma => write!(
                f,
                "the comma in the amount is out of place: Nage writes amounts as {example}, with commas between thousands and a point before {after}. Type it that way, then save again."
            ),
            ParseMoneyError::MisplacedPoint if none => write!(
                f,
                "the amount is not written as Nage writes amounts in this book, as {example}, without decimals. Type it that way, then save again."
            ),
            ParseMoneyError::MisplacedPoint => write!(
                f,
                "the point in the amount is out of place: Nage writes amounts as {example}, with one point, before {after}. Type it that way, then save again."
            ),
            error @ (ParseMoneyError::TooManyDecimals { .. } | ParseMoneyError::Overflow) => {
                write!(f, "{error}")
            }
        }
    }
}

/// Splits a leading minus, a leading `$` or both, in either order, from `text`: whether the
/// amount is negative, and the rest.
fn split_sign(text: &str) -> (bool, &str) {
    if let Some(rest) = text.strip_prefix('-') {
        (true, rest.strip_prefix('$').unwrap_or(rest))
    } else if let Some(rest) = text.strip_prefix('$') {
        match rest.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, rest),
        }
    } else {
        (false, text)
    }
}

/// Whether the commas, if any, before the decimal point are in their places: after a first group
/// of one to three digits that does not start with 0, between groups of exactly three (the parser
/// has already refused every character but digits, commas and points).
fn well_grouped(whole: &str) -> bool {
    match whole.split_once(',') {
        None => true,
        Some((first, rest)) => {
            (1..=3).contains(&first.len())
                && !first.starts_with('0')
                && rest.split(',').all(|group| group.len() == 3)
        }
    }
}
