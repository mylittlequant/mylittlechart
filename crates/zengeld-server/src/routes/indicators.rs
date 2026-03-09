//! `GET /api/v1/indicators` — return the current indicator snapshot.
//!
//! Optional query parameter:
//! - `symbol` — filter to a single symbol (e.g. `"BTCUSDT"`)

use std::sync::Arc;
use axum::{extract::{Query, State}, routing::get, Json, Router};
use serde::Deserialize;

use crate::AgentState;

// ---------------------------------------------------------------------------
// Request type
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct IndicatorsQuery {
    symbol: Option<String>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn get_indicators(
    State(state): State<Arc<AgentState>>,
    Query(q): Query<IndicatorsQuery>,
) -> Json<serde_json::Value> {
    let snapshot = state
        .indicator_snapshot
        .read()
        .expect("indicator_snapshot RwLock poisoned");

    if let Some(ref symbol) = q.symbol {
        let instances = snapshot.symbols.get(symbol).cloned().unwrap_or_default();
        Json(serde_json::json!({
            "symbol": symbol,
            "indicators": instances,
        }))
    } else {
        Json(serde_json::to_value(&*snapshot).unwrap_or_else(|_| serde_json::Value::Null))
    }
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the indicators routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route("/api/v1/indicators", get(get_indicators))
}
