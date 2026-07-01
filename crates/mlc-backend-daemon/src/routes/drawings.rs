//! Drawing CRUD + primitive catalog endpoints.
//!
//! - `GET    /v1/drawings`                             — list bubbles (summary)
//! - `GET    /v1/catalog/primitives`                   — list every registered PrimitiveMetadata
//! - `POST   /v1/drawings/:bubble_id`                   — add primitive to bubble
//! - `PATCH  /v1/drawings/:bubble_id/:primitive_id`     — update primitive
//! - `DELETE /v1/drawings/:bubble_id/:primitive_id`     — remove primitive
//!
//! The `PrimitiveRegistry` is a global `OnceLock` populated on first access —
//! it requires no explicit startup call. The primitive catalog endpoint just
//! reads from it.
//!
//! Per-bubble drawing model (Step 3.1): `DrawingManager` sessions are keyed
//! by `BubbleId`, not by chart panel. `DataBubble.primitives` is the single
//! source of truth; the daemon reads/writes it through the `Drawings` /
//! `DrawingsMut` facade on `BackendCore`, never through raw manager access.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_core::ids::BubbleId;

use crate::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DrawingRowOut {
    bubble_id: u64,
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
    let dreg = s.core.drawings();
    let bubbles = s.core.bubble();
    let drawings: Vec<DrawingRowOut> = bubbles
        .bubbles()
        .map(|b| DrawingRowOut {
            bubble_id: b.id.0,
            primitive_count: dreg.count(b.id),
            current_tool: dreg.current_tool(b.id).map(str::to_string),
            locked: dreg.is_locked(b.id),
            visible: dreg.is_visible(b.id),
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
    Path(bubble_id): Path<u64>,
    State(s): State<AppState>,
    Json(body): Json<CreatePrimitiveIn>,
) -> impl IntoResponse {
    let points = decode_points(&body.points);
    let bid = BubbleId(bubble_id);

    let prim = {
        use mlc_core::drawing::primitives_v2::PrimitiveRegistry;
        let registry = PrimitiveRegistry::global().read().unwrap();
        registry.create(&body.type_id, &points, None)
    };
    let Some(prim) = prim else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unknown primitive type_id"})),
        )
            .into_response();
    };
    let type_id = prim.type_id().to_string();
    let mut guard = s.core.drawings_mut();
    let prim_id = guard.add_external(bid, prim);
    (
        StatusCode::CREATED,
        Json(CreatePrimitiveOut { primitive_id: prim_id, type_id }),
    )
        .into_response()
}

async fn patch_drawing_primitive(
    Path((bubble_id, primitive_id)): Path<(u64, u64)>,
    State(s): State<AppState>,
    Json(body): Json<PatchPrimitiveIn>,
) -> impl IntoResponse {
    let bid = BubbleId(bubble_id);
    let mut guard = s.core.drawings_mut();

    if guard.get_data(bid, primitive_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(ref pts) = body.points {
        let decoded = decode_points(pts);
        guard.set_points(bid, primitive_id, &decoded);
    }
    if let Some(l) = body.locked {
        guard.set_lock(bid, primitive_id, l);
    }
    if let Some(v) = body.visible {
        guard.set_visibility(bid, primitive_id, v);
    }

    let Some(data) = guard.get_data(bid, primitive_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(points) = guard.primitives(bid).iter()
        .find(|p| p.data().id == primitive_id)
        .map(|p| p.points())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let out = PrimitiveUpdatedOut {
        primitive_id,
        type_id: data.type_id.clone(),
        locked: data.locked,
        visible: data.visible,
        points: encode_points(&points),
    };
    Json(out).into_response()
}

async fn delete_drawing_primitive(
    Path((bubble_id, primitive_id)): Path<(u64, u64)>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid = BubbleId(bubble_id);
    let mut guard = s.core.drawings_mut();
    if guard.delete(bid, primitive_id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/drawings", get(list_drawings))
        .route("/v1/catalog/primitives", get(catalog_primitives))
        .route(
            "/v1/drawings/:bubble_id",
            axum::routing::post(create_drawing_primitive),
        )
        .route(
            "/v1/drawings/:bubble_id/:primitive_id",
            axum::routing::patch(patch_drawing_primitive)
                .delete(delete_drawing_primitive),
        )
}
