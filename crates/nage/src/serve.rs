//! `nage serve` (SPEC 5.2, decisions 45, 47, 53, 54, 113 and 151 to 160): logging to stderr in
//! the log's own line (whole-second UTC stamps, no module name), the start-up line, the data
//! folder's in-use mark, the backup and the update (one core operation, before anything listens),
//! the check of the book, logged and never stopping the start, the dummy hash (made before anything
//! listens, decision 107), the signal handlers, the listener, the router and graceful shutdown,
//! which closes the database last. `serve` hands nage-web everything it needs: the storage, the
//! clock, the hashing with SPEC 5.3's parameters, the trust-proxy setting and the configured time
//! zone; nage-web reads no environment and no clock (decisions 42, 88 and 120).

use std::io::{self, IsTerminal};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::tz::TimeZone;
use nage_core::check::{self, Outcome};
use nage_core::password::HashParams;
use nage_core::storage::{Storage, StorageOptions};
use nage_core::timestamp::{Timestamp, TimestampError};
use nage_core::update::{
    self, Holder, Missing, Update, UpdateError, UpdateReport, Updated, Written,
};
use nage_web::{AppState, Clock, Hashing, HealthProbe};
use tokio::signal::unix::{SignalKind, signal};

use crate::config::ServeConfig;
use crate::log::Stamp;
use crate::migrate::update_refused;
use crate::shape::{Doing, IF_IT_FAILS_AGAIN, Refused, shown, system_words};

#[cfg(test)]
mod tests;

/// The workspace version, reported by the start-up line and `/health`.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long `/health` waits for its count query before answering 503 (decision 55); well
/// under the two seconds `nage health` gives the whole request.
const HEALTH_DEADLINE: Duration = Duration::from_secs(1);
/// The argon2id parameters every password of this server is hashed with (SPEC 5.3, decision
/// 107): m=19456 KiB, t=2, p=1. Tests of nage-web hand a cheap set instead; this binary never
/// does.
const HASH_PARAMS: HashParams = HashParams::SPEC;

/// A runtime failure of `serve`: exit code 1 (decision 45).
#[derive(Debug)]
pub(crate) enum ServeError {
    Logging(String),
    DataDir {
        path: PathBuf,
        source: io::Error,
    },
    /// The system clock gave an instant the backup's name cannot carry.
    Clock(TimestampError),
    /// The mark, the backup or the update failed (decisions 151 to 155).
    Update(UpdateError),
    Signal(io::Error),
    Listen {
        addr: SocketAddr,
        source: io::Error,
    },
    LocalAddr(io::Error),
    Serve(io::Error),
    Hashing(nage_core::password::HashError),
}

impl ServeError {
    /// The failure as the terminal prints it (decisions 157 and 159): Nage did not start, why in
    /// plain words with the system's own at the end, and what to do.
    pub(crate) fn refused(&self) -> Refused {
        let not_started = Doing::Serve.not_done();
        let report = format!("Start Nage again; {IF_IT_FAILS_AGAIN}");
        match self {
            Self::Logging(error) => Refused::new(
                not_started,
                format!("The log could not be started ({error})."),
                report,
            ),
            Self::DataDir { path, source } => Refused::new(
                not_started,
                format!(
                    "The data folder {} could not be found ({}).",
                    shown(path),
                    system_words(source)
                ),
                "Give the data folder as an absolute path with --data-dir or NAGE_DATA_DIR.",
            ),
            Self::Clock(error) => Refused::new(
                not_started,
                format!(
                    "The system clock gave a time Nage cannot use ({}).",
                    system_words(error)
                ),
                "Set the server's clock right, then start Nage again.",
            ),
            Self::Update(error) => update_refused(Doing::Serve, VERSION, error),
            Self::Signal(source) => Refused::new(
                not_started,
                format!(
                    "The signal handlers could not be installed ({}).",
                    system_words(source)
                ),
                report,
            ),
            Self::Listen { addr, source } if source.kind() == io::ErrorKind::AddrInUse => {
                Refused::new(
                    not_started,
                    format!(
                        "The address {addr} is already in use ({}).",
                        system_words(source)
                    ),
                    "Stop the other program on that port, or give another address with --bind or NAGE_BIND.",
                )
            }
            Self::Listen { addr, source } => Refused::new(
                not_started,
                format!(
                    "Nage could not listen on {addr} ({}).",
                    system_words(source)
                ),
                "Give an address of this machine with --bind or NAGE_BIND; a port below 1024 needs rights Nage does not have.",
            ),
            Self::LocalAddr(source) => Refused::new(
                not_started,
                format!(
                    "The address Nage listens on could not be read ({}).",
                    system_words(source)
                ),
                report,
            ),
            Self::Serve(source) => Refused::new(
                "The server stopped.",
                format!("Serving failed ({}).", system_words(source)),
                report,
            ),
            Self::Hashing(source) => Refused::new(
                not_started,
                format!(
                    "The password hashing could not be prepared ({}).",
                    system_words(source)
                ),
                report,
            ),
        }
    }
}

/// Runs the server until SIGTERM or SIGINT, then stops accepting, finishes the requests in
/// flight without a deadline, closes the database, lets go of the data folder's mark and returns
/// (decision 45).
///
/// The start goes in this order (assumption A13 of PLAN M2 slice 8): the log, the start-up line,
/// the mark, the backup and the update (`nage_core::update::open_and_update`, which creates a
/// missing data folder and database), the `backup written` and `database ready` lines, the check
/// of the book with its lines (broken rules are logged at ERROR and the start goes on, decision
/// 160), then the dummy hash, the signal handlers and the listener, so no request is answered on a
/// stale schema or an unchecked book (decision 54); every failure after the open closes the
/// storage, so a start that fails leaves no `-wal` or `-shm` file behind (decision 53).
pub(crate) async fn run(config: ServeConfig) -> Result<(), ServeError> {
    let ServeConfig {
        data_dir,
        bind,
        tz,
        tz_source,
        log,
        filter,
        no_color,
        trust_proxy,
    } = config;

    let ansi = !no_color && io::stderr().is_terminal();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(ansi)
        .with_target(false)
        .with_timer(Stamp)
        .try_init()
        .map_err(|error| ServeError::Logging(error.to_string()))?;

    let data_dir = std::path::absolute(&data_dir).map_err(|source| ServeError::DataDir {
        path: data_dir.clone(),
        source,
    })?;

    let tz_name = tz.iana_name().unwrap_or_default();
    tracing::info!(
        version = %VERSION,
        bind = %bind,
        data_dir = %shown(&data_dir),
        tz = %tz_name,
        tz_source = %tz_source,
        log = %log,
        "starting"
    );

    let now = Timestamp::from_jiff(jiff::Timestamp::now()).map_err(ServeError::Clock)?;
    let updated = update::open_and_update(Update {
        data_dir: &data_dir,
        now,
        version: VERSION,
        missing: Missing::Create,
        holder: Holder::Serve,
        options: &StorageOptions::default(),
    })
    .await;
    let Updated {
        storage,
        mark,
        report,
    } = match updated {
        Ok(updated) => updated,
        Err(error) => {
            if let Some(written) = written_before(&error)
                && let Some(backup) = &written.backup
            {
                log_backup(backup, &written.removed);
            }
            return Err(ServeError::Update(error));
        }
    };
    if let Some(backup) = &report.backup {
        log_backup(backup, &report.removed);
    }
    log_ready(&report);
    log_check(&storage).await;

    let served = listen_and_serve(bind, &storage, trust_proxy, tz).await;
    // Closed on success and on failure alike, and before the stopped line: the writer
    // connection closes last and checkpoints the WAL away (decision 53). The mark goes after it.
    storage.close().await;
    drop(mark);
    served?;
    tracing::info!("stopped");
    Ok(())
}

/// What a failed update wrote and removed before it failed, which the server logs before its
/// three lines (ruling P4 of the pause): a start whose change failed, or whose other copies could
/// not all be removed; `None` for every other failure, which comes before any backup.
fn written_before(error: &UpdateError) -> Option<&Written> {
    match error {
        UpdateError::Update { written, .. } => Some(written.as_ref()),
        UpdateError::RemoveOlder { written, .. } => Some(written.as_ref()),
        _ => None,
    }
}

/// The lines of the backup (decisions 151 and 160, ruling P4 of the pause): the backup written
/// first, then each other copy of the same step removed once it passed its check.
fn log_backup(backup: &Path, removed: &[PathBuf]) {
    tracing::info!(path = %shown(backup), "backup written");
    for copy in removed {
        tracing::info!(path = %shown(copy), "older backup removed");
    }
}

/// The lines of a book that breaks rules (decision 160, ruling P4 of the pause): `check failed
/// problems=<the number of lines>` at ERROR, then `check: <line>` at ERROR for each line, word for
/// word as nage-core writes it.
fn log_broken(violations: &[nage_core::check::Violation]) {
    tracing::error!(problems = violations.len(), "check failed");
    for violation in violations {
        tracing::error!("check: {violation}");
    }
}

/// Prints a start that failed on standard error, after the log's lines, as the three lines of
/// [`ServeError::refused`], and answers exit 1 (decisions 157, 159 and 160).
pub(crate) fn failed(error: &ServeError, screen: crate::shape::Screen) -> std::process::ExitCode {
    screen.refused(&error.refused(), 1)
}

/// `database ready` with the path, the number of changes applied and the schema version, as the
/// smoke and the deployment guide read them (decision 160).
fn log_ready(report: &UpdateReport) {
    let schema_version = report.schema_version.unwrap_or(0);
    tracing::info!(
        path = %shown(&report.database),
        applied = report.applied.len(),
        schema_version,
        "database ready"
    );
}

/// Runs the check of the book in one read transaction and logs what it found (SPEC 5.2, decisions
/// 97 and 160): `check ok` with the number of rules, `check skipped` while no book is set up, or
/// `check failed` with the number of broken rules and each line at ERROR, word for word. A check
/// that cannot run is logged at ERROR too. Nothing stops the start: the household keeps its book
/// (the maintainer's pick "Say so loudly and keep serving").
async fn log_check(storage: &Storage) {
    let report = match storage.begin_read().await {
        Ok(mut tx) => {
            let report = check::run(&mut tx).await;
            let _ = tx.rollback().await;
            report.map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    match report {
        Ok(report) => match &report.outcome {
            Outcome::NotInitialized => tracing::info!("check skipped: no book is set up yet"),
            Outcome::Checked { violations } if violations.is_empty() => {
                tracing::info!(rules = report.invariants, "check ok");
            }
            Outcome::Checked { violations } => log_broken(violations),
        },
        Err(error) => tracing::error!(error = %error, "check could not run"),
    }
}

/// Makes the dummy hash, installs the signal handlers, binds `bind`, logs the listening line and
/// serves the router over `storage` until a shutdown signal arrives and the requests in flight
/// are done (decisions 54 and 160 give this order). The router is handed the storage, the system
/// clock cut to the second, the hashing with [`HASH_PARAMS`], `trust_proxy` (decision 113) and
/// `zone`, the configured time zone, in which nage-web reads today (decision 120).
async fn listen_and_serve(
    bind: SocketAddr,
    storage: &Storage,
    trust_proxy: bool,
    zone: TimeZone,
) -> Result<(), ServeError> {
    // The dummy hash before the listener, with the parameters every new hash gets (decision
    // 107): no request pays for it, and a hasher that cannot hash stops the start here.
    let hashing = Hashing::argon2id(HASH_PARAMS);
    hashing.prepare().await.map_err(ServeError::Hashing)?;

    // Registered here, before the listening line, so a signal sent once that line is logged
    // is handled: `signal` registers when called, `ctrl_c` only when first polled.
    let mut terminate = signal(SignalKind::terminate()).map_err(ServeError::Signal)?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(ServeError::Signal)?;

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|source| ServeError::Listen { addr: bind, source })?;
    let addr = listener.local_addr().map_err(ServeError::LocalAddr)?;
    tracing::info!("listening on http://{addr}");

    let app = nage_web::router(AppState {
        version: VERSION,
        health: HealthProbe::storage(storage.clone(), HEALTH_DEADLINE),
        storage: storage.clone(),
        clock: Clock::new(|| Timestamp::from_jiff(jiff::Timestamp::now())),
        hashing,
        trust_proxy,
        zone,
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = interrupt.recv() => {}
            }
            tracing::info!("shutdown signal received");
        })
        .await
        .map_err(ServeError::Serve)
}
