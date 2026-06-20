//! Alert CRUD endpoints.
//!
//! - `GET    /v1/alerts`       — list all alerts
//! - `POST   /v1/alerts`       — create a price alert
//! - `PATCH  /v1/alerts/:id`   — update name / price / condition
//! - `DELETE /v1/alerts/:id`   — remove alert
//!
//! Alerts live in `BackendCore.managers` (the `Managers` struct) under the
//! field `alert: AlertManager`. Accessed via `core.bubble_mut().alert`.
//!
//! The create body uses `AlertManager::create_price_alert` (simplest public
//! constructor). For Drawing/Indicator/Signal alert sources, callers can use
//! PATCH after creation to update the source field if the AlertItem struct
//! is extended with a source-patch endpoint in the future.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_alerts::{AlertCondition, AlertStatus};

use crate::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AlertOut {
    id: u64,
    name: String,
    symbol: String,
    exchange: String,
    account_type: String,
    price: f64,
    condition: String,
    status: String,
    trigger_count: u32,
}

#[derive(Serialize)]
struct AlertsOut {
    alerts: Vec<AlertOut>,
}

#[derive(Deserialize)]
struct CreateAlertIn {
    /// Symbol this alert monitors (e.g. `"BTCUSDT"`).
    symbol: String,
    /// Human-readable label.
    name: String,
    /// Threshold price.
    price: f64,
    /// Condition string: one of `"crossing_up"`, `"crossing_down"`, `"crossing"`,
    /// `"greater_than"`, `"less_than"`. Defaults to `"crossing"`.
    #[serde(default = "default_condition")]
    condition: String,
}

fn default_condition() -> String {
    "crossing".to_string()
}

#[derive(Serialize)]
struct AlertCreatedOut {
    id: u64,
}

#[derive(Deserialize)]
struct PatchAlertIn {
    name: Option<String>,
    price: Option<f64>,
    condition: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_condition(s: &str) -> Option<AlertCondition> {
    match s {
        "crossing_up" => Some(AlertCondition::CrossingUp),
        "crossing_down" => Some(AlertCondition::CrossingDown),
        "crossing" => Some(AlertCondition::Crossing),
        "greater_than" => Some(AlertCondition::GreaterThan),
        "less_than" => Some(AlertCondition::LessThan),
        "entering_range" => Some(AlertCondition::EnteringRange),
        "exiting_range" => Some(AlertCondition::ExitingRange),
        "inside_range" => Some(AlertCondition::InsideRange),
        "outside_range" => Some(AlertCondition::OutsideRange),
        "moving_up" => Some(AlertCondition::MovingUp),
        "moving_down" => Some(AlertCondition::MovingDown),
        "moving_up_or_down" => Some(AlertCondition::MovingUpOrDown),
        _ => None,
    }
}

fn condition_str(c: AlertCondition) -> &'static str {
    match c {
        AlertCondition::CrossingUp => "crossing_up",
        AlertCondition::CrossingDown => "crossing_down",
        AlertCondition::Crossing => "crossing",
        AlertCondition::GreaterThan => "greater_than",
        AlertCondition::LessThan => "less_than",
        AlertCondition::EnteringRange => "entering_range",
        AlertCondition::ExitingRange => "exiting_range",
        AlertCondition::InsideRange => "inside_range",
        AlertCondition::OutsideRange => "outside_range",
        AlertCondition::MovingUp => "moving_up",
        AlertCondition::MovingDown => "moving_down",
        AlertCondition::MovingUpOrDown => "moving_up_or_down",
    }
}

fn status_str(s: AlertStatus) -> &'static str {
    match s {
        AlertStatus::Active => "active",
        AlertStatus::Triggered => "triggered",
        AlertStatus::Paused => "paused",
        AlertStatus::Expired => "expired",
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_alerts(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.bubble();
    let alerts: Vec<AlertOut> = guard
        .alert
        .items()
        .iter()
        .map(|a| AlertOut {
            id: a.id,
            name: a.name.clone(),
            symbol: a.symbol().to_string(),
            exchange: a.exchange.clone(),
            account_type: a.account_type.clone(),
            price: a.price,
            condition: condition_str(a.condition).to_string(),
            status: status_str(a.status).to_string(),
            trigger_count: a.trigger_count,
        })
        .collect();
    Json(AlertsOut { alerts })
}

async fn create_alert(
    State(s): State<AppState>,
    Json(body): Json<CreateAlertIn>,
) -> impl IntoResponse {
    let condition = match parse_condition(&body.condition) {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "unknown condition"})),
            )
                .into_response();
        }
    };
    let id = s
        .core
        .bubble_mut()
        .alert
        .create_price_alert(&body.symbol, &body.name, body.price, condition);
    (StatusCode::CREATED, Json(AlertCreatedOut { id })).into_response()
}

async fn patch_alert(
    Path(id): Path<u64>,
    State(s): State<AppState>,
    Json(body): Json<PatchAlertIn>,
) -> impl IntoResponse {
    let mut guard = s.core.bubble_mut();
    let alert = match guard.alert.get_mut(id) {
        Some(a) => a,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    if let Some(ref n) = body.name {
        alert.name = n.clone();
    }
    if let Some(p) = body.price {
        alert.price = p;
    }
    if let Some(ref cond_str) = body.condition {
        if let Some(c) = parse_condition(cond_str) {
            alert.condition = c;
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "unknown condition"})),
            )
                .into_response();
        }
    }
    // Re-borrow for output.
    let out = guard.alert.get(id).map(|a| AlertOut {
        id: a.id,
        name: a.name.clone(),
        symbol: a.symbol().to_string(),
        exchange: a.exchange.clone(),
        account_type: a.account_type.clone(),
        price: a.price,
        condition: condition_str(a.condition).to_string(),
        status: status_str(a.status).to_string(),
        trigger_count: a.trigger_count,
    });
    match out {
        Some(dto) => Json(dto).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_alert(
    Path(id): Path<u64>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let removed = s.core.bubble_mut().alert.remove(id);
    if removed {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/alerts", get(list_alerts).post(create_alert))
        .route(
            "/v1/alerts/:id",
            axum::routing::patch(patch_alert).delete(delete_alert),
        )
}
