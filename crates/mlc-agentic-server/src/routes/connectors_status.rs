//! Exchange connector status endpoint.
//!
//! Returns the current connection status of all configured exchange connectors.
//! The data is sourced from [`crate::AgentState::connector_snapshot`], which
//! is updated periodically by the main thread.
//!
//! Routes:
//! - `GET /api/v1/connectors` — return status of all exchange connectors

use std::sync::Arc;
use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::state::ConnectorSnapshot;
use crate::AgentState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/v1/connectors` — return status of all exchange connectors.
async fn get_connectors(
    State(state): State<Arc<AgentState>>,
) -> Result<Json<ConnectorSnapshot>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.connector_snapshot.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "connector snapshot lock poisoned".to_string(),
            }),
        )
    })?;

    Ok(Json(snap.clone()))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the connectors status routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route("/api/v1/connectors", get(get_connectors))
}
