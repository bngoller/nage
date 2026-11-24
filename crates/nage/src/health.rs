//! `nage health`, the container health check (SPEC 5.2, decision 44): `GET /health` against
//! the local server over plain HTTP/1.1, one two-second deadline over connect, request and
//! status line, exit 0 only on 200. Silent on success; three labelled lines on standard error
//! naming the URL otherwise (decision 157). The body is never read.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::process::ExitCode;
use std::time::Duration;

use http_body_util::Empty;
use hyper::body::Bytes;
use hyper::{Request, StatusCode, header};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// The whole probe, connect to status line, must finish within this.
const DEADLINE: Duration = Duration::from_secs(2);

/// The address to probe for a bind address: the unspecified addresses map to the loopback
/// of their family, the port is kept.
pub(crate) fn target_for(bind: SocketAddr) -> SocketAddr {
    match bind {
        SocketAddr::V4(addr) if addr.ip().is_unspecified() => {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, addr.port()))
        }
        SocketAddr::V6(addr) if addr.ip().is_unspecified() => {
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, addr.port(), 0, 0))
        }
        other => other,
    }
}

/// Probes `/health` on the server bound to `bind`: silent on 200, three lines on standard error
/// and exit 1 otherwise (decisions 44 and 157).
pub(crate) async fn run(bind: SocketAddr, screen: Screen) -> ExitCode {
    let target = target_for(bind);
    let url = format!("http://{target}/health");
    match tokio::time::timeout(DEADLINE, probe(target)).await {
        Ok(Ok(StatusCode::OK)) => ExitCode::SUCCESS,
        Ok(Ok(status)) => screen.refused(&answered(&url, status), 1),
        Ok(Err(error)) => screen.refused(&unreached(&url, &error), 1),
        Err(_elapsed) => screen.refused(&silent(&url), 1),
    }
}

use crate::shape::{Doing, KEEP_THE_LINES, Refused, Screen, system_words};

/// The server answered, with another status than 200.
fn answered(url: &str, status: StatusCode) -> Refused {
    Refused::new(
        Doing::Health.not_done(),
        format!("{url} answered {status}."),
        "Read the server's log to see why; it answers 503 while it cannot read its database.",
    )
}

/// No answer could be had: nothing listens, or the request failed.
fn unreached(url: &str, error: &ProbeError) -> Refused {
    match error {
        ProbeError::Connect(source) => Refused::new(
            Doing::Health.not_done(),
            format!("Nothing answered at {url} ({}).", system_words(source)),
            "Check that the server is running and listens on the address given with --bind or NAGE_BIND.",
        ),
        ProbeError::Http(source) => request_failed(url, source),
        ProbeError::Request(source) => request_failed(url, source),
    }
}

/// The request was sent, or could not be built, and no status came back: the system's own words
/// in one pair of brackets (decision 159).
fn request_failed(url: &str, source: &(dyn std::error::Error + 'static)) -> Refused {
    Refused::new(
        Doing::Health.not_done(),
        format!("The request to {url} failed ({}).", system_words(source)),
        format!(
            "Check that the program listening there is Nage, and the address given with --bind or NAGE_BIND. {KEEP_THE_LINES}"
        ),
    )
}

/// No answer within the deadline.
fn silent(url: &str) -> Refused {
    Refused::new(
        Doing::Health.not_done(),
        format!("{url} gave no answer within 2 seconds."),
        "Check that the server is running and not overloaded; a server that is still starting answers once it listens.",
    )
}

/// Why the probe got no status.
enum ProbeError {
    Connect(io::Error),
    Http(hyper::Error),
    Request(hyper::http::Error),
}

/// Connects, sends `GET /health` and returns the status of the answer.
async fn probe(target: SocketAddr) -> Result<StatusCode, ProbeError> {
    let stream = TcpStream::connect(target)
        .await
        .map_err(ProbeError::Connect)?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(ProbeError::Http)?;
    tokio::spawn(connection);
    let request = Request::get("/health")
        .header(header::HOST, target.to_string())
        .body(Empty::<Bytes>::new())
        .map_err(ProbeError::Request)?;
    let response = sender
        .send_request(request)
        .await
        .map_err(ProbeError::Http)?;
    Ok(response.status())
}
