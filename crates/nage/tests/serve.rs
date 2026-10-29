//! `nage serve` observed from outside the process: data directory, bind address, time
//! zone, log filter, start-up and request logs, and graceful shutdown (milestone 1 slice 2);
//! the database file, its migrations at start-up and the refusals of a corrupt or newer
//! database (milestone 1 slice 3).

mod common;

use std::ffi::OsStr;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::assert_no_side_files;

use common::{
    ServeProcess, VERSION, assert_config_error, assert_runtime_error, config_error_line, has_field,
    http_get, is_listening_line, is_request_line, is_starting_line, serve,
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
/// Proves: decision 45.
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
/// Proves: decision 45.
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
/// Proves: decisions 42, 45.
#[test]
fn serve_refuses_invalid_nage_bind() {
    let out = serve().without_bind().env("NAGE_BIND", "nonsense").run();
    assert_config_error(&out, "nage: NAGE_BIND:", "nonsense");
}

/// An invalid `--bind` refuses to start with exit 2, naming the flag and the value.
///
/// Proves: decisions 42, 45.
#[test]
fn serve_refuses_invalid_bind_flag() {
    let out = serve().bind("nonsense").run();
    assert_config_error(&out, "nage: --bind:", "nonsense");
}

/// A host name is not a bind address: exit 2.
///
/// Proves: decision 45.
#[test]
fn serve_refuses_hostname_bind() {
    let out = serve().bind("localhost:0").run();
    assert_config_error(&out, "nage: --bind:", "localhost:0");
}

/// A port already in use is a runtime failure: exit 1, naming the address. The failure comes
/// after the database was opened and migrated, and the failed start leaves no `-wal` or
/// `-shm` file beside `nage.sqlite`: every failure after the open closes the storage.
///
/// Proves: decisions 45, 53.
#[test]
fn serve_refuses_when_the_port_is_taken() {
    let taken = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port to occupy");
    let addr = taken.local_addr().expect("occupied address").to_string();
    let command = serve().bind(&addr);
    let out = command.run();
    assert_runtime_error(&out, "nage: cannot listen on", &addr);
    let db = command.dir().join("data").join("nage.sqlite");
    assert!(
        db.is_file(),
        "{} was not created before the bind failure",
        db.display()
    );
    assert_no_side_files(&db, "after the bind failure");
}

/// `--bind` wins over `NAGE_BIND`.
///
/// Proves: decision 42.
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
/// Proves: decision 45.
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
/// Proves: decision 43.
#[test]
fn serve_refuses_invalid_nage_tz() {
    let out = serve().env("NAGE_TZ", "Mars/Olympus").run();
    assert_config_error(&out, "nage: NAGE_TZ:", "Mars/Olympus");
}

/// An unknown `--tz` refuses to start with exit 2, naming the flag.
///
/// Proves: decision 42.
#[test]
fn serve_refuses_invalid_tz_flag() {
    let out = serve().args(["--tz", "Mars/Olympus"]).run();
    assert_config_error(&out, "nage: --tz:", "Mars/Olympus");
}

/// With `NAGE_TZ` unset the `TZ` variable is used.
///
/// Proves: decision 43.
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
/// Proves: decision 43.
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
/// Proves: decision 43.
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
/// Proves: decision 43.
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
/// Proves: decision 42.
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
/// Proves: decision 42.
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
/// Proves: decision 42.
#[test]
fn serve_treats_empty_nage_bind_as_unset() {
    assert_serves_on_the_default_bind(serve().without_bind().env("NAGE_BIND", "").spawn());
}

/// A variable that is not valid UTF-8 is a configuration error: `NAGE_TZ` set to the bytes
/// `ff fe` refuses to start with exit 2 and a line naming `NAGE_TZ` and saying it is not
/// valid UTF-8. (`NAGE_TZ`, because `serve()` always passes `--bind`, so `NAGE_BIND` would
/// never be read.)
///
/// Proves: decision 42.
#[test]
fn serve_refuses_a_non_utf8_variable() {
    let out = serve()
        .env("NAGE_TZ", OsStr::from_bytes(&[0xff, 0xfe]))
        .run();
    let line = config_error_line(&out, "nage: NAGE_TZ:");
    assert!(
        line.contains("not valid UTF-8"),
        "`{}`: the error line does not say the value is not valid UTF-8: {line}",
        out.what
    );
}

/// `--data-dir` wins over `NAGE_DATA_DIR`: the flag's directory gains `backups/`, the
/// variable's is never created.
///
/// Proves: decision 42.
#[test]
fn serve_data_dir_flag_beats_nage_data_dir_env() {
    let command = serve();
    let from_flag = command.dir().join("from-flag");
    let from_env = command.dir().join("from-env");
    let mut server = command
        .data_dir(&from_flag)
        .env("NAGE_DATA_DIR", &from_env)
        .spawn();
    server.wait_for_dir(&from_flag.join("backups"));
    server.wait_listening();
    assert!(
        !from_env.exists(),
        "{} (NAGE_DATA_DIR) was created although --data-dir names {}",
        from_env.display(),
        from_flag.display()
    );
    let status = server.stop();
    assert_eq!(status.code(), Some(0), "nage serve after SIGTERM: {status}");
    assert!(
        !from_env.exists(),
        "{} (NAGE_DATA_DIR) exists after the shutdown",
        from_env.display()
    );
}

/// `--log` wins over `NAGE_LOG`, so an invalid `NAGE_LOG` (`nage=loud`, a level that does
/// not exist) does not matter: the server starts with `log=info` and logs its listening line.
///
/// Proves: decision 42.
#[test]
fn serve_log_flag_beats_nage_log_env() {
    let mut server = serve()
        .args(["--log", "info"])
        .env("NAGE_LOG", "nage=loud")
        .spawn();
    let line = server.wait_starting();
    assert!(
        has_field(&line, "log", "info"),
        "the starting line lacks log=info: {line}"
    );
    server.wait_listening();
}

/// An empty `TZ` with `NAGE_TZ` unset counts as unset: the start-up line reports UTC with
/// the default source.
///
/// Proves: decisions 42 and 43.
#[test]
fn serve_treats_empty_tz_as_unset() {
    let mut server = serve().env("TZ", "").spawn();
    let line = server.wait_starting();
    for (key, value) in [("tz", "UTC"), ("tz_source", "default")] {
        assert!(
            has_field(&line, key, value),
            "the starting line lacks {key}={value}: {line}"
        );
    }
}

/// An invalid `NAGE_LOG` filter directive refuses to start with exit 2.
///
/// Proves: decision 47.
#[test]
fn serve_refuses_invalid_nage_log() {
    let out = serve().env("NAGE_LOG", "nage=loud").run();
    assert_config_error(&out, "nage: NAGE_LOG:", "nage=loud");
}

/// An invalid `--log` filter directive refuses to start with exit 2, naming the flag.
///
/// Proves: decision 42.
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
    // A later request's line is the sentinel: once it has arrived, the reader thread has
    // delivered every line the first request produced, so the count below is final.
    let sentinel = http_get(addr, "/sentinel", &[]);
    assert_eq!(
        sentinel.status, 404,
        "GET /sentinel on {addr}:\n{}",
        sentinel.head
    );
    server.wait_for_line("a request log line for GET /sentinel", |line| {
        is_request_line(line, "GET", "/sentinel")
    });
    let lines = server.lines();
    let events: Vec<&String> = lines
        .iter()
        .filter(|line| is_request_line(line, "GET", "/"))
        .collect();
    assert_eq!(
        events.len(),
        1,
        "GET /?token=secret did not produce exactly one request event: {events:#?}"
    );
    assert!(
        has_field(events[0], "status", "200"),
        "the request event for GET /?token=secret lacks status=200: {}",
        events[0]
    );
    for leaked in ["token", "secret", "a=b"] {
        let leaks: Vec<&String> = lines.iter().filter(|line| line.contains(leaked)).collect();
        assert!(leaks.is_empty(), "the log contains {leaked:?}: {leaks:#?}");
    }
}

/// `RUST_LOG` is ignored: with `RUST_LOG=trace` and `NAGE_LOG` unset the default `info`
/// filter applies, so a passing `/health` request, logged at debug, leaves no request line
/// before the later `GET /` request line has arrived. (The start-up line's `log=info` is
/// reported too, but it comes from the configuration string, not from the subscriber, so it
/// is not the proof.)
///
/// Proves: decision 47.
#[test]
fn serve_ignores_rust_log() {
    let mut server = serve().env("RUST_LOG", "trace").spawn();
    let addr = server.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(
        health.status, 200,
        "GET /health on {addr} with RUST_LOG=trace:\n{}",
        health.head
    );
    let index = http_get(addr, "/", &[]);
    assert_eq!(
        index.status, 200,
        "GET / on {addr} with RUST_LOG=trace:\n{}",
        index.head
    );
    server.wait_for_line("a request log line for GET /", |line| {
        is_request_line(line, "GET", "/")
    });
    let lines = server.lines();
    let logged: Vec<&String> = lines
        .iter()
        .filter(|line| has_field(line, "path", "/health"))
        .collect();
    assert!(
        logged.is_empty(),
        "a passing /health request was logged with RUST_LOG=trace and NAGE_LOG unset: {logged:#?}"
    );
    let starting = lines
        .iter()
        .find(|line| is_starting_line(line))
        .unwrap_or_else(|| panic!("no starting line among the lines seen: {lines:#?}"));
    assert!(
        has_field(starting, "log", "info"),
        "the starting line lacks log=info: {starting}"
    );
}

/// True when `token` is `YYYY-MM-DDTHH:MM:SS.` followed by one or more digits and `Z`:
/// RFC 3339 in UTC with sub-second precision. Checked by hand, character by character.
fn is_rfc3339_utc_with_subseconds(token: &str) -> bool {
    let bytes = token.as_bytes();
    let digit = |index: usize| bytes.get(index).is_some_and(u8::is_ascii_digit);
    let is = |index: usize, expected: u8| bytes.get(index) == Some(&expected);
    let date_time = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
        .into_iter()
        .all(digit)
        && is(4, b'-')
        && is(7, b'-')
        && is(10, b'T')
        && is(13, b':')
        && is(16, b':')
        && is(19, b'.');
    let Some(fraction) = token.get(20..).and_then(|rest| rest.strip_suffix('Z')) else {
        return false;
    };
    date_time && !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
}

/// Every log line, the start-up line first among them, begins with the subscriber's
/// timestamp: RFC 3339 in UTC with sub-second precision (`YYYY-MM-DDTHH:MM:SS.ffffffZ`), not
/// whole seconds and not a local offset. A request line is waited for so the check also
/// covers an event written while serving.
///
/// Proves: decision 47.
#[test]
fn serve_log_lines_carry_rfc3339_utc_timestamps_with_subseconds() {
    assert!(
        is_rfc3339_utc_with_subseconds("2026-10-28T12:34:56.123456Z")
            && !is_rfc3339_utc_with_subseconds("2026-10-28T12:34:56Z")
            && !is_rfc3339_utc_with_subseconds("2026-10-28T12:34:56.123456+01:00")
            && !is_rfc3339_utc_with_subseconds("2026-10-28 12:34:56.123456Z"),
        "the timestamp check accepts or rejects the wrong shapes"
    );
    let mut server = serve().env("TZ", "Asia/Tokyo").spawn();
    let starting = server.wait_starting();
    let stamp = starting.split_whitespace().next().unwrap_or_default();
    assert!(
        starting.starts_with(stamp) && is_rfc3339_utc_with_subseconds(stamp),
        "the starting line does not begin with an RFC 3339 UTC timestamp with sub-seconds: {starting}"
    );
    let addr = server.wait_listening();
    let index = http_get(addr, "/", &[]);
    assert_eq!(index.status, 200, "GET / on {addr}:\n{}", index.head);
    server.wait_for_line("a request log line for GET /", |line| {
        is_request_line(line, "GET", "/")
    });
    let lines = server.lines();
    for line in lines {
        let stamp = line.split_whitespace().next().unwrap_or_default();
        assert!(
            line.starts_with(stamp) && is_rfc3339_utc_with_subseconds(stamp),
            "a log line does not begin with an RFC 3339 UTC timestamp with sub-seconds: {line}"
        );
    }
}

/// A passing `/health` request is logged at debug, so the default `info` filter keeps
/// container health checks out of the log, and `NAGE_LOG=debug` shows them. Between the
/// listening line and the `GET /` request line nothing at all is logged under the default
/// filter: not the health request, not its count query (sqlx's statement logs stay at
/// debug, decision 53). A regression guard: green before slice 3, green after it.
///
/// Proves: decisions 47, 53.
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
    let lines = server.lines();
    let logged: Vec<&String> = lines
        .iter()
        .filter(|line| has_field(line, "path", "/health"))
        .collect();
    assert!(
        logged.is_empty(),
        "a passing /health request was logged under the default filter: {logged:#?}"
    );
    let listening = lines
        .iter()
        .position(|line| is_listening_line(line))
        .unwrap_or_else(|| panic!("no listening line among the lines seen: {lines:#?}"));
    let request = lines
        .iter()
        .position(|line| is_request_line(line, "GET", "/"))
        .unwrap_or_else(|| panic!("no GET / request line among the lines seen: {lines:#?}"));
    assert!(
        listening < request,
        "the GET / request line (index {request}) precedes the listening line (index {listening}): {lines:#?}"
    );
    let between = &lines[listening + 1..request];
    assert!(
        between.is_empty(),
        "lines logged under the default filter between the listening line and the GET / request line: {between:#?}"
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

/// `serve` creates `nage.sqlite` in the data directory, migrates it and logs one INFO
/// `database ready` event naming the file, `applied=1` and `schema_version=1`, before the
/// listening line (`wait_database_ready` fails if the listening line comes first). One
/// `GET /health` before the shutdown makes a read-only connection read the database, which
/// is what makes the close order observable: in WAL mode a reader that has read keeps a
/// shared lock on the database file, so a writer closing first could not checkpoint and a
/// read-only connection closing last could not remove the side files. After SIGTERM and
/// exit 0 the writer has closed last: the file stays, no `-wal` or `-shm` file is left
/// beside it.
///
/// Proves: decisions 53, 54.
#[test]
fn serve_creates_nage_sqlite_and_logs_database_ready() {
    let command = serve();
    let db = command.dir().join("data").join("nage.sqlite");
    let db_text = db.display().to_string();
    let mut server = command.spawn();
    let ready = server.wait_database_ready();
    assert!(
        db.is_file(),
        "{} does not exist after the database-ready line: {ready}",
        db.display()
    );
    assert!(
        ready.split_whitespace().any(|token| token == "INFO"),
        "the database-ready line is not logged at INFO: {ready}"
    );
    for (key, value) in [
        ("path", db_text.as_str()),
        ("applied", "1"),
        ("schema_version", "1"),
    ] {
        assert!(
            has_field(&ready, key, value),
            "the database-ready line lacks {key}={value}: {ready}"
        );
    }
    let addr = server.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(
        health.status, 200,
        "GET /health before shutdown on {addr}:\n{}",
        health.head
    );
    let status = server.stop();
    assert_eq!(status.code(), Some(0), "nage serve after SIGTERM: {status}");
    assert!(db.is_file(), "{} is gone after the shutdown", db.display());
    assert_no_side_files(&db, "after SIGTERM and exit 0");
}

/// A second start on the same data directory finds the schema current and applies nothing:
/// its `database ready` line says `applied=0 schema_version=1`.
///
/// Proves: SPEC 5.5.
#[test]
fn serve_restart_applies_nothing() {
    // `first` stays alive until the end: it owns the temporary directory the data directory
    // lives in.
    let mut first = ServeProcess::spawn();
    let ready = first.wait_database_ready();
    assert!(
        has_field(&ready, "applied", "1"),
        "the first start did not apply migration 1: {ready}"
    );
    first.wait_listening();
    let status = first.stop();
    assert_eq!(
        status.code(),
        Some(0),
        "the first nage serve after SIGTERM: {status}"
    );

    let mut second = serve().data_dir(first.dir().join("data")).spawn();
    let ready = second.wait_database_ready();
    for (key, value) in [("applied", "0"), ("schema_version", "1")] {
        assert!(
            has_field(&ready, key, value),
            "the restart's database-ready line lacks {key}={value}: {ready}"
        );
    }
    second.wait_listening();
    let status = second.stop();
    assert_eq!(
        status.code(),
        Some(0),
        "the restarted nage serve after SIGTERM: {status}"
    );
    drop(first);
}

/// A `nage.sqlite` that is not a database (8 KiB of text where the header should be) is a
/// runtime failure: exit 1 and an error line naming the file, from opening or migrating it;
/// the file is left as it was.
///
/// Proves: decisions 45, 54.
#[test]
fn serve_refuses_a_corrupt_database() {
    let command = serve();
    let data_dir = command.dir().join("data");
    fs::create_dir_all(&data_dir)
        .unwrap_or_else(|error| panic!("cannot pre-create {}: {error}", data_dir.display()));
    let db = data_dir.join("nage.sqlite");
    let garbage = "This is not an SQLite database. ".repeat(256);
    assert!(
        garbage.len() >= 4096,
        "the garbage is only {} bytes long",
        garbage.len()
    );
    fs::write(&db, &garbage)
        .unwrap_or_else(|error| panic!("cannot write garbage to {}: {error}", db.display()));

    let out = command.run();
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` with a corrupt database ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let line = out.stderr.lines().find(|line| {
        line.starts_with("nage: cannot open database")
            || line.starts_with("nage: cannot migrate database")
    });
    assert!(
        line.is_some_and(|line| line.contains(&db.display().to_string())),
        "`{}`: no stderr line starting with \"nage: cannot open database\" or \"nage: cannot migrate database\" names {}; stderr:\n{}",
        out.what,
        db.display(),
        out.stderr
    );
    let after = fs::read(&db)
        .unwrap_or_else(|error| panic!("cannot read {} after the refusal: {error}", db.display()));
    assert!(
        after == garbage.as_bytes(),
        "{} was removed or rewritten by the refusal",
        db.display()
    );
}

/// A database carrying a migration this binary does not know refuses to start: exit 1 and
/// a message saying a newer Nage wrote it, naming the file and the migration.
///
/// Proves: SPEC 5.5, decision 54.
#[test]
fn serve_refuses_a_newer_database() {
    let command = serve();
    let data_dir = command.dir().join("data");
    fs::create_dir_all(&data_dir)
        .unwrap_or_else(|error| panic!("cannot pre-create {}: {error}", data_dir.display()));
    let db = data_dir.join("nage.sqlite");
    let db_text = db.display().to_string();
    write_newer_database(&db);

    let out = command.run();
    assert_runtime_error(
        &out,
        "nage: cannot migrate database",
        "written by a newer Nage",
    );
    let line = out
        .stderr_line_starting("nage: cannot migrate database")
        .unwrap_or_default();
    for needle in [db_text.as_str(), "9999"] {
        assert!(
            line.contains(needle),
            "the refusal does not name {needle:?}: {line}"
        );
    }
}

/// Migrates a fresh database at `path` through nage-core, then records a migration 9999 in
/// sqlx's `_sqlx_migrations` table as a newer Nage would have left it (unchecked SQL on
/// migration bookkeeping, decision 57), and closes the storage. Runs on a current-thread
/// runtime of its own: the test process has none.
fn write_newer_database(path: &Path) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime for building the newer database");
    runtime.block_on(async {
        let storage = Storage::open(path, &StorageOptions::default())
            .await
            .unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        let report = storage
            .migrate()
            .await
            .unwrap_or_else(|error| panic!("cannot migrate {}: {error}", path.display()));
        assert_eq!(
            report.applied,
            1,
            "migrations applied to the fresh database {}",
            path.display()
        );
        let mut tx = storage
            .begin_write()
            .await
            .unwrap_or_else(|error| panic!("cannot begin a write on {}: {error}", path.display()));
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (9999, 'future', 1, x'00', 0)",
        )
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|error| {
            panic!("cannot record migration 9999 in {}: {error}", path.display())
        });
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("cannot commit migration 9999 to {}: {error}", path.display()));
        storage.close().await;
    });
}
