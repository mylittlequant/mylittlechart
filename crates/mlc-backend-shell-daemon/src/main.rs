//! `mlc-backend-shell-daemon` — Mode-2 backend.
//!
//! Owns one `Arc<BackendCore>`, no embedded `ChartApp`. Drives a 60Hz tick
//! loop that drains the live-data broadcast and writes into the shared
//! bubble store via `BackendCore::tick_writers`. Serves a full HTTP control
//! plane for external apps to attach, query, and observe events.
//!
//! ## Endpoints
//!
//! ### Health
//! - `GET  /health`
//!
//! ### Apps / sessions
//! - `POST   /v1/apps`                      — register AppSession
//! - `GET    /v1/apps`                      — list sessions
//! - `DELETE /v1/apps/:app_id`              — deregister
//! - `POST   /v1/apps/:app_id/heartbeat`    — keep-alive
//! - `POST   /v1/apps/_reap`                — force reap idle sessions
//!
//! ### Bubbles
//! - `GET    /v1/bubbles`                   — list ids
//! - `POST   /v1/bubbles`                   — create bubble
//! - `GET    /v1/bubbles/:bid`              — snapshot DTO
//! - `DELETE /v1/bubbles/:bid`              — remove
//! - `GET    /v1/bubbles/:bid/bars`         — OHLCV bars (?limit=N)
//! - `GET    /v1/bubble/:bid/bars/:kind`    — legacy count
//!
//! ### Exchanges
//! - `GET  /v1/exchanges`
//!
//! ### Mini-ticker
//! - `GET  /v1/mini-ticker`
//! - `GET  /v1/mini-ticker/:key`
//!
//! ### Presets
//! - `GET    /v1/presets`
//! - `GET    /v1/presets/:preset_id`
//! - `DELETE /v1/presets/:preset_id`
//!
//! ### Templates
//! - `GET  /v1/templates/summary`
//!
//! ### Watchlists
//! - `GET  /v1/watchlists`
//!
//! ### Drawings (full CRUD)
//! - `GET    /v1/drawings`
//! - `GET    /v1/catalog/primitives`
//! - `POST   /v1/drawings/:bubble_id`
//! - `PATCH  /v1/drawings/:bubble_id/:primitive_id`
//! - `DELETE /v1/drawings/:bubble_id/:primitive_id`
//!
//! ### Indicators (full CRUD)
//! - `GET    /v1/indicators`
//! - `GET    /v1/catalog/indicators`
//! - `POST   /v1/indicators`
//! - `PATCH  /v1/indicators/:id`
//! - `DELETE /v1/indicators/:id`
//!
//! ### Alerts (full CRUD)
//! - `GET    /v1/alerts`
//! - `POST   /v1/alerts`
//! - `PATCH  /v1/alerts/:id`
//! - `DELETE /v1/alerts/:id`
//!
//! ### Bridge
//! - `POST /v1/bridge/request-bars`
//!
//! ### Trading
//! - `GET  /v1/trading/connectors`
//!
//! ### Events (SSE)
//! - `GET  /v1/events`

mod routes;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tokio::sync::broadcast;

use mlc_backend::{BackendCore, BubbleEvent};
use mlc_backend_data_bridge::DataBridge;

const DEFAULT_PORT: u16 = 19470;
const TICK_PERIOD: Duration = Duration::from_millis(16);
const SESSION_IDLE_TIMEOUT_SECS: u64 = 60;
const EVENTS_CHANNEL_CAP: usize = 4096;

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<BackendCore>,
    pub tick_count: Arc<std::sync::atomic::AtomicU64>,
    pub events_tx: broadcast::Sender<BubbleEvent>,
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

    // Construct a dedicated multi-thread runtime for the DataBridge background
    // tasks. DataBridge::new() is now async; we block_on it here before the
    // HTTP runtime is created, so the two runtimes stay independent.
    let bridge_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("mlc-bridge")
        .enable_all()
        .build()?;
    let (bridge, _live_rx, _connector_ready_rx) = bridge_rt.block_on(DataBridge::new());
    // bridge_rt is kept alive by moving it into a local; drop order ensures
    // bridge tasks outlive core.
    let bridge = Arc::new(bridge);
    let core = BackendCore::new(bridge.clone(), None);
    // bridge_rt kept in scope until end of main so spawned bridge tasks live.
    let _bridge_rt = bridge_rt;

    tracing::info!(target: "mlc-backend-shell-daemon", "backend constructed, tick loop starting");

    let (events_tx, _) = broadcast::channel::<BubbleEvent>(EVENTS_CHANNEL_CAP);

    let state = AppState {
        core: core.clone(),
        tick_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        events_tx: events_tx.clone(),
    };

    // HTTP runtime — separate from the bridge runtime.
    let http_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .thread_name("mlc-backend-shell-daemon-http")
        .build()?;

    let tick_state = state.clone();
    http_rt.spawn(async move {
        let mut interval = tokio::time::interval(TICK_PERIOD);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let now_ms = wall_clock_ms();
            let mut events = tick_state.core.tick_writers(now_ms);
            // Budget-sliced catalogue ingestion (owner ruling 2026-09-05,
            // wasm-lag-measurements doc) — without this, `/v1/exchanges`
            // would never populate: `tick_writers`' `SymbolsLoaded` arm only
            // enqueues an exchange id now, `ingest_catalogues` does the
            // actual `SymbolInfo` → `CatalogueSymbol` conversion. No render
            // frame to protect here, so the native (larger) budget is fine.
            events.extend(tick_state.core.ingest_catalogues(mlc_backend::CATALOGUE_INGEST_BUDGET_NATIVE));
            for e in events {
                let _ = tick_state.events_tx.send(e);
            }
            tick_state
                .tick_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Reap idle sessions.
            let now_secs = now_ms / 1000;
            tick_state
                .core
                .reap_idle_sessions(now_secs, SESSION_IDLE_TIMEOUT_SECS);
        }
    });

    let app = Router::new()
        .route("/health", get(health))
        .merge(routes::apps::routes())
        .merge(routes::bubbles::routes())
        .merge(routes::exchanges::routes())
        .merge(routes::mini_ticker::routes())
        .merge(routes::presets::routes())
        .merge(routes::templates::routes())
        .merge(routes::watchlists::routes())
        .merge(routes::drawings::routes())
        .merge(routes::indicators::routes())
        .merge(routes::alerts::routes())
        .merge(routes::bridge::routes())
        .merge(routes::trading::routes())
        .merge(routes::events::routes())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    http_rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(target: "mlc-backend-shell-daemon", "listening on http://{addr}");
        tokio::select! {
            res = axum::serve(listener, app) => res?,
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(target: "mlc-backend-shell-daemon", "ctrl-c received, shutting down");
            }
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}

fn wall_clock_ms() -> u64 {
    use web_time::SystemTime;
    use web_time::UNIX_EPOCH;
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
