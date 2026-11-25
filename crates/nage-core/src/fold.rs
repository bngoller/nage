//! The fold and the name rules (SPEC 6.2, INV-10, decisions 93 and 95): the one case-insensitive
//! form of account names and usernames that the unique indexes compare, and the normalization
//! every stored name goes through first. The fold is NFKC, Rust's Unicode lowercase and NFKC
//! again on the normalized text; a change of the rule after v0.2.0 recomputes the stored columns
//! in a Rust-side step, and `nage check` recomputes every account's fold to notice drift (a
//! username's stored fold is not rechecked). No arithmetic here (the lint below).
#![cfg_attr(not(test), forbid(clippy::arithmetic_side_effects))]

use std::fmt;

#[cfg(test)]
mod tests;

/// The most characters a stored name may have (SPEC 6.2): Unicode scalar values of the
/// normalized text as typed, before any folding (decision 95).
pub const MAX_NAME_CHARS: usize = 64;

/// The stored form of a name (decision 95): trimmed of Unicode white space at both ends, every
/// inner run of white space collapsed to one ASCII space. A control character that is white space
/// (a tab, a line break) is trimmed or collapsed like any other; the others are kept here, and
/// [`checked_name`] refuses every control character left after trimming.
pub fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut space_pending = false;
    for character in text.trim().chars() {
        if character.is_whitespace() {
            space_pending = true;
        } else {
            if space_pending {
                normalized.push(' ');
                space_pending = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

/// The fold of a name or username (decisions 93 and 95): [`normalize`], then NFKC, then
/// `str::to_lowercase`, then NFKC again (so a letter typed as a base and a combining mark folds
/// like its precomposed form even where only the lowercase composes), then [`normalize`] once
/// more (NFKC can put a space in front of a lone accent). Idempotent: `fold(fold(x)) == fold(x)`.
/// Not Unicode case folding: ß and ss stay distinct, and the final-sigma rule applies to Σ only.
pub fn fold(text: &str) -> String {
    let normalized = normalize(text);
    let compatible: String =
        unicode_normalization::UnicodeNormalization::nfkc(normalized.as_str()).collect();
    let lowered = compatible.to_lowercase();
    let recomposed: String =
        unicode_normalization::UnicodeNormalization::nfkc(lowered.as_str()).collect();
    normalize(&recomposed)
}

/// The name as it is stored, or why it is refused (SPEC 6.2, decision 95): the text trimmed,
/// then refused for a control character anywhere in it (general category Cc: a tab, a line
/// break), normalized, refused when empty or longer than [`MAX_NAME_CHARS`], and refused when
/// the normalized text or its NFKC form holds a colon: every name, an account's, a username, a
/// display name and the book name alike (decision 200; an account's full name joins the path
/// with one).
pub fn checked_name(text: &str) -> Result<String, NameError> {
    let trimmed = text.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    let normalized = normalize(trimmed);
    if normalized.is_empty() {
        return Err(NameError::Empty);
    }
    let chars = normalized.chars().count();
    if chars > MAX_NAME_CHARS {
        return Err(NameError::TooLong { chars });
    }
    if normalized.contains(':') || fold(&normalized).contains(':') {
        return Err(NameError::Colon);
    }
    Ok(normalized)
}

/// Why a name was refused (SPEC 6.2, decision 95). Each message is a predicate that completes
/// "its name ...", "the username ..." or "the book name ...", so every caller's message names its
/// subject (W2: every refusal explains itself); it says what is wrong with what was typed, never
/// the rule as if it were the fault (decision 136, amending 114).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but white space.
    Empty,
    /// More than [`MAX_NAME_CHARS`] characters after normalization: `chars` is how many.
    TooLong { chars: usize },
    /// A control character: a tab, a line break, or another that cannot be shown, such as a
    /// pasted escape or null character (the pause's P9).
    Control,
    /// A colon, as typed or after NFKC.
    Colon,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("is empty"),
            Self::TooLong { chars } => write!(
                f,
                "is longer than {MAX_NAME_CHARS} characters (it has {chars})"
            ),
            Self::Control => f.write_str(
                "contains a tab, a line break or another character that cannot be shown",
            ),
            Self::Colon => f.write_str("contains a colon (:)"),
        }
    }
}

impl std::error::Error for NameError {}
