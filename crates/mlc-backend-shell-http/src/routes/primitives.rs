//! Drawing primitive CRUD endpoints.
//!
//! All mutation endpoints push an [`crate::state::AgentCommand`] into the
//! [`crate::AgentState::command_queue`]; the render thread drains the queue
//! each frame and applies the commands to the live chart state.
//!
//! Routes:
//! - `GET    /api/v1/windows/:window_id/charts/:chart_id/primitives`                          — list primitives on a chart
//! - `POST   /api/v1/windows/:window_id/charts/:chart_id/primitives`                          — add a new primitive
//! - `PATCH  /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id`            — update an existing primitive
//! - `DELETE /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id`            — remove a primitive

use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{AgentCommand, PrimitiveSummary, TerminalSnapshot, WindowSnapshot};
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
struct PrimitivePath {
    window_id: String,
    chart_id: u64,
    primitive_id: u64,
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AddPrimitiveRequest {
    type_id: String,
    points: Vec<[f64; 2]>,
    #[serde(default = "default_primitive_style")]
    style: crate::state::PrimitiveStyleDto,
    agent_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdatePrimitiveRequest {
    points: Option<Vec<[f64; 2]>>,
    style: Option<crate::state::PrimitiveStyleDto>,
    agent_id: Option<String>,
}

#[derive(Deserialize)]
struct DeleteQuery {
    agent_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PrimitivesListResponse {
    chart_id: u64,
    primitives: Vec<PrimitiveSummary>,
}

#[derive(Serialize)]
struct AcceptedResponse {
    queued: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Default style helper
// ---------------------------------------------------------------------------

fn default_primitive_style() -> crate::state::PrimitiveStyleDto {
    crate::state::PrimitiveStyleDto {
        color: "#e74c3c".to_string(),
        width: 2.0,
        style: "solid".to_string(),
        fill_color: None,
        fill_opacity: None,
    }
}

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

/// Find a window by id and return a clone to avoid holding the lock guard
/// across await points.
fn find_window_clone(snap: &TerminalSnapshot, window_id: &str) -> Option<WindowSnapshot> {
    snap.windows
        .iter()
        .find(|w| w.window_id == window_id)
        .cloned()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/windows/:window_id/charts/:chart_id/primitives`
///
/// Returns the list of primitives currently on the chart according to the
/// latest terminal snapshot. The list reflects state as of the most recent
/// frame; newly-queued commands may not yet be visible.
async fn list_primitives(
    State(state): State<Arc<AgentState>>,
    Path(ChartPath { window_id, chart_id }): Path<ChartPath>,
) -> Result<Json<PrimitivesListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.terminal_snapshot.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "terminal snapshot lock poisoned".to_string(),
            }),
        )
    })?;

    let window = find_window_clone(&snap, &window_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("window not found: {}", window_id),
            }),
        )
    })?;

    let chart = window
        .charts
        .into_iter()
        .find(|c| c.chart_id == chart_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("chart not found: {} in window {}", chart_id, window_id),
                }),
            )
        })?;

    Ok(Json(PrimitivesListResponse {
        chart_id,
        primitives: chart.primitives,
    }))
}

/// `POST /api/v1/windows/:window_id/charts/:chart_id/primitives`
///
/// Queues an [`AgentCommand::AddPrimitive`] and returns 202 Accepted.
async fn add_primitive(
    State(state): State<Arc<AgentState>>,
    Path(ChartPath { window_id, chart_id }): Path<ChartPath>,
    Json(body): Json<AddPrimitiveRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::AddPrimitive {
        window_id,
        chart_id,
        type_id: body.type_id,
        points: body.points,
        style: body.style,
        agent_id: body.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

/// `PATCH /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id`
///
/// Queues an [`AgentCommand::UpdatePrimitive`] and returns 202 Accepted.
async fn update_primitive(
    State(state): State<Arc<AgentState>>,
    Path(PrimitivePath { window_id, chart_id, primitive_id }): Path<PrimitivePath>,
    Json(body): Json<UpdatePrimitiveRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::UpdatePrimitive {
        window_id,
        chart_id,
        primitive_id,
        points: body.points,
        style: body.style,
        agent_id: body.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

/// `DELETE /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id`
///
/// Accepts an optional `?agent_id=xxx` query parameter.
/// Queues an [`AgentCommand::RemovePrimitive`] and returns 202 Accepted.
async fn remove_primitive(
    State(state): State<Arc<AgentState>>,
    Path(PrimitivePath { window_id, chart_id, primitive_id }): Path<PrimitivePath>,
    Query(q): Query<DeleteQuery>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    state.push_command(AgentCommand::RemovePrimitive {
        window_id,
        chart_id,
        primitive_id,
        agent_id: q.agent_id,
    });

    Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true })))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the primitives routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/primitives",
            get(list_primitives).post(add_primitive),
        )
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id",
            patch(update_primitive).delete(remove_primitive),
        )
}
