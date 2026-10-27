//! `nage health`, the container health check (SPEC 5.2, decision 44): `GET /health` against
//! the local server over plain HTTP/1.1, one two-second deadline over connect, request and
//! status line, exit 0 only on 200. Silent on success; one stderr line naming the URL
//! otherwise. The body is never read.

use std::fmt;
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

/// Probes `/health` on the server bound to `bind`.
pub(crate) async fn run(bind: SocketAddr) -> ExitCode {
    let target = target_for(bind);
    let url = format!("http://{target}/health");
    match tokio::time::timeout(DEADLINE, probe(target)).await {
        Ok(Ok(StatusCode::OK)) => ExitCode::SUCCESS,
        Ok(Ok(status)) => {
            eprintln!("nage health: {url} answered {status}");
            ExitCode::FAILURE
        }
        Ok(Err(error)) => {
            eprintln!("nage health: {url}: {error}");
            ExitCode::FAILURE
        }
        Err(_elapsed) => {
            eprintln!("nage health: {url}: no answer within 2 seconds");
            ExitCode::FAILURE
        }
    }
}

/// Why the probe got no status.
enum ProbeError {
    Connect(io::Error),
    Http(hyper::Error),
    Request(hyper::http::Error),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(error) => write!(f, "{error}"),
            Self::Http(error) => write!(f, "{error}"),
            Self::Request(error) => write!(f, "cannot build the request: {error}"),
        }
    }
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
