//! `nage serve` observed from outside the process: data directory, bind address, time
//! zone, log filter, start-up and request logs, and graceful shutdown (milestone 1 slice 2).

mod common;

use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpListener};

use common::{
    ServeProcess, VERSION, assert_config_error, assert_runtime_error, has_field, http_get,
    is_request_line, serve,
};

/// The start-up line names `127.0.0.1:8787`; then the server either listens there and stops
/// on SIGTERM, or, when the port is taken (a server of the maintainer's, or the other
/// default-bind test running in parallel), fails to bind it with exit 1. The exit status
/// after SIGTERM is not asserted here: the default address is the subject, shutdown is
/// proved by `serve_exits_zero_on_sigterm`.
#[track_caller]
fn assert_serves_on_the_default_bind(mut server: ServeProcess) {
    let starting = server.wait_starting();
    assert!(
        has_field(&starting, "bind", "127.0.0.1:8787"),
        "the starting line does not name bind=127.0.0.1:8787: {starting}"
    );
    let outcome = server.wait_for_line(
        "the listening line or the bind error for 127.0.0.1:8787",
        |line| {
            line.contains("listening on http://127.0.0.1:8787")
                || line.contains("cannot listen on 127.0.0.1:8787")
        },
    );
    if outcome.contains("listening on http://") {
        let _status = server.stop();
    } else {
        let status = server.wait_exit();
        assert_eq!(
            status.code(),
            Some(1),
            "nage serve after failing to bind 127.0.0.1:8787: {status}"
        );
    }
}

/// Spawns a default server, waits for the listening line, sends `SIG<signal>` and expects
/// exit code 0.
#[track_caller]
fn assert_exits_zero_on(signal: &str) {
    let mut server = ServeProcess::spawn();
    server.wait_listening();
    server.signal(signal);
    let status = server.wait_exit();
    assert_eq!(
        status.code(),
        Some(0),
        "nage serve after SIG{signal}: {status}"
    );
}

/// `nage serve` logs its listening line on stderr, answers `/health`, exits 0 on SIGTERM
/// and writes nothing to stdout.
///
/// Proves: Decision 45.
#[test]
fn serve_logs_listening_line_and_keeps_stdout_empty() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(
        health.status, 200,
        "GET /health on {addr}:\n{}",
        health.head
    );
    let status = server.stop();
    assert_eq!(status.code(), Some(0), "nage serve after SIGTERM: {status}");
    let stdout = server.stdout();
    assert_eq!(stdout, "", "nage serve wrote to stdout");
}

/// `--data-dir` names the data directory, created with its `backups/` subfolder (and any
/// missing parents) when it does not exist.
///
/// Proves: SPEC 5.2.
#[test]
fn serve_creates_data_dir_and_backups_from_flag() {
    let command = serve();
    let data_dir = command.dir().join("nested").join("books");
    let mut server = command.data_dir(&data_dir).spawn();
    server.wait_for_dir(&data_dir.join("backups"));
}

/// `NAGE_DATA_DIR` names the data directory when no flag is given.
///
/// Proves: SPEC 5.2.
#[test]
fn serve_creates_data_dir_from_env() {
    let command = serve().without_data_dir();
    let data_dir = command.dir().join("from-env");
    let mut server = command.env("NAGE_DATA_DIR", &data_dir).spawn();
    server.wait_for_dir(&data_dir.join("backups"));
}

/// With neither flag nor variable the data directory is `./data`, relative to the working
/// directory.
///
/// Proves: SPEC 5.2.
#[test]
fn serve_creates_default_data_dir_relative_to_cwd() {
    let command = serve().without_data_dir();
    let backups = command.dir().join("data").join("backups");
    let mut server = command.spawn();
    server.wait_for_dir(&backups);
}

/// A data directory that already exists, `backups/` included, is used as it is.
///
/// Proves: SPEC 5.2.
#[test]
fn serve_accepts_an_existing_data_dir() {
    let command = serve();
    let backups = command.dir().join("data").join("backups");
    fs::create_dir_all(&backups)
        .unwrap_or_else(|error| panic!("cannot pre-create {}: {error}", backups.display()));
    let mut server = command.spawn();
    server.wait_listening();
    let status = server.stop();
    assert_eq!(
        status.code(),
        Some(0),
        "nage serve with an existing data directory, after SIGTERM: {status}"
    );
}

/// A data directory that cannot be created is a runtime failure: exit 1, naming the path.
///
/// Proves: Decision 45.
#[test]
fn serve_refuses_when_data_dir_cannot_be_created() {
    let command = serve();
    let blocker = command.dir().join("blocker");
    fs::write(
        &blocker,
        "a file where the data directory's parent should be\n",
    )
    .unwrap_or_else(|error| panic!("cannot write {}: {error}", blocker.display()));
    let data_dir = blocker.join("data");
    let out = command.data_dir(&data_dir).run();
    assert_runtime_error(
        &out,
        "nage: cannot create data directory",
        &data_dir.display().to_string(),
    );
}

/// With neither `--bind` nor `NAGE_BIND` the server binds `127.0.0.1:8787`.
///
/// Proves: SPEC 5.2.
#[test]
fn serve_defaults_to_127_0_0_1_8787() {
    assert_serves_on_the_default_bind(serve().without_bind().spawn());
}

/// An invalid `NAGE_BIND` refuses to start with exit 2, naming the variable and the value.
///
/// Proves: Decisions 42, 45.
#[test]
fn serve_refuses_invalid_nage_bind() {
    let out = serve().without_bind().env("NAGE_BIND", "nonsense").run();
    assert_config_error(&out, "nage: NAGE_BIND:", "nonsense");
}

/// An invalid `--bind` refuses to start with exit 2, naming the flag and the value.
///
/// Proves: Decisions 42, 45.
#[test]
fn serve_refuses_invalid_bind_flag() {
    let out = serve().bind("nonsense").run();
    assert_config_error(&out, "nage: --bind:", "nonsense");
}

/// A host name is not a bind address: exit 2.
///
/// Proves: Decision 45.
#[test]
fn serve_refuses_hostname_bind() {
    let out = serve().bind("localhost:0").run();
    assert_config_error(&out, "nage: --bind:", "localhost:0");
}

/// A port already in use is a runtime failure: exit 1, naming the address.
///
/// Proves: Decision 45.
#[test]
fn serve_refuses_when_the_port_is_taken() {
    let taken = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port to occupy");
    let addr = taken.local_addr().expect("occupied address").to_string();
    let out = serve().bind(&addr).run();
    assert_runtime_error(&out, "nage: cannot listen on", &addr);
}

/// `--bind` wins over `NAGE_BIND`.
///
/// Proves: Decision 42.
#[test]
fn serve_bind_flag_beats_nage_bind_env() {
    let mut server = serve().env("NAGE_BIND", "203.0.113.1:8787").spawn();
    let addr = server.wait_listening();
    assert_eq!(
        addr.ip(),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        "nage serve listens on {addr}, not on the --bind address 127.0.0.1:0"
    );
}

/// The start-up line carries the version, the bind address, the data directory made
/// absolute, the time zone with its source and the log filter.
///
/// Proves: Decision 45.
#[test]
fn serve_logs_startup_line() {
    let command = serve().data_dir("data");
    let data_dir = command.dir().join("data").display().to_string();
    let mut server = command.spawn();
    let line = server.wait_starting();
    for (key, value) in [
        ("version", VERSION),
        ("bind", "127.0.0.1:0"),
        ("data_dir", data_dir.as_str()),
        ("tz", "UTC"),
        ("tz_source", "default"),
        ("log", "info"),
    ] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// A valid `NAGE_TZ` is used and reported with its source.
///
/// Proves: SPEC 5.2, decision 43.
#[test]
fn serve_accepts_valid_nage_tz() {
    let mut server = serve().env("NAGE_TZ", "America/Toronto").spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "America/Toronto"), ("tz_source", "NAGE_TZ")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// An unknown `NAGE_TZ` refuses to start with exit 2, naming the variable and the value.
///
/// Proves: Decision 43.
#[test]
fn serve_refuses_invalid_nage_tz() {
    let out = serve().env("NAGE_TZ", "Mars/Olympus").run();
    assert_config_error(&out, "nage: NAGE_TZ:", "Mars/Olympus");
}

/// An unknown `--tz` refuses to start with exit 2, naming the flag.
///
/// Proves: Decision 42.
#[test]
fn serve_refuses_invalid_tz_flag() {
    let out = serve().args(["--tz", "Mars/Olympus"]).run();
    assert_config_error(&out, "nage: --tz:", "Mars/Olympus");
}

/// With `NAGE_TZ` unset the `TZ` variable is used.
///
/// Proves: Decision 43.
#[test]
fn serve_falls_back_to_tz_when_nage_tz_is_unset() {
    let mut server = serve().env("TZ", "Europe/Berlin").spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "Europe/Berlin"), ("tz_source", "TZ")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// With `NAGE_TZ` set, `TZ` is not read, so an invalid `TZ` does not matter.
///
/// Proves: Decision 43.
#[test]
fn serve_ignores_tz_when_nage_tz_is_set() {
    let mut server = serve()
        .env("NAGE_TZ", "America/Toronto")
        .env("TZ", "Not/AZone")
        .spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "America/Toronto"), ("tz_source", "NAGE_TZ")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// An invalid `TZ` used as the fallback refuses to start, naming `TZ`, not `NAGE_TZ`.
///
/// Proves: Decision 43.
#[test]
fn serve_refuses_invalid_tz_fallback() {
    let out = serve().env("TZ", "Not/AZone").run();
    assert_config_error(&out, "nage: TZ:", "Not/AZone");
    let line = out.stderr_line_starting("nage: TZ:").unwrap_or_default();
    assert!(
        !line.contains("NAGE_TZ"),
        "the error for an invalid TZ names NAGE_TZ: {line}"
    );
}

/// A POSIX rule string, a file path and the "unknown" zone are not time-zone names: each
/// refuses to start. The legacy database name `EST5EDT` is a name and starts, so the
/// refusals reject the rule string, not every value with a digit in it.
///
/// Proves: Decision 43.
#[test]
fn serve_refuses_posix_rules_paths_and_unknown_zone() {
    {
        let mut server = serve().env("NAGE_TZ", "EST5EDT").spawn();
        let line = server.wait_starting();
        for (key, value) in [("tz", "EST5EDT"), ("tz_source", "NAGE_TZ")] {
            assert!(
                has_field(&line, key, value),
                "the legacy zone name EST5EDT was not accepted; the starting line lacks {key}={value}: {line}"
            );
        }
    }
    for value in ["EST5EDT,M3.2.0,M11.1.0", ":/etc/localtime", "Etc/Unknown"] {
        let out = serve().env("NAGE_TZ", value).run();
        assert_config_error(&out, "nage: NAGE_TZ:", value);
    }
}

/// `--tz` wins over `NAGE_TZ`, so an invalid `NAGE_TZ` does not matter.
///
/// Proves: Decision 42.
#[test]
fn serve_tz_flag_beats_nage_tz_env() {
    let mut server = serve()
        .args(["--tz", "Asia/Tokyo"])
        .env("NAGE_TZ", "Mars/Olympus")
        .spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "Asia/Tokyo"), ("tz_source", "--tz")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// Empty `NAGE_TZ`, `NAGE_DATA_DIR` and `NAGE_LOG` count as unset: the defaults apply.
///
/// Proves: Decision 42.
#[test]
fn serve_treats_empty_variables_as_unset() {
    let command = serve()
        .without_data_dir()
        .env("NAGE_TZ", "")
        .env("NAGE_DATA_DIR", "")
        .env("NAGE_LOG", "");
    let backups = command.dir().join("data").join("backups");
    let mut server = command.spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "UTC"), ("tz_source", "default"), ("log", "info")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
    server.wait_for_dir(&backups);
}

/// An empty `NAGE_BIND` counts as unset: the default address applies.
///
/// Proves: Decision 42.
#[test]
fn serve_treats_empty_nage_bind_as_unset() {
    assert_serves_on_the_default_bind(serve().without_bind().env("NAGE_BIND", "").spawn());
}

/// An invalid `NAGE_LOG` filter directive refuses to start with exit 2.
///
/// Proves: Decision 47.
#[test]
fn serve_refuses_invalid_nage_log() {
    let out = serve().env("NAGE_LOG", "nage=loud").run();
    assert_config_error(&out, "nage: NAGE_LOG:", "nage=loud");
}

/// An invalid `--log` filter directive refuses to start with exit 2, naming the flag.
///
/// Proves: Decision 42.
#[test]
fn serve_refuses_invalid_log_flag() {
    let out = serve().args(["--log", "nage=loud"]).run();
    assert_config_error(&out, "nage: --log:", "nage=loud");
}

/// SIGTERM shuts the server down gracefully with exit code 0.
///
/// Proves: SPEC 5.2, decision 45.
#[test]
fn serve_exits_zero_on_sigterm() {
    assert_exits_zero_on("TERM");
}

/// SIGINT shuts the server down gracefully with exit code 0.
///
/// Proves: SPEC 5.2, decision 45.
#[test]
fn serve_exits_zero_on_sigint() {
    assert_exits_zero_on("INT");
}

/// The request log names method, path without the query string, status and latency, and
/// never the query string or cookies.
///
/// Proves: SPEC 5.2, decision 47.
#[test]
fn serve_request_log_omits_query_string_and_cookies() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let response = http_get(addr, "/?token=secret", &[("Cookie", "a=b")]);
    assert_eq!(
        response.status, 200,
        "GET /?token=secret on {addr}:\n{}",
        response.head
    );
    server.wait_for_line("a request log line for GET / with status 200", |line| {
        is_request_line(line, "GET", "/") && has_field(line, "status", "200")
    });
    let lines = server.lines();
    for leaked in ["token", "secret", "a=b"] {
        let leaks: Vec<&String> = lines.iter().filter(|line| line.contains(leaked)).collect();
        assert!(leaks.is_empty(), "the log contains {leaked:?}: {leaks:#?}");
    }
}

/// A passing `/health` request is logged at debug, so the default `info` filter keeps
/// container health checks out of the log, and `NAGE_LOG=debug` shows them.
///
/// Proves: Decision 47.
#[test]
fn serve_logs_passing_health_requests_at_debug_only() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(
        health.status, 200,
        "GET /health on {addr}:\n{}",
        health.head
    );
    let index = http_get(addr, "/", &[]);
    assert_eq!(index.status, 200, "GET / on {addr}:\n{}", index.head);
    server.wait_for_line("a request log line for GET /", |line| {
        is_request_line(line, "GET", "/")
    });
    let logged: Vec<&String> = server
        .lines()
        .iter()
        .filter(|line| has_field(line, "path", "/health"))
        .collect();
    assert!(
        logged.is_empty(),
        "a passing /health request was logged under the default filter: {logged:#?}"
    );
    drop(server);

    let mut debug = serve().env("NAGE_LOG", "debug").spawn();
    let addr = debug.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(
        health.status, 200,
        "GET /health on {addr} with NAGE_LOG=debug:\n{}",
        health.head
    );
    let line = debug.wait_for_line(
        "a DEBUG request log line for GET /health with status 200",
        |line| is_request_line(line, "GET", "/health") && has_field(line, "status", "200"),
    );
    assert!(
        line.split_whitespace().any(|token| token == "DEBUG"),
        "the passing /health request is not logged at DEBUG: {line}"
    );
}
