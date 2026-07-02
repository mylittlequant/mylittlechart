//! Mini-ticker cache endpoints.
//!
//! - `GET /v1/mini-ticker`       — list all entries
//! - `GET /v1/mini-ticker/:key`  — single entry (404 on miss)
//!
//! Key format: `"{symbol}:{exchange_id_str}:{account_short_label}"`

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct MiniTickerEntry {
    key: String,
    last_price: f64,
    price_change_percent: f64,
    high_price: f64,
    low_price: f64,
    volume: f64,
}

#[derive(Serialize)]
struct MiniTickerListOut {
    entries: Vec<MiniTickerEntry>,
}

#[derive(Serialize)]
struct MiniTickerOut {
    key: String,
    last_price: f64,
    price_change_percent: f64,
    high_price: f64,
    low_price: f64,
    volume: f64,
}

async fn list_mini_ticker(State(s): State<AppState>) -> impl IntoResponse {
    let entries: Vec<MiniTickerEntry> = s
        .core
        .mini_ticker
        .iter()
        .map(|e| MiniTickerEntry {
            key: e.key().clone(),
            last_price: e.last_price,
            price_change_percent: e.price_change_percent,
            high_price: e.high_price,
            low_price: e.low_price,
            volume: e.volume,
        })
        .collect();
    Json(MiniTickerListOut { entries })
}

async fn get_mini_ticker(
    Path(key): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    match s.core.mini_ticker.get(&key) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(v) => Json(MiniTickerOut {
            key: key.clone(),
            last_price: v.last_price,
            price_change_percent: v.price_change_percent,
            high_price: v.high_price,
            low_price: v.low_price,
            volume: v.volume,
        })
        .into_response(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/mini-ticker", get(list_mini_ticker))
        .route("/v1/mini-ticker/:key", get(get_mini_ticker))
}
