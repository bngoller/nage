//! Process-level helpers for the `nage` binary tests (milestone 1 slice 2).
//!
//! Every spawned child lives in a guard whose `Drop` kills then waits, every wait has a
//! deadline, and nothing here reads or changes the test process's own environment: each
//! child starts from `env_clear()` and gets only the variables a test names.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nage_web::{AppState, HealthProbe};

/// The workspace version, which `nage --version`, the start-up line and `/health` report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `run` kills a child that has not exited by then.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for the start-up line, the listening line and the data directory.
pub const LISTEN_TIMEOUT: Duration = Duration::from_secs(15);
/// Deadline for any other stderr line.
pub const LINE_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for a server to exit after a signal or a start-up failure.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound for the paths that must not wait on the two-second health deadline.
pub const FAST: Duration = Duration::from_millis(1900);
const POLL: Duration = Duration::from_millis(10);

/// The `nage` binary with an empty environment and stdin closed.
pub fn nage() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nage"));
    command.env_clear().stdin(Stdio::null());
    command
}

/// `NAME="value" … nage <args>`, for assertion messages.
pub fn describe(command: &Command) -> String {
    let mut text = String::new();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            let _ = write!(
                text,
                "{}={:?} ",
                key.to_string_lossy(),
                value.to_string_lossy()
            );
        }
    }
    text.push_str("nage");
    for arg in command.get_args() {
        let _ = write!(text, " {}", arg.to_string_lossy());
    }
    text
}

/// A child that ran to completion.
pub struct Finished {
    /// The command line, for messages.
    pub what: String,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    /// From spawn to the observed exit.
    pub elapsed: Duration,
}

impl Finished {
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// The first stderr line that starts with `prefix`.
    pub fn stderr_line_starting(&self, prefix: &str) -> Option<&str> {
        self.stderr.lines().find(|line| line.starts_with(prefix))
    }
}

/// Runs `command` to completion with stdout and stderr captured; kills it and panics if it
/// has not exited within `RUN_TIMEOUT`.
#[track_caller]
pub fn run(command: &mut Command) -> Finished {
    let what = describe(command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("cannot spawn `{what}`: {error}"));
    let stdout = read_all(child.stdout.take().expect("stdout is piped"));
    let stderr = read_all(child.stderr.take().expect("stderr is piped"));
    let deadline = start + RUN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let stderr = join_text(stderr);
                panic!(
                    "`{what}` did not exit within {RUN_TIMEOUT:?} and was killed; stderr:\n{stderr}"
                );
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("cannot wait for `{what}`: {error}");
            }
        }
    };
    let elapsed = start.elapsed();
    Finished {
        what,
        status,
        stdout: join_text(stdout),
        stderr: join_text(stderr),
        elapsed,
    }
}

fn read_all(mut source: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = source.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn join_text(reader: JoinHandle<String>) -> String {
    reader
        .join()
        .unwrap_or_else(|_| String::from("<output reader panicked>"))
}

/// Asserts that `out` is a configuration error (exit 2, decision 45) with a stderr line that
/// starts with `prefix`, and returns that line.
#[track_caller]
pub fn config_error_line<'a>(out: &'a Finished, prefix: &str) -> &'a str {
    assert_eq!(
        out.code(),
        Some(2),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let line = out.stderr_line_starting(prefix);
    assert!(
        line.is_some(),
        "`{}`: no stderr line starts with {prefix:?}; stderr:\n{}",
        out.what,
        out.stderr
    );
    line.unwrap_or_default()
}

/// Asserts a configuration error whose line starts with `prefix` (the source) and names
/// `value` (decision 42).
#[track_caller]
pub fn assert_config_error(out: &Finished, prefix: &str, value: &str) {
    let line = config_error_line(out, prefix);
    assert!(
        line.contains(value),
        "`{}`: the error line does not name the value {value:?}: {line}",
        out.what
    );
}

/// Asserts a runtime failure (exit 1, decision 45) with a stderr line that starts with
/// `prefix` and names `needle`. Only the error line is searched: the start-up line also
/// names the bind address and the data directory.
#[track_caller]
pub fn assert_runtime_error(out: &Finished, prefix: &str, needle: &str) {
    assert_eq!(
        out.code(),
        Some(1),
        "exit code of `{}` ({}); stderr:\n{}",
        out.what,
        out.status,
        out.stderr
    );
    let line = out.stderr_line_starting(prefix);
    assert!(
        line.is_some_and(|line| line.contains(needle)),
        "`{}`: no stderr line starts with {prefix:?} and names {needle:?}; stderr:\n{}",
        out.what,
        out.stderr
    );
}

/// The value of the whitespace-separated `key=value` token in a log line.
pub fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key)?.strip_prefix('='))
}

/// True when the log line carries the token `key=value`.
pub fn has_field(line: &str, key: &str, value: &str) -> bool {
    field(line, key) == Some(value)
}

/// The start-up event: message `starting` with a `version` field (decision 45).
pub fn is_starting_line(line: &str) -> bool {
    line.split_whitespace().any(|token| token == "starting") && field(line, "version").is_some()
}

/// A request event for `method` and `path` with a status and an integer `latency_ms`
/// (decision 47).
pub fn is_request_line(line: &str, method: &str, path: &str) -> bool {
    has_field(line, "method", method)
        && has_field(line, "path", path)
        && field(line, "status").is_some()
        && field(line, "latency_ms").is_some_and(|value| value.parse::<u64>().is_ok())
}

/// The migration event: message `database ready` with an `applied` field (decision 54). The
/// `path` and `schema_version` fields are asserted by the tests, with their own messages.
pub fn is_database_ready_line(line: &str) -> bool {
    line.contains("database ready") && field(line, "applied").is_some()
}

/// The listening line (decision 45).
pub fn is_listening_line(line: &str) -> bool {
    line.contains("listening on http://")
}

/// Asserts that neither `<db>-wal` nor `<db>-shm` exists next to the database file `db`: a
/// clean close checkpoints the WAL and removes both (decision 53). `when` names the moment
/// in the message.
#[track_caller]
pub fn assert_no_wal_files(db: &Path, when: &str) {
    for suffix in ["-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        let side = PathBuf::from(name);
        assert!(
            !side.exists(),
            "the SQLite side file {} exists {when}",
            side.display()
        );
    }
}

/// A `nage serve` invocation with the slice-2 test defaults: a fresh temporary working
/// directory, `--bind 127.0.0.1:0` and `--data-dir <dir>/data`. Tests override only what
/// they test.
pub struct ServeCommand {
    dir: TempDir,
    bind: Option<String>,
    data_dir: Option<OsString>,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

/// `nage serve` with the defaults.
pub fn serve() -> ServeCommand {
    let dir = TempDir::new();
    let data_dir = dir.path().join("data").into_os_string();
    ServeCommand {
        dir,
        bind: Some(String::from("127.0.0.1:0")),
        data_dir: Some(data_dir),
        args: Vec::new(),
        envs: Vec::new(),
    }
}

impl ServeCommand {
    /// The working directory the server starts in.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Replaces the default `--bind` value.
    pub fn bind(mut self, address: &str) -> Self {
        self.bind = Some(address.to_owned());
        self
    }

    /// Passes no `--bind` flag.
    pub fn without_bind(mut self) -> Self {
        self.bind = None;
        self
    }

    /// Replaces the default `--data-dir` value.
    pub fn data_dir(mut self, path: impl AsRef<OsStr>) -> Self {
        self.data_dir = Some(path.as_ref().to_owned());
        self
    }

    /// Passes no `--data-dir` flag.
    pub fn without_data_dir(mut self) -> Self {
        self.data_dir = None;
        self
    }

    /// Appends arguments after the defaults.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        self
    }

    /// Sets one variable in the otherwise empty environment.
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.envs
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    pub fn command(&self) -> Command {
        let mut command = nage();
        command.arg("serve");
        if let Some(bind) = &self.bind {
            command.arg("--bind").arg(bind);
        }
        if let Some(data_dir) = &self.data_dir {
            command.arg("--data-dir").arg(data_dir);
        }
        command.args(&self.args);
        for (key, value) in &self.envs {
            command.env(key, value);
        }
        command.current_dir(self.dir.path());
        command
    }

    /// Runs the server to completion, for start-up failures.
    pub fn run(&self) -> Finished {
        run(&mut self.command())
    }

    /// Starts the server in a guard.
    pub fn spawn(self) -> ServeProcess {
        ServeProcess::start(self)
    }
}

/// A running `nage serve`. stderr is read line by line on a thread; every line seen is
/// kept for later assertions. `Drop` kills the process, then waits for it.
pub struct ServeProcess {
    child: Child,
    what: String,
    lines: Receiver<String>,
    seen: Vec<String>,
    closed: bool,
    status: Option<ExitStatus>,
    stdout: Option<JoinHandle<String>>,
    dir: TempDir,
}

impl ServeProcess {
    /// `nage serve` with the defaults only.
    pub fn spawn() -> Self {
        serve().spawn()
    }

    fn start(config: ServeCommand) -> Self {
        let mut command = config.command();
        let what = describe(&command);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("cannot spawn `{what}`: {error}"));
        let stdout = read_all(child.stdout.take().expect("stdout is piped"));
        let stderr = child.stderr.take().expect("stderr is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buffer = Vec::new();
            loop {
                buffer.clear();
                match reader.read_until(b'\n', &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buffer)
                            .trim_end_matches(['\r', '\n'])
                            .to_owned();
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            child,
            what,
            lines,
            seen: Vec::new(),
            closed: false,
            status: None,
            stdout: Some(stdout),
            dir: config.dir,
        }
    }

    /// The working directory the server runs in.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Every stderr line seen so far.
    pub fn lines(&mut self) -> &[String] {
        self.drain();
        &self.seen
    }

    /// Waits for the start-up event and returns its line.
    #[track_caller]
    pub fn wait_starting(&mut self) -> String {
        self.wait_for_line_within("the starting line", LISTEN_TIMEOUT, is_starting_line)
    }

    /// Waits for the `database ready` event (decision 54) and returns its line. The schema
    /// must be current before any request is answered, so a listening line seen first is a
    /// failure, reported at once instead of after the deadline.
    #[track_caller]
    pub fn wait_database_ready(&mut self) -> String {
        let line = self.wait_for_line_within("the database-ready line", LISTEN_TIMEOUT, |line| {
            is_database_ready_line(line) || is_listening_line(line)
        });
        assert!(
            is_database_ready_line(&line),
            "nage serve logged the listening line before the database-ready line; `{}`; stderr:\n{}",
            self.what,
            self.stderr_text()
        );
        line
    }

    /// Waits for `listening on http://<addr>` and returns the address.
    #[track_caller]
    pub fn wait_listening(&mut self) -> SocketAddr {
        const MARK: &str = "listening on http://";
        let line = self.wait_for_line_within("the listening line", LISTEN_TIMEOUT, |line| {
            line.contains(MARK)
        });
        let rest = line.split_once(MARK).map_or("", |(_, rest)| rest);
        let address = rest.split_whitespace().next().unwrap_or_default();
        address.parse().unwrap_or_else(|error| {
            panic!("the listening line names no socket address ({error}): {line}")
        })
    }

    /// Waits up to `LINE_TIMEOUT` for a stderr line, seen before or after this call, that
    /// `matches` accepts; `what` names it in the failure message.
    #[track_caller]
    pub fn wait_for_line(&mut self, what: &str, matches: impl Fn(&str) -> bool) -> String {
        self.wait_for_line_within(what, LINE_TIMEOUT, matches)
    }

    #[track_caller]
    fn wait_for_line_within(
        &mut self,
        what: &str,
        timeout: Duration,
        matches: impl Fn(&str) -> bool,
    ) -> String {
        let deadline = Instant::now() + timeout;
        let mut checked = 0;
        loop {
            if let Some(line) = self.seen[checked..].iter().find(|line| matches(line)) {
                return line.clone();
            }
            checked = self.seen.len();
            if self.closed {
                let status = self.status_within(Duration::from_secs(2));
                panic!(
                    "nage serve exited before {what} ({status}); `{}`; stderr:\n{}",
                    self.what,
                    self.stderr_text()
                );
            }
            let now = Instant::now();
            if now >= deadline {
                panic!(
                    "nage serve did not print {what} within {timeout:?}; `{}`; stderr:\n{}",
                    self.what,
                    self.stderr_text()
                );
            }
            match self.lines.recv_timeout(deadline - now) {
                Ok(line) => self.seen.push(line),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
            }
        }
    }

    /// Waits until `path` is a directory; panics if the server exits first.
    #[track_caller]
    pub fn wait_for_dir(&mut self, path: &Path) {
        let deadline = Instant::now() + LISTEN_TIMEOUT;
        loop {
            if path.is_dir() {
                return;
            }
            if let Some(status) = self.poll_exit() {
                if path.is_dir() {
                    return;
                }
                self.drain_until_closed();
                panic!(
                    "nage serve exited without creating {} ({status}); `{}`; stderr:\n{}",
                    path.display(),
                    self.what,
                    self.stderr_text()
                );
            }
            if Instant::now() >= deadline {
                self.drain();
                panic!(
                    "nage serve did not create {} within {LISTEN_TIMEOUT:?}; `{}`; stderr:\n{}",
                    path.display(),
                    self.what,
                    self.stderr_text()
                );
            }
            self.drain();
            thread::sleep(POLL);
        }
    }

    /// Sends `SIG<name>` (`TERM`, `INT`) through `kill -s`.
    #[track_caller]
    pub fn signal(&mut self, name: &str) {
        if let Some(status) = self.poll_exit() {
            self.drain_until_closed();
            panic!(
                "nage serve had already exited ({status}) before SIG{name}; `{}`; stderr:\n{}",
                self.what,
                self.stderr_text()
            );
        }
        let pid = self.child.id().to_string();
        let status = Command::new("kill")
            .args(["-s", name, pid.as_str()])
            .status()
            .unwrap_or_else(|error| panic!("cannot run `kill -s {name} {pid}`: {error}"));
        assert!(status.success(), "`kill -s {name} {pid}` failed: {status}");
    }

    /// Waits up to `EXIT_TIMEOUT` for the server to exit and returns its status.
    #[track_caller]
    pub fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.poll_exit() {
                self.drain_until_closed();
                return status;
            }
            if Instant::now() >= deadline {
                self.drain();
                panic!(
                    "nage serve did not exit within {EXIT_TIMEOUT:?}; `{}`; stderr:\n{}",
                    self.what,
                    self.stderr_text()
                );
            }
            self.drain();
            thread::sleep(POLL);
        }
    }

    /// SIGTERM, then `wait_exit`.
    #[track_caller]
    pub fn stop(&mut self) -> ExitStatus {
        self.signal("TERM");
        self.wait_exit()
    }

    /// Everything the server wrote to stdout; only after it exited.
    #[track_caller]
    pub fn stdout(&mut self) -> String {
        assert!(
            self.status.is_some(),
            "stdout of `{}` read before it exited",
            self.what
        );
        let reader = self.stdout.take();
        assert!(reader.is_some(), "stdout of `{}` read twice", self.what);
        reader.map(join_text).unwrap_or_default()
    }

    fn poll_exit(&mut self) -> Option<ExitStatus> {
        if self.status.is_none() {
            self.status = self
                .child
                .try_wait()
                .unwrap_or_else(|error| panic!("cannot poll `{}`: {error}", self.what));
        }
        self.status
    }

    fn status_within(&mut self, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.poll_exit() {
                return status.to_string();
            }
            if Instant::now() >= deadline {
                return String::from("stderr closed, process still running");
            }
            thread::sleep(POLL);
        }
    }

    fn drain(&mut self) {
        loop {
            match self.lines.try_recv() {
                Ok(line) => self.seen.push(line),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    break;
                }
            }
        }
    }

    fn drain_until_closed(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.closed {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match self.lines.recv_timeout(deadline - now) {
                Ok(line) => self.seen.push(line),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
            }
        }
    }

    fn stderr_text(&self) -> String {
        self.seen.join("\n")
    }
}

impl Drop for ServeProcess {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// An HTTP server on its own thread with a current-thread runtime, for `nage health` to
/// probe. Dropping it stops the server.
pub struct HealthServer {
    addr: SocketAddr,
    calls: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl HealthServer {
    /// The real `nage_web::router` with a probe that answers `ok` and counts its calls.
    pub fn start(ok: bool) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let health = HealthProbe::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            std::future::ready(ok)
        });
        let app = nage_web::router(AppState {
            version: VERSION,
            health,
        });
        Self::serve(app, calls)
    }

    /// An empty router: every path, `/health` included, answers 404.
    pub fn start_empty() -> Self {
        Self::serve(axum::Router::new(), Arc::new(AtomicUsize::new(0)))
    }

    fn serve(app: axum::Router, calls: Arc<AtomicUsize>) -> Self {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind a loopback port for the health server");
        listener
            .set_nonblocking(true)
            .expect("make the health server listener non-blocking");
        let addr = listener.local_addr().expect("health server address");
        let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime for the health server");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("tokio listener for the health server");
                let server = axum::serve(listener, app).into_future();
                tokio::select! {
                    result = server => result.expect("health server failed"),
                    _ = stopped => {}
                }
            });
        });
        Self {
            addr,
            calls,
            shutdown: Some(shutdown),
            thread: Some(thread),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many times the probe ran.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Drop for HealthServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A listener that is never accepted: connections complete in the kernel backlog and no
/// byte is ever answered.
pub struct BlackHole {
    listener: TcpListener,
}

impl BlackHole {
    pub fn new() -> Self {
        Self {
            listener: TcpListener::bind("127.0.0.1:0")
                .expect("bind a loopback port for the black hole"),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.listener.local_addr().expect("black hole address")
    }
}

impl Default for BlackHole {
    fn default() -> Self {
        Self::new()
    }
}

/// A loopback port nothing listens on (bound, read, released).
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    listener.local_addr().expect("loopback address").port()
}

/// A fresh directory under the system temporary directory, unique by process id, counter
/// and clock, with symlinks resolved so it compares equal to what a child reports as its
/// working directory. Removed on drop unless the test is panicking (kept for inspection).
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let base =
            std::path::absolute(std::env::temp_dir()).expect("absolute system temporary directory");
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        let created = base.join(format!("nage-test-{}-{count}-{nanos}", std::process::id()));
        std::fs::create_dir(&created)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", created.display()));
        let path = std::fs::canonicalize(&created)
            .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", created.display()));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for TempDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// One HTTP/1.1 answer, read to the end of the connection.
pub struct HttpResponse {
    pub status: u16,
    /// Status line and headers.
    pub head: String,
    pub body: Vec<u8>,
}

/// `GET <target>` over a plain `TcpStream` with `Connection: close` and `extra` headers.
#[track_caller]
pub fn http_get(addr: SocketAddr, target: &str, extra: &[(&str, &str)]) -> HttpResponse {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("cannot connect to {addr}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set the read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("set the write timeout");
    let mut request = format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in extra {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|error| panic!("cannot send GET {target} to {addr}: {error}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .unwrap_or_else(|error| panic!("no complete answer to GET {target} from {addr}: {error}"));
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| {
            panic!(
                "GET {target} on {addr}: no end of headers in {:?}",
                String::from_utf8_lossy(&raw)
            )
        });
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("GET {target} on {addr}: no status code in {head:?}"));
    HttpResponse { status, head, body }
}
