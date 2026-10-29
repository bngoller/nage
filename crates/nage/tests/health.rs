//! `nage health`, the container health check, against in-process servers built from the
//! real router, an empty router, a closed port, a listener that never answers and a real
//! `nage serve` (milestone 1 slice 2).

mod common;

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use common::{
    BlackHole, ClosedPort, FAST, HealthServer, ServeProcess, assert_config_error, nage, run,
};

/// `nage health` with an empty environment.
fn health() -> Command {
    let mut command = nage();
    command.arg("health");
    command
}

/// A 200 from `/health` exits 0, silently, without waiting on the deadline.
///
/// Proves: SPEC 5.2, decision 44.
#[test]
fn health_exits_zero_when_server_answers_200() {
    let server = HealthServer::start(true);
    let out = run(health().arg("--bind").arg(server.addr().to_string()));
    assert_eq!(server.calls(), 1, "probe calls during `{}`", out.what);
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert_eq!(out.stdout, "", "stdout of `{}`", out.what);
    assert_eq!(out.stderr, "", "stderr of `{}`", out.what);
    assert!(out.elapsed < FAST, "`{}` took {:?}", out.what, out.elapsed);
}

/// A 503 from `/health` exits 1 and says so on stderr.
///
/// Proves: decision 44.
#[test]
fn health_exits_nonzero_when_server_answers_503() {
    let server = HealthServer::start(false);
    let out = run(health().arg("--bind").arg(server.addr().to_string()));
    assert_eq!(server.calls(), 1, "probe calls during `{}`", out.what);
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    // `answered 503`, not a bare `503`: the URL on the same line carries an ephemeral port
    // that can contain those digits.
    assert!(
        out.stderr.contains("answered 503"),
        "stderr of `{}` does not name the status 503:\n{}",
        out.what,
        out.stderr
    );
}

/// Any status other than 200, here a 404 from a server without the route, exits 1.
///
/// Proves: decision 44.
#[test]
fn health_exits_nonzero_when_server_answers_404() {
    let server = HealthServer::start_empty();
    let out = run(health().arg("--bind").arg(server.addr().to_string()));
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(
        out.stderr.contains("answered 404"),
        "stderr of `{}` does not name the status 404:\n{}",
        out.what,
        out.stderr
    );
}

/// A refused connection exits 1 at once, with a line on stderr.
///
/// Proves: decision 44.
#[test]
fn health_exits_nonzero_when_nothing_listens() {
    let closed = ClosedPort::v4();
    let out = run(health().arg("--bind").arg(closed.addr().to_string()));
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(!out.stderr.is_empty(), "stderr of `{}` is empty", out.what);
    assert!(out.elapsed < FAST, "`{}` took {:?}", out.what, out.elapsed);
}

/// A server that accepts the connection and never answers is given up on after the
/// two-second deadline: exit 1.
///
/// Proves: SPEC 5.2, decision 44.
#[test]
fn health_times_out_when_server_never_responds() {
    let hole = BlackHole::new();
    let out = run(health().arg("--bind").arg(hole.addr().to_string()));
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(
        (Duration::from_millis(1900)..=Duration::from_secs(8)).contains(&out.elapsed),
        "`{}` gave up after {:?}, not after about two seconds",
        out.what,
        out.elapsed
    );
    assert!(
        out.stderr.contains("no answer within 2 seconds"),
        "stderr of `{}` does not report the deadline:\n{}",
        out.what,
        out.stderr
    );
}

/// Without `--bind` the target comes from `NAGE_BIND`.
///
/// Proves: decision 42.
#[test]
fn health_targets_nage_bind_env() {
    let server = HealthServer::start(true);
    let out = run(health().env("NAGE_BIND", server.addr().to_string()));
    assert_eq!(server.calls(), 1, "probe calls during `{}`", out.what);
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
}

/// `0.0.0.0` is probed as `127.0.0.1`, which the failure line shows (a successful probe
/// would not tell: Linux routes a connection to 0.0.0.0 to the loopback anyway).
///
/// Proves: SPEC 5.2, decision 44.
#[test]
fn health_maps_0_0_0_0_to_loopback() {
    let closed = ClosedPort::v4();
    let port = closed.port();
    let out = run(health().env("NAGE_BIND", format!("0.0.0.0:{port}")));
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let url = format!("http://127.0.0.1:{port}/health");
    assert!(
        out.stderr.contains(&url),
        "stderr of `{}` does not name {url}:\n{}",
        out.what,
        out.stderr
    );
}

/// `::` is probed as `::1`, which the failure line shows.
///
/// Proves: decision 44.
#[test]
fn health_maps_ipv6_unspecified_to_loopback() {
    let closed = ClosedPort::v6();
    assert!(
        closed.addr().is_ipv6(),
        "the closed port {} is not on IPv6",
        closed.addr()
    );
    let port = closed.port();
    let out = run(health().env("NAGE_BIND", format!("[::]:{port}")));
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let url = format!("http://[::1]:{port}/health");
    assert!(
        out.stderr.contains(&url),
        "stderr of `{}` does not name {url}:\n{}",
        out.what,
        out.stderr
    );
}

/// `--bind` wins over `NAGE_BIND`: the variable names a listener that never answers, the
/// flag a healthy server, and the probe is quick.
///
/// Proves: decision 42.
#[test]
fn health_bind_flag_beats_nage_bind_env() {
    let hole = BlackHole::new();
    let server = HealthServer::start(true);
    let out = run(health()
        .env("NAGE_BIND", hole.addr().to_string())
        .arg("--bind")
        .arg(server.addr().to_string()));
    assert_eq!(server.calls(), 1, "probe calls during `{}`", out.what);
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    assert!(out.elapsed < FAST, "`{}` took {:?}", out.what, out.elapsed);
}

/// An invalid `NAGE_BIND` is a configuration error: exit 2, naming the variable and the
/// value.
///
/// Proves: decision 42.
#[test]
fn health_rejects_invalid_nage_bind() {
    let out = run(health().env("NAGE_BIND", "nonsense"));
    assert_config_error(&out, "nage: NAGE_BIND:", "nonsense");
}

/// Port 0 is not a target: exit 2, naming the flag and the value.
///
/// Proves: decision 44.
#[test]
fn health_refuses_port_zero() {
    let out = run(health().args(["--bind", "127.0.0.1:0"]));
    assert_config_error(&out, "nage: --bind:", "127.0.0.1:0");
}

/// An empty `NAGE_BIND` counts as unset, so the probe targets the default
/// `127.0.0.1:8787`: either something answers 200 there (exit 0, for example a server of
/// the maintainer's) or the failure line names `http://127.0.0.1:8787/health`. A wrongly
/// rejected empty variable would exit 2 naming `NAGE_BIND`, which neither branch accepts.
///
/// Proves: decision 42.
#[test]
fn health_treats_empty_nage_bind_as_unset() {
    let out = run(health().env("NAGE_BIND", ""));
    let url = "http://127.0.0.1:8787/health";
    match out.code() {
        Some(0) => {}
        Some(1) => assert!(
            out.stderr
                .lines()
                .any(|line| line.starts_with("nage health:") && line.contains(url)),
            "`{}` failed without a line naming {url}; stderr:\n{}",
            out.what,
            out.stderr
        ),
        _ => panic!(
            "exit code of `{}` ({}) is neither 0 nor 1; stderr:\n{}",
            out.what, out.status, out.stderr
        ),
    }
    assert!(
        !out.stderr.contains("NAGE_BIND"),
        "`{}` names NAGE_BIND, so the empty variable was not treated as unset; stderr:\n{}",
        out.what,
        out.stderr
    );
}

/// A `ClosedPort` reserves a loopback port where nothing listens: while the guard lives, a
/// connection to it is refused and a second bind of the same address fails with
/// `AddrInUse` (`std::net::TcpListener::bind` sets `SO_REUSEADDR`, which Linux honours only
/// when both sockets set it), on IPv4 at `127.0.0.1` and on IPv6 at `[::1]`. Nothing is
/// asserted after the drop: the port's release would race with concurrent port-0 binds.
///
/// Proves: decision 59.
#[test]
fn closed_port_refuses_connections_and_holds_its_port() {
    for (closed, expected_ip) in [
        (ClosedPort::v4(), IpAddr::V4(Ipv4Addr::LOCALHOST)),
        (ClosedPort::v6(), IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ] {
        let addr = closed.addr();
        assert_eq!(
            addr.ip(),
            expected_ip,
            "the closed port's address family and loopback address"
        );
        assert_ne!(addr.port(), 0, "the closed port at {addr} has no port");
        match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(_) => panic!("a connection to the closed port {addr} was accepted"),
            Err(error) => assert_eq!(
                error.kind(),
                ErrorKind::ConnectionRefused,
                "connecting to the closed port {addr}: {error}"
            ),
        }
        match TcpListener::bind(addr) {
            Ok(_) => panic!("a second bind of the closed port {addr} succeeded"),
            Err(error) => assert_eq!(
                error.kind(),
                ErrorKind::AddrInUse,
                "binding the closed port {addr} again: {error}"
            ),
        }
        drop(closed);
    }
}

/// `nage health` reads only the bind address: an invalid time zone and a missing data
/// directory do not matter.
///
/// Proves: decision 44.
#[test]
fn health_ignores_tz_and_data_dir() {
    let server = HealthServer::start(true);
    let out = run(health()
        .env("NAGE_TZ", "Mars/Olympus")
        .env("NAGE_DATA_DIR", "/nonexistent/x")
        .arg("--bind")
        .arg(server.addr().to_string()));
    assert_eq!(server.calls(), 1, "probe calls during `{}`", out.what);
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
}

/// `nage health` passes against a running `nage serve`, as the container health check
/// runs it, and the server still stops cleanly afterwards.
///
/// Proves: SPEC 5.2 (container health check), PLAN M1 slice 2.
#[test]
fn health_succeeds_against_a_running_serve() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let out = run(health().arg("--bind").arg(addr.to_string()));
    assert_eq!(
        out.code(),
        Some(0),
        "exit code of `{}` against nage serve on {addr} ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let status = server.stop();
    assert_eq!(status.code(), Some(0), "nage serve after SIGTERM: {status}");
}
