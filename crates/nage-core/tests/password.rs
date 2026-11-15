//! Passwords (SPEC 5.3, decision 107): the rules at their limits, the hash made with the
//! parameters handed in, verification that tells a wrong password from a stored hash it cannot
//! read, the normalization that makes look-alike spellings one password, and the messages.
//! Every hash here is made with the cheap set of `nage_core::test_support` or with another small
//! set handed in, except in the one test that pins SPEC 5.3's parameters. The two properties,
//! which need no database, sit with the module (`src/password/tests.rs`).

use nage_core::password::{
    self, HashError, HashParams, MAX_PASSWORD_BYTES, MIN_PASSWORD_CHARS, PasswordError,
    Verification,
};
use nage_core::test_support::{CHEAP_HASH_PARAMS, cheap_hash};

/// The password most tests set: sixteen characters, no rule broken.
const PASSWORD: &str = "correct horse 42";

/// The pieces of a PHC string between its `$` signs: an empty one first, then the algorithm,
/// the version, the parameters, the salt and the hash.
fn pieces(hash: &str) -> Vec<&str> {
    hash.split('$').collect()
}

/// A password of exactly ten characters passes and one of nine is too short, the empty one
/// included. The count is of Unicode scalar values of the password as typed, not of bytes and
/// not of its normalized form: nine four-byte characters (36 bytes) are too short, ten two-byte
/// characters pass, ten characters that NFKC composes into five pass, and four ligatures that
/// NFKC spells out as twelve letters are too short. White space at either end counts: eight
/// letters between two spaces pass and seven do not.
///
/// Proves: SPEC 5.3 (minimum length 10), decision 107 (characters counted as typed; never
/// trimmed).
#[test]
fn a_password_has_at_least_ten_characters_as_typed() {
    assert_eq!(MIN_PASSWORD_CHARS, 10);
    for (password, label) in [
        (String::new(), "an empty password"),
        (String::from("123456789"), "nine digits"),
        ("\u{1F600}".repeat(9), "nine four-byte characters"),
        ("\u{FB03}".repeat(4), "four ligatures of three letters each"),
        (String::from(" 1234567 "), "seven digits between two spaces"),
    ] {
        assert_eq!(
            password::check(&password),
            Err(PasswordError::TooShort),
            "{label}"
        );
    }
    for (password, label) in [
        (String::from("1234567890"), "ten digits"),
        ("\u{00E9}".repeat(10), "ten two-byte letters"),
        (
            "e\u{0301}".repeat(5),
            "five letters each typed with a combining accent",
        ),
        (
            String::from(" 12345678 "),
            "eight digits between two spaces",
        ),
    ] {
        assert_eq!(password::check(&password), Ok(()), "{label}");
    }
}

/// A password of exactly 1,024 bytes passes and one of 1,025 is too long. The count is of UTF-8
/// bytes of the password as typed, not of characters and not of its normalized form: 512
/// two-byte letters pass and 513 do not, 341 three-byte ligatures (1,023 bytes) pass although
/// NFKC spells them out in more than eleven thousand bytes, and 342 full-width letters (1,026
/// bytes) are too long although NFKC makes them 342 bytes.
///
/// Proves: SPEC 5.3 (maximum 1 KiB), decision 107 (bytes counted as typed).
#[test]
fn a_password_has_at_most_1024_bytes_as_typed() {
    assert_eq!(MAX_PASSWORD_BYTES, 1024);
    for (password, label) in [
        ("a".repeat(1024), "1,024 one-byte letters"),
        ("\u{00E9}".repeat(512), "512 two-byte letters"),
        ("\u{FDFA}".repeat(341), "341 ligatures of eighteen letters"),
    ] {
        assert!(password.len() <= 1024, "{label}: {} bytes", password.len());
        assert_eq!(password::check(&password), Ok(()), "{label}");
    }
    for (password, label) in [
        ("a".repeat(1025), "1,025 one-byte letters"),
        ("\u{00E9}".repeat(513), "513 two-byte letters"),
        ("\u{FF21}".repeat(342), "342 full-width letters"),
    ] {
        assert!(password.len() > 1024, "{label}: {} bytes", password.len());
        assert_eq!(
            password::check(&password),
            Err(PasswordError::TooLong),
            "{label}"
        );
    }
}

/// There is no composition rule and nothing is trimmed: ten of the same letter, ten digits and
/// ten spaces pass; a password hashed with spaces at both ends and two spaces inside verifies
/// only as typed, not without the outer spaces and not with one inner space.
///
/// Proves: SPEC 5.3 (no composition rules), decision 107 (never trimmed).
#[test]
fn a_password_needs_no_mix_of_characters_and_keeps_its_spaces() {
    for password in ["aaaaaaaaaa", "0000000000", "          "] {
        assert_eq!(password::check(password), Ok(()), "{password:?}");
    }
    let spaced = "  open  sesame  ";
    let hash = cheap_hash(spaced);
    assert_eq!(password::verify(spaced, &hash), Verification::Verified);
    for other in [
        "open  sesame",
        "  open sesame  ",
        " open  sesame ",
        "  open  sesame",
    ] {
        assert_eq!(
            password::verify(other, &hash),
            Verification::WrongPassword,
            "{other:?} against the hash of {spaced:?}"
        );
    }
}

/// `hash` answers a PHC string that names argon2id, version 19 and the parameters handed in,
/// followed by a salt and a hash: the cheap set and another set each appear in their own
/// string. Two hashes of one password differ, since each draws its own salt, and both verify.
///
/// Proves: decision 107 (argon2id; the parameters handed in make the new hash; a fresh salt).
#[test]
fn hash_makes_an_argon2id_string_with_the_parameters_handed_in() {
    let cheap = cheap_hash(PASSWORD);
    let parts = pieces(&cheap);
    assert_eq!(parts.len(), 6, "{cheap}");
    assert_eq!(
        parts[..4],
        ["", "argon2id", "v=19", "m=8,t=1,p=1"],
        "{cheap}"
    );
    assert!(!parts[4].is_empty() && !parts[5].is_empty(), "{cheap}");

    let other = HashParams {
        memory_kib: 16,
        passes: 2,
        lanes: 1,
    };
    let second = password::hash(other, PASSWORD)
        .unwrap_or_else(|error| panic!("cannot hash with {other:?}: {error}"));
    assert_eq!(
        pieces(&second)[..4],
        ["", "argon2id", "v=19", "m=16,t=2,p=1"],
        "{second}"
    );

    let again = cheap_hash(PASSWORD);
    assert_ne!(cheap, again, "two hashes of one password are equal");
    assert_ne!(
        pieces(&cheap)[4],
        pieces(&again)[4],
        "two hashes share a salt"
    );
    for hash in [&cheap, &again, &second] {
        assert_eq!(password::verify(PASSWORD, hash), Verification::Verified);
    }
}

/// `HashParams::SPEC` is SPEC 5.3's set, m=19456 KiB, t=2, p=1; a hash made with it names it
/// and verifies. The one test that hashes at full cost (about a second in a debug build).
///
/// Proves: SPEC 5.3 (argon2id at m=19456 KiB, t=2, p=1), decision 107 (the set the binary
/// hands in).
#[test]
fn the_spec_parameters_are_m_19456_t_2_p_1() {
    assert_eq!(
        HashParams::SPEC,
        HashParams {
            memory_kib: 19_456,
            passes: 2,
            lanes: 1,
        }
    );
    let hash = password::hash(HashParams::SPEC, PASSWORD)
        .unwrap_or_else(|error| panic!("cannot hash with SPEC 5.3's parameters: {error}"));
    assert_eq!(
        pieces(&hash)[..4],
        ["", "argon2id", "v=19", "m=19456,t=2,p=1"],
        "{hash}"
    );
    assert_eq!(password::verify(PASSWORD, &hash), Verification::Verified);
}

/// `verify` accepts the password a hash was made from and refuses every other as a wrong
/// password: another digit, another case, the empty text. No rule is applied to the typed
/// text: one shorter than ten characters and one of five thousand bytes are verified like any
/// other and do not match, and a stored hash of `short`, a password the rules refuse (made
/// with the argon2 crate and the cheap set, since `hash` refuses it), verifies it.
///
/// Proves: decision 107 (verification; the rules are for setting a password, not for typing
/// one: `verify` applies none).
#[test]
fn verify_accepts_the_password_and_refuses_any_other() {
    let hash = cheap_hash(PASSWORD);
    assert_eq!(password::verify(PASSWORD, &hash), Verification::Verified);
    let long = "x".repeat(5000);
    for other in [
        "correct horse 43",
        "Correct horse 42",
        "correct horse 4",
        "correct horse 42 ",
        "",
        "short",
        long.as_str(),
    ] {
        assert_eq!(
            password::verify(other, &hash),
            Verification::WrongPassword,
            "a password of {} bytes against the hash of {PASSWORD:?}",
            other.len()
        );
    }
    let short_hash = "$argon2id$v=19$m=8,t=1,p=1$MDEyMzQ1Njc4OWFiY2RlZg$\
                      81X6JjzfetoxuCXtkAbLHW8X9hr9NkaUrhy22TDiqlg";
    assert_eq!(
        password::verify("short", short_hash),
        Verification::Verified,
        "the stored hash of a password the rules refuse, against that password"
    );
}

/// A stored hash names its own parameters and verifies whatever set new hashes are made with:
/// `verify` is handed none, and hashes made with two different sets each accept their password
/// and refuse another.
///
/// Proves: decision 107 (a stored hash verifies whatever set is handed in).
#[test]
fn verify_reads_the_parameters_from_the_stored_hash() {
    for params in [
        CHEAP_HASH_PARAMS,
        HashParams {
            memory_kib: 32,
            passes: 3,
            lanes: 2,
        },
    ] {
        let hash = password::hash(params, PASSWORD)
            .unwrap_or_else(|error| panic!("cannot hash with {params:?}: {error}"));
        assert_eq!(
            password::verify(PASSWORD, &hash),
            Verification::Verified,
            "{params:?}"
        );
        assert_eq!(
            password::verify("correct horse 43", &hash),
            Verification::WrongPassword,
            "{params:?}"
        );
    }
}

/// A stored text that is not a hash this version can verify against is told apart from a wrong
/// password, whether the typed password is the right one or not: the opaque text the tests of
/// slices 1 to 3 store, an empty text, a hash without its salt and hash, a hash without its
/// hash, another algorithm's name, an unknown version, parameters argon2 refuses, a character
/// outside the encoding, text after the hash. The undamaged hash still verifies.
///
/// Proves: decision 107 (verification tells a wrong password from a stored hash it cannot
/// read).
#[test]
fn verify_tells_a_wrong_password_from_a_stored_hash_it_cannot_read() {
    let good = cheap_hash(PASSWORD);
    let parts = pieces(&good);
    let (salt, digest) = (parts[4], parts[5]);
    let mut bad_character = good.clone();
    bad_character.pop();
    bad_character.push('!');
    let unreadable = [
        ("the opaque text of slices 1 to 3", String::from("hash")),
        ("an empty text", String::new()),
        (
            "a hash without its salt and its hash",
            String::from("$argon2id$v=19$m=8,t=1,p=1"),
        ),
        (
            "a hash without its hash",
            format!("$argon2id$v=19$m=8,t=1,p=1${salt}"),
        ),
        (
            "an algorithm that does not exist",
            good.replace("argon2id", "argon2x"),
        ),
        ("another algorithm", good.replace("argon2id", "scrypt")),
        ("an unknown version", good.replace("v=19", "v=99")),
        ("too little memory", good.replace("m=8", "m=1")),
        ("no passes", good.replace("t=1", "t=0")),
        ("a character outside the encoding", bad_character),
        ("text after the hash", format!("{good}$extra")),
        (
            "the salt and the hash without the rest",
            format!("{salt}${digest}"),
        ),
    ];
    for (label, stored) in &unreadable {
        assert_ne!(stored, &good, "{label} is the undamaged hash");
        for typed in [PASSWORD, "correct horse 43"] {
            assert_eq!(
                password::verify(typed, stored),
                Verification::UnreadableHash,
                "{label}, typed {typed:?}"
            );
        }
    }
    assert_eq!(password::verify(PASSWORD, &good), Verification::Verified);
}

/// Look-alike spellings of one letter make one password: a hash made from a password typed
/// with a precomposed accent, plain letters, plain digits and a plain space verifies the same
/// password typed with a combining accent, full-width letters, a ligature, a circled digit and
/// a no-break space, and the other way round. Nothing else is made equal: another case, the
/// letter without its accent and a missing space are wrong passwords.
///
/// Proves: decision 107 (NFKC before hashing and before verifying; no case folding).
#[test]
fn look_alike_spellings_of_a_password_are_one_password() {
    let plain = "caf\u{00E9} final 12";
    let look_alike = "c\u{FF41}fe\u{0301}\u{00A0}\u{FB01}nal \u{2460}\u{FF12}";
    assert_ne!(plain, look_alike);
    for (set, typed) in [(plain, look_alike), (look_alike, plain)] {
        let hash = cheap_hash(set);
        assert_eq!(password::verify(set, &hash), Verification::Verified);
        assert_eq!(
            password::verify(typed, &hash),
            Verification::Verified,
            "{typed:?} against the hash of {set:?}"
        );
        for other in [
            "Caf\u{00E9} final 12",
            "cafe final 12",
            "caf\u{00E9}final 12",
            "caf\u{00E9} \u{FB01}nal 1\u{00B2}2",
        ] {
            assert_eq!(
                password::verify(other, &hash),
                Verification::WrongPassword,
                "{other:?} against the hash of {set:?}"
            );
        }
    }
}

/// `hash` refuses a password that breaks a rule, with the rule's refusal, and accepts one at
/// each limit: nine characters and 1,025 bytes are refused, ten characters and 1,024 bytes are
/// hashed and verify, and so are ten characters that NFKC composes into five and 1,023 bytes
/// that NFKC spells out in more than eleven thousand, since the rules count the password as
/// typed.
///
/// Proves: decision 107 (every place that sets a password applies the same rules: no hash of a
/// password the rules refuse; the limits are counted as typed, not on the NFKC form).
#[test]
fn hash_refuses_a_password_that_breaks_a_rule() {
    assert_eq!(
        password::hash(CHEAP_HASH_PARAMS, "123456789"),
        Err(HashError::Password(PasswordError::TooShort))
    );
    assert_eq!(
        password::hash(CHEAP_HASH_PARAMS, ""),
        Err(HashError::Password(PasswordError::TooShort))
    );
    assert_eq!(
        password::hash(CHEAP_HASH_PARAMS, &"a".repeat(1025)),
        Err(HashError::Password(PasswordError::TooLong))
    );
    let longest = "a".repeat(1024);
    let composed = "e\u{0301}".repeat(5);
    let spelled_out = "\u{FDFA}".repeat(341);
    for password in [
        "1234567890",
        longest.as_str(),
        composed.as_str(),
        spelled_out.as_str(),
    ] {
        let hash = password::hash(CHEAP_HASH_PARAMS, password)
            .unwrap_or_else(|error| panic!("a password at a limit was refused: {error}"));
        assert_eq!(password::verify(password, &hash), Verification::Verified);
    }
}

/// A parameter set argon2 refuses (too little memory, no pass, no lane, less memory than the
/// lanes need) fails as `HashError::Hasher` with the hasher's own words, never with a panic and
/// never as a refusal of the password.
///
/// Proves: decision 107 (the parameters are handed in; a set argon2 refuses is a failure, not
/// a refusal).
#[test]
fn hash_fails_without_panicking_on_parameters_argon2_refuses() {
    for (memory_kib, passes, lanes) in [(1, 1, 1), (8, 0, 1), (8, 1, 0), (8, 1, 2)] {
        let params = HashParams {
            memory_kib,
            passes,
            lanes,
        };
        match password::hash(params, PASSWORD) {
            Err(HashError::Hasher(what)) => {
                assert!(!what.is_empty(), "{params:?} failed without a reason");
            }
            other => panic!("{params:?} answered {other:?}"),
        }
    }
}

/// Each refusal of a new password says what was refused, why and what to do instead; the two
/// do not share a message; a hash refused for its password has the rule's message, and a
/// failure of the hasher is named as one and passes the hasher's words on. The test prints
/// each moment with its message
/// (`cargo test -p nage-core --test password refusals_explain -- --nocapture` shows them).
///
/// Proves: decisions 102 and 107 (the password rules' messages, each in its three parts, the
/// same wherever a password is set).
#[test]
fn password_refusals_explain_themselves() {
    let cases: [(&str, &str, PasswordError, &str); 2] = [
        (
            "PasswordError::TooShort",
            "On the set-up page, the password sesame12 is chosen, which has 8 characters.",
            PasswordError::TooShort,
            "This password cannot be used because it has fewer than 10 characters, and a short password is easy to guess. Choose one with at least 10 characters; a few words in a row work well.",
        ),
        (
            "PasswordError::TooLong",
            "On the change-password page, a text of two thousand letters is pasted in as the new password.",
            PasswordError::TooLong,
            "This password cannot be used because it is longer than 1,024 bytes, about a thousand letters, which is the most Nage accepts. Choose a shorter one.",
        ),
    ];
    for (variant, moment, error, expected) in &cases {
        let message = error.to_string();
        println!("[{variant}] {moment} => {message}");
        assert_eq!(&message, expected, "the message of {error:?}");
        assert_eq!(
            HashError::Password(*error).to_string(),
            message,
            "a hash refused for its password has the rule's message"
        );
    }
    assert_ne!(cases[0].3, cases[1].3, "the two refusals share a message");
    assert_eq!(
        HashError::Hasher(String::from("memory cost is too small")).to_string(),
        "cannot hash the password: memory cost is too small",
        "a failure of the hasher is not a refusal"
    );
}
