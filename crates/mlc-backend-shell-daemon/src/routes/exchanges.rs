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
    let exchanges: Vec<String> = s.core.exchange_symbols().exchange_labels().collect();
    Json(ExchangeListOut { exchanges })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/exchanges", get(list_exchanges))
}
