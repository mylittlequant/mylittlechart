//! `mlc-backend-daemon` — Mode-2 backend.
//!
//! Owns one `Arc<BackendCore>`, no embedded `ChartApp`. Drives a 60Hz tick
//! loop that drains the live-data broadcast and writes into the shared
//! bubble store via `BackendCore::tick_bridge_only`. Serves a minimal HTTP
//! API for external apps to attach as `AppSession`, query bubbles, and
//! observe events.
//!
//! Co-exists with `mlc-headless` (Mode-1-without-GPU, owns a ChartApp). The
//! daemon is the "no ChartApp anywhere" entry point — N external apps
//! attach over HTTP.
//!
//! ## Endpoints (this binary)
//!
//! - `GET  /health`                    — alive probe + tick count + session count
//! - `GET  /v1/bubbles`                — list of bubble ids the backend owns
//! - `GET  /v1/bubble/:bid/bars/:kind` — `Vec<Bar>` for a bubble's kind slot
//! - `POST /v1/apps`                   — register an AppSession, returns `app_id`
//! - `GET  /v1/apps`                   — list active AppSession ids
//! - `DELETE /v1/apps/:app_id`         — deregister a session
//! - `POST /v1/apps/:app_id/heartbeat` — keep-alive
//! - `GET  /v1/exchanges`              — list of `ExchangeId` snapshots present
//!
//! Streaming events (`BubbleEvent`) to attached apps will use a long-poll
//! or WS upgrade in a follow-up. The HTTP surface here is the minimum
//! viable lifecycle so Mode-2 stops being a paper plan.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_backend_core::{AppSession, BackendCore};
use mlc_live_data::DataBridge;

const DEFAULT_PORT: u16 = 19470;
const TICK_PERIOD: Duration = Duration::from_millis(16);
const SESSION_IDLE_TIMEOUT_SECS: u64 = 60;

#[derive(Clone)]
struct AppState {
    core: Arc<BackendCore>,
    tick_count: Arc<std::sync::atomic::AtomicU64>,
}

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port = std::env::var("MLC_BACKEND_DAEMON_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    // CRITICAL: construct DataBridge synchronously FIRST. It owns its own
    // tokio runtime, built via block_on internally. Calling that from inside
    // a `#[tokio::main]` async fn panics ("Cannot start a runtime from
    // within a runtime"). The daemon's HTTP runtime is a separate Tokio
    // instance, spun up below after the bridge already exists.
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new();
    let bridge = Arc::new(bridge);
    let core = BackendCore::new(bridge.clone(), None);

    tracing::info!(target: "mlc-backend-daemon", "backend constructed, tick loop starting");

    let state = AppState {
        core: core.clone(),
        tick_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };

    // HTTP runtime — separate from the bridge runtime.
    let http_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .thread_name("mlc-backend-daemon-http")
        .build()?;

    let tick_state = state.clone();
    http_rt.spawn(async move {
        let mut interval = tokio::time::interval(TICK_PERIOD);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let now_ms = wall_clock_ms();
            tick_state.core.tick_bridge_only(now_ms);
            tick_state
                .tick_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Drop idle sessions whose heartbeat lapsed.
            let now_secs = now_ms / 1000;
            tick_state
                .core
                .reap_idle_sessions(now_secs, SESSION_IDLE_TIMEOUT_SECS);
        }
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/bubbles", get(list_bubbles))
        .route("/v1/bubble/:bid/bars/:kind", get(bubble_bars))
        .route("/v1/exchanges", get(list_exchanges))
        .route("/v1/apps", get(list_apps).post(register_app))
        .route("/v1/apps/:app_id", delete(deregister_app))
        .route("/v1/apps/:app_id/heartbeat", post(heartbeat_app))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    http_rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(target: "mlc-backend-daemon", "listening on http://{addr}");
        tokio::select! {
            res = axum::serve(listener, app) => res?,
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(target: "mlc-backend-daemon", "ctrl-c received, shutting down");
            }
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}

fn wall_clock_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Serialize)]
struct HealthOut {
    ok: bool,
    tick_count: u64,
    sessions: usize,
}

async fn health(State(s): State<AppState>) -> impl IntoResponse {
    Json(HealthOut {
        ok: true,
        tick_count: s.tick_count.load(std::sync::atomic::Ordering::Relaxed),
        sessions: s.core.sessions_snapshot().len(),
    })
}

#[derive(Serialize)]
struct BubbleListOut {
    bubble_ids: Vec<u64>,
}

async fn list_bubbles(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.bubble();
    let bubble_ids: Vec<u64> = guard.bubble.bubbles().map(|b| b.id.0).collect();
    Json(BubbleListOut { bubble_ids })
}

#[derive(Serialize)]
struct BarsOut {
    bid: u64,
    kind: String,
    count: usize,
}

async fn bubble_bars(
    Path((bid, kind)): Path<(u64, String)>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    use mlc_core::tag_manager::BubbleId;
    let bid_typed = BubbleId(bid);
    let guard = s.core.bubble();
    let count = guard
        .bubble
        .bubbles()
        .find(|b| b.id == bid_typed)
        .map(|b| b.bars.len())
        .unwrap_or(0);
    Json(BarsOut { bid, kind, count })
}

#[derive(Serialize)]
struct ExchangeListOut {
    exchanges: Vec<String>,
}

async fn list_exchanges(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.exchange_symbols_read();
    let exchanges: Vec<String> = guard.keys().map(|e| format!("{:?}", e)).collect();
    Json(ExchangeListOut { exchanges })
}

#[derive(Deserialize)]
struct RegisterIn {
    label: Option<String>,
}

#[derive(Serialize)]
struct RegisterOut {
    app_id: String,
}

async fn register_app(
    State(s): State<AppState>,
    Json(body): Json<RegisterIn>,
) -> impl IntoResponse {
    let now = wall_clock_ms() / 1000;
    let app_id = s.core.register_session(body.label, now);
    (StatusCode::CREATED, Json(RegisterOut { app_id }))
}

#[derive(Serialize)]
struct AppsListOut {
    apps: Vec<AppSessionOut>,
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
