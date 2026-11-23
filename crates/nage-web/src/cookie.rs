//! The session cookie and its token (SPEC 5.3, decision 108). A token is 32 random bytes from
//! the operating system, written in the cookie as 64 lowercase hexadecimal characters; the
//! database holds only the SHA-256 of those 32 bytes, which the core takes as a `TokenHash`.
//! The cookie is `nage_session` with `Path=/`, `HttpOnly`, `SameSite=Lax` and a `Max-Age`, and
//! `Secure` exactly when trust-proxy is on and the first value of `X-Forwarded-Proto` is
//! `https`: Nage never inspects the connection, and a browser drops a Secure cookie that
//! arrives over plain HTTP. No cookie crate: the header is written and read by hand.

use axum::http::{HeaderMap, HeaderValue};
use nage_core::sessions::TokenHash;

use crate::AppError;

#[cfg(test)]
mod tests;

/// A new session's token as the cookie carries it, with the hash the core stores.
pub(crate) struct NewToken {
    /// 64 lowercase hexadecimal characters.
    pub(crate) token: String,
    pub(crate) hash: TokenHash,
}

/// Draws 32 random bytes from the operating system (`getrandom`) and returns them as a token
/// with its hash. An operating system that gives none is `AppError::Internal`.
pub(crate) fn new_token() -> Result<NewToken, AppError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        AppError::Internal(format!(
            "the operating system gave no random bytes for a session token: {error}"
        ))
    })?;
    Ok(NewToken {
        token: hexadecimal(&bytes),
        hash: hash_of(&bytes),
    })
}

/// The hash of the token a cookie carried: the SHA-256 of the 32 bytes that `token` writes in
/// hexadecimal. `None` unless `token` is exactly 64 lowercase hexadecimal characters, so a
/// cookie of any other shape is never looked up.
pub(crate) fn token_hash(token: &str) -> Option<TokenHash> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    if token.len() != TOKEN_CHARS {
        return None;
    }
    let (pairs, _) = token.as_bytes().as_chunks::<2>();
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        *byte = (digit(*high)? << 4) | digit(*low)?;
    }
    Some(hash_of(&bytes))
}

/// The value of the request's cookie named `name`, as sent (decision 147): the first cookie of
/// exactly that name in the `Cookie` headers, whatever other cookies stand beside it; `None`
/// when there is none. The one reader of the session, the one-time message and the appearance:
/// every `Cookie` header is read lossily, a byte outside ASCII replaced, so that another app's
/// cookie on the same host name never hides Nage's own; only the white space HTTP allows before
/// a cookie, spaces and tabs, is dropped, so a name after any other white space (a no-break
/// space) is another cookie's, and a value is returned as it stands, a space after it included.
pub(crate) fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()))
        .find_map(|cookies| {
            cookies.split(';').find_map(|pair| {
                pair.trim_start_matches(COOKIE_SPACE)
                    .strip_prefix(name)
                    .and_then(|rest| rest.strip_prefix('='))
                    .map(str::to_owned)
            })
        })
}

/// The optional white space HTTP allows before a cookie in a `Cookie` header: the space and the
/// tab, and no other white space.
const COOKIE_SPACE: [char; 2] = [' ', '\t'];

/// The value of the request's `nage_session` cookie, as sent, through the one reader
/// ([`read`], decision 147). `None` when there is none.
pub(crate) fn session_cookie(headers: &HeaderMap) -> Option<String> {
    read(headers, SESSION_COOKIE)
}

/// Whether the answer whose headers are `headers` sets or clears the session cookie itself:
/// whether one of its `Set-Cookie` values is for the cookie named `nage_session`. A
/// `Set-Cookie` for any other cookie, such as the one-time message's (decision 117), does not
/// count, so that an answer which only sets or clears that one still has a session cookie
/// that names no valid session cleared, and a touched session's cookie sent again
/// (decision 108).
pub(crate) fn sets_session(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .any(|value| cookie_name(value.as_bytes()) == Some(SESSION_COOKIE.as_bytes()))
}

/// Whether a cookie set on the answer to this request carries `Secure` (SPEC 5.3): exactly
/// when `trust_proxy` is on and the first value of the first `X-Forwarded-Proto` header is
/// `https`, ignoring ASCII case and surrounding spaces.
pub(crate) fn is_secure(headers: &HeaderMap, trust_proxy: bool) -> bool {
    trust_proxy
        && headers
            .get(FORWARDED_PROTO)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|proto| proto.trim().eq_ignore_ascii_case("https"))
}

/// The `Set-Cookie` value that stores `token` for `max_age` seconds:
/// `nage_session=<token>; Path=/; HttpOnly; SameSite=Lax; Max-Age=<max_age>`, followed by
/// `; Secure` when `secure` is true.
pub(crate) fn set(token: &str, max_age: i64, secure: bool) -> HeaderValue {
    let secure_attribute = if secure { "; Secure" } else { "" };
    // A token is hexadecimal, so the value is always a valid header; should it ever not be,
    // the browser is told to drop its cookie instead.
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}{ATTRIBUTES}; Max-Age={max_age}{secure_attribute}"
    ))
    .unwrap_or_else(|_| clear(secure))
}

/// The `Set-Cookie` value that clears the cookie: an empty value with `Max-Age=0` and the
/// same attributes, `Secure` included when `secure` is true, so that it replaces the cookie
/// whichever way it was set.
pub(crate) fn clear(secure: bool) -> HeaderValue {
    if secure {
        HeaderValue::from_static("nage_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure")
    } else {
        HeaderValue::from_static("nage_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
    }
}

use axum::http::header;
use sha2::{Digest, Sha256};

use crate::SESSION_COOKIE;

/// How many random bytes a token has (SPEC 5.3: 32).
const TOKEN_BYTES: usize = 32;

/// How many characters a token has in the cookie: two hexadecimal digits per byte.
const TOKEN_CHARS: usize = 64;

/// The attributes every session cookie carries before its `Max-Age` (SPEC 5.3).
const ATTRIBUTES: &str = "; Path=/; HttpOnly; SameSite=Lax";

/// The header a reverse proxy names the client's scheme in.
const FORWARDED_PROTO: &str = "x-forwarded-proto";

/// The lowercase hexadecimal digits, by value.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// `bytes` as lowercase hexadecimal, two digits per byte.
fn hexadecimal(bytes: &[u8; TOKEN_BYTES]) -> String {
    let mut text = String::with_capacity(TOKEN_CHARS);
    for byte in bytes {
        for nibble in [byte >> 4, byte & 0x0f] {
            text.push(char::from(DIGITS[usize::from(nibble)]));
        }
    }
    text
}

/// The value of one lowercase hexadecimal digit, or `None` for any other byte.
fn digit(byte: u8) -> Option<u8> {
    DIGITS
        .iter()
        .position(|digit| *digit == byte)
        .and_then(|value| u8::try_from(value).ok())
}

/// The hash the core stores for a token: the SHA-256 of its bytes.
fn hash_of(bytes: &[u8; TOKEN_BYTES]) -> TokenHash {
    TokenHash::from_digest(Sha256::digest(bytes).into())
}

/// The name of the cookie a `Set-Cookie` value is for: the bytes before the first `=` of the
/// part before the first `;`, without the white space around them (RFC 6265, 5.2); `None`
/// when that part holds no `=`, a value a browser ignores.
fn cookie_name(value: &[u8]) -> Option<&[u8]> {
    let pair = value.split(|byte| *byte == b';').next()?;
    let at = pair.iter().position(|byte| *byte == b'=')?;
    Some(pair.split_at(at).0.trim_ascii())
}
