//! What the process tests of milestone 2 slice 5 need beyond `http_get` (decision 115): a
//! request with any method, headers and body, a form sent as a browser on one of Nage's own
//! pages sends it, the set-up of a running server through its page, and the session cookie
//! out of an answer.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;

use super::{HttpResponse, ServeProcess};

/// The admin the process tests set a server up with, and the password.
pub const ADMIN: &str = "sam";
pub const PASSWORD: &str = "correct horse 42";

/// `text` percent-encoded: every byte but ASCII letters, digits, `-`, `.`, `_` and `~`.
pub fn percent(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// One request over a plain `TcpStream` with `Connection: close`: the answer, or what went
/// wrong when the server gave no complete one.
pub fn http_request(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<HttpResponse, String> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|error| format!("cannot connect to {addr}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|error| format!("cannot set the read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("cannot set the write timeout: {error}"))?;
    let mut request =
        format!("{method} {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    if !body.is_empty() || method == "POST" {
        let _ = write!(request, "Content-Length: {}\r\n", body.len());
    }
    request.push_str("\r\n");
    let mut bytes = request.into_bytes();
    bytes.extend_from_slice(body);
    stream
        .write_all(&bytes)
        .map_err(|error| format!("cannot send {method} {target} to {addr}: {error}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("no complete answer to {method} {target} from {addr}: {error}"))?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            format!(
                "{method} {target} on {addr}: no end of headers in {:?}",
                String::from_utf8_lossy(&raw)
            )
        })?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("{method} {target} on {addr}: no status code in {head:?}"))?;
    Ok(HttpResponse { status, head, body })
}

/// The value of header `name` in the answer's head, ignoring the name's case.
pub fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response.head.lines().skip(1).find_map(|line| {
        let (found, value) = line.split_once(':')?;
        found.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// The session cookie an answer sets: its token and whether it carries `Secure`. Panics
/// unless the answer sets `nage_session` with a token.
#[track_caller]
pub fn session_cookie(response: &HttpResponse) -> (String, bool) {
    let cookie = header(response, "set-cookie")
        .unwrap_or_else(|| panic!("the answer sets no cookie:\n{}", response.head));
    let mut parts = cookie.split("; ");
    let token = parts
        .next()
        .and_then(|first| first.strip_prefix("nage_session="))
        .unwrap_or_else(|| panic!("the cookie set is not nage_session: {cookie}"));
    assert_eq!(token.len(), 64, "length of the session token in {cookie}");
    (token.to_owned(), parts.any(|part| part == "Secure"))
}

impl ServeProcess {
    /// Sends one request to this server at `addr`. When the server gives no complete answer
    /// (a handler that panicked closes the connection), the failure names what the server
    /// wrote on stderr until then.
    #[track_caller]
    pub fn send(
        &mut self,
        addr: SocketAddr,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> HttpResponse {
        match http_request(addr, method, target, headers, body) {
            Ok(response) => response,
            Err(error) => {
                // Give the server's last lines time to arrive.
                thread::sleep(Duration::from_millis(500));
                let stderr = self.lines().join("\n");
                panic!("{error}; `{}`; stderr:\n{stderr}", self.what);
            }
        }
    }

    /// `GET target` with `headers`.
    #[track_caller]
    pub fn get(
        &mut self,
        addr: SocketAddr,
        target: &str,
        headers: &[(&str, &str)],
    ) -> HttpResponse {
        self.send(addr, "GET", target, headers, &[])
    }

    /// `POST target` with `fields` as a URL-encoded form from the server's own origin, and
    /// with `headers` beside it.
    #[track_caller]
    pub fn post_form(
        &mut self,
        addr: SocketAddr,
        target: &str,
        headers: &[(&str, &str)],
        fields: &[(&str, &str)],
    ) -> HttpResponse {
        let mut body = String::new();
        for (name, value) in fields {
            if !body.is_empty() {
                body.push('&');
            }
            let _ = write!(body, "{}={}", percent(name), percent(value));
        }
        let origin = format!("http://{addr}");
        let mut all = vec![
            ("Origin", origin.as_str()),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ];
        all.extend_from_slice(headers);
        self.send(addr, "POST", target, &all, body.as_bytes())
    }

    /// Sets this server up through its set-up page, as the book `Household` in CAD with the
    /// admin [`ADMIN`] and [`PASSWORD`] and no starter chart, sending `headers` beside the
    /// form, and returns the answer, which must be the 303 to `/` with the session cookie.
    #[track_caller]
    pub fn set_up(&mut self, addr: SocketAddr, headers: &[(&str, &str)]) -> HttpResponse {
        let response = self.post_form(
            addr,
            "/setup",
            headers,
            &[
                ("book_name", "Household"),
                ("currency", "CAD"),
                ("username", ADMIN),
                ("display_name", "Sam"),
                ("password", PASSWORD),
                ("password_again", PASSWORD),
            ],
        );
        assert_eq!(
            response.status,
            303,
            "POST /setup on {addr}:\n{}\n{}",
            response.head,
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(
            header(&response, "location"),
            Some("/"),
            "where the set-up on {addr} leads:\n{}",
            response.head
        );
        response
    }

    /// Sets this server up as [`ServeProcess::set_up`] does and returns the `Cookie` header
    /// value of the admin's session.
    #[track_caller]
    pub fn signed_in(&mut self, addr: SocketAddr) -> String {
        let response = self.set_up(addr, &[]);
        let (token, _secure) = session_cookie(&response);
        format!("nage_session={token}")
    }
}
