//! `POST /api/v1/apps` and friends — `AppSession` lifecycle.
//!
//! These routes manage attached AppSessions on the per-process `BackendCore`.
//! Same surface as the Mode-2 daemon (`mlc-backend-shell-daemon`'s `/v1/apps`) so
//! external clients can speak one shape against either binary.
//!
//! Returns `503 Service Unavailable` if the host did not attach a backend
//! (i.e. `AgentState::with_core` was never called).

use std::sync::Arc;
use web_time::SystemTime;
use web_time::UNIX_EPOCH;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_backend::AppSession;

use crate::AgentState;

#[derive(Serialize)]
struct AppSessionOut {
    app_id: String,
    label: Option<String>,
    created_at_secs: u64,
    last_heartbeat_secs: u64,
}

impl From<AppSession> for AppSessionOut {
    fn from(s: AppSession) -> Self {
        Self {
            app_id: s.app_id,
            label: s.label,
            created_at_secs: s.created_at_secs,
            last_heartbeat_secs: s.last_heartbeat_secs,
        }
    }
}

#[derive(Deserialize)]
struct RegisterIn {
    label: Option<String>,
}

#[derive(Serialize)]
struct RegisterOut {
    app_id: String,
}

#[derive(Serialize)]
struct AppsListOut {
    apps: Vec<AppSessionOut>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn register_app(
    State(state): State<Arc<AgentState>>,
    Json(body): Json<RegisterIn>,
) -> impl IntoResponse {
    let Some(core) = state.core() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::Value::Null)).into_response();
    };
    let app_id = core.register_session(body.label, now_secs());
    (StatusCode::CREATED, Json(RegisterOut { app_id })).into_response()
}

async fn list_apps(State(state): State<Arc<AgentState>>) -> impl IntoResponse {
    let Some(core) = state.core() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::Value::Null)).into_response();
    };
    let apps: Vec<AppSessionOut> = core
        .sessions_snapshot()
        .into_iter()
        .map(AppSessionOut::from)
        .collect();
    Json(AppsListOut { apps }).into_response()
}

async fn deregister_app(
    Path(app_id): Path<String>,
    State(state): State<Arc<AgentState>>,
) -> impl IntoResponse {
    let Some(core) = state.core() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if core.deregister_session(&app_id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn heartbeat_app(
    Path(app_id): Path<String>,
    State(state): State<Arc<AgentState>>,
) -> impl IntoResponse {
    let Some(core) = state.core() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if core.heartbeat_session(&app_id, now_secs()) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/apps", get(list_apps).post(register_app))
        .route("/api/v1/apps/:app_id", delete(deregister_app))
        .route("/api/v1/apps/:app_id/heartbeat", post(heartbeat_app))
}
