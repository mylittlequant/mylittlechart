//! CRUD endpoints for indicator management.
//!
//! - `GET  /api/v1/windows/:window_id/charts/:chart_id/indicators`              — list indicators on a chart
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/indicators`              — add an indicator
//! - `PATCH /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id` — update indicator params
//! - `DELETE /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id` — remove an indicator
//!
//! GET reads from the [`TerminalSnapshot`]; POST/PATCH/DELETE push an
//! [`AgentCommand`] onto the shared queue and immediately return `202 Accepted`.

use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{AgentCommand, IndicatorSummary};
use crate::AgentState;

// ---------------------------------------------------------------------------
// Path extractors
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChartPath {
    window_id: String,
    chart_id: u64,
}

#[derive(Deserialize)]
struct IndicatorPath {
    window_id: String,
    chart_id: u64,
    indicator_id: u64,
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
// GET /api/v1/windows/:window_id/charts/:chart_id/indicators
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct IndicatorsListResponse {
    chart_id: u64,
    indicators: Vec<IndicatorSummary>,
}

async fn list_indicators(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
) -> Result<(StatusCode, Json<IndicatorsListResponse>), (StatusCode, Json<ErrorResponse>)> {
    let snapshot = state
        .terminal_snapshot
        .read()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "terminal snapshot lock poisoned".to_string(),
                }),
            )
        })?;

    let window = snapshot
        .windows
        .iter()
        .find(|w| w.window_id == path.window_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("window '{}' not found", path.window_id),
                }),
            )
        })?;

    let chart = window
        .charts
        .iter()
        .find(|c| c.chart_id == path.chart_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("chart {} not found in window '{}'", path.chart_id, path.window_id),
                }),
            )
        })?;

    Ok((
        StatusCode::OK,
        Json(IndicatorsListResponse {
            chart_id: chart.chart_id,
            indicators: chart.indicators.clone(),
        }),
    ))
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/charts/:chart_id/indicators
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AddIndicatorRequest {
    type_id: String,
    #[serde(default)]
    params: std::collections::HashMap<String, serde_json::Value>,
    agent_id: Option<String>,
}

async fn add_indicator(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<ChartPath>,
    Json(body): Json<AddIndicatorRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::AddIndicator {
        window_id: path.window_id,
        chart_id: path.chart_id,
        type_id: body.type_id,
        params: body.params,
        agent_id: body.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// PATCH /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UpdateIndicatorRequest {
    #[serde(default)]
    params: std::collections::HashMap<String, serde_json::Value>,
    agent_id: Option<String>,
}

async fn update_indicator(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<IndicatorPath>,
    Json(body): Json<UpdateIndicatorRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::UpdateIndicator {
        window_id: path.window_id,
        chart_id: path.chart_id,
        indicator_id: path.indicator_id,
        params: body.params,
        agent_id: body.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DeleteQuery {
    agent_id: Option<String>,
}

async fn remove_indicator(
    State(state): State<Arc<AgentState>>,
    Path(path): Path<IndicatorPath>,
    Query(query): Query<DeleteQuery>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::RemoveIndicator {
        window_id: path.window_id,
        chart_id: path.chart_id,
        indicator_id: path.indicator_id,
        agent_id: query.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the indicator CRUD sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/indicators",
            get(list_indicators).post(add_indicator),
        )
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id",
            patch(update_indicator).delete(remove_indicator),
        )
}
