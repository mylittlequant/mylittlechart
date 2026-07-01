//! `mlc-app-shell-headless` — mylittlechart full ChartApp without a GPU.
//!
//! Owns one `ChartApp` (a single window), an `AgentState`, and the same
//! axum HTTP server the desktop binary spawns. Ticks the chart at ~60 Hz
//! so live data flows the same way it does in the GPU build. External
//! clients drive everything via HTTP — switch symbol, change chart type,
//! query bar counts, fetch snapshots.
//!
//! ## Status — kept as the e2e harness (2026-06-20)
//!
//! After the backend-cutover (`Arc<BackendCore>` lifted bubbles / indicators
//! / drawings / presets / templates / watchlists / mini-ticker / exchanges
//! / trading / sessions into a shared data plane), the natural Mode-2 build
//! is `mlc-app-shell-daemon-lightweight` — a real *no-ChartApp* HTTP server that owns
//! only `BackendCore` and exposes its CRUD over `/v1/...`. That is the
//! right shape for "1 backend = N apps".
//!
//! This binary keeps ChartApp around without a GPU. After the cutover
//! that is technically redundant for production use cases — daemon is a
//! cleaner shape for any external agent talking to backend. But the
//! ChartApp orchestration layer (writer-arm split, per-app reactor in
//! `frame_lifecycle/react.rs`, preset park/unpark, sync_group, undo
//! commands, autosave, sidebar dirty flags) still needs a regression
//! harness that exercises *the real ChartApp*, not just BackendCore.
//! `mlc-app-shell-headless` is that harness — it loads a real profile, ticks at
//! 60 Hz against the live bridge, and accepts HTTP commands so
//! regression tests can drive end-to-end flows without a GPU.
//!
//! Out of scope: multi-window (only one ChartApp), screenshot (returns
//! "not supported"), GPU-rendered scenes.

use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use mlc_app::ChartApp;
use mlc_backend_agent_server::AgentState;
use mlc_app_agent_api::host::{AgentHost, WindowRef, WindowRefMut};
use mlc_app_agent_api::{commands, snapshots};

const DEFAULT_PORT: u16 = 19460;
const DEFAULT_SYMBOL: &str = "BTCUSDT";
const DEFAULT_WINDOW_ID: &str = "headless";
const VIRTUAL_WIDTH: u32 = 1920;
const VIRTUAL_HEIGHT: u32 = 1080;
const TICK_PERIOD: Duration = Duration::from_millis(16);

/// One window owned by the daemon: id + ChartApp.
struct HostWindow {
    window_id: String,
    chart: ChartApp,
}

struct HeadlessHost {
    windows: Vec<HostWindow>,
}

impl AgentHost for HeadlessHost {
    fn windows(&self) -> Vec<WindowRef<'_>> {
        self.windows.iter()
            .map(|w| WindowRef { window_id: &w.window_id, chart: &w.chart })
            .collect()
    }

    fn windows_mut(&mut self) -> Vec<WindowRefMut<'_>> {
        self.windows.iter_mut()
            .map(|w| WindowRefMut { window_id: &w.window_id, chart: &mut w.chart })
            .collect()
    }
    // default request_screenshot impl returns "not supported"
}

fn main() {
    let port = std::env::var("MLC_HEADLESS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let symbol = std::env::var("MLC_HEADLESS_SYMBOL")
        .unwrap_or_else(|_| DEFAULT_SYMBOL.to_string());

    eprintln!("[mlc-app-shell-headless] booting symbol={} on port {}", symbol, port);

    // Dedicated runtime for the axum HTTP server. The bridge has its OWN
    // runtime owned by `ChartApp::new` and enters it internally on every
    // `spawn_bg(...)`; this `server_rt` is purely for serving HTTP requests
    // off the tick loop.
    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mlc-app-shell-headless-srv")
        .build()
        .expect("build server tokio runtime");

    // `ChartApp::new` builds its own multi-thread tokio runtime and runs
    // `DataBridge::new().await` on it via `block_on`; the bridge captures
    // that runtime's `Handle` and enters it internally on every
    // `spawn_bg(...)` so the tick loop here doesn't need its own guard.
    let mut chart = ChartApp::new(&symbol);
    chart.resize(VIRTUAL_WIDTH, VIRTUAL_HEIGHT);
    let bridge = chart.bridge_arc();
    let core = chart.core.clone();

    let mut host = HeadlessHost {
        windows: vec![HostWindow {
            window_id: DEFAULT_WINDOW_ID.to_string(),
            chart,
        }],
    };

    let agent_state = Arc::new(AgentState::new(bridge, env!("CARGO_PKG_VERSION")));
    agent_state.attach_core(core);

    let _server_handle = mlc_backend_agent_server::start_server(agent_state.clone(), server_rt.handle(), port);
    eprintln!("[mlc-app-shell-headless] HTTP server spawned on http://127.0.0.1:{}", port);

    // Ctrl+C handler — flips an AtomicBool so the tick loop exits cleanly.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc_handler(move || stop.store(true, std::sync::atomic::Ordering::SeqCst));
    }

    let started = Instant::now();
    let mut snapshot_phase: u8 = 0;
    let mut last_snapshot = Instant::now();

    loop {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            eprintln!("[mlc-app-shell-headless] stop signalled, exiting");
            break;
        }

        let now_ms = started.elapsed().as_millis() as u64;
        for w in host.windows.iter_mut() {
            w.chart.tick(now_ms);
            w.chart.prepare_frame(VIRTUAL_WIDTH as f64, VIRTUAL_HEIGHT as f64);
            w.chart.drain_pending_cold_apply();
            w.chart.drain_pending_cure_after_preset_load();
        }

        commands::drain_agent_commands(&mut host, &agent_state);

        // Round-robin snapshot rebuild ~1 Hz when an agent is active.
        if last_snapshot.elapsed() >= Duration::from_secs(1) {
            if agent_state.accessed_within(Duration::from_secs(5)) {
                match snapshot_phase % 4 {
                    0 => snapshots::update_indicator_snapshot(&host, &agent_state),
                    1 => snapshots::update_terminal_snapshot(&host, &agent_state),
                    2 => snapshots::update_watchlist_snapshot(&host, &agent_state),
                    _ => snapshots::update_connector_snapshot(&agent_state),
                }
                snapshot_phase = snapshot_phase.wrapping_add(1);
            }
            last_snapshot = Instant::now();
        }

        std::thread::sleep(TICK_PERIOD);
    }
}

fn ctrlc_handler<F: FnOnce() + Send + 'static>(cb: F) {
    let mut cb = Some(cb);
    let _ = std::thread::Builder::new().name("mlc-app-shell-headless-ctrlc".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
            if let Some(cb) = cb.take() { cb(); }
        });
    });
}
