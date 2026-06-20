//! Indicator manager read endpoint.
//!
//! - `GET /v1/indicators` — list all active indicator instances with summary fields

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct IndicatorSummaryOut {
    id: u64,
    type_id: String,
    name: String,
    visible: bool,
    locked: bool,
    pane: usize,
    window_id: Option<u64>,
}

#[derive(Serialize)]
struct IndicatorsOut {
    indicators: Vec<IndicatorSummaryOut>,
}

async fn list_indicators(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.indicators();
    let indicators: Vec<IndicatorSummaryOut> = guard
        .get_all_instances()
        .into_iter()
        .map(|inst| IndicatorSummaryOut {
            id: inst.id,
            type_id: inst.type_id.clone(),
            name: inst.name.clone(),
            visible: inst.visible,
            locked: inst.locked,
            pane: inst.pane,
            window_id: inst.window_id,
        })
        .collect();
    Json(IndicatorsOut { indicators })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/indicators", get(list_indicators))
}
