//! `GET /api/v1/health` — liveness check.

use std::sync::Arc;
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::AgentState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: String,
    uptime_secs: u64,
}

async fn health(State(state): State<Arc<AgentState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: state.version.clone(),
        uptime_secs: state.start_time.elapsed().as_secs(),
    })
}

/// Build the health routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route("/api/v1/health", get(health))
}
