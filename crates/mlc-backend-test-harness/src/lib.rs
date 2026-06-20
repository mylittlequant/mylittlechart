//! `mlc-backend-test-harness` — programmatic test infrastructure for
//! `mlc-backend-core`, `mlc-backend-daemon`, and the `/api/v1/apps` surface in
//! `mlc-agentic-server`.
//!
//! This is **not** a `tests/` folder. It's a reusable library crate. Two
//! consumers today:
//!
//! 1. The `mlc-backend-harness-run` binary in this crate, which executes every
//!    scenario in order and writes a pass/fail report.
//! 2. Future CI hooks that may want to embed individual scenarios in a longer
//!    pipeline.
//!
//! ## What it actually checks
//!
//! Real `BackendCore` instances are constructed with a real `DataBridge`
//! tokio runtime. Synthetic `LiveUpdate` events are injected through
//! `DataBridge::debug_broadcast_tx()`. The harness then drives
//! `BackendCore::tick_writers()` and inspects:
//!
//! - bubble store contents after each write
//! - the `BubbleEvent` emitted per arm
//! - `SessionRegistry` lifecycle behaviour
//! - `DrawingRegistry` per-`ChartPanelId` semantics
//! - `IndicatorManager`'s `AtomicU64` `current_render_window_id` round-trip
//! - The Mode-2 daemon binary over real HTTP (spawned by the harness)
//!
//! Failures are returned as `HarnessError`, not panics. The runner binary
//! formats them into a report.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use mlc_backend_core::BackendCore;
use mlc_live_data::{DataBridge, LiveUpdate};
use tokio::sync::broadcast;

pub mod scenarios;

/// Test failure with a human-readable reason. Scenarios return this; the
/// runner collects them across all scenarios.
#[derive(Debug, Clone)]
pub struct HarnessError {
    pub scenario: &'static str,
    pub message: String,
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.scenario, self.message)
    }
}

impl std::error::Error for HarnessError {}

pub type HarnessResult<T> = Result<T, HarnessError>;

/// Wrapper around a real `Arc<BackendCore>` with helpers for synthetic
/// `LiveUpdate` injection and a clock the test controls.
pub struct BackendHarness {
    pub core: Arc<BackendCore>,
    pub bridge: Arc<DataBridge>,
    inject_tx: broadcast::Sender<LiveUpdate>,
    sim_now_ms: u64,
}

impl BackendHarness {
    /// Spin a real backend with no trading manager and no profile glue.
    pub fn fresh() -> Self {
        let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new();
        let bridge = Arc::new(bridge);
        let inject_tx = bridge.debug_broadcast_tx();
        let core = BackendCore::new(bridge.clone(), None);
        Self {
            core,
            bridge,
            inject_tx,
            sim_now_ms: 0,
        }
    }

    /// Push a synthetic `LiveUpdate` into the same broadcast channel the
    /// bridge uses for real connector traffic. `tick_writers` will receive it
    /// the next time it's called.
    pub fn inject(&self, update: LiveUpdate) -> HarnessResult<()> {
        self.inject_tx.send(update).map_err(|e| HarnessError {
            scenario: "inject",
            message: format!("broadcast send failed: {e}"),
        })?;
        Ok(())
    }

    /// Step the simulated wall clock by `ms` and call `tick_writers`. Returns
    /// the events emitted.
    pub fn tick(&mut self, ms: u64) -> Vec<mlc_backend_core::BubbleEvent> {
        self.sim_now_ms = self.sim_now_ms.saturating_add(ms);
        self.core.tick_writers(self.sim_now_ms)
    }

    /// Current simulated wall-clock (milliseconds).
    pub fn now_ms(&self) -> u64 {
        self.sim_now_ms
    }
}

/// Spawn the `mlc-backend-daemon` binary in the background. The binary is
/// expected to have been built (cargo `--bin mlc-backend-daemon`).
///
/// Returns a guard that kills the process on drop and the chosen port.
///
/// The default daemon port is `19470`; the harness picks a high port from the
/// ephemeral range to avoid collisions when run in CI alongside other daemons.
pub struct DaemonGuard {
    pub port: u16,
    child: Option<tokio::process::Child>,
}

impl DaemonGuard {
    pub async fn spawn() -> HarnessResult<Self> {
        Self::spawn_with_port(19470).await
    }

    pub async fn spawn_with_port(port: u16) -> HarnessResult<Self> {
        let exe_name = if cfg!(windows) {
            "mlc-backend-daemon.exe"
        } else {
            "mlc-backend-daemon"
        };

        // Try `target/debug/<exe>` then `target/release/<exe>`.
        let cwd = std::env::current_dir().map_err(|e| HarnessError {
            scenario: "daemon-spawn",
            message: format!("getcwd: {e}"),
        })?;

        let debug_path = cwd.join("target").join("debug").join(exe_name);
        let release_path = cwd.join("target").join("release").join(exe_name);

        let exe = if debug_path.exists() {
            debug_path
        } else if release_path.exists() {
            release_path
        } else {
            return Err(HarnessError {
                scenario: "daemon-spawn",
                message: format!(
                    "daemon binary not found at {debug:?} or {release:?} — run cargo build -p mylittlechart-backend-daemon first",
                    debug = debug_path,
                    release = release_path,
                ),
            });
        };

        let child = tokio::process::Command::new(exe)
            .env("MLC_BACKEND_DAEMON_PORT", port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| HarnessError {
                scenario: "daemon-spawn",
                message: format!("spawn: {e}"),
            })?;

        // Wait for /health to answer (up to 5s).
        let url = format!("http://127.0.0.1:{port}/health");
        let client = reqwest::Client::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    break;
                }
            }
            if std::time::Instant::now() > deadline {
                return Err(HarnessError {
                    scenario: "daemon-spawn",
                    message: format!("daemon did not become healthy on :{port} within 5s"),
                });
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok(Self {
            port,
            child: Some(child),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}
