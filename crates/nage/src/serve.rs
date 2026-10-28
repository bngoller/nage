//! `nage serve` (SPEC 5.2, decisions 45, 47, 53 and 54): logging to stderr, the data
//! directory, the start-up line, the database (opened and migrated before anything listens),
//! the signal handlers, the listener, the router and graceful shutdown, which closes the
//! database last.

use std::fmt;
use std::io::{self, IsTerminal};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use nage_core::storage::{MigrationReport, Storage, StorageOptions};
use nage_web::{AppState, HealthProbe};
use tokio::signal::unix::{SignalKind, signal};

use crate::config::ServeConfig;

/// The workspace version, reported by the start-up line and `/health`.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The database file inside the data directory (SPEC 5.2).
const DATABASE_FILE: &str = "nage.sqlite";
/// How long `/health` waits for its count query before answering 503 (decision 55); well
/// under the two seconds `nage health` gives the whole request.
const HEALTH_DEADLINE: Duration = Duration::from_secs(1);

/// A runtime failure of `serve`: exit code 1 (decision 45).
#[derive(Debug)]
pub(crate) enum ServeError {
    Logging(String),
    DataDir {
        path: PathBuf,
        source: io::Error,
    },
    Open {
        path: PathBuf,
        source: nage_core::storage::Error,
    },
    Migrate {
        path: PathBuf,
        source: nage_core::storage::Error,
    },
    Signal(io::Error),
    Listen {
        addr: SocketAddr,
        source: io::Error,
    },
    LocalAddr(io::Error),
    Serve(io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Logging(error) => write!(f, "cannot start logging: {error}"),
            Self::DataDir { path, source } => {
                write!(
                    f,
                    "cannot create data directory {}: {source}",
                    path.display()
                )
            }
            Self::Open { path, source } => {
                write!(f, "cannot open database {}: {source}", path.display())
            }
            Self::Migrate { path, source } => {
                write!(f, "cannot migrate database {}: {source}", path.display())
            }
            Self::Signal(source) => write!(f, "cannot install the signal handlers: {source}"),
            Self::Listen { addr, source } => write!(f, "cannot listen on {addr}: {source}"),
            Self::LocalAddr(source) => write!(f, "cannot read the listening address: {source}"),
            Self::Serve(source) => write!(f, "server failed: {source}"),
        }
    }
}

/// Runs the server until SIGTERM or SIGINT, then stops accepting, finishes the requests in
/// flight without a deadline, closes the database and returns (decision 45).
///
/// The database is opened and migrated before the signal handlers and the listener, so no
/// request is answered on a stale schema (decision 54); every failure after the open closes
/// the storage, so a start that fails leaves no `-wal` or `-shm` file behind (decision 53).
pub(crate) async fn run(config: ServeConfig) -> Result<(), ServeError> {
    let ServeConfig {
        data_dir,
        bind,
        tz,
        tz_source,
        log,
        filter,
        no_color,
    } = config;

    let ansi = !no_color && io::stderr().is_terminal();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(ansi)
        .try_init()
        .map_err(|error| ServeError::Logging(error.to_string()))?;

    let data_dir = std::path::absolute(&data_dir).map_err(|source| ServeError::DataDir {
        path: data_dir.clone(),
        source,
    })?;
    std::fs::create_dir_all(data_dir.join("backups")).map_err(|source| ServeError::DataDir {
        path: data_dir.clone(),
        source,
    })?;

    let tz_name = tz.iana_name().unwrap_or_default();
    tracing::info!(
        version = %VERSION,
        bind = %bind,
        data_dir = %data_dir.display(),
        tz = %tz_name,
        tz_source = %tz_source,
        log = %log,
        "starting"
    );

    let path = data_dir.join(DATABASE_FILE);
    let storage = Storage::open(&path, &StorageOptions::default())
        .await
        .map_err(|source| ServeError::Open {
            path: path.clone(),
            source,
        })?;
    let MigrationReport {
        applied,
        schema_version,
    } = match storage.migrate().await {
        Ok(report) => report,
        Err(source) => {
            storage.close().await;
            return Err(ServeError::Migrate { path, source });
        }
    };
    tracing::info!(
        path = %path.display(),
        applied,
        schema_version,
        "database ready"
    );

    let served = listen_and_serve(bind, &storage).await;
    // Closed on success and on failure alike, and before the stopped line: the writer
    // connection closes last and checkpoints the WAL away (decision 53).
    storage.close().await;
    served?;
    tracing::info!("stopped");
    Ok(())
}

/// Installs the signal handlers, binds `bind`, logs the listening line and serves the router
/// over `storage` until a shutdown signal arrives and the requests in flight are done.
async fn listen_and_serve(bind: SocketAddr, storage: &Storage) -> Result<(), ServeError> {
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
