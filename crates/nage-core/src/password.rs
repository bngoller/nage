//! Passwords (SPEC 5.3, decision 107): the rules a new password meets, the one normalization
//! every password goes through before it is hashed or verified, and the argon2id hash, made
//! with the parameters the caller hands in. Every place that sets a password (the set-up and
//! change-password pages, `nage init`, `nage user add` and `nage user reset-password`) calls
//! [`hash`], so the rules and their messages are the same everywhere; a sign-in and the
//! change-password page's current password call [`verify`].
//!
//! A hash takes real time and memory (about 19 MiB at SPEC 5.3's parameters), so the caller
//! hashes and verifies outside any database transaction, the server in a blocking thread
//! (SPEC 5.4); nothing here is async. The salt of a new hash is drawn from the operating
//! system by the `argon2` crate; nothing else here is random, and nothing here reads a clock
//! or the environment.

use std::fmt;

#[cfg(test)]
mod tests;

/// The fewest characters a password may have (SPEC 5.3): Unicode scalar values of the
/// password as typed, before any normalization.
pub const MIN_PASSWORD_CHARS: usize = 10;

/// The most bytes a password may have (SPEC 5.3: 1 KiB): UTF-8 bytes of the password as typed,
/// before any normalization.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// The argon2id parameters a new hash is made with (decision 107). The caller hands them in:
/// the binary hands [`HashParams::SPEC`], tests hand the cheap set of
/// `nage_core::test_support`. A stored hash names its own parameters, so [`verify`] takes
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashParams {
    /// The memory in KiB (argon2's `m`).
    pub memory_kib: u32,
    /// The number of passes over the memory (argon2's `t`).
    pub passes: u32,
    /// The degree of parallelism (argon2's `p`).
    pub lanes: u32,
}

impl HashParams {
    /// SPEC 5.3's parameters, which are also the `argon2` crate's defaults: m=19456 KiB, t=2,
    /// p=1.
    pub const SPEC: HashParams = HashParams {
        memory_kib: 19_456,
        passes: 2,
        lanes: 1,
    };
}

/// Checks a new password against the rules (SPEC 5.3, decision 107): at least
/// [`MIN_PASSWORD_CHARS`] characters and at most [`MAX_PASSWORD_BYTES`] bytes, both counted on
/// the password exactly as typed. Nothing is trimmed and there is no composition rule: spaces
/// at either end count and stay part of the password, and ten of the same letter pass. The two
/// refusals exclude each other: fewer than ten characters are at most 36 bytes.
pub fn check(password: &str) -> Result<(), PasswordError> {
    // Counting stops at the tenth character, so a long text is not walked to the end.
    if password.chars().take(MIN_PASSWORD_CHARS).count() < MIN_PASSWORD_CHARS {
        return Err(PasswordError::TooShort);
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(PasswordError::TooLong);
    }
    Ok(())
}

/// Hashes a new password with argon2id and `params` (decision 107): the password is checked
/// as [`check`] does (`HashError::Password` on a refusal, before any work), normalized to NFKC,
/// so that look-alike spellings of one letter (an accent typed as one character or as two, a
/// full-width letter) make one password, and hashed with a fresh salt. Returns the PHC string
/// (`$argon2id$v=19$m=...,t=...,p=...$salt$hash`) that `users.password_hash` stores. A
/// parameter set argon2 refuses, or an operating system that gives no salt, is
/// `HashError::Hasher`, never a panic.
pub fn hash(params: HashParams, password: &str) -> Result<String, HashError> {
    check(password)?;
    let argon2_params = Params::new(params.memory_kib, params.passes, params.lanes, None)
        .map_err(|error| HashError::Hasher(error.to_string()))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params)
        .hash_password(normalized(password).as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| HashError::Hasher(error.to_string()))
}

/// Verifies a typed password against a stored hash (decision 107): the password is normalized
/// to NFKC as [`hash`] does and verified with the algorithm, version and parameters the stored
/// string names, whatever set new hashes are made with. No rule is applied to the typed
/// password: one that is too short or too long is verified like any other, and it matches when
/// its NFKC form is that of the password the hash was made from (five letters typed precomposed,
/// five characters, match the same five set with combining accents, ten characters).
/// A stored text that is not a hash this version can verify against (not a PHC string,
/// a string without its salt or without its hash, an algorithm, version or parameters argon2
/// refuses) is [`Verification::UnreadableHash`], told apart from a wrong password, so that the
/// caller can verify against its dummy hash instead and log the damage.
pub fn verify(password: &str, stored_hash: &str) -> Verification {
    let Ok(stored) = PasswordHash::new(stored_hash) else {
        return Verification::UnreadableHash;
    };
    // The `argon2` crate answers a stored string without its salt or without its hash as a
    // wrong password; neither can be verified against, so it is unreadable.
    if stored.salt.is_none() || stored.hash.is_none() {
        return Verification::UnreadableHash;
    }
    // The algorithm, version and parameters come from the stored string, not from the
    // default the verifier is made with.
    match Argon2::default().verify_password(normalized(password).as_bytes(), &stored) {
        Ok(()) => Verification::Verified,
        Err(argon2::password_hash::Error::PasswordInvalid) => Verification::WrongPassword,
        Err(_) => Verification::UnreadableHash,
    }
}

/// What [`verify`] found (decision 107). Not a refusal with a message of its own: the sign-in
/// page shows one message for every miss (decision 106).
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification {
    /// The password is the one the stored hash was made from.
    Verified,
    /// The stored hash is readable and was made from another password.
    WrongPassword,
    /// The stored text is not a hash this version can verify against.
    UnreadableHash,
}

/// Why a new password was refused (SPEC 5.3, decision 107). Each message says what was
/// refused, why and what to do (decision 102) and can be shown beside the password box or on
/// a command's error line as it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordError {
    /// Fewer than [`MIN_PASSWORD_CHARS`] characters as typed, an empty password included.
    TooShort,
    /// More than [`MAX_PASSWORD_BYTES`] bytes as typed.
    TooLong,
}

impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooShort => {
                "This password cannot be used because it has fewer than 10 characters, and a short password is easy to guess. Choose one with at least 10 characters; a few words in a row work well."
            }
            Self::TooLong => {
                "This password cannot be used because it is longer than 1,024 bytes, about a thousand letters, which is the most Nage accepts. Choose a shorter one."
            }
        })
    }
}

impl std::error::Error for PasswordError {}

/// Why a hash could not be made (decision 107).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashError {
    /// The password breaks a rule: a refusal, whose message is the rule's.
    Password(PasswordError),
    /// The hasher itself failed, with its own message: the parameters are not a set argon2
    /// accepts, or the operating system gave no salt. Not a refusal (decision 102).
    Hasher(String),
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(error) => write!(f, "{error}"),
            Self::Hasher(what) => write!(f, "cannot hash the password: {what}"),
        }
    }
}

impl std::error::Error for HashError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Password(error) => Some(error),
            Self::Hasher(_) => None,
        }
    }
}

impl From<PasswordError> for HashError {
    fn from(error: PasswordError) -> Self {
        Self::Password(error)
    }
}

use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher as _, PasswordVerifier as _, Version,
};
use unicode_normalization::UnicodeNormalization as _;

/// The one normalization a password goes through before it is hashed or verified (decision
/// 107): NFKC, so that look-alike spellings of one letter make one password. Nothing else:
/// no trimming and no case folding.
fn normalized(password: &str) -> String {
    password.nfkc().collect()
}
