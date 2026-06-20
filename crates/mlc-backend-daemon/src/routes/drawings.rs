//! Drawing CRUD + primitive catalog endpoints.
//!
//! - `GET    /v1/drawings`                           — list panels (summary)
//! - `GET    /v1/catalog/primitives`                 — list every registered PrimitiveMetadata
//! - `POST   /v1/drawings/:panel_id`                 — add primitive to panel
//! - `PATCH  /v1/drawings/:panel_id/:primitive_id`   — update primitive
//! - `DELETE /v1/drawings/:panel_id/:primitive_id`   — remove primitive
//!
//! The `PrimitiveRegistry` is a global `OnceLock` populated on first access —
//! it requires no explicit startup call. The primitive catalog endpoint just
//! reads from it.
//!
//! Missing reactor pieces: In the vello flow, after a primitive is added the
//! render loop picks it up on the next frame via the DrawingRegistry already
//! stored on BackendCore. The daemon exposes the same BackendCore, so the
//! primitive IS persisted in `BackendCore::drawings`. A future render client
//! can read it back via GET /v1/drawings.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_core::state::chart_container_leaf::ChartPanelId;

use crate::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

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

#[derive(Deserialize)]
struct CreatePrimitiveIn {
    type_id: String,
    /// Points as [ts_ms, price] arrays.
    #[serde(default)]
    points: Vec<[f64; 2]>,
}

#[derive(Serialize)]
struct CreatePrimitiveOut {
    primitive_id: u64,
    primitive_index: usize,
    type_id: String,
}

#[derive(Deserialize)]
struct PatchPrimitiveIn {
    points: Option<Vec<[f64; 2]>>,
    locked: Option<bool>,
    visible: Option<bool>,
}

#[derive(Serialize)]
struct PrimitiveUpdatedOut {
    primitive_id: u64,
    primitive_index: usize,
    type_id: String,
    locked: bool,
    visible: bool,
    points: Vec<[f64; 2]>,
}

/// Catalog DTO for one primitive type.
#[derive(Serialize)]
struct PrimitiveDefOut {
    type_id: String,
    display_name: String,
    category: String,
    tooltip: String,
    default_color: String,
}

#[derive(Serialize)]
struct PrimitiveCatalogOut {
    definitions: Vec<PrimitiveDefOut>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn decode_points(raw: &[[f64; 2]]) -> Vec<(i64, f64)> {
    raw.iter().map(|p| (p[0] as i64, p[1])).collect()
}

fn encode_points(pts: &[(i64, f64)]) -> Vec<[f64; 2]> {
    pts.iter().map(|&(ts, price)| [ts as f64, price]).collect()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

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

async fn catalog_primitives(State(_s): State<AppState>) -> impl IntoResponse {
    use mlc_core::drawing::primitives_v2::PrimitiveRegistry;
    let registry = PrimitiveRegistry::global().read().unwrap();
    let definitions: Vec<PrimitiveDefOut> = registry
        .all()
        .map(|meta| PrimitiveDefOut {
            type_id: meta.type_id.to_string(),
            display_name: meta.display_name.to_string(),
            category: format!("{:?}", meta.kind),
            tooltip: meta.tooltip.to_string(),
            default_color: meta.default_color.to_string(),
        })
        .collect();
    Json(PrimitiveCatalogOut { definitions })
}

async fn create_drawing_primitive(
    Path(panel_id): Path<u64>,
    State(s): State<AppState>,
    Json(body): Json<CreatePrimitiveIn>,
) -> impl IntoResponse {
    let points = decode_points(&body.points);
    let pid = ChartPanelId(panel_id);
    let mut guard = s.core.drawings_mut();
    let dm = guard.entry_or_default(pid);
    match dm.add_primitive_from(&body.type_id, points) {
        None => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unknown primitive type_id"})),
        )
            .into_response(),
        Some(idx) => {
            let prim = &dm.primitives()[idx];
            let prim_id = prim.data().id;
            let type_id = prim.type_id().to_string();
            (
                StatusCode::CREATED,
                Json(CreatePrimitiveOut {
                    primitive_id: prim_id,
                    primitive_index: idx,
                    type_id,
                }),
            )
                .into_response()
        }
    }
}

async fn patch_drawing_primitive(
    Path((panel_id, primitive_id)): Path<(u64, u64)>,
    State(s): State<AppState>,
    Json(body): Json<PatchPrimitiveIn>,
) -> impl IntoResponse {
    let pid = ChartPanelId(panel_id);
    let mut guard = s.core.drawings_mut();
    let dm = match guard.get_mut(pid) {
        Some(dm) => dm,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let idx = match dm.find_index_by_id(primitive_id) {
        Some(i) => i,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    if let Some(ref pts) = body.points {
        let decoded = decode_points(pts);
        dm.set_points_at(idx, &decoded);
    }
    {
        let prims = dm.primitives_mut();
        if let Some(prim) = prims.get_mut(idx) {
            if let Some(l) = body.locked {
                prim.data_mut().locked = l;
            }
            if let Some(v) = body.visible {
                prim.data_mut().visible = v;
            }
        }
    }
    let prim = match dm.primitives().get(idx) {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let out = PrimitiveUpdatedOut {
        primitive_id: prim.data().id,
        primitive_index: idx,
        type_id: prim.type_id().to_string(),
        locked: prim.data().locked,
        visible: prim.data().visible,
        points: encode_points(&prim.points()),
    };
    Json(out).into_response()
}

async fn delete_drawing_primitive(
    Path((panel_id, primitive_id)): Path<(u64, u64)>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let pid = ChartPanelId(panel_id);
    let mut guard = s.core.drawings_mut();
    let dm = match guard.get_mut(pid) {
        Some(dm) => dm,
        None => return StatusCode::NOT_FOUND,
    };
    let idx = match dm.find_index_by_id(primitive_id) {
        Some(i) => i,
        None => return StatusCode::NOT_FOUND,
    };
    dm.delete_at(idx);
    StatusCode::NO_CONTENT
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/drawings", get(list_drawings))
        .route("/v1/catalog/primitives", get(catalog_primitives))
        .route(
            "/v1/drawings/:panel_id",
            axum::routing::post(create_drawing_primitive),
        )
        .route(
            "/v1/drawings/:panel_id/:primitive_id",
            axum::routing::patch(patch_drawing_primitive)
                .delete(delete_drawing_primitive),
        )
}
