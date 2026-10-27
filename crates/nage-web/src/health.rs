//! `GET /health` through an injectable probe (SPEC 5.2, decision 41).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::AppState;

type ProbeFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type ProbeFn = dyn Fn() -> ProbeFuture + Send + Sync;

/// Injectable check behind GET /health (SPEC 5.2, decision 41). Slice 3 wires it to the read pool.
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
