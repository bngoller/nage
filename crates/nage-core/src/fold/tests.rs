//! Tests of the fold and the name rules (SPEC 6.2, INV-10, decisions 93 and 95).

use proptest::prelude::*;

use super::{Colon, MAX_NAME_CHARS, NameError, checked_name, fold, normalize};

/// `normalize` trims Unicode white space at both ends and collapses every inner run of white
/// space (ASCII, no-break and typographic spaces, a tab) to one ASCII space, and leaves every
/// other character as typed.
///
/// Proves: decision 95.
#[test]
fn normalize_trims_and_collapses_white_space() {
    for (text, expected) in [
        ("Groceries", "Groceries"),
        ("  Groceries  ", "Groceries"),
        ("Dining  Out", "Dining Out"),
        ("Dining \t Out", "Dining Out"),
        ("\u{00A0}Dining\u{00A0}\u{00A0}Out\u{00A0}", "Dining Out"),
        ("Dining\u{2003}Out", "Dining Out"),
        ("Dining\u{3000}Out", "Dining Out"),
        ("", ""),
        ("   ", ""),
        ("Caf\u{00E9}", "Caf\u{00E9}"),
        ("a:b", "a:b"),
        ("x\u{200B}y", "x\u{200B}y"),
    ] {
        assert_eq!(normalize(text), expected, "normalize({text:?})");
    }
}

/// The fold is the same for names that differ in case, in white space, in the spelling of an
/// accent (a base letter and a combining mark against the precomposed letter), in ligatures,
/// fullwidth letters, superscripts, circled letters, a double-struck capital (which has no
/// lowercase of its own, so only NFKC before lowercasing folds it), the trademark sign (whose
/// compatibility form is the capitals TM) and the Kelvin sign, and for a letter whose
/// only precomposed form is lowercase (J with a caron). It is not Unicode case folding: ß and
/// ss stay distinct, the capital ẞ folds to ß, and a word-final Σ folds to ς while a typed σ
/// stays σ. A lone acute accent, which NFKC turns into a space and a combining mark, loses the
/// space.
///
/// Proves: decision 95, INV-10 (Core half: what "ignoring case" means).
#[test]
fn fold_merges_case_compatibility_and_composition() {
    for (text, expected) in [
        ("Groceries", "groceries"),
        ("GROCERIES", "groceries"),
        ("  Groceries ", "groceries"),
        ("Dining  Out", "dining out"),
        ("Caf\u{00E9}", "caf\u{00E9}"),
        ("Cafe\u{0301}", "caf\u{00E9}"),
        ("CAF\u{00C9}", "caf\u{00E9}"),
        ("O\u{FB03}ce", "office"),
        ("\u{FF21}\u{FF22}\u{FF23}", "abc"),
        ("\u{212A}elvin", "kelvin"),
        ("x\u{00B2}", "x2"),
        ("\u{24BD}ello", "hello"),
        ("\u{210D}ome", "home"),
        ("J\u{030C}", "\u{01F0}"),
        ("\u{01F0}", "\u{01F0}"),
        ("Stra\u{1E9E}E", "stra\u{00DF}e"),
        ("Stra\u{00DF}e", "stra\u{00DF}e"),
        ("STRASSE", "strasse"),
        (
            "\u{039F}\u{0394}\u{03A5}\u{03A3}\u{03A3}\u{0395}\u{03A5}\u{03A3}",
            "\u{03BF}\u{03B4}\u{03C5}\u{03C3}\u{03C3}\u{03B5}\u{03C5}\u{03C2}",
        ),
        (
            "\u{03BF}\u{03B4}\u{03C5}\u{03C3}\u{03C3}\u{03B5}\u{03C5}\u{03C3}",
            "\u{03BF}\u{03B4}\u{03C5}\u{03C3}\u{03C3}\u{03B5}\u{03C5}\u{03C3}",
        ),
        ("\u{00B4}x", "\u{0301}x"),
        ("a\u{FF1A}b", "a:b"),
        ("\u{2122}", "tm"),
        ("", ""),
    ] {
        assert_eq!(fold(text), expected, "fold({text:?})");
    }
    assert_ne!(
        fold("Stra\u{00DF}e"),
        fold("STRASSE"),
        "ß folds apart from ss"
    );
}

/// `checked_name` returns the normalized name and refuses, with the reason, an empty or
/// white-space name, more than 64 characters (counted as characters of the normalized text as
/// typed, so 64 accented letters pass, one Arabic ligature that NFKC expands to 18 characters
/// counts as one, and 66 characters whose inner run of three spaces collapses to one pass as
/// 64), a control character anywhere inside (a tab, a line break, a bell, a C1 control; a line
/// break at the end is trimmed first), and, when colons are refused, a colon as typed or after
/// NFKC (a fullwidth or a small colon). A zero-width space passes (a recorded limit).
///
/// Proves: SPEC 6.2, decision 95.
#[test]
fn checked_name_normalizes_and_refuses_by_rule() {
    assert_eq!(MAX_NAME_CHARS, 64);
    let sixty_four = "x".repeat(64);
    let sixty_five = "x".repeat(65);
    let accented = "\u{00E9}".repeat(64);
    let spaced = format!("{}   {}", "x".repeat(32), "y".repeat(31));
    let collapsed = format!("{} {}", "x".repeat(32), "y".repeat(31));
    assert_eq!(
        (spaced.chars().count(), collapsed.chars().count()),
        (66, 64)
    );
    for (text, colon, expected) in [
        (
            "  Dining  Out ",
            Colon::Refused,
            Ok(String::from("Dining Out")),
        ),
        ("Food\n", Colon::Refused, Ok(String::from("Food"))),
        ("a:b", Colon::Allowed, Ok(String::from("a:b"))),
        (sixty_four.as_str(), Colon::Refused, Ok(sixty_four.clone())),
        (accented.as_str(), Colon::Refused, Ok(accented.clone())),
        ("\u{FDFA}", Colon::Refused, Ok(String::from("\u{FDFA}"))),
        (spaced.as_str(), Colon::Refused, Ok(collapsed.clone())),
        ("x\u{200B}y", Colon::Refused, Ok(String::from("x\u{200B}y"))),
        ("", Colon::Refused, Err(NameError::Empty)),
        ("   ", Colon::Refused, Err(NameError::Empty)),
        ("\u{00A0}", Colon::Refused, Err(NameError::Empty)),
        (sixty_five.as_str(), Colon::Refused, Err(NameError::TooLong)),
        (sixty_five.as_str(), Colon::Allowed, Err(NameError::TooLong)),
        ("a\tb", Colon::Refused, Err(NameError::Control)),
        ("a\nb", Colon::Allowed, Err(NameError::Control)),
        ("a\u{0007}b", Colon::Refused, Err(NameError::Control)),
        ("a\u{0080}b", Colon::Refused, Err(NameError::Control)),
        ("a:b", Colon::Refused, Err(NameError::Colon)),
        ("a\u{FF1A}b", Colon::Refused, Err(NameError::Colon)),
        ("a\u{FE55}b", Colon::Refused, Err(NameError::Colon)),
        (":", Colon::Refused, Err(NameError::Colon)),
    ] {
        assert_eq!(
            checked_name(text, colon),
            expected,
            "checked_name({text:?}, {colon:?})"
        );
    }
}

/// Each refusal has its own message, a predicate that completes "the name ...".
///
/// Proves: W2 (every refusal explains itself), decision 95.
#[test]
fn name_refusals_explain_themselves() {
    let messages: Vec<String> = [
        NameError::Empty,
        NameError::TooLong,
        NameError::Control,
        NameError::Colon,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "is required",
            "has at most 64 characters",
            "cannot contain control characters such as a tab or a line break",
            "cannot contain a colon",
        ]
    );
}

/// Text that leans on the characters the fold handles specially: a lone acute accent, the
/// combining caron, J and j, ǰ, spaces of three kinds, the ffi ligature, ẞ and ß, the three
/// sigmas, the Kelvin sign, a dotted capital I with its dot, colons of two widths, iota
/// subscript and alpha, the trademark sign and a double-struck capital, besides any character.
fn hostile_text() -> impl Strategy<Value = String> {
    let special = prop::sample::select(vec![
        '\u{00B4}', '\u{030C}', 'J', 'j', '\u{01F0}', ' ', '\u{00A0}', '\u{3000}', '\u{FB03}',
        '\u{1E9E}', '\u{00DF}', '\u{03A3}', '\u{03C3}', '\u{03C2}', '\u{212A}', '\u{0130}', 'i',
        '\u{0307}', ':', '\u{FF1A}', '\u{0345}', '\u{03B1}', '\u{FDFA}', '\u{24BD}', '\u{2122}',
        '\u{210D}',
    ]);
    prop::collection::vec(prop_oneof![3 => any::<char>(), 2 => special], 0..12)
        .prop_map(|characters| characters.into_iter().collect())
}

proptest! {
    #![proptest_config(crate::test_support::proptest_config(file!()))]

    /// The fold is idempotent (`fold(fold(x)) == fold(x)`), trimmed, free of inner runs of white
    /// space, and never changes a text that is already folded, for arbitrary text and for text
    /// built from the characters the fold handles specially.
    ///
    /// Proves: decision 95.
    #[test]
    fn fold_is_idempotent_and_normalized(text in hostile_text()) {
        let once = fold(&text);
        let twice = fold(&once);
        prop_assert_eq!(&twice, &once, "fold is not idempotent on {:?}", text);
        prop_assert_eq!(once.trim(), once.as_str(), "fold is not trimmed on {:?}", text);
        prop_assert!(!once.contains("  "), "fold holds two spaces on {:?}", text);
        prop_assert!(
            !once.chars().any(|character| character.is_whitespace() && character != ' '),
            "fold holds white space other than a space on {:?}", text
        );
    }

    /// Whatever `checked_name` accepts is already normalized (a second normalization changes
    /// nothing), non-empty, within 64 characters, free of control characters and, when colons
    /// are refused, free of colons as typed and after folding.
    ///
    /// Proves: SPEC 6.2, decision 95.
    #[test]
    fn accepted_names_are_normalized_and_within_the_rules(text in hostile_text()) {
        if let Ok(name) = checked_name(&text, Colon::Refused) {
            prop_assert_eq!(normalize(&name), name.clone());
            prop_assert!(!name.is_empty());
            prop_assert!(name.chars().count() <= MAX_NAME_CHARS);
            prop_assert!(!name.chars().any(char::is_control));
            prop_assert!(!name.contains(':'));
            prop_assert!(!fold(&name).contains(':'));
        }
    }
}
