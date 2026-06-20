//! Drawing registry read endpoint.
//!
//! - `GET /v1/drawings` — list every (panel_id, primitive_count, current_tool, locked, visible)

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct DrawingRowOut {
    panel_id: u64,
    primitive_count: usize,
    current_tool: Option<String>,
    locked: bool,
    visible: bool,
}

#[derive(Serialize)]
struct DrawingsOut {
    drawings: Vec<DrawingRowOut>,
}

async fn list_drawings(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.drawings();
    let drawings: Vec<DrawingRowOut> = guard
        .iter()
        .map(|(panel_id, dm)| DrawingRowOut {
            panel_id: panel_id.0,
            primitive_count: dm.count(),
            current_tool: dm.current_tool().map(str::to_string),
            locked: dm.is_locked(),
            visible: dm.is_visible(),
        })
        .collect();
    Json(DrawingsOut { drawings })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/drawings", get(list_drawings))
}
