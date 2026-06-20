//! `mlc-headless` — mylittlechart backend daemon, no GPU.
//!
//! Owns one `ChartApp` (a single window), an `AgentState`, and the same
//! axum HTTP server the desktop binary spawns. Ticks the chart at ~60 Hz
//! so live data flows the same way it does in the GPU build. External
//! clients drive everything via HTTP — switch symbol, change chart type,
//! query bar counts, fetch snapshots.
//!
//! Out of scope for now: multi-window (only one ChartApp instance is held),
//! screenshot (the host returns "not supported"), GPU-rendered scenes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mlc_app::ChartApp;
use mlc_agentic_server::AgentState;
use mlc_headless_core::host::{AgentHost, WindowRef, WindowRefMut};
use mlc_headless_core::{commands, snapshots};

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

    eprintln!("[mlc-headless] booting symbol={} on port {}", symbol, port);

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

    // Server runtime: dedicated multi-thread runtime for axum + the snapshot
    // freshness check. The ChartApp drives its own bridge / connectors
    // runtime internally; we do NOT share it.
    let server_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mlc-headless-srv")
        .build()
        .expect("build server tokio runtime");
    let _server_handle = mlc_agentic_server::start_server(agent_state.clone(), &server_rt, port);
    eprintln!("[mlc-headless] HTTP server spawned on http://127.0.0.1:{}", port);

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
            eprintln!("[mlc-headless] stop signalled, exiting");
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
    let _ = std::thread::Builder::new().name("mlc-headless-ctrlc".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
            if let Some(cb) = cb.take() { cb(); }
        });
    });
}
