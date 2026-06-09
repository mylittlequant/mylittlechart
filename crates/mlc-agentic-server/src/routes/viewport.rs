//! Write endpoints for viewport control and symbol switching.
//!
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/viewport` — pan/zoom the chart
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/symbol`   — switch symbol/exchange/timeframe
//!
//! Both handlers push an [`AgentCommand`] onto the shared queue and immediately
//! return `202 Accepted`.  Validation of window_id / chart_id happens on the
//! render thread when it drains the queue.

use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::AgentCommand;
use crate::AgentState;

// ---------------------------------------------------------------------------
// Shared path extractor
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChartPath {
    window_id: String,
    chart_id: u64,
}

// ---------------------------------------------------------------------------
// Shared response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AcceptedResponse {
    queued: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/charts/:chart_id/viewport
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ViewportRequest {
    view_start: Option<f64>,
    bar_spacing: Option<f64>,
    /// Named mode: `"focus"`, `"analyze"`, `"fit"`.  Takes precedence over
    /// numeric fields when present.
    mode: Option<String>,
}

async fn set_viewport(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
    Json(body): Json<ViewportRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    // At least one field must be provided.
    if body.view_start.is_none() && body.bar_spacing.is_none() && body.mode.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "at least one of view_start, bar_spacing, or mode must be set".to_string(),
            }),
        ));
    }

    state.push_command(AgentCommand::SetViewport {
        window_id: path.window_id,
        chart_id: path.chart_id,
        view_start: body.view_start,
        bar_spacing: body.bar_spacing,
        mode: body.mode,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/charts/:chart_id/symbol
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SwitchSymbolRequest {
    symbol: String,
    exchange: String,
    timeframe: String,
    /// "S" = Spot (default), "FC" = FuturesCross, "M" = Margin, etc.
    #[serde(default = "default_account_type")]
    account_type: String,
}

fn default_account_type() -> String {
    "S".to_string()
}

async fn switch_symbol(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
    Json(body): Json<SwitchSymbolRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::SwitchSymbol {
        window_id: path.window_id,
        chart_id: path.chart_id,
        symbol: body.symbol,
        exchange: body.exchange,
        timeframe: body.timeframe,
        account_type: body.account_type,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the viewport and symbol-switch sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/viewport",
            post(set_viewport),
        )
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/symbol",
            post(switch_symbol),
        )
}
