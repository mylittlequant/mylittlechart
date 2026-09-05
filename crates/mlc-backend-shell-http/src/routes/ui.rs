//! Admin-tier UI test hooks — owner ruling 2026-09-05: the native control
//! plane on `:17420` is the sanctioned way to drive and inspect the native
//! app (paid forum, Events panel, other overlays) without a mouse or any
//! system input.
//!
//! Routes:
//! - `POST /api/v1/windows/:window_id/ui/dispatch` — replay a click by
//!   widget id (toolbar ids and panel/overlay ids both accepted)
//! - `POST /api/v1/windows/:window_id/ui/text`      — set a TextFieldStore
//!   field's text directly
//! - `GET  /api/v1/windows/:window_id/ui/state`      — read-only snapshot
//!   of account/overlay/forum/events/toast state
//!
//! Like every other Agent API route (see the crate's own module doc), these
//! three sit behind `auth_middleware`: a `ReadOnly` key gets `403` on
//! `ui/dispatch` and `ui/text` (both mutate — `POST`), while `ui/state`
//! stays reachable to either role since it is `GET`-only. A read-only key
//! never gets agent control over the app; it only ever gets to look at it.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{AgentCommand, WindowUiState};
use crate::AgentState;

// ---------------------------------------------------------------------------
// Path extractor
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WindowPath {
    window_id: String,
}

// ---------------------------------------------------------------------------
// Error envelope (shared by all three handlers)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/ui/dispatch
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UiDispatchRequest {
    hit_id: String,
}

#[derive(Serialize)]
struct UiDispatchResponse {
    accepted: bool,
}

/// `POST /api/v1/windows/:window_id/ui/dispatch` — fire-and-forget, same
/// shape as `controls.rs`'s writes: an unknown `hit_id` is still accepted
/// (the app's own click router silently ignores ids it does not recognise).
async fn dispatch_ui_click(
    State(state): State<Arc<AgentState>>,
    Path(WindowPath { window_id }): Path<WindowPath>,
    Json(body): Json<UiDispatchRequest>,
) -> (StatusCode, Json<UiDispatchResponse>) {
    state.push_command(AgentCommand::UiDispatch {
        window_id,
        hit_id: body.hit_id,
    });
    (StatusCode::ACCEPTED, Json(UiDispatchResponse { accepted: true }))
}

// ---------------------------------------------------------------------------
// POST /api/v1/windows/:window_id/ui/text
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UiTextRequest {
    field: String,
    text: String,
}

#[derive(Serialize)]
struct AcceptedResponse {
    queued: bool,
}

/// `POST /api/v1/windows/:window_id/ui/text` — like `screenshot.rs`'s
/// `RequestScreenshot`, this awaits a real answer from the render thread
/// instead of firing and forgetting: `field` is validated there against
/// `mlc_app::text_input::known_field_ids()` (the only place that list is
/// reachable from — this crate never depends on `ChartApp`), and an
/// unknown field comes back as `400`.
async fn set_ui_text(
    State(state): State<Arc<AgentState>>,
    Path(WindowPath { window_id }): Path<WindowPath>,
    Json(body): Json<UiTextRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

    state.push_command(AgentCommand::UiSetText {
        window_id,
        field: body.field,
        text: body.text,
        response_tx: tx,
    });

    match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(()))) => Ok((StatusCode::ACCEPTED, Json(AcceptedResponse { queued: true }))),
        Ok(Ok(Err(message))) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message }))),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "render thread dropped the response channel".to_string(),
            }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse {
                error: "ui text set timed out after 5 seconds".to_string(),
            }),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/windows/:window_id/ui/state
// ---------------------------------------------------------------------------

/// `GET /api/v1/windows/:window_id/ui/state` — reads from
/// [`crate::AgentState::ui_state_snapshot`], same pattern as `windows.rs`.
async fn get_ui_state(
    State(state): State<Arc<AgentState>>,
    Path(WindowPath { window_id }): Path<WindowPath>,
) -> Result<Json<WindowUiState>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.ui_state_snapshot.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "ui state snapshot lock poisoned".to_string(),
            }),
        )
    })?;

    snap.windows
        .iter()
        .find(|w| w.window_id == window_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("window not found: {}", window_id),
                }),
            )
        })
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the UI test-hook routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/windows/:window_id/ui/dispatch", post(dispatch_ui_click))
        .route("/api/v1/windows/:window_id/ui/text", post(set_ui_text))
        .route("/api/v1/windows/:window_id/ui/state", get(get_ui_state))
}
