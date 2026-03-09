//! `GET /api/v1/bars` — return cached OHLCV bars for a given exchange/symbol/timeframe.
//!
//! Query parameters:
//! - `exchange`   — exchange identifier string (e.g. `"binance"`)
//! - `symbol`     — trading pair (e.g. `"BTCUSDT"`)
//! - `timeframe`  — timeframe name (e.g. `"1h"`)
//! - `limit`      — optional maximum number of bars to return (default: all)

use std::sync::Arc;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use live_data::ExchangeId;

use crate::AgentState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct BarsQuery {
    exchange: String,
    symbol: String,
    timeframe: String,
    /// Optional cap on the number of bars returned (most-recent bars are kept).
    limit: Option<usize>,
}

#[derive(Serialize)]
struct BarDto {
    /// Unix timestamp in seconds.
    t: i64,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
    v: f64,
}

#[derive(Serialize)]
struct BarsResponse {
    exchange: String,
    symbol: String,
    timeframe: String,
    count: usize,
    bars: Vec<BarDto>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn get_bars(
    State(state): State<Arc<AgentState>>,
    Query(q): Query<BarsQuery>,
) -> Result<Json<BarsResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Resolve the exchange id string → ExchangeId enum variant.
    let exchange_id = ExchangeId::from_str(&q.exchange).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("unknown exchange: {}", q.exchange),
            }),
        )
    })?;

    // Look up bars in the bridge cache.
    let bars = state
        .bridge
        .get_cached_bars(&exchange_id, &q.symbol, &q.timeframe)
        .unwrap_or_default();

    // Apply optional limit (keep the most-recent `limit` bars).
    let bars = match q.limit {
        Some(n) if n < bars.len() => bars[bars.len() - n..].to_vec(),
        _ => bars,
    };

    let count = bars.len();
    let dtos: Vec<BarDto> = bars
        .into_iter()
        .map(|b| BarDto {
            t: b.timestamp,
            o: b.open,
            h: b.high,
            l: b.low,
            c: b.close,
            v: b.volume,
        })
        .collect();

    Ok(Json(BarsResponse {
        exchange: q.exchange,
        symbol: q.symbol,
        timeframe: q.timeframe,
        count,
        bars: dtos,
    }))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the bars routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route("/api/v1/bars", get(get_bars))
}
