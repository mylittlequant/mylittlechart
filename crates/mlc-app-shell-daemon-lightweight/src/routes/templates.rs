//! Template manager summary endpoint.
//!
//! - `GET /v1/templates/summary` — counts for each template bucket

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct TemplateSummaryOut {
    primitive_templates: usize,
    indicator_templates: usize,
    compare_templates: usize,
    chart_templates: usize,
    indicator_sets: usize,
}

async fn templates_summary(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.template_manager();
    Json(TemplateSummaryOut {
        primitive_templates: guard.primitive_templates.len(),
        indicator_templates: guard.indicator_templates.len(),
        compare_templates: guard.compare_templates.len(),
        chart_templates: guard.chart_templates.len(),
        indicator_sets: guard.indicator_sets.len(),
    })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/templates/summary", get(templates_summary))
}
