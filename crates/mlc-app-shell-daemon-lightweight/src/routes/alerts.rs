//! Alert CRUD endpoints.
//!
//! - `GET    /v1/alerts[?bubble_id=]`     — list alerts; folds across all
//!   bubbles when `bubble_id` is absent, scopes to one bubble when present.
//! - `POST   /v1/alerts[?bubble_id=]`     — create a price alert; targets
//!   the given bubble when `bubble_id` is present, falls back to the first
//!   bubble (today's behavior) when absent.
//! - `PATCH  /v1/alerts/:id`              — update name / price / condition
//! - `DELETE /v1/alerts/:id`              — remove alert
//!
//! Alerts live per-bubble in `DataBubble.alerts`, accessed exclusively
//! through `core.alerts()` / `core.alerts_mut()` (Step 3.0). PATCH/DELETE
//! resolve their owning bubble via `find_owner` since the route does not
//! carry it directly.
//!
//! `mlc-app-shell-daemon-lightweight` HTTP/WS service surface is out of scope for the
//! doctrine cutover wave (full REST bubble-scoping is deferred) — see
//! `docs/mlc/plans/step-3.0-alert-manager-design.md` §5.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_core::alert::{AlertCondition, AlertPatch, AlertSpec, AlertStatus};
use mlc_core::ids::BubbleId;

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
struct BubbleScopeQuery {
    bubble_id: Option<u64>,
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

fn to_out(a: &mlc_core::alert::AlertItem) -> AlertOut {
    AlertOut {
        id: a.id,
        name: a.name.clone(),
        symbol: a.symbol().to_string(),
        exchange: a.exchange().to_string(),
        account_type: a.account_type().as_label().to_string(),
        price: a.price,
        condition: condition_str(a.condition).to_string(),
        status: status_str(a.status).to_string(),
        trigger_count: a.trigger_count,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_alerts(
    State(s): State<AppState>,
    Query(q): Query<BubbleScopeQuery>,
) -> impl IntoResponse {
    let alerts: Vec<AlertOut> = match q.bubble_id {
        Some(raw) => s.core.alerts().items(BubbleId(raw)).iter().map(to_out).collect(),
        None => s.core.alerts().items_all().iter().map(to_out).collect(),
    };
    Json(AlertsOut { alerts })
}

async fn create_alert(
    State(s): State<AppState>,
    Query(q): Query<BubbleScopeQuery>,
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
    let target_bid = match q.bubble_id {
        Some(raw) => Some(BubbleId(raw)),
        // Fall back to the first available bubble (today's behavior) when
        // the caller does not know its bubble.
        None => s.core.bubbles().bubble_ids().next(),
    };
    let bid = match target_bid {
        Some(bid) => bid,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "no bubble available"})),
            ).into_response();
        }
    };
    let spec = AlertSpec {
        source: mlc_core::alert::AlertSource::Price { symbol: body.symbol },
        name: body.name,
        price: body.price,
        condition,
        ..AlertSpec::price_default()
    };
    let id = s.core.alerts_mut().add(bid, spec);
    if id == 0 {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "bubble not found"})),
        ).into_response();
    }
    (StatusCode::CREATED, Json(AlertCreatedOut { id })).into_response()
}

async fn patch_alert(
    Path(id): Path<u64>,
    State(s): State<AppState>,
    Json(body): Json<PatchAlertIn>,
) -> impl IntoResponse {
    // Find the bubble owning this alert.
    let bid = match s.core.alerts().find_owner(id) {
        Some((bid, _)) => bid,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let condition = match body.condition {
        Some(ref cond_str) => match parse_condition(cond_str) {
            Some(c) => Some(c),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "unknown condition"})),
                )
                    .into_response();
            }
        },
        None => None,
    };
    let patch = AlertPatch {
        name: body.name,
        price: body.price,
        condition,
        ..AlertPatch::default()
    };
    if !s.core.alerts_mut().update(bid, id, patch) {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Return the updated alert.
    match s.core.alerts().get(bid, id) {
        Some(a) => Json(to_out(a)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_alert(
    Path(id): Path<u64>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let bid_opt = s.core.alerts().find_owner(id).map(|(bid, _)| bid);
    let removed = match bid_opt {
        Some(bid) => s.core.alerts_mut().remove(bid, id),
        None => false,
    };
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
