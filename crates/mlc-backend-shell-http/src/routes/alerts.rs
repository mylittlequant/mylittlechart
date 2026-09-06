//! Account-wide alert Agent API — create, list, delete, and test-deliver
//! alerts that watch an instrument regardless of any open window (Arc C3,
//! `docs/mlc/plans/alerts-account-wide-and-telegram-delivery-2026-09-06.md`
//! §5).
//!
//! Routes:
//! - `GET    /api/v1/windows/:window_id/alerts`               — list
//! - `POST   /api/v1/windows/:window_id/alerts`                — create
//! - `DELETE /api/v1/windows/:window_id/alerts/:alert_id`      — delete
//! - `POST   /api/v1/windows/:window_id/alerts/test-delivery`  — test Telegram
//!
//! `:window_id` rides along for URL-family consistency with `ui.rs`/
//! `share.rs` (`/api/v1/windows/:window_id/...`) but is otherwise unused:
//! alerts are account-wide, not window- or chart-scoped (`AccountAlertStore`
//! keys on `(account_id, InstrumentAddress, condition)`, never a window or
//! bubble id) — any window's caller sees and edits the same account-wide
//! set.
//!
//! Every write (`POST`/`DELETE`) needs an `AgentKeyRole::Control` key —
//! `auth_middleware` already refuses a `ReadOnly` key on those methods with
//! `403` before this module ever runs.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{
    AgentCommand, AlertAddressDto, AlertConditionDto, AlertListEntry, TestAlertDeliveryError,
};
use crate::AgentState;

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

// `window_id` is present in the URL (family consistency, see module doc)
// but not declared here at all — axum's `Path` extractor ignores captured
// segments a target struct does not name, so there is nothing to mark
// unused.
#[derive(Deserialize)]
struct AlertPath {
    alert_id: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct AcceptedResponse {
    accepted: bool,
}

// ---------------------------------------------------------------------------
// GET /api/v1/windows/:window_id/alerts
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AlertsListResponse {
    alerts: Vec<AlertListEntry>,
}

async fn list_alerts(
    State(state): State<Arc<AgentState>>,
) -> Result<Json<AlertsListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.push_command(AgentCommand::ListAlerts { response_tx: tx });

    match tokio::time::timeout(RESPONSE_TIMEOUT, rx).await {
        Ok(Ok(Ok(alerts))) => Ok(Json(AlertsListResponse { alerts })),
        Ok(Ok(Err(message))) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: message }))),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "render thread dropped the response channel".to_string() }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse { error: "list alerts timed out after 5 seconds".to_string() }),
        )),
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/alerts
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateAlertRequest {
    address: AlertAddressDto,
    condition: AlertConditionDto,
}

#[derive(Serialize)]
struct CreateAlertResponse {
    accepted: bool,
    id: u64,
}

/// `POST /api/v1/windows/:window_id/alerts` — only `condition.kind ==
/// "price_cross"` is accepted today (the only kind `AgentCommand::
/// CreateAlert`'s handler builds a spec for); an unrecognised `kind` or
/// `direction` answers `400` here, before ever reaching the render thread.
async fn create_alert(
    State(state): State<Arc<AgentState>>,
    Json(body): Json<CreateAlertRequest>,
) -> Result<(StatusCode, Json<CreateAlertResponse>), (StatusCode, Json<ErrorResponse>)> {
    if body.condition.kind != "price_cross" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: format!("unsupported condition kind: {}", body.condition.kind) }),
        ));
    }
    if let Some(ref direction) = body.condition.direction {
        if !matches!(direction.as_str(), "up" | "down" | "either") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse { error: format!("unsupported direction: {}", direction) }),
            ));
        }
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    state.push_command(AgentCommand::CreateAlert {
        address: body.address,
        condition: body.condition,
        response_tx: tx,
    });

    match tokio::time::timeout(RESPONSE_TIMEOUT, rx).await {
        Ok(Ok(Ok(id))) => Ok((StatusCode::CREATED, Json(CreateAlertResponse { accepted: true, id }))),
        Ok(Ok(Err(message))) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: message }))),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "render thread dropped the response channel".to_string() }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse { error: "create alert timed out after 5 seconds".to_string() }),
        )),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/windows/:window_id/alerts/:alert_id
// ---------------------------------------------------------------------------

async fn delete_alert(
    State(state): State<Arc<AgentState>>,
    Path(AlertPath { alert_id }): Path<AlertPath>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.push_command(AgentCommand::DeleteAlert { id: alert_id, response_tx: tx });

    match tokio::time::timeout(RESPONSE_TIMEOUT, rx).await {
        Ok(Ok(Ok(true))) => Ok(StatusCode::NO_CONTENT),
        Ok(Ok(Ok(false))) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: format!("alert not found: {}", alert_id) }),
        )),
        Ok(Ok(Err(message))) => Err((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse { error: message }))),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "render thread dropped the response channel".to_string() }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse { error: "delete alert timed out after 5 seconds".to_string() }),
        )),
    }
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/alerts/test-delivery
// ---------------------------------------------------------------------------

/// `POST /api/v1/windows/:window_id/alerts/test-delivery` — queues
/// [`AgentCommand::TestAlertDelivery`], which reuses the Vault tab's own
/// `tg_test_pending` send path rather than duplicating the Telegram call
/// here. `202` means the message was queued for send, not that it was
/// delivered — the actual result logs to stderr the same way the Vault
/// tab's own "Test message" button's result does.
async fn test_alert_delivery(
    State(state): State<Arc<AgentState>>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.push_command(AgentCommand::TestAlertDelivery { response_tx: tx });

    match tokio::time::timeout(RESPONSE_TIMEOUT, rx).await {
        Ok(Ok(Ok(()))) => Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { accepted: true }))),
        Ok(Ok(Err(TestAlertDeliveryError::Sealed))) => Err((
            StatusCode::LOCKED,
            Json(ErrorResponse { error: "vault sealed".to_string() }),
        )),
        Ok(Ok(Err(TestAlertDeliveryError::NoToken))) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "no Telegram bot token configured".to_string() }),
        )),
        Ok(Ok(Err(TestAlertDeliveryError::NoWindow))) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse { error: "no window open".to_string() }),
        )),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "render thread dropped the response channel".to_string() }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse { error: "test delivery timed out after 5 seconds".to_string() }),
        )),
    }
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the alerts routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/windows/:window_id/alerts", get(list_alerts).post(create_alert))
        .route("/api/v1/windows/:window_id/alerts/:alert_id", axum::routing::delete(delete_alert))
        .route("/api/v1/windows/:window_id/alerts/test-delivery", post(test_alert_delivery))
}
