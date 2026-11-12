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

/// Whether a name may hold a colon: refused in account names, whose full names join the path
/// with one (SPEC 6.2); allowed in usernames, display names and the book name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colon {
    /// A colon passes.
    Allowed,
    /// A colon, as typed or after NFKC (a fullwidth or small colon), is refused.
    Refused,
}

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
/// break), normalized, refused when empty or longer than [`MAX_NAME_CHARS`], and, under
/// [`Colon::Refused`], when the normalized text or its NFKC form holds a colon.
pub fn checked_name(text: &str, colon: Colon) -> Result<String, NameError> {
    let trimmed = text.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    let normalized = normalize(trimmed);
    if normalized.is_empty() {
        return Err(NameError::Empty);
    }
    if normalized.chars().count() > MAX_NAME_CHARS {
        return Err(NameError::TooLong);
    }
    if colon == Colon::Refused && (normalized.contains(':') || fold(&normalized).contains(':')) {
        return Err(NameError::Colon);
    }
    Ok(normalized)
}

/// Why a name was refused (SPEC 6.2, decision 95). Each message is a predicate that completes
/// "the name ...", "the username ..." or "the book name ...", so every caller's message names its
/// subject (W2: every refusal explains itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but white space.
    Empty,
    /// More than [`MAX_NAME_CHARS`] characters after normalization.
    TooLong,
    /// A control character such as a tab or a line break.
    Control,
    /// A colon, as typed or after NFKC.
    Colon,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "is required",
            Self::TooLong => "has at most 64 characters",
            Self::Control => "cannot contain control characters such as a tab or a line break",
            Self::Colon => "cannot contain a colon",
        })
    }
}

impl std::error::Error for NameError {}
