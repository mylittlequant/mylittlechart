//! Write endpoints for chart-type, timeframe, preset and layout switching.
//!
//! All handlers push an [`AgentCommand`] onto the shared queue and return
//! `202 Accepted` immediately. Actual mutation happens on the render / tick
//! thread when the queue is drained.
//!
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/chart_type`
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/timeframe`
//! - `POST /api/v1/windows/:window_id/preset`
//! - `POST /api/v1/windows/:window_id/layout`

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

#[derive(Deserialize)]
struct ChartPath {
    window_id: String,
    chart_id: u64,
}

#[derive(Deserialize)]
struct WindowPath {
    window_id: String,
}

#[derive(Serialize)]
struct AcceptedResponse {
    queued: bool,
}

// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChartTypeRequest {
    chart_type: String,
}

async fn switch_chart_type(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
    Json(body): Json<ChartTypeRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::SwitchChartType {
        window_id: path.window_id,
        chart_id: path.chart_id,
        chart_type: body.chart_type,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true }))
}

// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TimeframeRequest {
    timeframe_id: String,
}

async fn switch_timeframe(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
    Json(body): Json<TimeframeRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::SwitchTimeframe {
        window_id: path.window_id,
        chart_id: path.chart_id,
        timeframe_id: body.timeframe_id,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true }))
}

// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PresetRequest {
    preset_id: String,
}

async fn switch_preset(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<WindowPath>,
    Json(body): Json<PresetRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::SwitchPreset {
        window_id: path.window_id,
        preset_id: body.preset_id,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true }))
}

// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LayoutRequest {
    /// One of: `"single"`, `"h"`, `"v"`, `"2x2"`, `"2l1r"`, `"1l2r"`,
    /// `"2t1b"`, `"1t2b"`, `"3cols"`, `"3rows"`, `"1big3small"`.
    kind: String,
}

async fn set_internal_layout(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<WindowPath>,
    Json(body): Json<LayoutRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::SetInternalLayout {
        window_id: path.window_id,
        kind: body.kind,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true }))
}

// ---------------------------------------------------------------------------

pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/chart_type",
            post(switch_chart_type),
        )
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/timeframe",
            post(switch_timeframe),
        )
        .route("/api/v1/windows/:window_id/preset", post(switch_preset))
        .route("/api/v1/windows/:window_id/layout", post(set_internal_layout))
}
