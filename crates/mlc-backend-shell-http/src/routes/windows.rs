//! Window/tab/layout/chart discovery endpoints.
//!
//! All endpoints are read-only and source their data from
//! [`crate::AgentState::terminal_snapshot`].
//!
//! Routes:
//! - `GET /api/v1/windows`                                — list all windows
//! - `GET /api/v1/windows/:window_id/tabs`               — tabs for one window
//! - `GET /api/v1/windows/:window_id/layout`             — layout tree for one window
//! - `GET /api/v1/windows/:window_id/charts`             — all charts in a window
//! - `GET /api/v1/windows/:window_id/charts/:chart_id`   — full detail of one chart

use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::state::{ChartSnapshot, LayoutNode, TabSnapshot, TerminalSnapshot, WindowSnapshot};
use crate::AgentState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct WindowSummary {
    window_id: String,
    tab_count: usize,
    chart_count: usize,
    active_tab_id: String,
}

#[derive(Serialize)]
struct WindowsResponse {
    windows: Vec<WindowSummary>,
}

#[derive(Serialize)]
struct TabsResponse {
    window_id: String,
    tabs: Vec<TabSnapshot>,
}

#[derive(Serialize)]
struct LayoutResponse {
    window_id: String,
    layout: LayoutNode,
}

#[derive(Serialize)]
struct ChartsResponse {
    window_id: String,
    charts: Vec<ChartSnapshot>,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Find a window by id, returning a clone to avoid holding the lock guard
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

/// `GET /api/v1/windows` — list all windows with summary info.
async fn get_windows(
    State(state): State<Arc<AgentState>>,
) -> Result<Json<WindowsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.terminal_snapshot.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "terminal snapshot lock poisoned".to_string(),
            }),
        )
    })?;

    let windows = snap
        .windows
        .iter()
        .map(|w| WindowSummary {
            window_id: w.window_id.clone(),
            tab_count: w.tabs.len(),
            chart_count: w.charts.len(),
            active_tab_id: w.active_tab_id.clone(),
        })
        .collect();

    Ok(Json(WindowsResponse { windows }))
}

/// `GET /api/v1/windows/:window_id/tabs` — tabs for one window.
async fn get_tabs(
    State(state): State<Arc<AgentState>>,
    Path(window_id): Path<String>,
) -> Result<Json<TabsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    Ok(Json(TabsResponse {
        window_id,
        tabs: window.tabs,
    }))
}

/// `GET /api/v1/windows/:window_id/layout` — layout tree for one window.
async fn get_layout(
    State(state): State<Arc<AgentState>>,
    Path(window_id): Path<String>,
) -> Result<Json<LayoutResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    Ok(Json(LayoutResponse {
        window_id,
        layout: window.layout,
    }))
}

/// `GET /api/v1/windows/:window_id/charts` — all charts in a window.
async fn get_charts(
    State(state): State<Arc<AgentState>>,
    Path(window_id): Path<String>,
) -> Result<Json<ChartsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    Ok(Json(ChartsResponse {
        window_id,
        charts: window.charts,
    }))
}

/// `GET /api/v1/windows/:window_id/charts/:chart_id` — full detail of one chart.
async fn get_chart_detail(
    State(state): State<Arc<AgentState>>,
    Path((window_id, chart_id)): Path<(String, u64)>,
) -> Result<Json<ChartSnapshot>, (StatusCode, Json<ErrorResponse>)> {
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

    Ok(Json(chart))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the windows routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/windows", get(get_windows))
        .route("/api/v1/windows/:window_id/tabs", get(get_tabs))
        .route("/api/v1/windows/:window_id/layout", get(get_layout))
        .route("/api/v1/windows/:window_id/charts", get(get_charts))
        .route(
            "/api/v1/windows/:window_id/charts/:chart_id",
            get(get_chart_detail),
        )
}
