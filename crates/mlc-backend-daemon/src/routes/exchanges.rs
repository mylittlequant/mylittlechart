//! Exchange symbols endpoints.
//!
//! - `GET /v1/exchanges` — list exchange ids present in the symbol cache

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct ExchangeListOut {
    exchanges: Vec<String>,
}

async fn list_exchanges(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.exchange_symbols_read();
    let exchanges: Vec<String> = guard.keys().map(|e| e.as_str().to_string()).collect();
    Json(ExchangeListOut { exchanges })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/exchanges", get(list_exchanges))
}
