//! Indicator CRUD + catalog endpoints.
//!
//! - `GET    /v1/indicators`           — list all active instances (summary)
//! - `GET    /v1/catalog/indicators`   — list every known IndicatorDefinition
//! - `POST   /v1/indicators`           — create instance from type_id
//! - `PATCH  /v1/indicators/:id`       — update params / visibility / name
//! - `DELETE /v1/indicators/:id`       — remove instance
//!
//! Catalog notes: `IndicatorManager::new()` already calls `register_from_catalog()`
//! internally, so the catalog is populated the moment `BackendCore::new` is called
//! (which creates a fresh `IndicatorManager`). No additional populate call is needed
//! in `main.rs`.
//!
//! Missing reactor pieces: In the full vello flow, `create_instance` is followed by
//! `assign_window`, then the tick loop recalculates for that window. The daemon does
//! the same data operations — create + assign — but has no ChartApp tick that would
//! feed bars into the indicator and populate `instance.values`. That is correct:
//! the daemon proves `BackendCore` can hold indicator state; recalc on bar data
//! requires a client to send bars or for a bubble to exist.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use mlc_indicators::managers::indicator_manager::IndicatorValue;

use crate::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

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

#[derive(Serialize)]
struct IndicatorCreatedOut {
    id: u64,
    type_id: String,
    name: String,
}

#[derive(Deserialize)]
struct CreateIndicatorIn {
    type_id: String,
    #[serde(default)]
    params: HashMap<String, serde_json::Value>,
    window_id: Option<u64>,
}

#[derive(Deserialize)]
struct PatchIndicatorIn {
    #[serde(default)]
    params: HashMap<String, serde_json::Value>,
    visible: Option<bool>,
    locked: Option<bool>,
    name: Option<String>,
}

#[derive(Serialize)]
struct IndicatorUpdatedOut {
    id: u64,
    type_id: String,
    name: String,
    visible: bool,
    locked: bool,
    pane: usize,
    window_id: Option<u64>,
}

/// Catalog DTO for one indicator definition.
#[derive(Serialize)]
struct IndicatorDefOut {
    type_id: String,
    name: String,
    short_name: String,
    description: String,
    overlay: bool,
    category: String,
}

#[derive(Serialize)]
struct IndicatorCatalogOut {
    definitions: Vec<IndicatorDefOut>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a `serde_json::Value` from the HTTP body into an `IndicatorValue`.
///
/// Supports number → Int or Float, bool → Bool, string → String.
fn json_to_indicator_value(v: &serde_json::Value) -> Option<IndicatorValue> {
    match v {
        serde_json::Value::Bool(b) => Some(IndicatorValue::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(IndicatorValue::Int(i as i32))
            } else if let Some(f) = n.as_f64() {
                Some(IndicatorValue::Float(f))
            } else {
                None
            }
        }
        serde_json::Value::String(s) => Some(IndicatorValue::String(s.clone())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

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

async fn catalog_indicators(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.indicators();
    let definitions: Vec<IndicatorDefOut> = guard
        .get_definitions()
        .into_iter()
        .map(|def| IndicatorDefOut {
            type_id: def.type_id.clone(),
            name: def.name.clone(),
            short_name: def.short_name.clone(),
            description: def.description.clone(),
            overlay: def.overlay,
            category: format!("{:?}", def.category),
        })
        .collect();
    Json(IndicatorCatalogOut { definitions })
}

async fn create_indicator(
    State(s): State<AppState>,
    Json(body): Json<CreateIndicatorIn>,
) -> impl IntoResponse {
    let mut guard = s.core.indicators_mut();
    let id = match guard.create_instance(&body.type_id) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "unknown type_id"})),
            )
                .into_response();
        }
    };

    // Apply params from body.
    for (k, v) in &body.params {
        if let Some(iv) = json_to_indicator_value(v) {
            if let Some(inst) = guard.get_instance_mut(id) {
                inst.params.insert(k.clone(), iv);
            }
        }
    }

    // Assign to window if requested.
    if let Some(wid) = body.window_id {
        guard.assign_window(id, Some(wid));
    }

    let (type_id, name) = guard
        .get_instance(id)
        .map(|inst| (inst.type_id.clone(), inst.name.clone()))
        .unwrap_or_default();

    (StatusCode::CREATED, Json(IndicatorCreatedOut { id, type_id, name })).into_response()
}

async fn patch_indicator(
    Path(id): Path<u64>,
    State(s): State<AppState>,
    Json(body): Json<PatchIndicatorIn>,
) -> impl IntoResponse {
    let mut guard = s.core.indicators_mut();
    {
        let inst = match guard.get_instance_mut(id) {
            Some(i) => i,
            None => return StatusCode::NOT_FOUND.into_response(),
        };
        if let Some(v) = body.visible {
            inst.visible = v;
        }
        if let Some(l) = body.locked {
            inst.locked = l;
        }
        if let Some(ref n) = body.name {
            inst.name = n.clone();
        }
        for (k, v) in &body.params {
            if let Some(iv) = json_to_indicator_value(v) {
                inst.params.insert(k.clone(), iv);
            }
        }
    }
    let out = guard
        .get_instance(id)
        .map(|inst| IndicatorUpdatedOut {
            id: inst.id,
            type_id: inst.type_id.clone(),
            name: inst.name.clone(),
            visible: inst.visible,
            locked: inst.locked,
            pane: inst.pane,
            window_id: inst.window_id,
        });
    match out {
        Some(dto) => Json(dto).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_indicator(
    Path(id): Path<u64>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let removed = s.core.indicators_mut().remove_instance(id);
    if removed.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/indicators", get(list_indicators).post(create_indicator))
        .route(
            "/v1/indicators/:id",
            axum::routing::patch(patch_indicator).delete(delete_indicator),
        )
        .route("/v1/catalog/indicators", get(catalog_indicators))
}
