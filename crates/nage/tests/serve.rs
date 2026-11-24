//! `nage serve` observed from outside the process: data directory, bind address, time
//! zone, log filter, start-up and request logs, and graceful shutdown (milestone 1 slice 2);
//! the database file, its migrations at start-up and the refusals of a corrupt or newer
//! database (milestone 1 slice 3); the in-use mark, the backup, the update and the check before
//! the listener, the log of each start and every start failure a process can reach, pinned as
//! text snapshots (milestone 2 slice 8, decisions 151 to 160).

mod common;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use nage_core::storage::{Storage, StorageOptions};
use nage_core::test_support::{TestDir, assert_no_side_files};
use nage_core::timestamp::Timestamp;

use common::commands::{self, hold_mark, plant};
use common::transcript::{new_stamps, transcript};
use common::web::PASSWORD;
use common::{
    Finished, ServeCommand, ServeProcess, VERSION, assert_config_error, assert_runtime_error,
    config_error_line, has_field, http_get, is_listening_line, is_request_line, is_starting_line,
    nage, run, serve,
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
                || line.starts_with("Why: The address 127.0.0.1:8787 is already in use")
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

/// A data directory that cannot be created is a runtime failure: exit 1, the reason naming the
/// path (decision 157's three lines).
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
        "Why: The folder ",
        &format!("{} could not be created", data_dir.display()),
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
    assert_config_error(&out, "Why: NAGE_BIND gives ", "nonsense");
}

/// An invalid `--bind` refuses to start with exit 2, naming the flag and the value.
///
/// Proves: decisions 42, 45.
#[test]
fn serve_refuses_invalid_bind_flag() {
    let out = serve().bind("nonsense").run();
    assert_config_error(&out, "Why: --bind gives ", "nonsense");
}

/// A host name is not a bind address: exit 2.
///
/// Proves: decision 45.
#[test]
fn serve_refuses_hostname_bind() {
    let out = serve().bind("localhost:0").run();
    assert_config_error(&out, "Why: --bind gives ", "localhost:0");
}

/// A port already in use is a runtime failure: exit 1, the reason naming the address. The failure comes
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
    assert_runtime_error(
        &out,
        "Why: The address ",
        &format!("{addr} is already in use"),
    );
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
    assert_config_error(&out, "Why: NAGE_TZ gives ", "Mars/Olympus");
}

/// An unknown `--tz` refuses to start with exit 2, naming the flag.
///
/// Proves: decision 42.
#[test]
fn serve_refuses_invalid_tz_flag() {
    let out = serve().args(["--tz", "Mars/Olympus"]).run();
    assert_config_error(&out, "Why: --tz gives ", "Mars/Olympus");
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
    assert_config_error(&out, "Why: TZ gives ", "Not/AZone");
    let line = out
        .stderr_line_starting("Why: TZ gives ")
        .unwrap_or_default();
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
        assert_config_error(&out, "Why: NAGE_TZ gives ", value);
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
    let line = config_error_line(&out, "Why: NAGE_TZ holds ");
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
    assert_config_error(&out, "Why: NAGE_LOG gives ", "nage=loud");
}

/// An invalid `--log` filter directive refuses to start with exit 2, naming the flag.
///
/// Proves: decision 42.
#[test]
fn serve_refuses_invalid_log_flag() {
    let out = serve().args(["--log", "nage=loud"]).run();
    assert_config_error(&out, "Why: --log gives ", "nage=loud");
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
/// never the query string or cookies. The start page is behind the sign-in since milestone 2
/// slice 5, so the server is set up through its page first and the requests carry the
/// admin's session cookie beside another cookie; that cookie as it was sent, its name with its
/// token, never reaches the log either (nage-web's `auth_log` looks for the token alone).
///
/// Proves: SPEC 5.2, decision 47, decision 109 (the cookie is never logged).
#[test]
fn serve_request_log_omits_query_string_and_cookies() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let session = server.signed_in(addr);
    let cookies = format!("a=b; {session}");
    let response = server.get(addr, "/?token=secret", &[("Cookie", &cookies)]);
    assert_eq!(
        response.status, 200,
        "GET /?token=secret on {addr}:\n{}",
        response.head
    );
    // A later request's line is the sentinel: once it has arrived, the reader thread has
    // delivered every line the first request produced, so the count below is final.
    let sentinel = server.get(addr, "/sentinel", &[("Cookie", &session)]);
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
    for leaked in ["token", "secret", "a=b", session.as_str(), "nage_session"] {
        let leaks: Vec<&String> = lines.iter().filter(|line| line.contains(leaked)).collect();
        assert!(leaks.is_empty(), "the log contains {leaked:?}: {leaks:#?}");
    }
}

/// `RUST_LOG` is ignored: with `RUST_LOG=trace` and `NAGE_LOG` unset the default `info`
/// filter applies, so a passing `/health` request, logged at debug, leaves no request line
/// before the later `GET /` request line has arrived. (The start-up line's `log=info` is
/// reported too, but it comes from the configuration string, not from the subscriber, so it
/// is not the proof.) The start page is asked for with the session of a set-up through the
/// page, since it is behind the sign-in.
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
    let session = server.signed_in(addr);
    let index = server.get(addr, "/", &[("Cookie", &session)]);
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

/// A passing `/health` request is logged at debug, so the default `info` filter keeps
/// container health checks out of the log, and `NAGE_LOG=debug` shows them. Between the
/// listening line and the next request line, the set-up through the page that opens the
/// session for `GET /`, nothing at all is logged under the default filter: not the health
/// request, not its count query (sqlx's statement logs stay at debug, decision 53). A
/// regression guard: green before slice 3, green after it.
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
    let session = server.signed_in(addr);
    let index = server.get(addr, "/", &[("Cookie", &session)]);
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
        .position(|line| is_request_line(line, "POST", "/setup"))
        .unwrap_or_else(|| panic!("no POST /setup request line among the lines seen: {lines:#?}"));
    assert!(
        listening < request,
        "the POST /setup request line (index {request}) precedes the listening line (index {listening}): {lines:#?}"
    );
    let between = &lines[listening + 1..request];
    assert!(
        between.is_empty(),
        "lines logged under the default filter between the listening line and the POST /setup request line: {between:#?}"
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
/// `database ready` event naming the file, `applied=2` and `schema_version=2`, before the
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
        ("applied", "2"),
        ("schema_version", "2"),
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
/// its `database ready` line says `applied=0 schema_version=2`.
///
/// Proves: SPEC 5.5.
#[test]
fn serve_restart_applies_nothing() {
    // `first` stays alive until the end: it owns the temporary directory the data directory
    // lives in.
    let mut first = ServeProcess::spawn();
    let ready = first.wait_database_ready();
    assert!(
        has_field(&ready, "applied", "2"),
        "the first start did not apply both migrations: {ready}"
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
    for (key, value) in [("applied", "0"), ("schema_version", "2")] {
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
/// runtime failure: exit 1 and a reason naming the file that could not be opened (decision
/// 157's three lines); the file is left as it was.
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
    let line = out.stderr_line_starting("Why: The database ");
    assert!(
        line.is_some_and(|line| line.contains(&format!("{} could not be opened", db.display()))),
        "`{}`: no stderr line starting with \"Why: The database \" says {} could not be opened; stderr:\n{}",
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

/// A database carrying a migration this binary does not know refuses to start: exit 1, the
/// first line naming this version, the reason saying a newer Nage wrote it and naming the
/// migration, and what now naming the data folder's backups folder (decision 152).
///
/// Proves: SPEC 5.5, decision 54.
#[test]
fn serve_refuses_a_newer_database() {
    let command = serve();
    let data_dir = command.dir().join("data");
    fs::create_dir_all(&data_dir)
        .unwrap_or_else(|error| panic!("cannot pre-create {}: {error}", data_dir.display()));
    let db = data_dir.join("nage.sqlite");
    write_newer_database(&db);

    let out = command.run();
    assert_runtime_error(
        &out,
        "Why: A newer Nage wrote this database",
        "the change 9999",
    );
    assert!(
        out.stderr
            .lines()
            .any(|line| line == format!("Error: Nage {VERSION} did not start.")),
        "the refusal does not name this version; stderr:\n{}",
        out.stderr
    );
    let now = out.stderr_line_starting("What now: ").unwrap_or_default();
    let backups = data_dir.join("backups").display().to_string();
    assert!(
        now.contains(&backups),
        "what now does not name {backups}: {now}"
    );
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
            2,
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

/// The stamps of the copies the tests plant, which their transcripts keep as they are.
const OLD_STAMPS: [&str; 2] = ["2026-01-01T00-00-00Z", "2027-03-14T02-00-00Z"];

/// The name of the older copy of the step 0001 the tests plant beside the backups.
fn older_copy() -> String {
    format!("{}-pre-migrate-0001-to-{VERSION}.sqlite", OLD_STAMPS[0])
}

/// The data folder of the working directory `dir`.
fn data(dir: &Path) -> PathBuf {
    dir.join("data")
}

/// The backups folder of the working directory `dir`.
fn backups_folder(dir: &Path) -> PathBuf {
    data(dir).join("backups")
}

/// The names of the files in the backups folder of `dir`, sorted; none when it is missing.
fn backups(dir: &Path) -> Vec<String> {
    let folder = backups_folder(dir);
    let Ok(entries) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", folder.display()))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Plants a file named `name` in the backups folder of `dir`.
fn plant_backup(dir: &Path, name: &str) {
    let folder = backups_folder(dir);
    fs::create_dir_all(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    fs::write(folder.join(name), b"a copy a test planted")
        .unwrap_or_else(|error| panic!("cannot plant {name}: {error}"));
}

/// Sets the mode of `path` to `mode`.
fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("cannot change the mode of {}: {error}", path.display()));
}

/// The permission bits of `path`.
fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// Asserts that `out` ended with the exit code `code`, showing what it printed otherwise (a stub's
/// panic shows only on standard error).
#[track_caller]
fn assert_exit(out: &Finished, code: i32) {
    assert_eq!(
        out.code(),
        Some(code),
        "exit code of `{}` ({}); stdout:\n{}\nstderr:\n{}",
        out.what,
        out.status,
        out.stdout,
        out.stderr
    );
}

/// `nage serve --data-dir <dir>/data --bind <bind>` in the working directory `dir`.
fn serve_in(dir: &Path, bind: &str) -> Command {
    let mut command = nage();
    command
        .current_dir(dir)
        .arg("serve")
        .arg("--data-dir")
        .arg(data(dir))
        .arg("--bind")
        .arg(bind);
    command
}

/// Runs a start of `nage serve` in `dir` on `bind` that is expected to fail: the process and its
/// transcript, the working directory shown as `/srv/nage`, each stamp of a backup the process
/// wrote as `[STAMP]`; the caller adds what else varies.
fn start_that_fails(dir: &Path, bind: &str) -> (Finished, common::transcript::Transcript) {
    let mut command = serve_in(dir, bind);
    let shown = transcript(&command).dir(dir);
    let out = run(&mut command);
    let shown = shown.stamps(&new_stamps(&out.stderr, &OLD_STAMPS));
    (out, shown)
}

/// What readies a working directory before a server starts in it.
type Prepare = fn(&Path);

/// Starts `nage serve --data-dir <dir>/data --bind 127.0.0.1:0` in a working directory of its own,
/// which `prepare` readies first: the server, the working directory and the transcript's start.
fn server_in(prepare: Prepare) -> (ServeProcess, PathBuf, common::transcript::Transcript) {
    let command: ServeCommand = serve().without_bind().without_data_dir();
    let dir = command.dir().to_path_buf();
    prepare(&dir);
    let command = command.args([
        OsString::from("--data-dir"),
        data(&dir).into_os_string(),
        OsString::from("--bind"),
        OsString::from("127.0.0.1:0"),
    ]);
    let shown = transcript(&command.command()).dir(&dir);
    (command.spawn(), dir, shown)
}

/// The lines `server` logged up to its listening line, waited for, as its transcript shows them:
/// the stamps as `[TIME]`, the port it listens on as `[PORT]` and each stamp of a backup it wrote
/// as `[STAMP]`.
fn log_until_listening(
    server: &mut ServeProcess,
    shown: common::transcript::Transcript,
) -> (Vec<String>, String) {
    let addr = server.wait_listening();
    let mut lines = server.lines().to_vec();
    if let Some(at) = lines.iter().position(|line| is_listening_line(line)) {
        lines.truncate(at + 1);
    }
    let log = shown
        .port(addr.port())
        .stamps(&new_stamps(&lines.join("\n"), &OLD_STAMPS))
        .log(&lines);
    (lines, log)
}

/// Leaves `dir` with an empty database file in `data/`, a zero-byte file as a cut-off `nage init`
/// leaves it.
fn zero_byte_database_in(dir: &Path) {
    fs::create_dir_all(data(dir).join("backups"))
        .unwrap_or_else(|error| panic!("cannot create the data folder: {error}"));
    fs::write(commands::database(dir), b"")
        .unwrap_or_else(|error| panic!("cannot write the empty file: {error}"));
}

/// Leaves `dir` with a copy of the 0.1.0 book and the older copy of its step in the backups
/// folder.
fn behind_with_an_older_copy_in(dir: &Path) {
    commands::behind_in(dir);
    plant_backup(dir, &older_copy());
}

/// Every log of a start, up to the listening line, pinned as text: a first start on a new data
/// folder, an empty file a cut-off `nage init` left finished, an update after its backup, an
/// update whose backup replaced the older copy of its step (`older backup removed` after `backup
/// written`), a book that passes the check (`check ok rules=10`), one that breaks two rules
/// (`check failed problems=2`, then each line at ERROR, word for word as nage-core writes it), and
/// one whose check cannot run (a table dropped: `check could not run` at ERROR, the start going
/// on).
///
/// Proves: decision 160, ruling P4 (the log of a first start, an empty file finished, an update, a replaced copy after the backup written, a book that passes and one that breaks a rule, counted as problems)
#[test]
fn serve_reads_as_its_log_lines() {
    let cases: [(&str, Prepare); 7] = [
        ("log_first_start", |_| {}),
        ("log_check_cannot_run", |dir| {
            commands::set_up_in(dir);
            plant(&commands::database(dir), &["DROP TABLE postings"]);
        }),
        ("log_empty_file_finished", zero_byte_database_in),
        ("log_update", commands::behind_in),
        (
            "log_update_replacing_an_older_copy",
            behind_with_an_older_copy_in,
        ),
        ("log_book_passes", |dir| {
            commands::set_up_in(dir);
        }),
        ("log_broken_book", commands::broken_book_in),
    ];
    for (name, prepare) in cases {
        let (mut server, dir, shown) = server_in(prepare);
        let (_lines, transcript) = log_until_listening(&mut server, shown);
        let status = server.stop();
        assert!(status.success(), "{name}: the server stopped with {status}");
        if name == "log_update_replacing_an_older_copy" {
            let kept = backups(&dir);
            assert!(
                kept.len() == 1 && !kept.contains(&older_copy()),
                "{name}: the backups afterwards: {kept:?}"
            );
        }
        insta::assert_snapshot!(name, transcript);
    }
}

/// Every start failure a process can reach prints the log's lines, then the three lines of
/// decision 157 on standard error, the system's own words in one pair of brackets, and exits 1:
/// a taken address (after the update and the check), a data folder that cannot be created, a mark
/// whose file cannot be taken, the folder held by `nage migrate`, a backups folder Nage may not
/// write in, a file at the backup's name, and a change that fails after its backup was written and
/// the older copy of its step removed, which are logged before the three lines. A start that fails
/// before the update changes nothing.
///
/// Proves: decisions 152, 159 and 160, ruling P9 (every start failure a process can reach in three lines after the log, exit 1; a failed change after its backup and removal lines)
#[test]
fn serve_start_failures_read_as_their_transcripts() {
    // A taken address: the glibc of the test host words error 98 "Address already in use", the
    // musl of the image "Address in use"; the transcript shows the image's words.
    let dir = TestDir::new();
    let taken = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port to occupy");
    let port = taken.local_addr().expect("the occupied address").port();
    let (out, shown) = start_that_fails(dir.path(), &format!("127.0.0.1:{port}"));
    assert_exit(&out, 1);
    let transcript = shown
        .port(port)
        .redact("Address already in use", "Address in use")
        .of(&out);
    assert_no_side_files(&commands::database(dir.path()), "after the bind failure");
    insta::assert_snapshot!("address_in_use", transcript);
    drop(taken);

    // A data folder that cannot be created: the working directory may not be written.
    let dir = TestDir::new();
    set_mode(dir.path(), 0o500);
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    set_mode(dir.path(), 0o700);
    assert_exit(&out, 1);
    assert!(!data(dir.path()).exists(), "the data folder was created");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("data_folder_cannot_be_created", transcript);

    // The mark's file cannot be taken: a folder stands at its name.
    let dir = TestDir::new();
    fs::create_dir_all(data(dir.path()).join("nage.lock"))
        .unwrap_or_else(|error| panic!("cannot plant the folder: {error}"));
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    assert_exit(&out, 1);
    assert!(
        !commands::database(dir.path()).exists(),
        "the database was created"
    );
    let transcript = shown.of(&out);
    insta::assert_snapshot!("mark_cannot_be_taken", transcript);

    // The folder held by nage migrate.
    let dir = commands::behind();
    let mark = hold_mark(&data(dir.path()), "migrate");
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    drop(mark);
    assert_exit(&out, 1);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("migrate_working_on_the_folder", transcript);

    // A backups folder Nage may not write in.
    let dir = commands::behind();
    let folder = backups_folder(dir.path());
    fs::create_dir_all(&folder)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", folder.display()));
    set_mode(&folder, 0o500);
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    set_mode(&folder, 0o700);
    assert_exit(&out, 1);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    assert!(backups(dir.path()).is_empty(), "a backup was written");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("backup_failed", transcript);

    // A file at the backup's name: every name of the seconds around the start is taken.
    let dir = commands::behind();
    let now = jiff::Timestamp::now().as_second();
    let taken: Vec<String> = (now - 2..=now + 8)
        .map(|second| {
            let stamp = Timestamp::from_second(second)
                .unwrap_or_else(|error| panic!("{second} is no instant of Nage: {error}"));
            nage_core::backup::name(stamp, 1, VERSION)
        })
        .collect();
    for name in &taken {
        plant_backup(dir.path(), name);
    }
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    assert_exit(&out, 1);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    assert_eq!(backups(dir.path()).len(), taken.len(), "the backups folder");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("backup_name_taken", transcript);

    // A change that fails after its backup and the removal of the older copy of its step.
    let dir = commands::behind();
    plant(
        &commands::database(dir.path()),
        &["CREATE TABLE accounts (planted INTEGER)"],
    );
    plant_backup(dir.path(), &older_copy());
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    assert_exit(&out, 1);
    assert_eq!(commands::schema_version(dir.path()), Some(1));
    let kept = backups(dir.path());
    assert!(
        kept.len() == 1 && !kept.contains(&older_copy()),
        "the backups after the failed change: {kept:?}"
    );
    let transcript = shown.of(&out);
    insta::assert_snapshot!("change_failed", transcript);
}

/// A start on a database one change behind writes the backup, updates the database and checks
/// the book before it listens: `backup written`, `database ready`, the check's line and only then
/// the listening line; the backup carries the step 0001 and this version in its name and passes
/// its check.
///
/// Proves: decision 160, SPEC 5.2 (the backup, the update and the check before the listening line; assumption A13)
#[test]
fn serve_backs_up_updates_and_checks_before_it_listens() {
    let (mut server, dir, shown) = server_in(commands::behind_in);
    let (lines, _log) = log_until_listening(&mut server, shown);
    let at = |what: &str, found: &dyn Fn(&str) -> bool| {
        lines
            .iter()
            .position(|line| found(line))
            .unwrap_or_else(|| panic!("no {what} line; the log:\n{}", lines.join("\n")))
    };
    let backup = at("backup written", &|line| {
        line.contains(" backup written path=")
    });
    let ready = at("database ready", &|line| line.contains(" database ready "));
    let check = at("check", &|line| line.contains(" check skipped"));
    let listening = at("listening", &|line| is_listening_line(line));
    assert!(
        backup < ready && ready < check && check < listening,
        "the order of the start; the log:\n{}",
        lines.join("\n")
    );
    let status = server.stop();
    assert!(status.success(), "the server stopped with {status}");
    let written = backups(&dir);
    assert_eq!(written.len(), 1, "the backups: {written:?}");
    let name = nage_core::backup::parse(&written[0])
        .unwrap_or_else(|| panic!("{} is not a backup's name", written[0]));
    assert_eq!((name.step, name.version.as_str()), (1, VERSION));
    let path = backups_folder(&dir).join(&written[0]);
    commands::block_on(nage_core::update::verify_backup(&path, 1))
        .unwrap_or_else(|reason| panic!("the backup did not pass its check: {reason}"));
    assert_eq!(commands::schema_version(&dir), Some(2));
}

/// A book that breaks rules is said loudly and the server serves as usual: `check failed
/// problems=2` and each broken rule's line at ERROR before the listening line, then `/health`
/// answers 200 and the server stops cleanly.
///
/// Proves: decision 160 (say so loudly and keep serving)
#[test]
fn serve_logs_broken_rules_at_error_and_keeps_serving() {
    let (mut server, _dir, shown) = server_in(commands::broken_book_in);
    let (lines, _log) = log_until_listening(&mut server, shown);
    let errors: Vec<&String> = lines
        .iter()
        .filter(|line| {
            line.get(20..)
                .is_some_and(|rest| rest.starts_with(" ERROR "))
        })
        .collect();
    assert_eq!(
        errors.len(),
        3,
        "the ERROR lines; the log:\n{}",
        lines.join("\n")
    );
    assert!(
        errors[0].ends_with(" ERROR check failed problems=2")
            && errors[1..]
                .iter()
                .all(|line| line.contains(" ERROR check: ")),
        "the ERROR lines: {errors:#?}"
    );
    let addr = server.wait_listening();
    let health = http_get(addr, "/health", &[]);
    assert_eq!(health.status, 200, "GET /health:\n{}", health.head);
    let status = server.stop();
    assert!(status.success(), "the server stopped with {status}");
}

/// A second server on a data folder a server already runs on is refused, exit 1, naming the
/// running server, and changes nothing: the first keeps serving, its mark still names it, and no
/// backup is written.
///
/// Proves: decision 153, ruling P5 (one server per data folder, the holder named)
#[test]
fn a_second_server_on_the_same_data_folder_is_refused_and_changes_nothing() {
    let (mut first, dir, shown) = server_in(|_| {});
    let (_lines, _log) = log_until_listening(&mut first, shown);
    let addr = first.wait_listening();
    let (out, shown) = start_that_fails(&dir, "127.0.0.1:0");
    assert_exit(&out, 1);
    let health = http_get(addr, "/health", &[]);
    assert_eq!(health.status, 200, "GET /health on the first server");
    assert_eq!(
        fs::read_to_string(data(&dir).join("nage.lock")).unwrap_or_default(),
        "serve",
        "the mark's file"
    );
    assert!(backups(&dir).is_empty(), "a backup was written");
    let status = first.stop();
    assert!(status.success(), "the first server stopped with {status}");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("another_server_on_the_folder", transcript);
}

/// A database a newer Nage wrote refuses to start, exit 1, without a byte written to it: with no
/// backup this version can open, what now says so; with copies in the backups folder, it names the
/// newest copy at this version's last change (2027-03-14), not a newer copy of an older step, an
/// older copy of the same step, or a copy of a step this version does not know.
///
/// Proves: decision 152, SPEC 5.5 (the older-binary message names the newest backup, exit 1)
#[test]
fn an_older_nage_names_the_newest_backup_it_can_open() {
    let dir = commands::ahead();
    let database = commands::database(dir.path());
    let before =
        fs::read(&database).unwrap_or_else(|error| panic!("cannot read the database: {error}"));
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    assert_exit(&out, 1);
    let transcript = shown.of(&out);
    insta::assert_snapshot!("older_nage_without_a_backup", transcript);

    for name in [
        "2027-01-01T00-00-00Z-pre-migrate-0002-to-0.2.5.sqlite",
        "2027-03-14T02-00-00Z-pre-migrate-0002-to-0.3.0.sqlite",
        "2027-05-01T00-00-00Z-pre-migrate-0001-to-0.3.0.sqlite",
        "2027-06-01T00-00-00Z-pre-migrate-0003-to-0.4.0.sqlite",
    ] {
        plant_backup(dir.path(), name);
    }
    let (out, shown) = start_that_fails(dir.path(), "127.0.0.1:0");
    assert_exit(&out, 1);
    assert_eq!(
        fs::read(&database).unwrap_or_else(|error| panic!("cannot read the database: {error}")),
        before,
        "the newer database was written"
    );
    assert_eq!(backups(dir.path()).len(), 4, "the backups folder");
    let transcript = shown.of(&out);
    insta::assert_snapshot!("older_nage_with_a_backup", transcript);
}

/// The commands that take no mark work beside a running server: `nage migrate --dry-run`, `nage
/// init` on the database the server created, `nage user add`, `nage user reset-password` and `nage
/// check`, each exit 0, and the server keeps serving.
///
/// Proves: decision 153 (check, init, user add, user reset-password and the dry run beside a server)
#[test]
fn the_commands_that_take_no_mark_work_beside_a_running_server() {
    let mut server = ServeProcess::spawn();
    let addr = server.wait_listening();
    let dir = server.dir().to_path_buf();
    let password = format!("{PASSWORD}\n");

    let dry_run =
        run(nage()
            .current_dir(&dir)
            .args(["migrate", "--data-dir", "data", "--dry-run"]));
    assert_exit(&dry_run, 0);
    let init = commands::command(
        &dir,
        &[
            "init",
            "--data-dir",
            "data",
            "--book-name",
            "Household",
            "--currency",
            "CAD",
            "--username",
            "sam",
            "--display-name",
            "Sam",
        ],
        Some(password.as_bytes()),
    );
    assert_exit(&init, 0);
    let add = commands::command(
        &dir,
        &[
            "user",
            "add",
            "--data-dir",
            "data",
            "--by",
            "sam",
            "--username",
            "alex",
            "--display-name",
            "Alex",
        ],
        Some(password.as_bytes()),
    );
    assert_exit(&add, 0);
    let reset = commands::command(
        &dir,
        &[
            "user",
            "reset-password",
            "--data-dir",
            "data",
            "--by",
            "sam",
            "--username",
            "alex",
        ],
        Some(password.as_bytes()),
    );
    assert_exit(&reset, 0);
    commands::assert_check_ok(&dir);

    let health = http_get(addr, "/health", &[]);
    assert_eq!(health.status, 200, "GET /health after the commands");
    let status = server.stop();
    assert!(status.success(), "the server stopped with {status}");
}

/// `nage` started through a shell under the umask `000`, as `sh -c 'umask 000 && exec "$0"
/// "$@"' nage <args>`.
fn under_a_loose_umask(dir: &Path, args: &[&OsStr]) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .env_clear()
        .current_dir(dir)
        .arg("-c")
        .arg("umask 000 && exec \"$0\" \"$@\"")
        .arg(env!("CARGO_BIN_EXE_nage"))
        .args(args);
    command
}

/// Asserts that `path` has the permission bits `want`.
#[track_caller]
fn assert_mode(path: &Path, want: u32, who: &str) {
    assert_eq!(
        mode(path),
        want,
        "the mode of {} as {who} made it",
        path.display()
    );
}

/// What Nage creates in the data folder only the account it runs as can open, whatever the umask:
/// `nage migrate`, `nage serve` and `nage init`, each started through a shell under the umask
/// `000`, create folders with mode 0700 and files with mode 0600 (the mark's file, the backups
/// folder and a backup; the data folder, the backups folder, the database and the mark's file; the
/// data folder, the backups folder and the database).
///
/// Proves: decision 154, ruling P3 (serve, nage migrate and nage init create with 0700 and 0600, each started through a shell under a loose umask)
#[test]
fn the_data_folder_nage_makes_is_private_whoever_makes_it() {
    // nage migrate on a folder that holds only a copy of the 0.1.0 book.
    let dir = commands::behind();
    let data_dir = data(dir.path());
    let out = run(&mut under_a_loose_umask(
        dir.path(),
        &[
            OsStr::new("migrate"),
            OsStr::new("--data-dir"),
            data_dir.as_os_str(),
        ],
    ));
    assert_exit(&out, 0);
    assert_mode(&data_dir.join("nage.lock"), 0o600, "nage migrate");
    assert_mode(&backups_folder(dir.path()), 0o700, "nage migrate");
    let written = backups(dir.path());
    assert_eq!(written.len(), 1, "the backups: {written:?}");
    assert_mode(
        &backups_folder(dir.path()).join(&written[0]),
        0o600,
        "nage migrate",
    );

    // nage serve on a new data folder, stopped by a taken address once the folder is made.
    let dir = TestDir::new();
    let data_dir = data(dir.path());
    let taken = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port to occupy");
    let bind = taken
        .local_addr()
        .expect("the occupied address")
        .to_string();
    let out = run(&mut under_a_loose_umask(
        dir.path(),
        &[
            OsStr::new("serve"),
            OsStr::new("--data-dir"),
            data_dir.as_os_str(),
            OsStr::new("--bind"),
            OsStr::new(&bind),
        ],
    ));
    drop(taken);
    assert_exit(&out, 1);
    assert_mode(&data_dir, 0o700, "nage serve");
    assert_mode(&backups_folder(dir.path()), 0o700, "nage serve");
    assert_mode(&commands::database(dir.path()), 0o600, "nage serve");
    assert_mode(&data_dir.join("nage.lock"), 0o600, "nage serve");

    // nage init on a new data folder.
    let dir = TestDir::new();
    let data_dir = data(dir.path());
    let password = format!("{PASSWORD}\n");
    let out = commands::run_with_stdin(
        &mut under_a_loose_umask(
            dir.path(),
            &[
                "init",
                "--data-dir",
                "data",
                "--book-name",
                "Household",
                "--currency",
                "CAD",
                "--username",
                "sam",
                "--display-name",
                "Sam",
                "--password-stdin",
            ]
            .map(OsStr::new),
        ),
        password.as_bytes(),
    );
    assert_exit(&out, 0);
    assert_mode(&data_dir, 0o700, "nage init");
    assert_mode(&backups_folder(dir.path()), 0o700, "nage init");
    assert_mode(&commands::database(dir.path()), 0o600, "nage init");
}
