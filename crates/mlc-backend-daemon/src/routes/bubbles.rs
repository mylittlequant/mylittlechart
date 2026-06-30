//! Bubble read + write endpoints.
//!
//! - `GET  /v1/bubbles`              — list bubble ids
//! - `POST /v1/bubbles`              — create a bubble
//! - `GET  /v1/bubbles/:bid`         — full bubble snapshot DTO
//! - `DELETE /v1/bubbles/:bid`       — remove a bubble
//! - `GET  /v1/bubbles/:bid/bars`    — OHLCV bars array (limit query param)
//! - `GET  /v1/bubble/:bid/bars/:kind` — legacy count endpoint (preserved)

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_core::ids::BubbleId;
use mlc_core::state::Timeframe;

use crate::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn timeframe_ms(name: &str) -> u64 {
    match name {
        "1m" => 60_000,
        "3m" => 180_000,
        "5m" => 300_000,
        "15m" => 900_000,
        "30m" => 1_800_000,
        "1h" | "1H" => 3_600_000,
        "2h" | "2H" => 7_200_000,
        "4h" | "4H" => 14_400_000,
        "6h" | "6H" => 21_600_000,
        "8h" | "8H" => 28_800_000,
        "12h" | "12H" => 43_200_000,
        "1d" | "1D" => 86_400_000,
        "3d" | "3D" => 259_200_000,
        "1w" | "1W" => 604_800_000,
        "1M" => 2_592_000_000,
        other => {
            // Try numeric prefix: "60" → 60 000 ms, "3600" → 3_600_000 ms
            if let Ok(n) = other.trim_end_matches(|c: char| !c.is_numeric()).parse::<u64>() {
                n * 1_000
            } else {
                60_000
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct BubbleListOut {
    bubble_ids: Vec<u64>,
}

#[derive(Serialize)]
struct BubbleSnapshotOut {
    id: u64,
    symbol: String,
    exchange: String,
    account_type: String,
    timeframe_name: String,
    bars_len: usize,
    primitives_count: usize,
    indicator_configs_count: usize,
    members_count: usize,
    auto_created: bool,
    scale_mode: String,
}

#[derive(Serialize)]
struct CreateBubbleOut {
    bid: u64,
}

#[derive(Deserialize)]
struct CreateBubbleIn {
    symbol: String,
    exchange: String,
    #[serde(default = "default_account_type")]
    account_type: String,
    timeframe: String,
    #[serde(default = "default_color")]
    color: [f32; 4],
}

fn default_account_type() -> String {
    "S".to_string()
}

fn default_color() -> [f32; 4] {
    [0.2, 0.6, 1.0, 1.0]
}

#[derive(Serialize)]
struct BarDto {
    ts: i64,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
    v: f64,
}

#[derive(Serialize)]
struct BarsOut {
    bid: u64,
    count: usize,
    bars: Vec<BarDto>,
}

#[derive(Deserialize)]
struct BarsQuery {
    limit: Option<usize>,
}

// Legacy count DTO (backwards compat for existing callers of /v1/bubble/:bid/bars/:kind)
#[derive(Serialize)]
struct BarsCountOut {
    bid: u64,
    kind: String,
    count: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_bubbles(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.bubble();
    let bubble_ids: Vec<u64> = guard.bubbles().map(|b| b.id.0).collect();
    Json(BubbleListOut { bubble_ids })
}

async fn create_bubble(
    State(s): State<AppState>,
    Json(body): Json<CreateBubbleIn>,
) -> impl IntoResponse {
    let ms = timeframe_ms(&body.timeframe);
    let tf = Timeframe::new(&body.timeframe, ms as i64);
    let bid = {
        let mut mgr = s.core.bubble_mut();
        mgr.create_group_auto(body.color, body.symbol, tf)
    };
    // Set exchange + account_type on the newly created bubble.
    {
        let mut mgr = s.core.bubble_mut();
        if let Some(bubble) = mgr.bubble_mut(bid) {
            bubble.exchange = body.exchange;
            bubble.account_type = body.account_type;
        }
    }
    (StatusCode::CREATED, Json(CreateBubbleOut { bid: bid.0 }))
}

async fn get_bubble(
    Path(bid): Path<u64>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid_typed = BubbleId(bid);
    let guard = s.core.bubble();
    match guard.bubble(bid_typed) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(b) => {
            let out = BubbleSnapshotOut {
                id: b.id.0,
                symbol: b.symbol.clone(),
                exchange: b.exchange.clone(),
                account_type: b.account_type.clone(),
                timeframe_name: b.timeframe.name.clone(),
                bars_len: b.bars.len(),
                primitives_count: b.primitives.len(),
                indicator_configs_count: b.indicator_configs.len(),
                members_count: b.members.len(),
                auto_created: b.auto_created,
                scale_mode: format!("{:?}", b.scale_mode),
            };
            Json(out).into_response()
        }
    }
}

async fn delete_bubble(
    Path(bid): Path<u64>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid_typed = BubbleId(bid);
    let exists = s.core.bubble().bubble(bid_typed).is_some();
    if !exists {
        return StatusCode::NOT_FOUND;
    }
    s.core.bubble_mut().remove_group(bid_typed);
    StatusCode::NO_CONTENT
}

async fn get_bubble_bars(
    Path(bid): Path<u64>,
    Query(q): Query<BarsQuery>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid_typed = BubbleId(bid);
    let guard = s.core.bubble();
    match guard.bubble(bid_typed) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(b) => {
            const DEFAULT_LIMIT: usize = 200;
            const MAX_LIMIT: usize = 5000;
            let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
            let bars_slice = if b.bars.len() > limit {
                &b.bars[b.bars.len() - limit..]
            } else {
                &b.bars
            };
            let dtos: Vec<BarDto> = bars_slice
                .iter()
                .map(|bar| BarDto {
                    ts: bar.timestamp,
                    o: bar.open,
                    h: bar.high,
                    l: bar.low,
                    c: bar.close,
                    v: bar.volume,
                })
                .collect();
            Json(BarsOut { bid, count: dtos.len(), bars: dtos }).into_response()
        }
    }
}

/// Legacy: `GET /v1/bubble/:bid/bars/:kind` — returns count only.
async fn bubble_bars_count(
    Path((bid, kind)): Path<(u64, String)>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid_typed = BubbleId(bid);
    let guard = s.core.bubble();
    let count = guard
        .bubbles()
        .find(|b| b.id == bid_typed)
        .map(|b| b.bars.len())
        .unwrap_or(0);
    Json(BarsCountOut { bid, kind, count })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/bubbles", get(list_bubbles).post(create_bubble))
        .route("/v1/bubbles/:bid", get(get_bubble).delete(delete_bubble))
        .route("/v1/bubbles/:bid/bars", get(get_bubble_bars))
        // Legacy endpoint preserved for callers of the old shape.
        .route("/v1/bubble/:bid/bars/:kind", get(bubble_bars_count))
}
