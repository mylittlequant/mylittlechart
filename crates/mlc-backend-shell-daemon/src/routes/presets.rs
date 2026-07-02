//! Chart preset endpoints.
//!
//! - `GET    /v1/presets`            — list all preset ids + names
//! - `GET    /v1/presets/:preset_id` — full ChartPreset as JSON value
//! - `DELETE /v1/presets/:preset_id` — remove preset by key

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct PresetSummary {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct PresetListOut {
    presets: Vec<PresetSummary>,
}

async fn list_presets(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.presets();
    let presets: Vec<PresetSummary> = guard
        .iter()
        .map(|(k, v)| PresetSummary {
            id: k.clone(),
            name: v.name.clone(),
        })
        .collect();
    Json(PresetListOut { presets })
}

async fn get_preset(
    Path(preset_id): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let guard = s.core.presets();
    match guard.get(&preset_id) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(p) => match serde_json::to_value(p) {
            Ok(v) => Json(v).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        },
    }
}

async fn delete_preset(
    Path(preset_id): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let exists = s.core.presets().contains_key(&preset_id);
    if !exists {
        return StatusCode::NOT_FOUND.into_response();
    }
    s.core.presets_mut().remove(&preset_id);
    // Core removal already succeeded — the preset is gone from every
    // window's view regardless of what happens below. A failure to also
    // remove the on-disk `.json`/`.enc` file is reported as a warning, not
    // a failed request: the caller's DELETE intent (make it disappear) is
    // satisfied either way, but a leftover file on disk is worth surfacing
    // instead of silently leaving an orphan a future disk-backed reload
    // would resurrect.
    match mlc_core::preset::storage::delete_preset(&preset_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "warning": format!("preset '{}' removed from memory but its on-disk file could not be deleted: {}", preset_id, e),
            })),
        )
            .into_response(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/presets", get(list_presets))
        .route(
            "/v1/presets/:preset_id",
            get(get_preset).delete(delete_preset),
        )
}
