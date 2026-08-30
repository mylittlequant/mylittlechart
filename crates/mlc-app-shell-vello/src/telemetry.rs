//! Thread-safe telemetry snapshot handed to `mlc_updater::start()`.
//!
//! `App` cannot implement `mlc_updater::TelemetrySource` itself: the trait
//! requires `Send + Sync + 'static` so the updater's background task (running
//! on a Tokio worker thread) can hold an `Arc<dyn TelemetrySource>`, but `App`
//! owns winit `Window`s and GPU surfaces that are not `Send`. This struct
//! mirrors just the handful of scalar fields telemetry needs — `App` refreshes
//! it once a second from its own single-threaded state. Same
//! producer/consumer shape as `App::perf_sys_sample` (background sampler
//! writes, main thread reads), with the writer/reader roles swapped: here the
//! main thread writes and the updater's background task reads.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct AppTelemetry {
    started_at: web_time::Instant,
    /// Shared connector pool — `ws_task_count_total()` is cheap and already
    /// thread-safe (called from async HTTP-agent handlers elsewhere), so it
    /// is read live here rather than mirrored through an extra atomic.
    bridge: Arc<mlc_backend_data_bridge::DataBridge>,
    gpu_name: Mutex<String>,
    screen_width: AtomicU32,
    screen_height: AtomicU32,
    connector_count: AtomicU32,
    window_count: AtomicU32,
    fps_bits: AtomicU32,
    total_bars: AtomicU64,
}

impl AppTelemetry {
    pub fn new(bridge: Arc<mlc_backend_data_bridge::DataBridge>) -> Self {
        Self {
            started_at: web_time::Instant::now(),
            bridge,
            gpu_name: Mutex::new(String::new()),
            screen_width: AtomicU32::new(0),
            screen_height: AtomicU32::new(0),
            connector_count: AtomicU32::new(0),
            window_count: AtomicU32::new(0),
            fps_bits: AtomicU32::new(0.0_f32.to_bits()),
            total_bars: AtomicU64::new(0),
        }
    }

    /// Called once, right after the GPU adapter is queried in `create_window`
    /// (the same spot that sets `App::gpu_name`).
    pub fn set_gpu_name(&self, name: &str) {
        let mut g = self.gpu_name.lock().unwrap_or_else(|e| e.into_inner());
        g.clear();
        g.push_str(name);
    }

    /// Called wherever a window's inner size becomes known — window creation
    /// and `WindowEvent::Resized` — so telemetry never has to open a new
    /// event path of its own.
    pub fn set_screen_size(&self, width: u32, height: u32) {
        self.screen_width.store(width, Ordering::Relaxed);
        self.screen_height.store(height, Ordering::Relaxed);
    }

    /// Refresh the scalar counters. Called once a second from `App`'s
    /// existing per-second maintenance pass (main thread) — never from
    /// `PerformanceData`, which only updates while the monitoring panel is
    /// open and would otherwise make telemetry silently go to zero.
    pub fn update_periodic(&self, fps: f64, connector_count: usize, window_count: usize, total_bars: u64) {
        self.fps_bits.store((fps as f32).to_bits(), Ordering::Relaxed);
        self.connector_count.store(connector_count as u32, Ordering::Relaxed);
        self.window_count.store(window_count as u32, Ordering::Relaxed);
        self.total_bars.store(total_bars, Ordering::Relaxed);
    }
}

impl mlc_updater::TelemetrySource for AppTelemetry {
    fn collect(&self) -> mlc_updater::telemetry::TelemetryPayload {
        let gpu_name = self
            .gpu_name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        mlc_updater::telemetry::TelemetryPayload {
            device_id: mlc_updater::telemetry::get_or_create_device_id(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            gpu_name,
            screen_width: self.screen_width.load(Ordering::Relaxed),
            screen_height: self.screen_height.load(Ordering::Relaxed),
            connector_count: self.connector_count.load(Ordering::Relaxed),
            window_count: self.window_count.load(Ordering::Relaxed),
            avg_fps: f32::from_bits(self.fps_bits.load(Ordering::Relaxed)),
            uptime_secs: self.started_at.elapsed().as_secs(),
            total_bars: self.total_bars.load(Ordering::Relaxed),
            ws_connections: self.bridge.ws_task_count_total() as u32,
        }
    }
}
