//! `GET /health` through an injectable probe (SPEC 5.2, decision 41).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use nage_core::storage::Storage;
use serde::Serialize;

use crate::AppState;

type ProbeFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type ProbeFn = dyn Fn() -> ProbeFuture + Send + Sync;

/// Injectable check behind GET /health (SPEC 5.2, decisions 41 and 55). `serve` builds it with
/// [`HealthProbe::storage`]; tests of other routes use [`HealthProbe::always`].
#[derive(Clone)]
pub struct HealthProbe(Arc<ProbeFn>);

impl HealthProbe {
    /// A probe that runs `probe` on every check.
    pub fn new<F, Fut>(probe: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        Self(Arc::new(move || -> ProbeFuture { Box::pin(probe()) }))
    }

    /// A probe that always answers `ok`.
    pub fn always(ok: bool) -> Self {
        Self::new(move || std::future::ready(ok))
    }

    /// A probe that runs `probe` on every check under `deadline`: a check that misses it
    /// logs one `warn` line naming the health probe and answers `false` (decision 55). The
    /// check is dropped at the deadline, so the request never waits out a stuck probe.
    pub fn with_deadline<F, Fut>(deadline: Duration, probe: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        Self::new(move || {
            let check = probe();
            async move {
                match tokio::time::timeout(deadline, check).await {
                    Ok(ok) => ok,
                    Err(_elapsed) => {
                        let deadline_ms = u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX);
                        tracing::warn!(deadline_ms, "health probe missed its deadline");
                        false
                    }
                }
            }
        })
    }

    /// The production probe: `storage.count_books()` under `deadline` through
    /// [`HealthProbe::with_deadline`]; healthy whenever the count succeeds, whatever the
    /// count, and a failed count logs one `warn` line with the error (SPEC 5.2, decision 55).
    /// A check cut short by the deadline logs the deadline line instead, so every failing
    /// check logs exactly one `warn` line.
    pub fn storage(storage: Storage, deadline: Duration) -> Self {
        Self::with_deadline(deadline, move || {
            let storage = storage.clone();
            async move {
                match storage.count_books().await {
                    Ok(_count) => true,
                    Err(error) => {
                        tracing::warn!(%error, "health probe failed");
                        false
                    }
                }
            }
        })
    }

    /// Runs the probe once; `true` means healthy.
    pub async fn check(&self) -> bool {
        (self.0)().await
    }
}

/// The JSON body of `/health`: `{"version":"…","ok":…}`, fields in that order. Only the
/// status code is a contract (SPEC 5.2).
#[derive(Serialize)]
pub(crate) struct HealthBody {
    version: &'static str,
    ok: bool,
}

/// Runs the probe on every request: 200 when it passes, 503 when it fails, the same body
/// shape either way.
pub(crate) async fn get_health(State(state): State<AppState>) -> (StatusCode, Json<HealthBody>) {
    let ok = state.health.check().await;
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = HealthBody {
        version: state.version,
        ok,
    };
    (status, Json(body))
}
