//! Watchlist discovery endpoints.
//!
//! Returns all user-configured watchlists along with their symbol entries.
//! The data is sourced from [`crate::AgentState::watchlist_snapshot`], which
//! is updated by the main thread whenever watchlists change.
//!
//! Routes:
//! - `GET /api/v1/watchlists` — return all watchlists with their items

use std::sync::Arc;
use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::state::WatchlistSnapshot;
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

/// `GET /api/v1/watchlists` — return all watchlists with their symbol entries.
async fn get_watchlists(
    State(state): State<Arc<AgentState>>,
) -> Result<Json<WatchlistSnapshot>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.watchlist_snapshot.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "watchlist snapshot lock poisoned".to_string(),
            }),
        )
    })?;

    Ok(Json(snap.clone()))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the watchlists routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route("/api/v1/watchlists", get(get_watchlists))
}
