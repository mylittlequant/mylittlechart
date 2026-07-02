//! Session / app lifecycle endpoints.
//!
//! - `POST /v1/apps`                    — register an AppSession
//! - `GET  /v1/apps`                    — list active sessions
//! - `DELETE /v1/apps/:app_id`          — deregister a session
//! - `POST /v1/apps/:app_id/heartbeat`  — keep-alive
//! - `POST /v1/apps/_reap`              — force-reap idle sessions

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_backend::AppSession;

use crate::AppState;

fn wall_clock_ms() -> u64 {
    use web_time::SystemTime;
    use web_time::UNIX_EPOCH;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Serialize)]
pub struct AppSessionOut {
    pub app_id: String,
    pub label: Option<String>,
    pub created_at_secs: u64,
    pub last_heartbeat_secs: u64,
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

async fn register_app(
    State(s): State<AppState>,
    Json(body): Json<RegisterIn>,
) -> impl IntoResponse {
    let now = wall_clock_ms() / 1000;
    let app_id = s.core.register_session(body.label, now);
    (StatusCode::CREATED, Json(RegisterOut { app_id }))
}

async fn list_apps(State(s): State<AppState>) -> impl IntoResponse {
    let apps: Vec<AppSessionOut> = s
        .core
        .sessions_snapshot()
        .into_iter()
        .map(AppSessionOut::from)
        .collect();
    Json(AppsListOut { apps })
}

async fn deregister_app(
    Path(app_id): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    if s.core.deregister_session(&app_id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn heartbeat_app(
    Path(app_id): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let now = wall_clock_ms() / 1000;
    if s.core.heartbeat_session(&app_id, now) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

#[derive(Deserialize)]
struct ReapIn {
    idle_secs: u64,
}

#[derive(Serialize)]
struct ReapOut {
    reaped: usize,
}

async fn reap_apps(State(s): State<AppState>, Json(body): Json<ReapIn>) -> impl IntoResponse {
    let now = wall_clock_ms() / 1000;
    let reaped = s.core.reap_idle_sessions(now, body.idle_secs);
    Json(ReapOut { reaped })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/apps", get(list_apps).post(register_app))
        .route("/v1/apps/:app_id", delete(deregister_app))
        .route("/v1/apps/:app_id/heartbeat", post(heartbeat_app))
        .route("/v1/apps/_reap", post(reap_apps))
}

