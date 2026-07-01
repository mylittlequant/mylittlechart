//! Trading read-only endpoints.
//!
//! - `GET /v1/trading/connectors` — list trading connector sessions

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct ConnectorsOut {
    connectors: Vec<ConnectorEntry>,
}

#[derive(Serialize)]
struct ConnectorEntry {
    exchange_id: String,
    account_type: String,
    is_paper: bool,
}

async fn list_connectors(State(s): State<AppState>) -> impl IntoResponse {
    let connectors = match s.core.trading_lock() {
        None => Vec::new(),
        Some(guard) => {
            // TradingManager.snapshot() returns Arc<RwLock<TradingSnapshot>>.
            // TradingSnapshot.session is a single Option<SessionInfo>.
            // Return it as a single-element list (or empty if no session).
            let snap = guard.snapshot();
            let read = snap.read();
            match read {
                Err(_) => Vec::new(),
                Ok(g) => match &g.session {
                    None => Vec::new(),
                    Some(si) => vec![ConnectorEntry {
                        exchange_id: si.exchange_id.as_str().to_string(),
                        account_type: si.account_type.short_label().to_string(),
                        is_paper: si.is_paper,
                    }],
                },
            }
        }
    };
    Json(ConnectorsOut { connectors })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/trading/connectors", get(list_connectors))
}
