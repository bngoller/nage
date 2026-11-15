//! The two properties of the passwords (SPEC 5.3, decision 107), which need no database: the
//! rules over arbitrary typed text, and a hash against the spellings NFKC makes equal. The
//! other password tests are `tests/password.rs`.

use proptest::prelude::*;
use unicode_normalization::UnicodeNormalization as _;

use super::{PasswordError, Verification, check, hash, verify};
use crate::test_support::CHEAP_HASH_PARAMS;

/// A character as it may be typed or pasted: any Unicode scalar value, or one of a few chosen
/// for what a wrong rule would do with them: letters and digits of one to four UTF-8 bytes,
/// white space of several kinds (a trimming rule would drop it at either end), a combining
/// accent, and compatibility characters whose NFKC form is shorter (a full-width letter, a
/// modifier capital) or longer (a ligature of three letters, a ligature of eighteen).
fn typed_character() -> impl Strategy<Value = char> {
    prop_oneof![
        any::<char>(),
        prop::sample::select(vec![
            'a',
            'Z',
            '7',
            ' ',
            '\t',
            '\n',
            '\u{00A0}',
            '\u{3000}',
            '\u{00E9}',
            '\u{0301}',
            '\u{00DF}',
            '\u{FF21}',
            '\u{1D2E}',
            '\u{FB03}',
            '\u{FDFA}',
            '\u{1F600}',
        ]),
    ]
}

/// A typed password: up to 24 characters, around the lower limit, or 250 to 1,100 characters,
/// whose bytes lie on both sides of the upper limit.
fn typed_password() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::collection::vec(typed_character(), 0..=24),
        prop::collection::vec(typed_character(), 250..=1100),
    ]
    .prop_map(|characters| characters.into_iter().collect())
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// The rules depend on two counts of the password exactly as typed and on nothing else:
    /// fewer than 10 Unicode scalar values is too short, more than 1,024 UTF-8 bytes is too
    /// long, and everything between passes, whatever the characters are: white space at either
    /// end counts, and a character counts once whether NFKC would shorten it, lengthen it or
    /// leave it. The oracle is the rule written out with its two numbers.
    ///
    /// Proves: SPEC 5.3 (minimum length 10, maximum 1 KiB, no composition rules), decision 107
    /// (characters and bytes counted as typed; never trimmed).
    #[test]
    fn the_rules_depend_only_on_the_characters_and_bytes_typed(password in typed_password()) {
        let characters = password.chars().count();
        let bytes = password.len();
        let expected = if characters < 10 {
            Err(PasswordError::TooShort)
        } else if bytes > 1024 {
            Err(PasswordError::TooLong)
        } else {
            Ok(())
        };
        prop_assert_eq!(
            check(&password),
            expected,
            "a password of {} characters and {} bytes",
            characters,
            bytes
        );
    }
}

/// The spellings of one letter that NFKC makes equal, and letters that stay distinct from each
/// other: an accented letter (precomposed, or with a combining accent), the plain letter, a
/// small and a capital letter (plain or full-width), a ligature, a digit (plain, circled or
/// full-width) and a space (plain or no-break).
const LETTERS: &[&[&str]] = &[
    &["\u{00E9}", "e\u{0301}"],
    &["e"],
    &["a", "\u{FF41}"],
    &["A", "\u{FF21}"],
    &["fi", "\u{FB01}"],
    &["1", "\u{2460}", "\u{FF11}"],
    &[" ", "\u{00A0}"],
];

/// Two passwords of ten to sixteen letters drawn from [`LETTERS`]: a spelling, and either
/// another spelling of the same letters, the same with one letter replaced, or the same with a
/// space added at one end.
fn two_passwords() -> impl Strategy<Value = (String, String)> {
    (
        prop::collection::vec(
            (
                0..LETTERS.len(),
                any::<prop::sample::Index>(),
                any::<prop::sample::Index>(),
            ),
            10..=16,
        ),
        prop_oneof![
            Just(None),
            (any::<prop::sample::Index>(), 0..LETTERS.len()).prop_map(Some),
        ],
        0..3_u8,
    )
        .prop_map(|(letters, replaced, space)| {
            let first: String = letters
                .iter()
                .map(|(letter, spelling, _)| {
                    LETTERS[*letter][spelling.index(LETTERS[*letter].len())]
                })
                .collect();
            let position =
                replaced.map(|(position, letter)| (position.index(letters.len()), letter));
            let mut second = String::new();
            if space == 1 {
                second.push(' ');
            }
            for (index, (letter, _, spelling)) in letters.iter().enumerate() {
                let letter = match position {
                    Some((position, other)) if position == index => other,
                    _ => *letter,
                };
                second.push_str(LETTERS[letter][spelling.index(LETTERS[letter].len())]);
            }
            if space == 2 {
                second.push(' ');
            }
            (first, second)
        })
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// For two passwords that are spellings of the same letters, of letters that differ in one
    /// place, or of the same letters with a space at one end, the second verifies against the
    /// hash of the first exactly when their NFKC forms are equal: a look-alike spelling is the
    /// same password, and another letter, another case or an added space is not. The oracle is
    /// NFKC itself, from the crate the core normalizes with; the value is the pairs.
    ///
    /// Proves: decision 107 (NFKC and nothing else before hashing and verifying: no trimming,
    /// no case folding).
    #[test]
    fn a_hash_verifies_exactly_the_spellings_nfkc_makes_equal((first, second) in two_passwords()) {
        let same = first.nfkc().eq(second.nfkc());
        let hash = hash(CHEAP_HASH_PARAMS, &first);
        prop_assert!(hash.is_ok(), "{:?} was refused: {:?}", first, hash);
        let hash = hash.unwrap_or_default();
        prop_assert_eq!(verify(&first, &hash), Verification::Verified);
        let expected = if same {
            Verification::Verified
        } else {
            Verification::WrongPassword
        };
        prop_assert_eq!(
            verify(&second, &hash),
            expected,
            "{:?} against the hash of {:?}",
            second,
            first
        );
    }
}
