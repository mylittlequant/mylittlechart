//! Catalog discovery endpoints.
//!
//! Returns static definitions of all available indicator types and drawing
//! primitive types. The data is sourced from pre-populated snapshots in
//! [`crate::AgentState`] and never triggers live computation.
//!
//! Routes:
//! - `GET /api/v1/catalog/indicators`              — all indicator definitions
//! - `GET /api/v1/catalog/indicators?search=<q>`   — filtered by search query
//! - `GET /api/v1/catalog/primitives`              — all drawing primitive definitions

use std::sync::Arc;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{CatalogIndicator, CatalogPrimitive};
use crate::AgentState;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct IndicatorSearchQuery {
    /// Optional search string — matches against `type_id`, `name`, `short_name`,
    /// and `category` (case-insensitive substring match).
    search: Option<String>,
}

#[derive(Serialize)]
struct IndicatorsResponse {
    total: usize,
    indicators: Vec<CatalogIndicator>,
}

#[derive(Serialize)]
struct PrimitivesResponse {
    total: usize,
    primitives: Vec<CatalogPrimitive>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/catalog/indicators` — return all (or filtered) indicator definitions.
///
/// Optional `?search=<query>` performs a case-insensitive substring match
/// against `type_id`, `name`, `short_name`, and `category`.
async fn get_indicators(
    State(state): State<Arc<AgentState>>,
    Query(q): Query<IndicatorSearchQuery>,
) -> Result<Json<IndicatorsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let catalog = state.indicator_catalog.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "indicator catalog lock poisoned".to_string(),
            }),
        )
    })?;

    let indicators: Vec<CatalogIndicator> = match q.search.as_deref() {
        Some(query) if !query.is_empty() => {
            let lower = query.to_lowercase();
            catalog
                .indicators
                .iter()
                .filter(|ind| {
                    ind.type_id.to_lowercase().contains(&lower)
                        || ind.name.to_lowercase().contains(&lower)
                        || ind.short_name.to_lowercase().contains(&lower)
                        || ind.category.to_lowercase().contains(&lower)
                })
                .cloned()
                .collect()
        }
        _ => catalog.indicators.clone(),
    };

    let total = indicators.len();
    Ok(Json(IndicatorsResponse { total, indicators }))
}

/// `GET /api/v1/catalog/primitives` — return all drawing primitive definitions.
async fn get_primitives(
    State(state): State<Arc<AgentState>>,
) -> Result<Json<PrimitivesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let catalog = state.primitive_catalog.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "primitive catalog lock poisoned".to_string(),
            }),
        )
    })?;

    let primitives = catalog.primitives.clone();
    let total = primitives.len();
    Ok(Json(PrimitivesResponse { total, primitives }))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the catalog routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/catalog/indicators", get(get_indicators))
        .route("/api/v1/catalog/primitives", get(get_primitives))
}
