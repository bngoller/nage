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

/// The bytes hashed are the NFKC form of the password, not merely one form that look-alike
/// spellings share: a hash made by [`hash`] from ten accented letters typed with combining
/// accents is the hash of the ten precomposed letters' UTF-8 bytes, which the `argon2` crate
/// itself verifies against those bytes (NFKD would hash the combining spelling's bytes and fail
/// here); and a literal PHC string, made by the `argon2` crate with the cheap set from the
/// precomposed bytes, verifies the spelling with combining accents through [`verify`] and does
/// not verify the combining spelling's own bytes.
///
/// Proves: decision 107 (NFKC before hashing, pinned by a literal hash), inherited gap s5-final-core-r1-2
#[test]
fn the_bytes_hashed_are_the_nfkc_form() {
    use argon2::{Argon2, PasswordHash, PasswordVerifier as _};

    let composed = "\u{00E9}".repeat(10);
    let combining = "e\u{0301}".repeat(10);
    assert_ne!(composed.as_bytes(), combining.as_bytes());

    let made = hash(CHEAP_HASH_PARAMS, &combining)
        .unwrap_or_else(|error| panic!("the combining spelling was refused: {error}"));
    let parsed = PasswordHash::new(&made)
        .unwrap_or_else(|error| panic!("the hash {made:?} is not a PHC string: {error}"));
    assert!(
        Argon2::default()
            .verify_password(composed.as_bytes(), &parsed)
            .is_ok(),
        "the hash of the combining spelling is not the hash of the precomposed bytes: {made}"
    );
    assert!(
        Argon2::default()
            .verify_password(combining.as_bytes(), &parsed)
            .is_err(),
        "the hash of the combining spelling is the hash of its own bytes: {made}"
    );

    let literal = COMPOSED_HASH;
    assert_eq!(
        composed_literal(),
        literal,
        "the literal is the argon2 crate's hash of the precomposed bytes"
    );
    assert_eq!(
        verify(&combining, literal),
        Verification::Verified,
        "the combining spelling against a literal hash of the precomposed bytes"
    );
    assert_eq!(
        verify(&composed, literal),
        Verification::Verified,
        "the precomposed spelling against a literal hash of the precomposed bytes"
    );
}

/// The PHC string of the cheap set (m=8, t=1, p=1) and the salt `saltsaltsaltsalt` over the UTF-8
/// bytes of ten precomposed U+00E9, as the `argon2` crate makes it.
const COMPOSED_HASH: &str =
    "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHRzYWx0c2FsdA$RzeNteHRNsHRz+vG4VqHIrTneifE+PiJivgK4WxYqfM";

/// [`COMPOSED_HASH`] made again by the `argon2` crate from those bytes, never through [`hash`].
fn composed_literal() -> String {
    use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher as _, Version};

    let params = Params::new(8, 1, 1, None)
        .unwrap_or_else(|error| panic!("the cheap set is refused: {error}"));
    let made: PasswordHash = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_with_salt("\u{00E9}".repeat(10).as_bytes(), b"saltsaltsaltsalt")
        .unwrap_or_else(|error| panic!("the literal hash could not be made: {error}"));
    made.to_string()
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
