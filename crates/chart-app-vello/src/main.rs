//! chart-app-vello — minimal winit + vello runner for chart-app
//!
//! Supports multiple windows sharing a single DataBridge (tokio runtime +
//! connector pool).  Each window has its own ChartApp with independent
//! tabs/presets but receives live updates via broadcast channels.
//! Creates windows on demand; closing the last window exits the process.

mod chrome;

/// Win32 cursor position polling helpers.
///
/// When the user places the first point of a drawing primitive (is_drawing() is true)
/// the mouse is not pressed, so winit stops sending CursorMoved events once the cursor
/// leaves the window boundary and the preview freezes.  Instead of relying on OS capture
/// (which winit interferes with), we poll GetCursorPos on every frame so the preview
/// updates continuously regardless of cursor position.
#[cfg(target_os = "windows")]
mod win32_capture {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    extern "system" {
        fn GetCursorPos(lpPoint: *mut POINT) -> i32;
        fn ScreenToClient(hWnd: isize, lpPoint: *mut POINT) -> i32;
    }

    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }

    /// Get cursor position in window-local coordinates.
    pub fn get_cursor_pos(window: &Window) -> Option<(f64, f64)> {
        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_ref() {
                let mut pt = POINT { x: 0, y: 0 };
                unsafe {
                    if GetCursorPos(&mut pt) != 0 {
                        ScreenToClient(h.hwnd.get() as isize, &mut pt);
                        return Some((pt.x as f64, pt.y as f64));
                    }
                }
            }
        }
        None
    }
}

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Pipelined GPU rendering ──────────────────────────────────────────────────
//
// The render thread owns no GPU resources itself.  Instead it receives a list
// of raw `PerWindowState` pointers (as `usize`) from the main thread and calls
// `submit_window_gpu` on each.  The main thread builds the next frame's scene
// into `pw.scene` while the GPU thread renders the previous frame's scene from
// `pw.gpu_scene`.  A swap (`std::mem::swap(&mut pw.scene, &mut pw.gpu_scene)`)
// happens just before each GPU submit, so the two threads never touch the same
// `Scene` simultaneously.

/// Sent from the main thread to the persistent GPU render thread.
enum GpuCommand {
    /// Submit the scenes currently in `pw.gpu_scene` for each address.
    /// Each entry is `(pw_addr, msaa_samples)`.
    Submit {
        window_addrs: Vec<usize>,
        msaa_samples: u8,
        render_cx_addr: usize,
    },
    /// Shut down the render thread cleanly.
    Shutdown,
}

/// Sent from the GPU render thread back to the main thread after a frame.
struct GpuDone {
    /// Set to `true` if any window returned an OOM error from the surface.
    close_all: bool,
    /// Wall-clock µs spent on GPU submit across all windows.
    total_gpu_us: u64,
}

use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::{self, PresentMode};
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, Scene};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{CursorIcon, Window, WindowId},
};
use zengeld_chart::CursorStyle;
use sysinfo::{System, Pid, ProcessesToUpdate, ProcessRefreshKind};

use vello_context::VelloGpuRenderContext;

fn cursor_style_to_winit(style: CursorStyle) -> CursorIcon {
    match style {
        CursorStyle::Default => CursorIcon::Default,
        CursorStyle::Pointer => CursorIcon::Pointer,
        CursorStyle::Grab => CursorIcon::Grab,
        CursorStyle::Grabbing => CursorIcon::Grabbing,
        CursorStyle::Move => CursorIcon::Move,
        CursorStyle::NsResize => CursorIcon::NsResize,
        CursorStyle::EwResize => CursorIcon::EwResize,
        CursorStyle::NeswResize => CursorIcon::NeswResize,
        CursorStyle::NwseResize => CursorIcon::NwseResize,
        CursorStyle::Crosshair => CursorIcon::Crosshair,
        CursorStyle::NotAllowed => CursorIcon::NotAllowed,
        CursorStyle::None => CursorIcon::Default,
    }
}

/// Per-window state: GPU resources, chart instance, and input state.
struct PerWindowState {
    // GPU rendering
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    /// The scene being built on the main thread for the *next* GPU frame.
    scene: Scene,
    /// The scene currently being rendered (or ready to render) by the GPU thread.
    ///
    /// Before signalling the GPU thread, the main thread swaps `scene` and
    /// `gpu_scene` so the GPU thread renders the completed scene while the
    /// main thread begins building the next frame without blocking.
    gpu_scene: Scene,
    /// Cached toolbar sub-scene — only rebuilt when `toolbar_dirty` is set.
    /// On rebuild the scene is composited into `scene` via `Scene::append`.
    toolbar_scene: Scene,
    /// True when the toolbar needs to be redrawn into `toolbar_scene`.
    /// Set on hover/click/resize/tab-switch; cleared after rebuild.
    toolbar_dirty: bool,
    /// Cached sidebar sub-scene — only rebuilt when `sidebar_dirty_scene` is set.
    /// On rebuild the scene is composited into `scene` via `Scene::append`.
    sidebar_scene: Scene,
    /// True when the sidebar needs to be redrawn into `sidebar_scene`.
    /// Set when sidebar data changes, mouse enters sidebar area, resize, or
    /// theme changes.  Cleared after rebuild.  Distinct from
    /// `chart.sidebar_data_dirty` which guards data population; this flag
    /// guards the vector-graphics rebuild.
    sidebar_dirty_scene: bool,
    // ChartApp — per-window tabs/presets, shared DataBridge via broadcast
    chart: chart_app::ChartApp,
    // Input state
    last_mouse_pos: (f64, f64),
    mouse_pressed: bool,
    drag_start_pos: Option<(f64, f64)>,
    last_drag_pos: Option<(f64, f64)>,
    last_click: Option<(std::time::Instant, f64, f64)>,
    screenshot_pending: bool,
    /// Pending agent screenshot requests — each entry is `(chart_id, response_tx)`.
    ///
    /// Accumulated by [`drain_agent_commands`] and processed at the end of
    /// each render pass, after the GPU frame has been submitted.
    pending_agent_screenshots: Vec<(u64, tokio::sync::oneshot::Sender<Result<zengeld_server::state::ScreenshotData, String>>)>,
    modifiers: winit::keyboard::ModifiersState,
    drawing_capture: bool,
    // Chrome
    chrome_state: chrome::ChromeState,
    /// App shutdown signal — set by chrome X button or OS-level CloseRequested.
    /// When any window has this flag, about_to_wait saves ALL windows and exits.
    close_requested: bool,
    /// Set to true when the new-window button is clicked; drained in about_to_wait.
    spawn_new_window: bool,
    /// Unique identifier for this window (mirrors `chart.window_id`).
    /// Stored here so we can identify the primary window during coordinated save
    /// without having to borrow `chart` when iterating over all windows.
    window_id: String,
    /// Set when context menu "Close Window" is selected; drained in about_to_wait.
    close_window_requested: bool,
    /// Set when context menu "Delete Window" is selected; drained in about_to_wait.
    delete_window_requested: bool,
    /// Last sidebar row index the cursor was hovering over.
    ///
    /// Used to suppress `sidebar_dirty_scene` when the cursor moves within
    /// the same row — hover highlight only changes at row boundaries (36 px).
    last_sidebar_hover_row: Option<usize>,
    /// Active render backend for this window — synced from `App.render_backend`
    /// each frame before the parallel scene-build phase.
    render_backend: sidebar_content::state::RenderBackend,
    /// Instanced renderer for the wGPU backend (created lazily).
    instanced_renderer: Option<uzor_backend_wgpu_instanced::InstancedRenderer>,
    /// Unified draw command list from the last chart render (instanced backend).
    /// Preserves painter's z-order — later entries draw on top of earlier ones.
    instanced_commands: Vec<uzor_backend_wgpu_instanced::DrawCmd>,
    /// GPU-side copy of draw commands (double-buffered like scene/gpu_scene).
    /// The GPU thread consumes this while the main thread fills `instanced_commands`.
    gpu_instanced_commands: Vec<uzor_backend_wgpu_instanced::DrawCmd>,
    /// CPU-rendered chart pixels (RGBA8, stored from build phase for GPU upload).
    cpu_chart_pixels: Vec<u8>,
    /// Dimensions of the CPU-rendered chart image.
    cpu_chart_dims: (u32, u32),
    /// GPU-side copies for the GPU thread (double-buffered like gpu_scene).
    gpu_cpu_chart_pixels: Vec<u8>,
    gpu_cpu_chart_dims: (u32, u32),
    /// VelloHybrid renderer (created lazily on first use).
    hybrid_renderer: Option<vello_hybrid::Renderer>,
    /// VelloHybrid render context built during chart render phase.
    hybrid_ctx: Option<uzor_backend_vello_hybrid::VelloHybridRenderContext>,
    /// GPU-side double-buffered copy for the GPU thread.
    gpu_hybrid_ctx: Option<uzor_backend_vello_hybrid::VelloHybridRenderContext>,
}

/// A pending request to open a new window.
struct SpawnRequest {
    /// The window that spawned this request — used for cascade placement.
    cascade_from: Option<WindowId>,
}

/// Application-level shared state — single source of truth for data that is
/// shared across all windows (watchlist, connector preferences).
///
/// These fields were previously duplicated inside each `ChartApp`. Moving them
/// here means there is one authoritative copy; per-window copies are synced
/// from/to `AppState` each frame via `sync_app_state_to_window` /
/// `sync_app_state_from_window`.
struct AppState {
    /// Watchlist manager — all lists, groups, and symbols.
    watchlist_manager: chart_app::WatchlistManager,
    /// Per-exchange enabled/disabled flag (keyed by `ExchangeId::as_str()`).
    connector_enabled: std::collections::HashMap<String, bool>,
    /// All chart presets loaded at startup (keyed by preset id).
    presets: std::collections::HashMap<String, zengeld_chart::preset::preset::ChartPreset>,
    /// Preset ids that have been modified but not yet persisted.
    preset_dirty_ids: std::collections::HashSet<String>,
    /// Settings snapshots — shared across all windows (last-used settings per category).
    snapshots: zengeld_chart::user_manager::manager::SettingsSnapshots,
    /// Template manager — single source of truth for all template types across windows.
    template_manager: zengeld_chart::templates::manager::TemplateManager,
    /// Active theme preset name (e.g. "dark", "light").
    /// Single source of truth — synced to all windows each frame.
    theme_preset: String,
    /// Device identity — read-only after startup, shared to avoid stale per-window copies.
    device_name: String,
    app_version: String,

    // ── Sync dirty flags ──────────────────────────────────────────────────────
    // Set to `true` when the corresponding data changes; reset after syncing to
    // all windows.  Prevents per-frame deep clones when nothing changed.
    /// Presets map changed — need to clone to all windows.
    presets_dirty: bool,
    /// Template manager changed — need to clone to all windows.
    templates_dirty: bool,
    /// Settings snapshots changed — need to clone to all windows.
    snapshots_dirty: bool,
    /// Watchlist manager changed — need to clone to all windows.
    watchlists_dirty: bool,
    /// Connector enabled map changed — need to clone to all windows.
    connectors_dirty: bool,

    // ── Performance settings ──────────────────────────────────────────────────

    /// Indicator recalculation mode — controls CPU/accuracy trade-off.
    /// Synced to every window's `indicator_manager.recalc_mode` each frame.
    recalc_mode: chart_app::RecalcMode,

    // ── Agent API server settings ────────────────────────────────────────────
    /// Whether the server is enabled.
    server_enabled: bool,
    /// Port the server listens on.
    server_port: u16,
    /// Registered API keys with permission tiers.
    ///
    /// Canonical source — keys managed via the REST API are reflected here and
    /// persisted to the user profile on the next save_all() call.
    agent_api_keys: Vec<zengeld_chart::StoredApiKey>,
}

impl AppState {
    /// Initialise from a loaded watchlist file and user profile.
    fn from_profile(
        profile: &zengeld_chart::UserProfile,
        presets: std::collections::HashMap<String, zengeld_chart::preset::preset::ChartPreset>,
        snapshots: zengeld_chart::user_manager::manager::SettingsSnapshots,
        template_manager: zengeld_chart::templates::manager::TemplateManager,
    ) -> Self {
        let default_wl = || {
            chart_app::WatchlistManager::new(vec![
                chart_app::WatchlistSymbol::new("BTCUSDT".to_string(), "Binance".to_string()),
            ])
        };

        let watchlist_manager = {
            let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
            if watchlists_path.exists() {
                zengeld_chart::load_json::<chart_app::WatchlistManager>(&watchlists_path)
                    .unwrap_or_else(|e| {
                        eprintln!("[AppState] Failed to load watchlists: {}", e);
                        default_wl()
                    })
            } else {
                default_wl()
            }
        };

        // Parse recalc_mode from the profile string.
        let recalc_mode = match profile.recalc_mode.as_str() {
            "PerTick" => chart_app::RecalcMode::PerTick,
            "PerBar"  => chart_app::RecalcMode::PerBar,
            _         => chart_app::RecalcMode::PerFrame, // default / "PerFrame"
        };

        Self {
            watchlist_manager,
            connector_enabled: profile.connector_enabled.clone(),
            presets,
            preset_dirty_ids: std::collections::HashSet::new(),
            snapshots,
            template_manager,
            theme_preset: profile.active_theme.clone(),
            device_name: profile.device_name.clone(),
            app_version: profile.app_version.clone(),
            // Start dirty so the first frame syncs everything to all windows.
            presets_dirty: true,
            templates_dirty: true,
            snapshots_dirty: true,
            watchlists_dirty: true,
            connectors_dirty: true,
            recalc_mode,
            server_enabled: profile.server_enabled,
            server_port: profile.server_port,
            agent_api_keys: {
                // Start with whatever the profile already has.
                let mut keys = profile.agent_api_keys.clone();

                // Migrate legacy single-key field if present and no new keys yet.
                if keys.is_empty() && !profile.agent_api_key.is_empty() {
                    let created_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    keys.push(zengeld_chart::StoredApiKey {
                        key_hash: zengeld_server::state::hash_key(&profile.agent_api_key),
                        label: "migrated-legacy-key".to_string(),
                        tier: "admin".to_string(),
                        created_at,
                        agent_id: None,
                        source: "local".to_string(),
                    });
                }
                keys
            },
        }
    }
}

/// Shared telemetry values written by the App main loop and read by the updater thread.
struct TelemetryShared {
    gpu_name: std::sync::Mutex<String>,
    screen_width: std::sync::atomic::AtomicU32,
    screen_height: std::sync::atomic::AtomicU32,
    window_count: std::sync::atomic::AtomicU32,
    avg_fps_bits: std::sync::atomic::AtomicU32, // f32 bits via f32::to_bits / f32::from_bits
    total_bars: std::sync::atomic::AtomicU64,
    /// Number of unique exchanges (not data streams) currently connected.
    connector_count: std::sync::atomic::AtomicU32,
    /// Total number of active WebSocket connections across all streams.
    ws_connections: std::sync::atomic::AtomicU32,
}

impl TelemetryShared {
    fn new() -> Self {
        Self {
            gpu_name: std::sync::Mutex::new(String::new()),
            screen_width: std::sync::atomic::AtomicU32::new(0),
            screen_height: std::sync::atomic::AtomicU32::new(0),
            window_count: std::sync::atomic::AtomicU32::new(0),
            avg_fps_bits: std::sync::atomic::AtomicU32::new(f32::to_bits(0.0)),
            total_bars: std::sync::atomic::AtomicU64::new(0),
            connector_count: std::sync::atomic::AtomicU32::new(0),
            ws_connections: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

/// Application state — owns all per-window state and the shared render context.
struct App<'s> {
    render_cx: RenderContext,
    windows: HashMap<WindowId, PerWindowState>,
    /// Queue of window spawn requests, drained in about_to_wait.
    pending_spawns: Vec<SpawnRequest>,
    /// Set to true when ALL windows should close.
    close_all_requested: bool,
    /// Default symbol for new charts.
    default_symbol: String,
    /// Shared DataBridge — tokio runtime + connector pool, created once at startup.
    bridge: std::sync::Arc<live_data::DataBridge>,
    /// Saved window states loaded from profile at startup — used in resumed() to restore windows.
    saved_windows: Vec<zengeld_chart::WindowState>,
    /// User profile loaded once at startup.
    profile: zengeld_chart::UserProfile,
    /// UserManager loaded once at startup — shared across all windows.
    ///
    /// Each `new_window()` call clones the relevant fields (presets, templates,
    /// profile snapshot) instead of re-reading from disk. This eliminates
    /// redundant disk I/O when multiple windows are opened.
    user_manager: zengeld_chart::UserManager,
    /// Window that last received focus — used by save_all() to pick a deterministic
    /// source for sidebar/toolbar/preset state instead of relying on HashMap iteration order.
    last_focused: Option<WindowId>,
    /// Application-level shared state (watchlist, connector preferences).
    app_state: AppState,
    /// Dedicated mpsc receiver for `ConnectorReady` events.
    ///
    /// Using mpsc instead of a broadcast subscription avoids holding back the
    /// broadcast buffer: the broadcast consumer at the app level only cared
    /// about `ConnectorReady` and silently dropped every other message, but as
    /// a broadcast subscriber it still had to consume those messages for the
    /// buffer slots to be freed.  A lagging broadcast subscriber caused
    /// "[App] app_live_rx lagged by N messages" and stalled all other receivers.
    app_connector_ready_rx: live_data::ConnectorReadyReceiver,
    _phantom: std::marker::PhantomData<&'s ()>,

    /// Shared state for the internal Agent API server.
    agent_state: Option<std::sync::Arc<zengeld_server::AgentState>>,
    /// Wall-clock time of the last indicator snapshot update.
    last_indicator_snapshot: std::time::Instant,

    /// Alert delivery engine (Telegram, webhook, toast).
    alert_delivery: Option<alert_delivery::AlertDelivery>,
    /// Receiver for toast notifications from the delivery engine.
    toast_rx: Option<tokio::sync::mpsc::UnboundedReceiver<alert_delivery::ToastNotification>>,
    /// Active toast notifications to render as overlays.
    active_toasts: Vec<alert_delivery::ToastNotification>,

    /// Handle for the OTA updater background task.
    updater_handle: Option<zengeld_updater::UpdaterHandle>,

    /// Frame timing — last frame's Instant for FPS calculation.
    last_frame_instant: std::time::Instant,
    /// Rolling FPS average (exponential moving average).
    fps_ema: f64,
    /// Last frame time in ms.
    last_frame_time_ms: f64,
    /// Current FPS limit (0 = unlimited/Poll, otherwise WaitUntil target).
    fps_limit: u32,
    /// Current MSAA sample count (0=off, 4, 8, 16).
    msaa_samples: u8,
    /// Maximum bars per window (0 = unlimited).
    max_bars: usize,
    /// Whether frame timing logs are printed to stderr (toggled from Performance panel).
    perf_log_enabled: bool,
    /// Current selected render backend.
    render_backend: sidebar_content::state::RenderBackend,
    /// Whether VSync is enabled.
    vsync_enabled: bool,
    /// System info — CPU/RAM metrics, refreshed once per second.
    sys: System,
    /// Our process ID for per-process CPU/memory tracking.
    self_pid: Pid,
    /// GPU adapter name (queried once at startup).
    gpu_name: String,
    /// GPU driver info (queried once at startup).
    gpu_driver: String,
    /// Frame counter for timing reports.
    frame_count: u64,
    /// Last time a timing summary was printed.
    last_timing_report: std::time::Instant,
    /// Cached connector count from the last `bridge.collect_metrics()` call.
    /// Refreshed once per second in the indicator-snapshot timer block to avoid
    /// calling `collect_metrics()` (which locks and allocates) every frame.
    cached_connector_count: usize,
    /// Scene-build wall-clock time (µs) from the previous frame, written after
    /// the parallel scene-build phase and read in the next frame's populate block.
    cached_scene_us: u64,
    /// GPU render time (µs) from the previous frame, written after GpuDone is
    /// received and read in the next frame's populate block.
    cached_gpu_us: u64,

    // ── Pipelined GPU render thread ───────────────────────────────────────
    /// Sender side of the command channel to the GPU render thread.
    /// `None` before the first window is created (thread spawned lazily).
    gpu_cmd_tx: Option<std::sync::mpsc::SyncSender<GpuCommand>>,
    /// Receiver side of the result channel from the GPU render thread.
    gpu_done_rx: Option<std::sync::mpsc::Receiver<GpuDone>>,
    /// Handle to the GPU render thread (kept to join on shutdown).
    gpu_thread: Option<std::thread::JoinHandle<()>>,
    /// `true` while the GPU render thread is busy with the previous frame.
    gpu_frame_pending: bool,

    /// Shared atomics written each second so the telemetry thread can read live values.
    telemetry_shared: std::sync::Arc<TelemetryShared>,
}

/// Render toast notifications as semi-transparent overlays in the top-right corner.
///
/// Toasts are stacked from top-right downward. Each toast fades out during its
/// final 20% of display time.  The drawing is done with the flat-rect API from
/// [`uzor::render::RenderContext`] so it works with any backend.
fn render_toasts(
    ctx: &mut dyn uzor::render::RenderContext,
    toasts: &[alert_delivery::ToastNotification],
    window_width: f64,
    window_height: f64,
) {
    use uzor::render::RenderContext;

    let toast_width = 320.0_f64;
    let toast_height = 64.0_f64;
    let padding = 12.0_f64;
    let margin = 8.0_f64;
    let border_thickness = 1.0_f64;

    // Stack toasts from top-right, going downward, below the chrome strip.
    let start_x = window_width - toast_width - margin;
    let start_y = 40.0 + margin; // below chrome strip (32px) + small gap

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for (i, toast) in toasts.iter().enumerate() {
        let y = start_y + (i as f64) * (toast_height + margin);
        if y + toast_height > window_height {
            break; // don't render off-screen
        }

        // Calculate fade-out alpha (last 20% of lifetime fades to zero).
        let remaining = toast.remaining_fraction(now);
        let alpha = if remaining < 0.2 {
            (remaining / 0.2) as f32
        } else {
            1.0_f32
        };

        if alpha <= 0.0 {
            continue;
        }

        let a = alpha as f64;

        // ── Shadow ──────────────────────────────────────────────────────────
        // Simple drop shadow: darker bg offset by 2px.
        ctx.set_fill_color(&format!("rgba(0,0,0,{:.2})", a * 0.4));
        ctx.fill_rect(start_x + 2.0, y + 2.0, toast_width, toast_height);

        // ── Background ──────────────────────────────────────────────────────
        // Dark navy background matching the chart theme.
        ctx.set_fill_color(&format!("rgba(20,24,33,{:.2})", a * 0.92));
        ctx.fill_rect(start_x, y, toast_width, toast_height);

        // ── Border (1px blue accent, four sides via filled rects) ───────────
        let border_a = a * 0.6;
        let border_color = format!("rgba(59,130,246,{:.2})", border_a);
        ctx.set_fill_color(&border_color);
        // top
        ctx.fill_rect(start_x, y, toast_width, border_thickness);
        // bottom
        ctx.fill_rect(start_x, y + toast_height - border_thickness, toast_width, border_thickness);
        // left
        ctx.fill_rect(start_x, y + border_thickness, border_thickness, toast_height - border_thickness * 2.0);
        // right
        ctx.fill_rect(start_x + toast_width - border_thickness, y + border_thickness, border_thickness, toast_height - border_thickness * 2.0);

        // ── Title (accent blue, bold 12px) ───────────────────────────────────
        ctx.set_font("bold 12px sans-serif");
        ctx.set_fill_color(&format!("rgba(59,130,246,{:.2})", a));
        ctx.set_text_align(uzor::render::TextAlign::Left);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
        ctx.fill_text(&toast.title, start_x + padding, y + padding + 6.0);

        // ── Message (muted white, normal 11px) ───────────────────────────────
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&format!("rgba(220,220,230,{:.2})", a * 0.85));
        ctx.fill_text(&toast.message, start_x + padding, y + padding + 6.0 + 18.0);
    }
}

/// Phase 1 of rendering: build the Vello scene for a single window (CPU-only).
///
/// Performs all vector-graphics work: chrome sync, toolbar/sidebar cache
/// management, chart render, overlay compositing.  No GPU calls are made here
/// so multiple windows can run this phase concurrently via
/// `std::thread::scope`.
///
/// Returns the wall-clock microseconds spent building the scene so callers can
/// track the parallel phase duration correctly.
fn build_window_scene(pw: &mut PerWindowState, active_toasts: &[alert_delivery::ToastNotification], frame_time: u64) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let t0 = std::time::Instant::now();

    pw.scene.reset();

    // Sync chrome colours from the chart theme.
    {
        let theme = pw.chart.panel_app.theme_manager.current();
        pw.chrome_state.colors.background = theme.chart.background.clone();
        pw.chrome_state.colors.icon_normal = theme.colors.text_primary.clone();
        pw.chrome_state.colors.icon_hover = theme.colors.text_primary.clone();
        pw.chrome_state.colors.separator = theme.colors.toolbar_divider.clone();
        pw.chrome_state.colors.tab_accent = theme.colors.accent.clone();
    }

    // Sync tabs from open_tabs order.
    {
        let open_tabs = &pw.chart.panel_app.open_tabs;
        let active_id = &pw.chart.panel_app.active_preset_id;
        let mut tabs: Vec<chrome::Tab> = open_tabs
            .iter()
            .filter_map(|tab_id| {
                pw.chart.panel_app.presets.get(tab_id).map(|p| chrome::Tab {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    active: &p.id == active_id,
                })
            })
            .collect();
        if tabs.is_empty() {
            tabs.push(chrome::Tab {
                id: "__default__".to_string(),
                name: "Chart".to_string(),
                active: true,
            });
        } else if !tabs.iter().any(|t| t.active) {
            tabs.insert(
                0,
                chrome::Tab {
                    id: "__default__".to_string(),
                    name: "Chart".to_string(),
                    active: true,
                },
            );
        }
        pw.chrome_state.tabs = tabs;
    }

    use sidebar_content::state::RenderBackend;
    let is_vello_gpu = pw.render_backend == RenderBackend::VelloGpu;

    if is_vello_gpu {
        // ── VelloGpu: everything renders into pw.scene via vello ──────────────

        // Render chrome strip
        {
            let mut chrome_ctx =
                VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state);
            chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64);
        }

        // Toolbar dirty-cache
        if pw.toolbar_dirty {
            pw.toolbar_scene.reset();
            let mut tb_ctx = VelloGpuRenderContext::new(
                &mut pw.toolbar_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_toolbar_only(&mut tb_ctx);
            pw.toolbar_dirty = false;
        }

        // Chart content (chart panels, modals) — toolbar skipped because
        // it is composited from the cached toolbar_scene below.
        {
            let mut render_ctx = VelloGpuRenderContext::new(
                &mut pw.scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render(&mut render_ctx, frame_time, true);
        }

        let sidebar_is_open = pw.chart.sidebar_state.is_right_open();

        // Sidebar scene rebuild (after render, within the same input frame)
        if sidebar_is_open && pw.sidebar_dirty_scene {
            pw.sidebar_scene.reset();
            let mut sb_ctx = VelloGpuRenderContext::new(
                &mut pw.sidebar_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_sidebar_only(&mut sb_ctx);
            pw.sidebar_dirty_scene = false;
        }

        // Composite cached sidebar on top of chart content (below toolbar).
        if sidebar_is_open {
            pw.scene.append(&pw.sidebar_scene, None);
        }

        // Composite cached toolbar on top of chart content.
        pw.scene.append(&pw.toolbar_scene, None);

        // Render chrome context menu overlay
        if pw.chrome_state.context_menu.open {
            let mut overlay_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::render_context_menu(&mut overlay_ctx, &pw.chrome_state.context_menu, &pw.chrome_state.colors);
        }

        // Render toast notification overlays (top-right corner)
        if !active_toasts.is_empty() {
            let mut toast_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            render_toasts(&mut toast_ctx, active_toasts, width as f64, height as f64);
        }
    } else {
        // ── Non-VelloGpu: full backend switch ─────────────────────────────────
        // Everything renders through the selected backend only.
        // No vello scene is built — pw.scene stays empty (reset above).
        // pw.chart.render() renders EVERYTHING: chrome, toolbar, sidebar, modals.
        // We still need chrome hit-zone registration via update_tab_widths.
        //
        // Note: sidebar/toolbar dirty flags are NOT cleared here since those
        // caches are vello-only.  The alt backend re-renders everything each
        // frame so dirty tracking is not needed.

        // Bring uzor::render::RenderContext trait into scope so that
        // save/translate/restore etc. are callable on concrete context types.
        use uzor::render::RenderContext as _;

        match pw.render_backend {
            RenderBackend::InstancedWgpu => {
                // Render chrome at y_offset=0 into a temporary context.
                let mut chrome_ctx = instanced_context::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    0.0,
                    None,
                    None,
                );
                chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state);
                chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64);

                // Render chart + toolbar + sidebar + modals at y_offset=CHROME_HEIGHT.
                let mut chart_ctx = instanced_context::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    chrome::CHROME_HEIGHT as f32,
                    None,
                    None,
                );
                pw.chart.render(&mut chart_ctx, frame_time, false);

                // Render chrome context menu overlay (at y_offset=0, absolute coords).
                if pw.chrome_state.context_menu.open {
                    let mut overlay_ctx = instanced_context::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    chrome::render_context_menu(
                        &mut overlay_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&overlay_ctx.inner().draw_commands);
                }

                // Render toast overlays.
                if !active_toasts.is_empty() {
                    let mut toast_ctx = instanced_context::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    render_toasts(&mut toast_ctx, active_toasts, width as f64, height as f64);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&toast_ctx.inner().draw_commands);
                }

                // Merge chrome instances into chart instances and store for GPU submission.
                let chrome_inner = chrome_ctx.inner();
                chart_ctx.inner_mut().draw_commands.extend_from_slice(&chrome_inner.draw_commands);

                pw.instanced_commands = chart_ctx.inner().draw_commands.clone();
            }
            RenderBackend::VelloCpu => {
                let mut cpu_ctx = vello_cpu_context::VelloCpuChartRenderContext::new(
                    1.0, // dpr
                    None,
                    None,
                );
                cpu_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0 (no offset).
                chrome::update_tab_widths(&mut cpu_ctx, &mut pw.chrome_state);
                chrome::render(&mut cpu_ctx, &pw.chrome_state, width as f64);
                // Render chart + toolbar + sidebar + modals offset by CHROME_HEIGHT.
                cpu_ctx.save();
                cpu_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut cpu_ctx, frame_time, false);
                cpu_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut cpu_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut cpu_ctx, active_toasts, width as f64, height as f64);
                }
                // Rasterize to pixel buffer.
                let pixel_count = (width as usize) * (height as usize) * 4;
                let mut pixels = vec![0u8; pixel_count];
                cpu_ctx.inner_mut().render_to_pixmap_rgba8(
                    &mut pixels,
                    width.min(u16::MAX as u32) as u16,
                    height.min(u16::MAX as u32) as u16,
                );
                pw.cpu_chart_dims = (width, height);
                pw.cpu_chart_pixels = pixels;
            }
            RenderBackend::TinySkia => {
                let mut skia_ctx = tiny_skia_context::TinySkiaChartRenderContext::new(
                    width, height, 1.0, None, None,
                );
                // Clear to background color (Pixmap::new() initializes to transparent).
                skia_ctx.set_fill_color("#131722");
                skia_ctx.fill_rect(0.0, 0.0, width as f64, height as f64);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut skia_ctx, &mut pw.chrome_state);
                chrome::render(&mut skia_ctx, &pw.chrome_state, width as f64);
                // Render chart content below the chrome strip.
                skia_ctx.save();
                skia_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut skia_ctx, frame_time, false);
                skia_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut skia_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut skia_ctx, active_toasts, width as f64, height as f64);
                }
                let pixels = skia_ctx.inner().pixels();
                pw.cpu_chart_dims = (width, height);
                pw.cpu_chart_pixels = pixels.to_vec();
            }
            RenderBackend::VelloHybrid => {
                let mut hybrid_ctx = vello_hybrid_context::VelloHybridChartRenderContext::new(
                    1.0, None, None,
                );
                hybrid_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut hybrid_ctx, &mut pw.chrome_state);
                chrome::render(&mut hybrid_ctx, &pw.chrome_state, width as f64);
                // Render chart + toolbar + sidebar + modals at y=CHROME_HEIGHT.
                hybrid_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut hybrid_ctx, frame_time, false);
                hybrid_ctx.translate(0.0, -chrome::CHROME_HEIGHT);
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut hybrid_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut hybrid_ctx, active_toasts, width as f64, height as f64);
                }
                // Move the inner uzor context (owns the vello_hybrid::Scene)
                // into PerWindowState for GPU submission.
                let mut inner = uzor_backend_vello_hybrid::VelloHybridRenderContext::new(1.0);
                std::mem::swap(&mut inner, hybrid_ctx.inner_mut());
                pw.hybrid_ctx = Some(inner);
            }
            RenderBackend::VelloGpu => unreachable!("handled above"),
        }
    }

    t0.elapsed().as_micros() as u64
}

/// Phase 2 of rendering: submit the built scene to the GPU and present.
///
/// Runs `render_to_texture`, handles screenshot captures, blits to the swap
/// chain surface, and calls `present()`.  Must run on the same thread that
/// owns the wgpu surface (GPU work is inherently sequential across windows).
///
/// Returns the GPU-submit wall-clock microseconds and sets `*close_all` on
/// catastrophic surface error.
///
/// Note: kept for reference. The pipelined path uses
/// [`submit_window_gpu_from_gpu_scene`] which renders `pw.gpu_scene` instead.
#[allow(dead_code)]
fn submit_window_gpu(pw: &mut PerWindowState, render_cx: &RenderContext, close_all: &mut bool, msaa_samples: u8) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let dev_id = pw.surface.dev_id;
    let device = &render_cx.devices[dev_id].device;
    let queue = &render_cx.devices[dev_id].queue;

    let base_color = vello::peniko::color::AlphaColor::from_rgba8(0x13, 0x17, 0x22, 0xff);

    let t0 = std::time::Instant::now();

    pw.renderer
        .render_to_texture(
            device,
            queue,
            &pw.scene,
            &pw.surface.target_view,
            &vello::RenderParams {
                base_color,
                width,
                height,
                antialiasing_method: match msaa_samples {
                    0 => AaConfig::Area,
                    8 => AaConfig::Msaa8,
                    _ => AaConfig::Msaa16,
                },
            },
        )
        .expect("render failed");

    let render_tex_us = t0.elapsed().as_micros() as u64;

    // Screenshot capture
    if pw.screenshot_pending {
        pw.screenshot_pending = false;
        let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
        let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
        match capture_screenshot(device, queue, &pw.surface, crop) {
            Some((pixels, img_width, img_height)) => {
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        let img = arboard::ImageData {
                            width: img_width as usize,
                            height: img_height as usize,
                            bytes: std::borrow::Cow::Borrowed(&pixels),
                        };
                        if let Err(e) = clipboard.set_image(img) {
                            eprintln!("[Screenshot] Clipboard error: {e}");
                        } else {
                            eprintln!("[Screenshot] Copied to clipboard");
                        }
                    }
                    Err(e) => eprintln!("[Screenshot] Failed to open clipboard: {e}"),
                }
                if let Some(png_bytes) = encode_png(&pixels, img_width, img_height) {
                    let filename = format!("screenshot_{}.png", timestamp_for_filename());
                    let path = screenshot_save_dir().join(&filename);
                    match std::fs::write(&path, &png_bytes) {
                        Ok(_) => eprintln!("[Screenshot] Saved {} bytes to: {}", png_bytes.len(), path.display()),
                        Err(e) => eprintln!("[Screenshot] Failed to write file: {e}"),
                    }
                }
            }
            None => eprintln!("[Screenshot] Capture failed"),
        }
    }

    // Alert screenshot capture — attach PNG bytes to pending delivery events.
    if pw.chart.pending_alert_screenshot {
        pw.chart.pending_alert_screenshot = false;
        if !pw.chart.pending_delivery_events.is_empty() {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, img_width, img_height)) => {
                    match encode_png(&pixels, img_width, img_height) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AlertScreenshot] Captured {}x{} PNG ({} bytes) for {} alert(s)",
                                img_width, img_height, png_bytes.len(),
                                pw.chart.pending_delivery_events.len()
                            );
                            for event in pw.chart.pending_delivery_events.iter_mut() {
                                event.screenshot = Some(png_bytes.clone());
                            }
                        }
                        None => eprintln!("[AlertScreenshot] PNG encoding failed"),
                    }
                }
                None => eprintln!("[AlertScreenshot] GPU capture failed"),
            }
        }
    }

    // Agent screenshot requests — respond to pending HTTP handler waiters.
    if !pw.pending_agent_screenshots.is_empty() {
        let agent_screenshots = std::mem::take(&mut pw.pending_agent_screenshots);
        for (chart_id, tx) in agent_screenshots {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, w, h)) => {
                    match encode_png(&pixels, w, h) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AgentScreenshot] Captured {}x{} PNG ({} bytes) for chart {}",
                                w, h, png_bytes.len(), chart_id
                            );
                            let _ = tx.send(Ok(zengeld_server::state::ScreenshotData {
                                png_bytes,
                                width: w,
                                height: h,
                            }));
                        }
                        None => {
                            eprintln!("[AgentScreenshot] PNG encoding failed for chart {}", chart_id);
                            let _ = tx.send(Err("PNG encoding failed".to_string()));
                        }
                    }
                }
                None => {
                    eprintln!("[AgentScreenshot] GPU capture failed for chart {}", chart_id);
                    let _ = tx.send(Err("screenshot capture failed".to_string()));
                }
            }
        }
    }

    // Present
    let present_t0 = std::time::Instant::now();
    let surface_texture = match pw.surface.surface.get_current_texture() {
        Ok(t) => t,
        Err(wgpu::SurfaceError::OutOfMemory) => {
            *close_all = true;
            return render_tex_us;
        }
        Err(_) => return render_tex_us,
    };

    let surface_view = surface_texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("blit"),
    });
    pw.surface
        .blitter
        .copy(device, &mut encoder, &pw.surface.target_view, &surface_view);
    queue.submit([encoder.finish()]);
    surface_texture.present();
    let present_us = present_t0.elapsed().as_micros() as u64;

    render_tex_us + present_us
}

/// Pipelined variant of [`submit_window_gpu`] used by the GPU render thread.
///
/// Identical to `submit_window_gpu` except that it renders `pw.gpu_scene`
/// instead of `pw.scene`.  `gpu_scene` holds the scene built by the main
/// thread during the *previous* frame; `scene` is the one the main thread is
/// building for the *current* frame while this function runs concurrently.
fn submit_window_gpu_from_gpu_scene(
    pw: &mut PerWindowState,
    render_cx: &RenderContext,
    close_all: &mut bool,
    msaa_samples: u8,
) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let dev_id = pw.surface.dev_id;
    let device = &render_cx.devices[dev_id].device;
    let queue = &render_cx.devices[dev_id].queue;

    let t0 = std::time::Instant::now();

    use sidebar_content::state::RenderBackend as RB;
    let is_vello_gpu = pw.render_backend == RB::VelloGpu;
    let is_instanced = pw.render_backend == RB::InstancedWgpu;
    let is_cpu_backend = matches!(pw.render_backend, RB::VelloCpu | RB::TinySkia);
    let base_color = vello::peniko::color::AlphaColor::from_rgba8(0x13, 0x17, 0x22, 0xff);

    if is_vello_gpu {
        // ── VelloGpu: full scene rendered by vello ──────────────────────────
        pw.renderer
            .render_to_texture(
                device,
                queue,
                &pw.gpu_scene,
                &pw.surface.target_view,
                &vello::RenderParams {
                    base_color,
                    width,
                    height,
                    antialiasing_method: match msaa_samples {
                        0 => AaConfig::Area,
                        8 => AaConfig::Msaa8,
                        _ => AaConfig::Msaa16,
                    },
                },
            )
            .expect("render failed");
    } else if is_cpu_backend {
        // ── CPU backends: write full-screen pixels directly to target texture ──
        // The CPU context rendered everything (chrome + chart + overlays) so we
        // write the entire pixel buffer at origin (0,0).
        if !pw.gpu_cpu_chart_pixels.is_empty() {
            let (cw, ch) = pw.gpu_cpu_chart_dims;
            if cw > 0 && ch > 0 && cw == width && ch == height {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &pw.surface.target_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pw.gpu_cpu_chart_pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * cw),
                        rows_per_image: Some(ch),
                    },
                    wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
                );
            }
        }
    }
    // InstancedWgpu and VelloHybrid: no texture upload here — instanced renders
    // directly to the swapchain surface below, hybrid uses its own renderer.

    let render_tex_us = t0.elapsed().as_micros() as u64;

    // Screenshot capture
    if pw.screenshot_pending {
        pw.screenshot_pending = false;
        let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
        let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
        match capture_screenshot(device, queue, &pw.surface, crop) {
            Some((pixels, img_width, img_height)) => {
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        let img = arboard::ImageData {
                            width: img_width as usize,
                            height: img_height as usize,
                            bytes: std::borrow::Cow::Borrowed(&pixels),
                        };
                        if let Err(e) = clipboard.set_image(img) {
                            eprintln!("[Screenshot] Clipboard error: {e}");
                        } else {
                            eprintln!("[Screenshot] Copied to clipboard");
                        }
                    }
                    Err(e) => eprintln!("[Screenshot] Failed to open clipboard: {e}"),
                }
                if let Some(png_bytes) = encode_png(&pixels, img_width, img_height) {
                    let filename = format!("screenshot_{}.png", timestamp_for_filename());
                    let path = screenshot_save_dir().join(&filename);
                    match std::fs::write(&path, &png_bytes) {
                        Ok(_) => eprintln!("[Screenshot] Saved {} bytes to: {}", png_bytes.len(), path.display()),
                        Err(e) => eprintln!("[Screenshot] Failed to write file: {e}"),
                    }
                }
            }
            None => eprintln!("[Screenshot] Capture failed"),
        }
    }

    // Alert screenshot capture — attach PNG bytes to pending delivery events.
    if pw.chart.pending_alert_screenshot {
        pw.chart.pending_alert_screenshot = false;
        if !pw.chart.pending_delivery_events.is_empty() {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, img_width, img_height)) => {
                    match encode_png(&pixels, img_width, img_height) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AlertScreenshot] Captured {}x{} PNG ({} bytes) for {} alert(s)",
                                img_width, img_height, png_bytes.len(),
                                pw.chart.pending_delivery_events.len()
                            );
                            for event in pw.chart.pending_delivery_events.iter_mut() {
                                event.screenshot = Some(png_bytes.clone());
                            }
                        }
                        None => eprintln!("[AlertScreenshot] PNG encoding failed"),
                    }
                }
                None => eprintln!("[AlertScreenshot] GPU capture failed"),
            }
        }
    }

    // Agent screenshot requests — respond to pending HTTP handler waiters.
    if !pw.pending_agent_screenshots.is_empty() {
        let agent_screenshots = std::mem::take(&mut pw.pending_agent_screenshots);
        for (chart_id, tx) in agent_screenshots {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, w, h)) => {
                    match encode_png(&pixels, w, h) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AgentScreenshot] Captured {}x{} PNG ({} bytes) for chart {}",
                                w, h, png_bytes.len(), chart_id
                            );
                            let _ = tx.send(Ok(zengeld_server::state::ScreenshotData {
                                png_bytes,
                                width: w,
                                height: h,
                            }));
                        }
                        None => {
                            eprintln!("[AgentScreenshot] PNG encoding failed for chart {}", chart_id);
                            let _ = tx.send(Err("PNG encoding failed".to_string()));
                        }
                    }
                }
                None => {
                    eprintln!("[AgentScreenshot] GPU capture failed for chart {}", chart_id);
                    let _ = tx.send(Err("screenshot capture failed".to_string()));
                }
            }
        }
    }

    // Present
    let present_t0 = std::time::Instant::now();
    let surface_texture = match pw.surface.surface.get_current_texture() {
        Ok(t) => t,
        Err(wgpu::SurfaceError::OutOfMemory) => {
            *close_all = true;
            return render_tex_us;
        }
        Err(_) => return render_tex_us,
    };

    let surface_view = surface_texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    if is_vello_gpu || is_cpu_backend {
        // ── VelloGpu / CPU: blit target_texture → swapchain surface ──────────
        // For VelloGpu: vello rendered into target_view above.
        // For CPU: pixel buffer was written into target_texture above.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("blit"),
        });
        pw.surface
            .blitter
            .copy(device, &mut encoder, &pw.surface.target_view, &surface_view);
        queue.submit([encoder.finish()]);
    }

    if is_instanced {
        // ── Instanced backend: render all instances directly to swapchain ─────
        // Everything (chrome + chart + overlays) is in the instance buffers.
        // Use clear_color = background so the surface is cleared first.
        if pw.instanced_renderer.is_none() {
            pw.instanced_renderer = Some(
                uzor_backend_wgpu_instanced::InstancedRenderer::new(
                    device,
                    queue,
                    surface_texture.texture.format(),
                )
            );
        }
        if let Some(ref mut inst_renderer) = pw.instanced_renderer {
            let clear = wgpu::Color { r: 0.075, g: 0.09, b: 0.133, a: 1.0 };
            inst_renderer.render(
                device,
                queue,
                &surface_view,
                width,
                height,
                &pw.gpu_instanced_commands,
                Some(clear),  // LoadOp::Clear — full frame, no vello content underneath
                None,         // no scissor — everything is in instances
            );
        }
    }

    // ── VelloHybrid backend: render scene onto swapchain ─────────────────────
    if pw.render_backend == RB::VelloHybrid {
        if let Some(ref hybrid_ctx) = pw.gpu_hybrid_ctx {
            // Lazily create the vello_hybrid::Renderer.
            if pw.hybrid_renderer.is_none() {
                pw.hybrid_renderer = Some(vello_hybrid::Renderer::new(
                    device,
                    &vello_hybrid::RenderTargetConfig {
                        format: surface_texture.texture.format(),
                        width,
                        height,
                    },
                ));
            }
            if let Some(ref mut renderer) = pw.hybrid_renderer {
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("vello_hybrid"),
                });
                let _ = hybrid_ctx.render(renderer, device, queue, &mut encoder, &surface_view);
                queue.submit([encoder.finish()]);
            }
        }
    }

    surface_texture.present();
    let present_us = present_t0.elapsed().as_micros() as u64;

    render_tex_us + present_us
}

/// Get current time as milliseconds since Unix epoch
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build a timestamp string suitable for a filename (YYYYMMDD_HHMMSS).
///
/// Uses `SystemTime` to avoid pulling in the `chrono` crate.
fn timestamp_for_filename() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Simple decomposition: seconds since epoch -> date/time components.
    // Accurate enough for filenames; does not handle leap seconds.
    let seconds_per_minute = 60u64;
    let seconds_per_hour = 3600u64;
    let seconds_per_day = 86400u64;

    let s = secs % seconds_per_minute;
    let m = (secs / seconds_per_minute) % 60;
    let h = (secs / seconds_per_hour) % 24;

    // Days since epoch (1970-01-01)
    let mut days = secs / seconds_per_day;

    // Convert days to year/month/day (Gregorian calendar)
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let months = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u64;
    let mut day = days + 1;
    for (i, &days_in_month) in months.iter().enumerate() {
        let dim = if i == 1 && is_leap_year(year) {
            29
        } else {
            days_in_month
        };
        if day <= dim {
            break;
        }
        day -= dim;
        month += 1;
    }

    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        year, month, day, h, m, s
    )
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Recreate `surface.target_texture` with `COPY_SRC` and `RENDER_ATTACHMENT` added to the usage flags.
///
/// Vello's `create_targets` only sets `STORAGE_BINDING | TEXTURE_BINDING`.
/// Without `COPY_SRC`, `copy_texture_to_buffer` fails; without `RENDER_ATTACHMENT`, the instanced wgpu backend cannot use the texture as a render target.
/// Since all fields on `RenderSurface` are `pub`, we can drop and replace the
/// texture in-place.  The view must be recreated from the new texture.
fn add_copy_src_to_target_texture(surface: &mut RenderSurface<'_>, device: &wgpu::Device) {
    let old = &surface.target_texture;
    let size = old.size();

    let new_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target_texture_with_copy_src"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let new_view = new_texture.create_view(&wgpu::TextureViewDescriptor::default());
    surface.target_texture = new_texture;
    surface.target_view = new_view;
}

/// Perform a synchronous GPU readback of the render texture.
///
/// Returns raw RGBA pixels (after optional crop) and the final `(width, height)`,
/// or `None` on failure.  The caller is responsible for PNG encoding and
/// clipboard operations so that both can share the same pixel buffer.
///
/// `crop` is `Some((x, y, w, h))` in texture-pixel coordinates.  Coordinates
/// are clamped to the texture boundary to avoid panics on out-of-bounds rects.
fn capture_screenshot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    surface: &RenderSurface<'_>,
    crop: Option<(u32, u32, u32, u32)>,
) -> Option<(Vec<u8>, u32, u32)> {
    let texture = &surface.target_texture;
    let size = texture.size();
    let full_width = size.width;
    let full_height = size.height;

    if full_width == 0 || full_height == 0 {
        eprintln!("[Screenshot] Texture has zero dimension ({full_width}x{full_height})");
        return None;
    }

    let bytes_per_pixel = 4u32; // Rgba8Unorm
    let unpadded_bytes_per_row = full_width * bytes_per_pixel;

    // wgpu requires rows aligned to 256 bytes (COPY_BYTES_PER_ROW_ALIGNMENT)
    const ALIGNMENT: u32 = 256;
    let padded_bytes_per_row =
        ((unpadded_bytes_per_row + ALIGNMENT - 1) / ALIGNMENT) * ALIGNMENT;

    let buffer_size = (padded_bytes_per_row * full_height) as u64;

    // Create a staging buffer for GPU -> CPU transfer
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot_staging_buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Encode the copy command
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("screenshot_copy_encoder"),
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(full_height),
            },
        },
        wgpu::Extent3d {
            width: full_width,
            height: full_height,
            depth_or_array_layers: 1,
        },
    );

    queue.submit(std::iter::once(encoder.finish()));

    // Map the staging buffer and wait for completion via a channel
    let buffer_slice = staging_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });

    // Poll until the mapping callback fires
    loop {
        match device.poll(wgpu::PollType::Poll) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[Screenshot] Device poll error: {e:?}");
                return None;
            }
        }
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(e)) => {
                eprintln!("[Screenshot] Buffer map error: {e:?}");
                return None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Mapping not yet complete; spin and try again
                std::hint::spin_loop();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                eprintln!("[Screenshot] Map channel disconnected unexpectedly");
                return None;
            }
        }
    }

    // Read the full pixel data (strip row padding)
    let data = buffer_slice.get_mapped_range();
    let mut full_pixels: Vec<u8> =
        Vec::with_capacity((full_width * full_height * bytes_per_pixel) as usize);

    for row in 0..full_height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        full_pixels.extend_from_slice(&data[start..end]);
    }

    drop(data);
    staging_buffer.unmap();

    // Apply optional crop
    let (pixels, out_width, out_height) = if let Some((cx, cy, cw, ch)) = crop {
        // Clamp to texture bounds to avoid panics
        let cx = cx.min(full_width);
        let cy = cy.min(full_height);
        let cw = cw.min(full_width - cx);
        let ch = ch.min(full_height - cy);

        if cw == 0 || ch == 0 {
            eprintln!("[Screenshot] Crop rect is empty after clamping — using full frame");
            (full_pixels, full_width, full_height)
        } else {
            let mut cropped = Vec::with_capacity((cw * ch * bytes_per_pixel) as usize);
            for row in cy..(cy + ch) {
                let start = ((row * full_width + cx) * bytes_per_pixel) as usize;
                let end = start + (cw * bytes_per_pixel) as usize;
                cropped.extend_from_slice(&full_pixels[start..end]);
            }
            (cropped, cw, ch)
        }
    } else {
        (full_pixels, full_width, full_height)
    };

    Some((pixels, out_width, out_height))
}

/// Return the directory where screenshots should be saved.
///
/// Prefers `%USERPROFILE%\Pictures\Screenshots` on Windows; falls back to the
/// current working directory so the function always returns a usable path.
fn screenshot_save_dir() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let dir = std::path::PathBuf::from(home)
            .join("Pictures")
            .join("Screenshots");
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    std::env::current_dir().unwrap_or_default()
}

/// Encode raw RGBA pixels to PNG bytes.
fn encode_png(pixels: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut png_bytes: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = match encoder.write_header() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[Screenshot] PNG header error: {e}");
                return None;
            }
        };
        if let Err(e) = writer.write_image_data(pixels) {
            eprintln!("[Screenshot] PNG write error: {e}");
            return None;
        }
        // writer (and encoder) drop here, releasing the borrow on png_bytes
    }
    Some(png_bytes)
}

impl App<'_> {
    fn new(
        symbol: &str,
        bridge: std::sync::Arc<live_data::DataBridge>,
        saved_windows: Vec<zengeld_chart::WindowState>,
        profile: zengeld_chart::UserProfile,
        user_manager: zengeld_chart::UserManager,
        app_connector_ready_rx: live_data::ConnectorReadyReceiver,
    ) -> Self {
        let app_state = AppState::from_profile(&profile, user_manager.presets.clone(), user_manager.snapshots.clone(), user_manager.template_manager.clone());

        // Convert StoredApiKey entries to ApiKeyEntry (the server type).
        let server_keys: Vec<zengeld_server::state::ApiKeyEntry> = app_state
            .agent_api_keys
            .iter()
            .map(|k| zengeld_server::state::ApiKeyEntry {
                key_hash: k.key_hash.clone(),
                label: k.label.clone(),
                tier: k.tier.clone(),
                permissions: zengeld_server::state::Permissions::from_tier(&k.tier),
                created_at: k.created_at,
                agent_id: k.agent_id.clone(),
                // Treat any stored key as Local unless it was explicitly marked cloud.
                source: if k.source == "cloud" {
                    zengeld_server::state::KeySource::Cloud
                } else {
                    zengeld_server::state::KeySource::Local
                },
            })
            .collect();

        // Start the internal Agent API server on the DataBridge's tokio runtime.
        let agent_state = std::sync::Arc::new(zengeld_server::AgentState::new(
            bridge.clone(),
            env!("CARGO_PKG_VERSION").to_string(),
            server_keys,
        ));
        // ── Populate indicator catalog (one-time, at startup) ─────────────────
        {
            use zengeld_server::state::{
                IndicatorCatalogSnapshot, CatalogIndicator, CatalogParam, CatalogOutput,
            };
            use zengeld_terminal_indicators::{
                IndicatorManager as TmpMgr,
                IndicatorParamType, IndicatorParamValue,
            };

            let tmp_mgr = TmpMgr::new();
            let defs = tmp_mgr.get_definitions();

            let indicators: Vec<CatalogIndicator> = defs
                .into_iter()
                .map(|def| {
                    let category = format!("{:?}", def.category).to_lowercase();

                    let params: Vec<CatalogParam> = def.params.iter().map(|p| {
                        let (param_type_str, min, max) = match &p.param_type {
                            IndicatorParamType::Int { min, max, .. } => {
                                ("int".to_string(), Some(*min as f64), Some(*max as f64))
                            }
                            IndicatorParamType::Float { min, max, .. } => {
                                ("float".to_string(), Some(*min), Some(*max))
                            }
                            IndicatorParamType::Bool => ("bool".to_string(), None, None),
                            IndicatorParamType::Select { .. } => ("select".to_string(), None, None),
                            IndicatorParamType::Color => ("color".to_string(), None, None),
                            IndicatorParamType::Source => ("source".to_string(), None, None),
                        };

                        let default_value = match &p.default_value {
                            IndicatorParamValue::Int(v) => {
                                serde_json::Value::Number(serde_json::Number::from(*v))
                            }
                            IndicatorParamValue::Float(v) => {
                                serde_json::Number::from_f64(*v)
                                    .map(serde_json::Value::Number)
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            IndicatorParamValue::Bool(v) => serde_json::Value::Bool(*v),
                            IndicatorParamValue::String(s) => {
                                serde_json::Value::String(s.clone())
                            }
                            IndicatorParamValue::Color(s) => {
                                serde_json::Value::String(s.clone())
                            }
                        };

                        CatalogParam {
                            name: p.name.clone(),
                            display_name: p.display_name.clone(),
                            param_type: param_type_str,
                            default_value,
                            min,
                            max,
                        }
                    }).collect();

                    let outputs: Vec<CatalogOutput> = def.outputs.iter().map(|o| {
                        CatalogOutput {
                            name: o.name.clone(),
                            color: Some(o.color.clone()),
                        }
                    }).collect();

                    CatalogIndicator {
                        type_id: def.type_id.clone(),
                        name: def.name.clone(),
                        short_name: def.short_name.clone(),
                        category,
                        description: def.description.clone(),
                        overlay: def.overlay,
                        params,
                        outputs,
                    }
                })
                .collect();

            eprintln!("[App] Indicator catalog populated: {} types", indicators.len());
            if let Ok(mut cat) = agent_state.indicator_catalog.write() {
                *cat = IndicatorCatalogSnapshot { indicators };
            }
        }

        // ── Populate primitive catalog (one-time, at startup) ─────────────────
        {
            use zengeld_server::state::{PrimitiveCatalogSnapshot, CatalogPrimitive};
            use zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry;
            use zengeld_chart::drawing::primitives_v2::{PrimitiveKind, ClickBehavior};

            let registry = PrimitiveRegistry::global();
            let guard = registry.read().unwrap();

            let primitives: Vec<CatalogPrimitive> = guard.all().map(|meta| {
                let kind = match meta.kind {
                    PrimitiveKind::Line        => "lines",
                    PrimitiveKind::Channel     => "channels",
                    PrimitiveKind::Shape       => "shapes",
                    PrimitiveKind::Fibonacci   => "fibonacci",
                    PrimitiveKind::Gann        => "gann",
                    PrimitiveKind::Pattern     => "patterns",
                    PrimitiveKind::Annotation  => "annotations",
                    PrimitiveKind::Measurement => "measurement",
                    PrimitiveKind::Trading     => "trading",
                    PrimitiveKind::Signal      => "signals",
                };

                let click_behavior = match meta.click_behavior {
                    ClickBehavior::SingleClick    => "SingleClick",
                    ClickBehavior::TwoPoint       => "TwoPoint",
                    ClickBehavior::ThreePoint     => "ThreePoint",
                    ClickBehavior::FourPoint      => "FourPoint",
                    ClickBehavior::MultiPoint(_)  => "MultiPoint",
                    ClickBehavior::ClickDrag      => "ClickDrag",
                    ClickBehavior::FreehandDrag   => "FreehandDrag",
                };

                CatalogPrimitive {
                    type_id: meta.type_id.to_string(),
                    display_name: meta.display_name.to_string(),
                    kind: kind.to_string(),
                    click_behavior: click_behavior.to_string(),
                    default_color: meta.default_color.to_string(),
                    supports_text: meta.supports_text,
                    has_levels: meta.has_levels,
                }
            }).collect();

            eprintln!("[App] Primitive catalog populated: {} types", primitives.len());
            if let Ok(mut cat) = agent_state.primitive_catalog.write() {
                *cat = PrimitiveCatalogSnapshot { primitives };
            }
        }

        // Only start the server if enabled in profile
        let _server_handle = if app_state.server_enabled {
            Some(zengeld_server::start_server(
                agent_state.clone(),
                bridge.runtime(),
                app_state.server_port,
            ))
        } else {
            eprintln!("[App] Agent API server disabled in settings");
            None
        };

        // Initialize alert delivery engine. AlertDelivery::new spawns a tokio
        // task, so we must enter the bridge's runtime context first.
        let (alert_delivery_engine, toast_rx) = {
            let _guard = bridge.runtime().enter();
            alert_delivery::AlertDelivery::new(profile.notification_settings.clone())
        };

        // Shared atomics for live telemetry — App writes each second, AppTelemetry reads.
        let telemetry_shared = std::sync::Arc::new(TelemetryShared::new());

        // Start the OTA updater background task.
        let updater_handle = {
            struct AppTelemetry {
                start_time: std::time::Instant,
                shared: std::sync::Arc<TelemetryShared>,
            }

            impl zengeld_updater::TelemetrySource for AppTelemetry {
                fn collect(&self) -> zengeld_updater::telemetry::TelemetryPayload {
                    use std::sync::atomic::Ordering::Relaxed;
                    let gpu_name = self.shared.gpu_name.lock()
                        .map(|g| g.clone())
                        .unwrap_or_default();
                    zengeld_updater::telemetry::TelemetryPayload {
                        device_id: zengeld_updater::telemetry::get_or_create_device_id(),
                        app_version: env!("CARGO_PKG_VERSION").to_string(),
                        os: std::env::consts::OS.to_string(),
                        arch: std::env::consts::ARCH.to_string(),
                        gpu_name,
                        screen_width: self.shared.screen_width.load(Relaxed),
                        screen_height: self.shared.screen_height.load(Relaxed),
                        connector_count: self.shared.connector_count.load(Relaxed),
                        window_count: self.shared.window_count.load(Relaxed),
                        avg_fps: f32::from_bits(self.shared.avg_fps_bits.load(Relaxed)),
                        uptime_secs: self.start_time.elapsed().as_secs(),
                        total_bars: self.shared.total_bars.load(Relaxed),
                        ws_connections: self.shared.ws_connections.load(Relaxed),
                    }
                }
            }

            let source = std::sync::Arc::new(AppTelemetry {
                start_time: std::time::Instant::now(),
                shared: telemetry_shared.clone(),
            });

            let connected = profile.client_mode == zengeld_chart::user_profile::profile::ClientMode::Connected;
            Some(zengeld_updater::start(bridge.runtime().handle(), source, connected))
        };

        Self {
            render_cx: RenderContext::new(),
            windows: HashMap::new(),
            pending_spawns: Vec::new(),
            close_all_requested: false,
            default_symbol: symbol.to_string(),
            bridge,
            saved_windows,
            profile,
            user_manager,
            last_focused: None,
            app_state,
            app_connector_ready_rx,
            _phantom: std::marker::PhantomData,
            agent_state: Some(agent_state),
            last_indicator_snapshot: std::time::Instant::now(),
            alert_delivery: Some(alert_delivery_engine),
            toast_rx: Some(toast_rx),
            active_toasts: Vec::new(),
            last_frame_instant: std::time::Instant::now(),
            fps_ema: 60.0,
            last_frame_time_ms: 16.0,
            fps_limit: 0,
            msaa_samples: 8,
            max_bars: 0,
            perf_log_enabled: false,
            render_backend: sidebar_content::state::RenderBackend::VelloGpu,
            vsync_enabled: true,
            sys: {
                let mut s = System::new();
                s.refresh_cpu_usage();
                s.refresh_memory();
                s.refresh_processes(
                    ProcessesToUpdate::Some(&[Pid::from_u32(std::process::id())]),
                    false,
                );
                s
            },
            self_pid: Pid::from_u32(std::process::id()),
            gpu_name: String::new(),
            gpu_driver: String::new(),
            frame_count: 0,
            last_timing_report: std::time::Instant::now(),
            cached_connector_count: 0,
            cached_scene_us: 0,
            cached_gpu_us: 0,
            gpu_cmd_tx: None,
            gpu_done_rx: None,
            gpu_thread: None,
            gpu_frame_pending: false,
            updater_handle,
            telemetry_shared,
        }
    }

    /// Create and register a new window.
    ///
    /// `restore` — optional saved [`WindowState`] to restore position, size, tabs,
    /// and active preset.  `cascade_from` — for user-spawned windows, the source
    /// window to cascade placement from.  All windows are equal — no "primary".
    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        restore: Option<&zengeld_chart::WindowState>,
        cascade_from: Option<WindowId>,
    ) {
        let mut attrs = Window::default_attributes()
            .with_title("chart-app-vello")
            .with_inner_size(winit::dpi::LogicalSize::new(1200u32, 800u32))
            .with_decorations(false);

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_undecorated_shadow(true);
        }

        // Apply saved position/size from restore state — position is set in window
        // attributes BEFORE creation so the OS honors it.
        if let Some(ws) = restore {
            use winit::dpi::Position;
            if let (Some(x), Some(y)) = (ws.x, ws.y) {
                attrs = attrs.with_position(Position::Physical(
                    winit::dpi::PhysicalPosition::new(x, y),
                ));
            }
            if let (Some(w), Some(h)) = (ws.width, ws.height) {
                attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(w, h));
            }
        } else if let Some(from_id) = cascade_from {
            // Cascade offset from the requesting window.
            use winit::dpi::Position;
            if let Some(existing) = self.windows.get(&from_id) {
                if let Ok(pos) = existing.window.outer_position() {
                    attrs = attrs.with_position(Position::Physical(
                        winit::dpi::PhysicalPosition::new(pos.x + 30, pos.y + 30),
                    ));
                }
            }
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );

        let size = window.inner_size();

        let mut surface = pollster::block_on(self.render_cx.create_surface(
            window.clone(),
            size.width,
            size.height,
            PresentMode::AutoNoVsync,
        ))
        .expect("Failed to create render surface");

        let dev_id = surface.dev_id;
        let device = &self.render_cx.devices[dev_id].device;

        // Vello creates the target texture without COPY_SRC, which breaks screenshot
        // readback. Recreate it with COPY_SRC added so copy_texture_to_buffer works.
        add_copy_src_to_target_texture(&mut surface, device);

        let renderer = Renderer::new(
            device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::all(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .expect("Failed to create renderer");

        // Reuse the saved window_id when restoring; generate a fresh one for new windows.
        let window_id = if let Some(ws) = restore {
            if ws.window_id.is_empty() {
                format!(
                    "win_{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                )
            } else {
                ws.window_id.clone()
            }
        } else {
            format!(
                "win_{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            )
        };

        // All windows are equal — all use the shared bridge and profile.
        let live_update_rx = self.bridge.add_listener();
        let mut chart = chart_app::ChartApp::new_window(
            self.bridge.clone(),
            live_update_rx,
            window_id.clone(),
            restore,
            &self.profile,
            &self.user_manager,
        );
        // Apply the app-level theme so new windows start with the correct preset.
        if !self.app_state.theme_preset.is_empty() {
            chart.panel_app.theme_manager.set_preset(&self.app_state.theme_preset);
        }
        // Initialise the new window directly from AppState so it has current data
        // without waiting for the next dirty-flag sync pass.
        chart.sidebar_state.watchlist_manager = self.app_state.watchlist_manager.clone();
        chart.sidebar_state.connector_enabled = self.app_state.connector_enabled.clone();
        chart.panel_app.presets = self.app_state.presets.clone();
        chart.panel_app.user_manager.snapshots = self.app_state.snapshots.clone();
        chart.panel_app.template_manager = self.app_state.template_manager.clone();
        // Sync server settings so the User Settings modal shows current state.
        chart.panel_app.user_settings_state.server_enabled = self.app_state.server_enabled;
        chart.panel_app.user_settings_state.server_port = self.app_state.server_port;
        chart.panel_app.user_settings_state.server_status = if self.app_state.server_enabled {
            "running".to_string()
        } else {
            "stopped".to_string()
        };
        // Sync connection mode from the loaded profile.
        chart.panel_app.user_settings_state.client_mode_connected =
            self.user_manager.profile.client_mode
                == zengeld_chart::user_profile::profile::ClientMode::Connected;
        // API keys are now managed via /api/v1/keys REST endpoint.
        // Show key count in the UI instead of the raw key string.
        chart.panel_app.user_settings_state.api_key = format!(
            "{} key(s) registered",
            self.app_state.agent_api_keys.len()
        );

        let chrome_px = (chrome::CHROME_HEIGHT * window.scale_factor()) as u32;
        chart.resize(size.width, size.height.saturating_sub(chrome_px));

        let win_id = window.id();
        let pw = PerWindowState {
            window,
            surface,
            renderer,
            scene: Scene::new(),
            gpu_scene: Scene::new(),
            toolbar_scene: Scene::new(),
            toolbar_dirty: true,
            sidebar_scene: Scene::new(),
            sidebar_dirty_scene: true,
            chart,
            last_mouse_pos: (0.0, 0.0),
            mouse_pressed: false,
            drag_start_pos: None,
            last_drag_pos: None,
            last_click: None,
            screenshot_pending: false,
            pending_agent_screenshots: Vec::new(),
            modifiers: winit::keyboard::ModifiersState::default(),
            drawing_capture: false,
            chrome_state: chrome::ChromeState::new("chart-app-vello"),
            close_requested: false,
            spawn_new_window: false,
            window_id,
            close_window_requested: false,
            delete_window_requested: false,
            last_sidebar_hover_row: None,
            render_backend: sidebar_content::state::RenderBackend::VelloGpu,
            instanced_renderer: None,
            instanced_commands: Vec::new(),
            gpu_instanced_commands: Vec::new(),
            cpu_chart_pixels: Vec::new(),
            cpu_chart_dims: (0, 0),
            gpu_cpu_chart_pixels: Vec::new(),
            gpu_cpu_chart_dims: (0, 0),
            hybrid_renderer: None,
            hybrid_ctx: None,
            gpu_hybrid_ctx: None,
        };

        self.windows.insert(win_id, pw);

        // Spawn the GPU render thread on the first window creation.
        // The thread is persistent across all frames so we avoid per-frame
        // spawn overhead.
        if self.gpu_cmd_tx.is_none() {
            self.spawn_gpu_thread();
        }
    }

    /// Wait for the GPU render thread to finish the current frame, if one is pending.
    ///
    /// Called before any operation that accesses GPU-owned fields (`surface`,
    /// `renderer`, `gpu_scene`) — e.g. `resize_surface` in `window_event`.
    /// In the common case (GPU already done) this returns immediately without
    /// blocking because the channel already has a message waiting.
    ///
    /// Uses a 16 ms timeout so that resize events during a slow GPU frame do
    /// not stall the main thread long enough for the broadcast receiver to fall
    /// behind and start dropping ticks.  If the GPU thread does not respond
    /// within the budget, the wait is skipped and `gpu_frame_pending` is cleared
    /// optimistically — the GPU thread will still finish in the background.
    fn wait_for_gpu_frame(&mut self) {
        if !self.gpu_frame_pending {
            return;
        }
        if let Some(ref done_rx) = self.gpu_done_rx {
            match done_rx.recv_timeout(std::time::Duration::from_millis(16)) {
                Ok(done) => {
                    if done.close_all {
                        self.close_all_requested = true;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // GPU thread is still working; skip the wait to keep the
                    // main thread responsive.  The GPU thread will finish on its
                    // own and the next frame will pick up the done signal.
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("[App] GPU render thread channel closed (wait_for_gpu_frame)");
                    self.close_all_requested = true;
                }
            }
        }
        self.gpu_frame_pending = false;
    }

    /// Spawn the persistent GPU render thread.
    ///
    /// The thread waits on a sync channel for `GpuCommand::Submit`, calls
    /// `submit_window_gpu` for every window address it receives, then sends a
    /// `GpuDone` back.  It exits cleanly on `GpuCommand::Shutdown`.
    ///
    /// # Safety
    ///
    /// The caller (main thread in `about_to_wait`) guarantees:
    ///   1. Each `pw_addr` in `Submit.window_addrs` is the address of a live
    ///      `PerWindowState` on the heap that no other thread is concurrently
    ///      mutating.
    ///   2. `render_cx_addr` is the address of a live `RenderContext` that is
    ///      not mutated while the GPU thread is running.
    ///   3. After signalling `Submit`, the main thread does not access the GPU
    ///      fields (`gpu_scene`, `renderer`, `surface`) of any PerWindowState
    ///      until it receives `GpuDone`.  It MAY safely read/write `scene`,
    ///      `chart`, and all other non-GPU fields during this window.
    fn spawn_gpu_thread(&mut self) {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::sync_channel::<GpuCommand>(1);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<GpuDone>();

        let handle = std::thread::Builder::new()
            .name("gpu-render".to_string())
            .spawn(move || {
                loop {
                    let cmd = match cmd_rx.recv() {
                        Ok(c) => c,
                        Err(_) => break, // channel closed
                    };

                    match cmd {
                        GpuCommand::Shutdown => break,
                        GpuCommand::Submit { window_addrs, msaa_samples, render_cx_addr } => {
                            let mut close_all = false;
                            let mut total_gpu_us = 0u64;

                            // SAFETY: each address is a unique live PerWindowState;
                            // the main thread waits for GpuDone before touching
                            // gpu_scene, renderer, or surface on any of them.
                            let render_cx: &RenderContext =
                                unsafe { &*(render_cx_addr as *const RenderContext) };

                            for pw_addr in window_addrs {
                                let pw: &mut PerWindowState =
                                    unsafe { &mut *(pw_addr as *mut PerWindowState) };

                                // submit_window_gpu reads pw.gpu_scene (not pw.scene).
                                let gpu_us = submit_window_gpu_from_gpu_scene(
                                    pw,
                                    render_cx,
                                    &mut close_all,
                                    msaa_samples,
                                );
                                total_gpu_us += gpu_us;
                            }

                            let _ = done_tx.send(GpuDone { close_all, total_gpu_us });
                        }
                    }
                }
            })
            .expect("Failed to spawn gpu-render thread");

        self.gpu_cmd_tx = Some(cmd_tx);
        self.gpu_done_rx = Some(done_rx);
        self.gpu_thread = Some(handle);
    }

    /// Save all window state and the user profile to disk.
    ///
    /// `exclude_window_ids` — window ids to exclude from the saved profile
    /// (used for deleted windows that should not be persisted).
    fn save_all(&mut self, exclude_window_ids: &[String]) {
        // 1. Autosave every window's active preset.
        for pw in self.windows.values_mut() {
            pw.chart.autosave_snapshot();
        }

        // 1b. Drain preset actions queued by autosave_snapshot into AppState.
        for pw in self.windows.values_mut() {
            for action in pw.chart.preset_actions.drain(..) {
                match action {
                    chart_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.app_state.presets.insert(id, preset);
                    }
                    chart_app::PresetAction::Delete { id } => {
                        self.app_state.presets.remove(&id);
                    }
                    chart_app::PresetAction::Rename { id, new_name } => {
                        if let Some(p) = self.app_state.presets.get_mut(&id) {
                            p.name = new_name;
                        }
                    }
                }
            }
        }

        // 2. Sync OS position/size into ChartApp fields.
        //    Skip minimized windows — Windows returns (-32000, -32000) for iconic state.
        for pw in self.windows.values_mut() {
            if pw.window.is_minimized().unwrap_or(false) {
                continue; // Keep the pre-minimized position/size already stored.
            }
            if let Ok(pos) = pw.window.outer_position() {
                if pos.x > -30000 && pos.y > -30000 {
                    pw.chart.window_x = Some(pos.x);
                    pw.chart.window_y = Some(pos.y);
                }
            }
            let sz = pw.window.inner_size();
            if sz.width > 0 && sz.height > 0 {
                pw.chart.window_width = Some(sz.width);
                pw.chart.window_height = Some(sz.height);
            }
        }

        // 3. Collect window states, excluding deleted ones.
        let window_states: Vec<zengeld_chart::WindowState> = self.windows.values()
            .filter(|pw| !exclude_window_ids.contains(&pw.window_id))
            .map(|pw| pw.chart.build_window_state())
            .collect();

        for ws in &window_states {
            eprintln!("[save_all] window_id={} pos=({:?},{:?}) size=({:?},{:?}) tabs={} active={}",
                ws.window_id, ws.x, ws.y, ws.width, ws.height,
                ws.open_tabs.len(), ws.active_preset_id);
        }

        // 4. Build profile from in-memory profile base, updating window list and
        //    sidebar/toolbar state from the preferred window (last focused, or first
        //    available as fallback) for deterministic output instead of HashMap order.
        let preferred_key = self.last_focused
            .filter(|id| self.windows.contains_key(id))
            .or_else(|| self.windows.keys().next().copied());

        let mut profile = self.profile.clone();
        profile.windows = window_states;

        // Use AppState as the canonical source for connector_enabled, theme, and
        // device identity (replaces per-window copies that were previously written here).
        profile.connector_enabled = self.app_state.connector_enabled.clone();
        profile.active_theme = self.app_state.theme_preset.clone();
        profile.device_name = self.app_state.device_name.clone();
        profile.app_version = self.app_state.app_version.clone();
        profile.recalc_mode = match self.app_state.recalc_mode {
            chart_app::RecalcMode::PerTick  => "PerTick".to_string(),
            chart_app::RecalcMode::PerFrame => "PerFrame".to_string(),
            chart_app::RecalcMode::PerBar   => "PerBar".to_string(),
        };
        profile.server_enabled = self.app_state.server_enabled;
        profile.server_port = self.app_state.server_port;
        // Persist the current key registry (managed via the REST API).
        // The legacy `agent_api_key` field is kept empty after migration so
        // we don't double-migrate on the next load.
        profile.agent_api_keys = self.app_state.agent_api_keys.clone();
        profile.agent_api_key = String::new();

        // Persist notification settings from the first window's alert settings state.
        if let Some(pw) = self.windows.values().next() {
            profile.notification_settings =
                pw.chart.panel_app.alert_settings_state.notification_settings.clone();
        }

        if let Some(key) = preferred_key {
            if let Some(pw) = self.windows.get(&key) {
                profile.sidebar_visible = pw.chart.sidebar_state.is_right_open();
                profile.sidebar_panel = chart_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                profile.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                profile.inline_bar_x = Some(inline.x);
                profile.inline_bar_y = Some(inline.y);
                let dock_str = match inline.dock_edge {
                    zengeld_chart::InlineDockEdge::Bottom => "Bottom",
                    zengeld_chart::InlineDockEdge::Top => "Top",
                    zengeld_chart::InlineDockEdge::Free => "Free",
                };
                profile.inline_bar_dock = Some(dock_str.to_string());
            }
        }

        // 5. Save profile.json once.
        if let Err(e) = zengeld_chart::save_profile(&profile) {
            eprintln!("[App] Failed to save profile: {}", e);
        } else {
            // Keep in-memory profile up to date.
            self.profile = profile;
        }

        // 6. Save watchlists.json from AppState (single source of truth).
        {
            let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
            if let Err(e) = zengeld_chart::save_json(&watchlists_path, &self.app_state.watchlist_manager) {
                eprintln!("[App] Failed to save watchlists: {}", e);
            }
        }

        // 7. Save settings snapshots from AppState (single canonical source of truth).
        {
            let path = zengeld_chart::get_user_data_dir().join("settings_snapshots.json");
            if let Err(e) = zengeld_chart::save_json(&path, &self.app_state.snapshots) {
                eprintln!("[App] Failed to save settings snapshots: {}", e);
            }
        }

        // 8. Save templates from AppState (single canonical source of truth).
        if let Err(e) = self.app_state.template_manager.save_to_default_dir() {
            eprintln!("[App] Failed to save templates: {:?}", e);
        }
        // Call save_user_state() for any remaining per-window state (snapshots compat).
        if let Some(key) = preferred_key {
            if let Some(pw) = self.windows.get_mut(&key) {
                pw.chart.panel_app.save_user_state();
            }
        }

        // 9. Save all presets from AppState (single canonical source of truth).
        for preset in self.app_state.presets.values() {
            if let Err(e) = zengeld_chart::preset::storage::save_preset(preset) {
                eprintln!("[App] failed to save preset {}: {}", preset.id, e);
            }
        }
    }

    /// Drain app-level broadcast messages each frame.
    ///
    /// Handles `ConnectorReady` at the application level so that the symbol list
    /// is always requested when a connector becomes available, even if no
    /// per-window tick is running at that moment.
    ///
    /// The per-window `ChartApp::tick()` also receives and handles these messages
    /// independently — both handlers running is harmless (the bridge is a broadcast
    /// channel and every receiver gets its own independent copy of every message).
    fn tick_app_state(&mut self) {
        loop {
            match self.app_connector_ready_rx.try_recv() {
                Ok(exchange_id) => {
                    let eid_str = exchange_id.as_str();
                    if self.app_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                        self.bridge.request_symbols(exchange_id);
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
    }

    /// Build and push a [`TerminalSnapshot`] from all open windows.
    ///
    /// Called at most once per second from `about_to_wait()` alongside
    /// `update_indicator_snapshot`.  Captures window/tab/chart/layout
    /// structure without computed values.
    fn update_terminal_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use std::collections::HashMap as StdHashMap;
        use zengeld_server::state::{
            TerminalSnapshot, WindowSnapshot, TabSnapshot, ChartSnapshot,
            ViewportSnapshot, IndicatorSummary, PrimitiveSummary, LayoutNode,
        };

        // Recursive helper: convert a PanelNode into a LayoutNode.
        fn build_layout_node(
            node: &uzor::panels::PanelNode<zengeld_chart::ChartSubPanel>,
            leaf_to_chart: &StdHashMap<uzor::panels::LeafId, zengeld_chart::ChartId>,
        ) -> LayoutNode {
            match node {
                uzor::panels::PanelNode::Leaf(leaf) => {
                    let chart_id = leaf_to_chart.get(&leaf.id).map(|c| c.0).unwrap_or(0);
                    LayoutNode::Leaf { chart_id, leaf_id: leaf.id.0 }
                }
                uzor::panels::PanelNode::Branch(branch) => {
                    let axis = match branch.layout {
                        uzor::panels::WindowLayout::SplitHorizontal => "horizontal",
                        uzor::panels::WindowLayout::SplitVertical   => "vertical",
                        _                                           => "grid",
                    };
                    LayoutNode::Split {
                        axis: axis.to_string(),
                        proportions: branch.proportions.clone(),
                        children: branch.children
                            .iter()
                            .map(|c| build_layout_node(c, leaf_to_chart))
                            .collect(),
                    }
                }
            }
        }

        let mut snap = TerminalSnapshot::default();

        for pw in self.windows.values() {
            // Build preset-id → name lookup.
            let preset_name: StdHashMap<&str, &str> = pw.chart.panel_app.presets
                .iter()
                .map(|(id, p)| (id.as_str(), p.name.as_str()))
                .collect();

            // Tabs
            let active_tab_id = pw.chart.panel_app.active_preset_id.clone();
            let tabs: Vec<TabSnapshot> = pw.chart.panel_app.open_tabs
                .iter()
                .map(|pid| TabSnapshot {
                    name: preset_name.get(pid.as_str()).unwrap_or(&"").to_string(),
                    active: *pid == active_tab_id,
                    preset_id: pid.clone(),
                })
                .collect();

            // Build leaf→ChartId map for layout tree.
            let leaf_to_chart: StdHashMap<uzor::panels::LeafId, zengeld_chart::ChartId> =
                pw.chart.panel_app.panel_grid
                    .iter_windows()
                    .map(|(lid, w)| (lid, w.id))
                    .collect();

            // Charts
            let charts: Vec<ChartSnapshot> = pw.chart.panel_app.panel_grid
                .iter_windows()
                .map(|(leaf_id, cw)| {
                    let viewport = &cw.viewport;
                    let bars_visible = if viewport.bar_spacing > 0.0 {
                        (viewport.chart_width / viewport.bar_spacing).ceil() as usize
                    } else {
                        0
                    };

                    let indicators: Vec<IndicatorSummary> = pw.chart.indicator_manager
                        .instances_iter()
                        .filter(|inst| inst.symbol == cw.symbol)
                        .map(|inst| IndicatorSummary {
                            id: inst.id,
                            type_id: inst.type_id.clone(),
                            name: inst.name.clone(),
                        })
                        .collect();

                    let primitives: Vec<PrimitiveSummary> = cw.drawing_manager
                        .primitives()
                        .iter()
                        .map(|p| {
                            let d = p.data();
                            PrimitiveSummary { id: d.id, type_id: d.type_id.clone() }
                        })
                        .collect();

                    ChartSnapshot {
                        chart_id: cw.id.0,
                        leaf_id: leaf_id.0,
                        symbol: cw.symbol.clone(),
                        exchange: cw.exchange.clone(),
                        timeframe: cw.timeframe.name.clone(),
                        bar_count: cw.bars.len(),
                        viewport: ViewportSnapshot {
                            view_start: viewport.view_start,
                            bar_spacing: viewport.bar_spacing,
                            chart_width: viewport.chart_width,
                            chart_height: viewport.chart_height,
                            bars_visible,
                        },
                        indicator_count: indicators.len(),
                        primitive_count: primitives.len(),
                        indicators,
                        primitives,
                    }
                })
                .collect();

            // Layout tree from docking root.
            let root = pw.chart.panel_app.panel_grid.docking().tree().root();
            let layout = LayoutNode::Split {
                axis: match root.layout {
                    uzor::panels::WindowLayout::SplitHorizontal => "horizontal",
                    uzor::panels::WindowLayout::SplitVertical   => "vertical",
                    _                                           => "grid",
                }.to_string(),
                proportions: root.proportions.clone(),
                children: root.children
                    .iter()
                    .map(|c| build_layout_node(c, &leaf_to_chart))
                    .collect(),
            };

            snap.windows.push(WindowSnapshot {
                window_id: pw.window_id.clone(),
                tabs,
                active_tab_id,
                charts,
                layout,
            });
        }

        if let Ok(mut s) = agent_state.terminal_snapshot.write() {
            *s = snap;
        }
    }

    /// Drain agent commands pushed by HTTP handlers and apply them to the
    /// chart state.  Called every frame from `about_to_wait()`.
    fn drain_agent_commands(&mut self) {
        let agent_state = match self.agent_state {
            Some(ref s) => s.clone(),
            None => return,
        };

        let commands = agent_state.drain_commands();
        if commands.is_empty() { return; }

        for cmd in commands {
            match cmd {
                zengeld_server::state::AgentCommand::SetViewport {
                    window_id, chart_id, view_start, bar_spacing, mode,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if let Some(cw) = pw.chart.panel_app.panel_grid.windows_mut().values_mut()
                            .find(|cw| cw.id.0 == chart_id)
                        {
                            if let Some(mode_str) = &mode {
                                match mode_str.as_str() {
                                    "focus" => {
                                        let bar_count = cw.bars.len();
                                        if bar_count > 0 {
                                            let visible = if cw.viewport.bar_spacing > 0.0 {
                                                (cw.viewport.chart_width / cw.viewport.bar_spacing).ceil() as usize
                                            } else {
                                                1
                                            };
                                            cw.viewport.view_start = (bar_count as f64) - (visible as f64);
                                        }
                                    }
                                    "fit" => {
                                        let bar_count = cw.bars.len();
                                        if bar_count > 0 && cw.viewport.chart_width > 0.0 {
                                            cw.viewport.bar_spacing = cw.viewport.chart_width / bar_count as f64;
                                            cw.viewport.view_start = 0.0;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if let Some(vs) = view_start { cw.viewport.view_start = vs; }
                            if let Some(bs) = bar_spacing { cw.viewport.bar_spacing = bs; }
                            eprintln!("[AgentCommand] SetViewport: window={}, chart={}", window_id, chart_id);
                        } else {
                            eprintln!("[AgentCommand] chart not found: {} in window {}", chart_id, window_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::SwitchSymbol {
                    window_id, chart_id, symbol, exchange, timeframe,
                } => {
                    eprintln!(
                        "[AgentCommand] SwitchSymbol: window={}, chart={}, symbol={}/{}/{}",
                        window_id, chart_id, exchange, symbol, timeframe,
                    );
                    // TODO: implement actual symbol switch via DataBridge request
                }

                // ── Indicator CRUD ──────────────────────────────────────
                zengeld_server::state::AgentCommand::AddIndicator {
                    window_id, chart_id, type_id, params, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        // Find the symbol for this chart
                        let symbol = pw.chart.panel_app.panel_grid.windows()
                            .values().find(|cw| cw.id.0 == chart_id)
                            .map(|cw| cw.symbol.clone());
                        let bars: Vec<zengeld_chart::Bar> = pw.chart.panel_app.panel_grid.windows()
                            .values().find(|cw| cw.id.0 == chart_id)
                            .map(|cw| cw.bars.clone())
                            .unwrap_or_default();

                        if let Some(symbol) = symbol {
                            if let Some(new_id) = pw.chart.indicator_manager.create_instance(&type_id, &symbol) {
                                // Set window_id scope
                                if let Some(inst) = pw.chart.indicator_manager.get_instance_mut(new_id) {
                                    inst.window_id = Some(chart_id);
                                    // Apply custom params
                                    for (k, v) in &params {
                                        use zengeld_terminal_indicators::IndicatorParamValue;
                                        let iv = match v {
                                            serde_json::Value::Number(n) => {
                                                if let Some(i) = n.as_i64() {
                                                    IndicatorParamValue::Int(i as i32)
                                                } else {
                                                    IndicatorParamValue::Float(n.as_f64().unwrap_or(0.0))
                                                }
                                            }
                                            serde_json::Value::Bool(b) => IndicatorParamValue::Bool(*b),
                                            serde_json::Value::String(s) => IndicatorParamValue::String(s.clone()),
                                            _ => continue,
                                        };
                                        inst.set_param(k, iv);
                                    }
                                }
                                pw.chart.indicator_manager.calculate(new_id, &bars);
                                pw.chart.sync_sub_panes_from_manager();
                                eprintln!("[AgentCommand] AddIndicator: type={}, id={}, chart={}", type_id, new_id, chart_id);
                            } else {
                                eprintln!("[AgentCommand] AddIndicator: unknown type_id '{}'", type_id);
                            }
                        } else {
                            eprintln!("[AgentCommand] AddIndicator: chart not found: {}", chart_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] AddIndicator: window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::UpdateIndicator {
                    window_id, chart_id: _, indicator_id, params, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if let Some(inst) = pw.chart.indicator_manager.get_instance_mut(indicator_id) {
                            use zengeld_terminal_indicators::IndicatorParamValue;
                            for (k, v) in &params {
                                let iv = match v {
                                    serde_json::Value::Number(n) => {
                                        if let Some(i) = n.as_i64() {
                                            IndicatorParamValue::Int(i as i32)
                                        } else {
                                            IndicatorParamValue::Float(n.as_f64().unwrap_or(0.0))
                                        }
                                    }
                                    serde_json::Value::Bool(b) => IndicatorParamValue::Bool(*b),
                                    serde_json::Value::String(s) => IndicatorParamValue::String(s.clone()),
                                    _ => continue,
                                };
                                inst.set_param(k, iv);
                            }
                            let symbol = inst.symbol.clone();
                            // Get bars for recalculation
                            let bars: Vec<zengeld_chart::Bar> = pw.chart.panel_app.panel_grid.windows()
                                .values().find(|cw| cw.symbol == symbol)
                                .map(|cw| cw.bars.clone())
                                .unwrap_or_default();
                            pw.chart.indicator_manager.calculate(indicator_id, &bars);
                            eprintln!("[AgentCommand] UpdateIndicator: id={}", indicator_id);
                        } else {
                            eprintln!("[AgentCommand] UpdateIndicator: instance not found: {}", indicator_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] UpdateIndicator: window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::RemoveIndicator {
                    window_id, chart_id: _, indicator_id, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if pw.chart.indicator_manager.remove_instance(indicator_id).is_some() {
                            pw.chart.sync_sub_panes_from_manager();
                            eprintln!("[AgentCommand] RemoveIndicator: id={}", indicator_id);
                        } else {
                            eprintln!("[AgentCommand] RemoveIndicator: not found: {}", indicator_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] RemoveIndicator: window not found: {}", window_id);
                    }
                }

                // ── Primitive CRUD ─────────────────────────────────────
                zengeld_server::state::AgentCommand::AddPrimitive {
                    window_id, chart_id, type_id, points, style, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let pts: Vec<(f64, f64)> = points.iter().map(|p| (p[0], p[1])).collect();
                        let color_str = style.color.clone();
                        let registry = zengeld_chart::drawing::primitives_v2::PrimitiveRegistry::global().read().unwrap();
                        if let Some(mut prim) = registry.create(&type_id, &pts, Some(&color_str)) {
                            prim.data_mut().color = zengeld_chart::drawing::primitives_v2::PrimitiveColor {
                                stroke: style.color,
                                fill: style.fill_color,
                            };
                            prim.data_mut().width = style.width;
                            drop(registry);
                            let cw = pw.chart.panel_app.panel_grid.windows_mut().values_mut()
                                .find(|cw| cw.id.0 == chart_id);
                            if let Some(cw) = cw {
                                cw.drawing_manager.add_external_primitive(prim);
                                eprintln!("[AgentCommand] AddPrimitive: type={}, chart={}", type_id, chart_id);
                            } else {
                                eprintln!("[AgentCommand] AddPrimitive: chart not found: {}", chart_id);
                            }
                        } else {
                            drop(registry);
                            eprintln!("[AgentCommand] AddPrimitive: unknown type: {}", type_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] AddPrimitive: window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::UpdatePrimitive {
                    window_id, chart_id, primitive_id, points, style, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let cw = pw.chart.panel_app.panel_grid.windows_mut().values_mut()
                            .find(|cw| cw.id.0 == chart_id);
                        if let Some(cw) = cw {
                            let prim = cw.drawing_manager.primitives_mut()
                                .iter_mut().find(|p| p.data().id == primitive_id);
                            if let Some(prim) = prim {
                                if let Some(pts) = points {
                                    let new_pts: Vec<(f64, f64)> = pts.iter().map(|p| (p[0], p[1])).collect();
                                    prim.set_points(&new_pts);
                                }
                                if let Some(s) = style {
                                    prim.data_mut().color = zengeld_chart::drawing::primitives_v2::PrimitiveColor {
                                        stroke: s.color,
                                        fill: s.fill_color,
                                    };
                                    prim.data_mut().width = s.width;
                                }
                                eprintln!("[AgentCommand] UpdatePrimitive: id={}", primitive_id);
                            } else {
                                eprintln!("[AgentCommand] UpdatePrimitive: primitive not found: {}", primitive_id);
                            }
                        } else {
                            eprintln!("[AgentCommand] UpdatePrimitive: chart not found: {}", chart_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] UpdatePrimitive: window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::RemovePrimitive {
                    window_id, chart_id, primitive_id, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let removed = {
                            let cw = pw.chart.panel_app.panel_grid.windows_mut().values_mut()
                                .find(|cw| cw.id.0 == chart_id);
                            if let Some(cw) = cw {
                                let idx = cw.drawing_manager.primitives()
                                    .iter().position(|p| p.data().id == primitive_id);
                                if let Some(idx) = idx {
                                    cw.drawing_manager.remove(idx);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };
                        if removed {
                            eprintln!("[AgentCommand] RemovePrimitive: id={}", primitive_id);
                        } else {
                            eprintln!("[AgentCommand] RemovePrimitive: not found: {} in chart {}", primitive_id, chart_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] RemovePrimitive: window not found: {}", window_id);
                    }
                }

                // ── Screenshot (Phase 5) ───────────────────────────────
                zengeld_server::state::AgentCommand::RequestScreenshot {
                    window_id, chart_id, agent_id: _, response_tx,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        pw.pending_agent_screenshots.push((chart_id, response_tx));
                        // Request a redraw so the screenshot is captured this frame.
                        pw.window.request_redraw();
                        eprintln!("[AgentCommand] RequestScreenshot queued: window={}, chart={}", window_id, chart_id);
                    } else {
                        let _ = response_tx.send(Err(format!("window not found: {}", window_id)));
                        eprintln!("[AgentCommand] RequestScreenshot: window not found: {}", window_id);
                    }
                }

                zengeld_server::state::AgentCommand::CreateKey { label, tier } => {
                    let raw_key = agent_state.create_key_for_ui(&label, &tier);
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.user_settings_state.last_created_key = Some(raw_key.clone());
                    }
                    // Sync keys to AppState and persist immediately
                    self.sync_keys_from_agent(&agent_state);
                    eprintln!("[AgentCommand] CreateKey: label={}, tier={}", label, tier);
                }

                zengeld_server::state::AgentCommand::DeleteKey { label } => {
                    agent_state.remove_key(&label);
                    // Sync keys to AppState and persist immediately
                    self.sync_keys_from_agent(&agent_state);
                    eprintln!("[AgentCommand] DeleteKey: label={}", label);
                }
            }
        }
    }

    /// Build and push an [`IndicatorSnapshot`] from all open windows.
    ///
    /// Sync API keys from AgentState back to AppState and persist to disk immediately.
    /// Called after CreateKey / DeleteKey / Regenerate to ensure keys survive crashes.
    fn sync_keys_from_agent(&mut self, agent_state: &std::sync::Arc<zengeld_server::AgentState>) {
        let server_keys = agent_state.list_keys();
        self.app_state.agent_api_keys = server_keys.iter().map(|k| {
            zengeld_chart::StoredApiKey {
                key_hash: k.key_hash.clone(),
                label: k.label.clone(),
                tier: k.tier.clone(),
                created_at: k.created_at,
                agent_id: k.agent_id.clone(),
                source: match k.source {
                    zengeld_server::state::KeySource::Cloud => "cloud".to_string(),
                    zengeld_server::state::KeySource::Local => "local".to_string(),
                },
            }
        }).collect();
        // Persist profile with updated keys
        let mut profile = self.profile.clone();
        profile.agent_api_keys = self.app_state.agent_api_keys.clone();
        profile.agent_api_key = String::new();
        if let Err(e) = zengeld_chart::save_profile(&profile) {
            eprintln!("[App] Failed to persist keys: {}", e);
        } else {
            self.profile = profile;
            eprintln!("[App] Keys persisted to profile ({} keys)", self.app_state.agent_api_keys.len());
        }
    }

    /// Called at most once per second from `about_to_wait()` to avoid
    /// per-frame allocations.  Iterates all per-window indicator managers
    /// and collects instance metadata + computed output series.
    fn update_indicator_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{IndicatorSnapshot, IndicatorInstanceSnapshot, IndicatorOutputSnapshot};
        use zengeld_terminal_indicators::IndicatorParamValue;

        let mut snapshot = IndicatorSnapshot::default();

        for pw in self.windows.values() {
            for inst in pw.chart.indicator_manager.instances_iter() {
                // Convert params: IndicatorParamValue → serde_json::Value
                let params: std::collections::HashMap<String, serde_json::Value> = inst
                    .params
                    .iter()
                    .map(|(k, v)| {
                        let json_val = match v {
                            IndicatorParamValue::Int(n)    => serde_json::Value::Number(serde_json::Number::from(*n)),
                            IndicatorParamValue::Float(f)  => serde_json::Number::from_f64(*f)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null),
                            IndicatorParamValue::Bool(b)   => serde_json::Value::Bool(*b),
                            IndicatorParamValue::String(s) => serde_json::Value::String(s.clone()),
                            IndicatorParamValue::Color(c)  => serde_json::Value::String(c.clone()),
                        };
                        (k.clone(), json_val)
                    })
                    .collect();

                // Convert computed output series (HashMap<String, Vec<f64>>)
                let outputs: Vec<IndicatorOutputSnapshot> = inst
                    .values
                    .iter()
                    .map(|(name, vals)| IndicatorOutputSnapshot {
                        name: name.clone(),
                        values: vals.clone(),
                    })
                    .collect();

                let instance_snap = IndicatorInstanceSnapshot {
                    id: inst.id,
                    type_id: inst.type_id.clone(),
                    type_name: inst.name.clone(),
                    symbol: inst.symbol.clone(),
                    window_id: inst.window_id,
                    params,
                    outputs,
                };

                snapshot
                    .symbols
                    .entry(inst.symbol.clone())
                    .or_default()
                    .push(instance_snap);
            }
        }

        if let Ok(mut snap) = agent_state.indicator_snapshot.write() {
            *snap = snapshot;
        }
    }

    /// Build and push a [`WatchlistSnapshot`] from the current AppState.
    ///
    /// Called at most once per second from `about_to_wait()`.
    fn update_watchlist_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{WatchlistSnapshot, WatchlistEntry, WatchlistItemEntry};

        let wm = &self.app_state.watchlist_manager;
        let active_id = wm.active_list_id;

        let watchlists: Vec<WatchlistEntry> = wm.lists.iter().map(|list| {
            let items: Vec<WatchlistItemEntry> = list.all_symbols().into_iter().map(|ws| {
                WatchlistItemEntry {
                    symbol: ws.symbol.clone(),
                    exchange: ws.exchange.clone(),
                    category: String::new(),
                }
            }).collect();

            WatchlistEntry {
                id: list.id,
                name: list.name.clone(),
                active: list.id == active_id,
                items,
            }
        }).collect();

        if let Ok(mut snap) = agent_state.watchlist_snapshot.write() {
            *snap = WatchlistSnapshot { watchlists };
        }
    }

    /// Build and push a [`ConnectorSnapshot`] from the live-data bridge.
    ///
    /// Called at most once per second from `about_to_wait()`.
    fn update_connector_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{ConnectorSnapshot, ConnectorEntry};

        let metrics = self.bridge.collect_metrics();

        let connectors: Vec<ConnectorEntry> = metrics.into_iter().map(|(eid, stats, ws_count)| {
            ConnectorEntry {
                exchange_id: eid.as_str().to_string(),
                active: stats.http_requests > 0,
                ws_active: ws_count > 0,
                symbol_count: 0,
            }
        }).collect();

        if let Ok(mut snap) = agent_state.connector_snapshot.write() {
            *snap = ConnectorSnapshot { connectors };
        }
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Only create windows once
        if !self.windows.is_empty() {
            return;
        }

        if self.saved_windows.is_empty() {
            eprintln!("[App] No saved windows — creating default");
            // No saved windows — create one with defaults
            self.create_window(event_loop, None, None);
        } else {
            // Restore ALL windows equally from saved state — no "primary" distinction
            let windows_to_restore = self.saved_windows.clone();
            for ws in &windows_to_restore {
                eprintln!("[App] Restoring window: id={} pos=({:?},{:?}) size=({:?},{:?}) tabs={} active={}",
                    ws.window_id, ws.x, ws.y, ws.width, ws.height,
                    ws.open_tabs.len(), ws.active_preset_id);
                self.create_window(event_loop, Some(ws), None);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let _t0 = std::time::Instant::now();

        // ── Frame timing ────────────────────────────────────────────────────
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame_instant);
        self.last_frame_instant = now;
        let dt_ms = dt.as_secs_f64() * 1000.0;
        if dt_ms > 0.1 {
            // Exponential moving average with α = 0.1
            let instant_fps = 1000.0 / dt_ms;
            self.fps_ema = self.fps_ema * 0.9 + instant_fps * 0.1;
            self.last_frame_time_ms = dt_ms;
        }

        // ── GPU info: query once ────────────────────────────────────────────
        if self.gpu_name.is_empty() && !self.render_cx.devices.is_empty() {
            let info = self.render_cx.devices[0].adapter().get_info();
            self.gpu_name = info.name;
            self.gpu_driver = info.driver_info;
            eprintln!("[App] GPU: {} ({})", self.gpu_name, self.gpu_driver);
            // Write to shared so telemetry thread can read it.
            if let Ok(mut g) = self.telemetry_shared.gpu_name.lock() {
                *g = self.gpu_name.clone();
            }
        }

        // ── OTA updater status check ─────────────────────────────────────────
        if let Some(ref handle) = self.updater_handle {
            if handle.status_rx.has_changed().unwrap_or(false) {
                let status = handle.status_rx.borrow().clone();
                match &status {
                    zengeld_updater::UpdateStatus::UpdateAvailable(info) => {
                        eprintln!("[Updater] Update available: v{}", info.version);
                        let msg = format!("Update v{} available — click to install", info.version);
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        self.active_toasts.push(alert_delivery::ToastNotification {
                            title: "Update Available".to_string(),
                            message: msg,
                            timestamp: ts,
                            duration_ms: 8000,
                        });
                    }
                    zengeld_updater::UpdateStatus::Error(e) => {
                        eprintln!("[Updater] Error: {}", e);
                    }
                    _ => {}
                }
            }
        }

        // ── App-level tick ────────────────────────────────────────────────
        // Process app-level broadcast messages (ConnectorReady → request_symbols).
        self.tick_app_state();
        let _t1 = std::time::Instant::now();

        // ── Drain watchlist actions from all windows → AppState ────────────
        // Windows queue WatchlistAction instead of mutating directly.
        // App applies them to the single AppState watchlist.
        let mut watchlist_had_actions = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.watchlist_actions.drain(..) {
                watchlist_had_actions = true;
                match action {
                    chart_app::WatchlistAction::Toggle { symbol, exchange } => {
                        let now_in = self.app_state.watchlist_manager.toggle_symbol(&symbol, &exchange);
                        eprintln!("[App] watchlist toggle: {}:{} -> in_watchlist={}", symbol, exchange, now_in);
                        if now_in {
                            if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                                let enabled = self.app_state.connector_enabled
                                    .get(eid.as_str()).copied().unwrap_or(true);
                                if enabled {
                                    self.bridge.ensure_connector(eid);
                                    self.bridge.subscribe_mini_ticker(eid, &symbol);
                                }
                            }
                            if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                                list.order_snapshot = None;
                            }
                        } else {
                            if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                                self.bridge.unsubscribe_mini_ticker(eid, &symbol);
                            }
                            if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                                if let Some(ref mut snap) = list.order_snapshot {
                                    snap.retain(|s| s.symbol != symbol);
                                }
                            }
                        }
                    }
                    chart_app::WatchlistAction::Remove { symbol, exchange } => {
                        self.app_state.watchlist_manager.remove_symbol(&symbol, &exchange);
                    }
                    chart_app::WatchlistAction::ClearOrderSnapshot => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            list.order_snapshot = None;
                        }
                    }
                    chart_app::WatchlistAction::Reorder { from_idx, to_idx } => {
                        self.app_state.watchlist_manager.reorder_symbol(from_idx, to_idx);
                    }
                    chart_app::WatchlistAction::CreateList { name } => {
                        let new_id = self.app_state.watchlist_manager.create_list(name);
                        self.app_state.watchlist_manager.active_list_id = new_id;
                    }
                    chart_app::WatchlistAction::RenameList { id, new_name } => {
                        if let Some(list) = self.app_state.watchlist_manager.lists.iter_mut().find(|l| l.id == id) {
                            list.name = new_name;
                        }
                    }
                    chart_app::WatchlistAction::DeleteList { id } => {
                        self.app_state.watchlist_manager.delete_list(id);
                    }
                    chart_app::WatchlistAction::SetActiveList { id } => {
                        self.app_state.watchlist_manager.active_list_id = id;
                    }
                    chart_app::WatchlistAction::SetColorFlag { symbol, exchange, color } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            let color_str = color.as_deref().unwrap_or("");
                            list.set_color_flag(&symbol, &exchange, color_str);
                        }
                    }
                    chart_app::WatchlistAction::MoveToGroup { .. } => {
                        // TODO: implement group move when group support is needed
                    }
                    chart_app::WatchlistAction::RemoveFromGroup { .. } => {
                        // TODO: implement remove from group when group support is needed
                    }
                    chart_app::WatchlistAction::SetSeparatorOffsets { offsets } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            list.column_config.separator_offsets = Some(offsets);
                        }
                    }
                    chart_app::WatchlistAction::ResetSeparatorOffsets => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            list.column_config.separator_offsets = None;
                        }
                    }
                    chart_app::WatchlistAction::SetSeparatorOffset { index, value } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            if list.column_config.separator_offsets.is_none() {
                                // Default: 6 columns → 5 separators, evenly spaced
                                list.column_config.separator_offsets = Some(vec![0.0; 5]);
                            }
                            if let Some(ref mut offsets) = list.column_config.separator_offsets {
                                if index < offsets.len() {
                                    offsets[index] = value;
                                }
                            }
                        }
                    }
                    chart_app::WatchlistAction::ToggleColumnVisibility { column } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            match column.as_str() {
                                "exchange"    => list.column_config.show_exchange    = !list.column_config.show_exchange,
                                "last_price"  => list.column_config.show_last_price  = !list.column_config.show_last_price,
                                "change_pct"  => list.column_config.show_change_pct  = !list.column_config.show_change_pct,
                                "change_abs"  => list.column_config.show_change_abs  = !list.column_config.show_change_abs,
                                "volume"      => list.column_config.show_volume      = !list.column_config.show_volume,
                                "high_low"    => list.column_config.show_high_low    = !list.column_config.show_high_low,
                                _ => {}
                            }
                            // Reset separator offsets when column visibility changes
                            list.column_config.separator_offsets = None;
                        }
                    }
                    chart_app::WatchlistAction::SortCycle => {
                        // Clear order snapshot to trigger re-sort on next render
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            list.order_snapshot = None;
                        }
                    }
                    chart_app::WatchlistAction::ResetSort => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            list.order_snapshot = None;
                        }
                    }
                }
            }
        }
        if watchlist_had_actions {
            self.app_state.watchlists_dirty = true;
        }

        // ── Drain connector actions from all windows → AppState ──────────────
        let mut connectors_had_actions = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.connector_actions.drain(..) {
                connectors_had_actions = true;
                match action {
                    chart_app::ConnectorAction::ToggleEnabled { exchange_id } => {
                        let enabled = self.app_state.connector_enabled
                            .entry(exchange_id.clone())
                            .or_insert(true);
                        *enabled = !*enabled;
                        eprintln!("[App] connector toggle: {} -> enabled={}", exchange_id, *enabled);
                    }
                }
            }
        }
        if connectors_had_actions {
            self.app_state.connectors_dirty = true;
        }

        // ── Drain preset actions from all windows → AppState ────────────
        let mut presets_had_actions = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.preset_actions.drain(..) {
                presets_had_actions = true;
                match action {
                    chart_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.app_state.presets.insert(id.clone(), preset);
                        self.app_state.preset_dirty_ids.insert(id);
                    }
                    chart_app::PresetAction::Delete { id } => {
                        self.app_state.presets.remove(&id);
                        if let Err(e) = zengeld_chart::preset::storage::delete_preset(&id) {
                            eprintln!("[App] failed to delete preset file {}: {}", id, e);
                        }
                        self.app_state.preset_dirty_ids.remove(&id);
                    }
                    chart_app::PresetAction::Rename { id, new_name } => {
                        if let Some(preset) = self.app_state.presets.get_mut(&id) {
                            preset.name = new_name;
                            self.app_state.preset_dirty_ids.insert(id);
                        }
                    }
                }
            }
        }
        if presets_had_actions {
            self.app_state.presets_dirty = true;
        }

        // ── Drain snapshot actions from all windows → AppState ──────────
        let mut snapshots_had_actions = false;
        for (_wid, pw) in self.windows.iter_mut() {
            for action in pw.chart.snapshot_actions.drain(..) {
                snapshots_had_actions = true;
                match action {
                    chart_app::SnapshotAction::ChartSettings(data) => {
                        self.app_state.snapshots.chart_settings = Some(data);
                    }
                    chart_app::SnapshotAction::PrimitiveSettings { type_id, data } => {
                        self.app_state.snapshots.primitive_settings.insert(type_id, data);
                    }
                    chart_app::SnapshotAction::IndicatorSettings { type_id, data } => {
                        self.app_state.snapshots.indicator_settings.insert(type_id, data);
                    }
                    chart_app::SnapshotAction::CompareSettings(data) => {
                        self.app_state.snapshots.compare_settings = Some(data);
                    }
                }
            }
        }
        if snapshots_had_actions {
            self.app_state.snapshots_dirty = true;
        }

        // ── Drain template actions from all windows → AppState ──────────
        let mut templates_had_actions = false;
        for (_wid, pw) in self.windows.iter_mut() {
            for action in pw.chart.template_actions.drain(..) {
                templates_had_actions = true;
                match action {
                    chart_app::TemplateAction::AddPrimitive(tmpl) => {
                        let _ = self.app_state.template_manager.add_primitive_template(tmpl);
                    }
                    chart_app::TemplateAction::RemovePrimitive { id } => {
                        let _ = self.app_state.template_manager.remove_primitive_template(&id);
                    }
                    chart_app::TemplateAction::AddIndicator(tmpl) => {
                        let _ = self.app_state.template_manager.add_indicator_template(tmpl);
                    }
                    chart_app::TemplateAction::RemoveIndicator { id } => {
                        let _ = self.app_state.template_manager.remove_indicator_template(&id);
                    }
                    chart_app::TemplateAction::AddCompare(tmpl) => {
                        let _ = self.app_state.template_manager.add_compare_template(tmpl);
                    }
                    chart_app::TemplateAction::RemoveCompare { id } => {
                        let _ = self.app_state.template_manager.remove_compare_template(&id);
                    }
                    chart_app::TemplateAction::AddChart(tmpl) => {
                        let _ = self.app_state.template_manager.add_chart_template(tmpl);
                    }
                    chart_app::TemplateAction::RemoveChart { id } => {
                        let _ = self.app_state.template_manager.remove_chart_template(&id);
                    }
                    chart_app::TemplateAction::AddIndicatorSet(set) => {
                        let _ = self.app_state.template_manager.add_indicator_set(set);
                    }
                    chart_app::TemplateAction::RemoveIndicatorSet { id } => {
                        let _ = self.app_state.template_manager.remove_indicator_set(&id);
                    }
                }
            }
        }
        if templates_had_actions {
            self.app_state.templates_dirty = true;
        }

        // ── Drain performance actions from all windows ─────────────────────
        let mut reset_instanced_renderer = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.perf_actions.drain(..) {
                match action {
                    chart_app::PerfAction::SetFpsLimit(v) => {
                        self.fps_limit = v;
                        eprintln!("[App] FPS limit → {}", v);
                    }
                    chart_app::PerfAction::SetMsaa(v) => {
                        self.msaa_samples = v;
                        eprintln!("[App] MSAA → {}", v);
                    }
                    chart_app::PerfAction::SetMaxBars(v) => {
                        self.max_bars = v;
                        eprintln!("[App] Max bars → {}", v);
                    }
                    chart_app::PerfAction::SetRecalcMode(ref mode) => {
                        self.app_state.recalc_mode = match mode.as_str() {
                            "PerTick" => chart_app::RecalcMode::PerTick,
                            "PerBar"  => chart_app::RecalcMode::PerBar,
                            _         => chart_app::RecalcMode::PerFrame,
                        };
                        eprintln!("[App] RecalcMode → {:?}", self.app_state.recalc_mode);
                    }
                    chart_app::PerfAction::TogglePerfLog => {
                        self.perf_log_enabled = !self.perf_log_enabled;
                        eprintln!("[App] Perf logging → {}", self.perf_log_enabled);
                    }
                    chart_app::PerfAction::SetBackend(ref name) => {
                        use sidebar_content::state::RenderBackend;
                        let new_backend = match name.as_str() {
                            "Instanced wGPU" => RenderBackend::InstancedWgpu,
                            "Vello CPU" => RenderBackend::VelloCpu,
                            "Vello Hybrid" => RenderBackend::VelloHybrid,
                            "Tiny-Skia CPU" => RenderBackend::TinySkia,
                            _ => RenderBackend::VelloGpu,
                        };
                        if new_backend != self.render_backend {
                            reset_instanced_renderer = true;
                        }
                        self.render_backend = new_backend;
                        eprintln!("[App] Backend → {:?}", self.render_backend);
                    }
                    chart_app::PerfAction::ToggleVsync => {
                        self.vsync_enabled = !self.vsync_enabled;
                        eprintln!("[App] VSync → {}", self.vsync_enabled);
                    }
                }
            }
        }
        // Note: we intentionally do NOT drop pw.instanced_renderer on backend
        // switch.  The GPU thread may still be using it for the in-flight frame.
        // The renderer is lazily created with surface_texture.format() so it
        // always matches the swapchain; keeping it alive just costs some GPU memory.
        let _ = reset_instanced_renderer;
        let _t2 = std::time::Instant::now();

        // ── Flush dirty presets to disk ──────────────────────────────────
        if !self.app_state.preset_dirty_ids.is_empty() {
            let ids: Vec<String> = self.app_state.preset_dirty_ids.drain().collect();
            for id in ids {
                if let Some(preset) = self.app_state.presets.get(&id) {
                    if let Err(e) = zengeld_chart::preset::storage::save_preset(preset) {
                        eprintln!("[App] failed to save preset {}: {}", id, e);
                    }
                }
            }
        }

        // ── Dirty-flag persistence ──────────────────────────────────────
        // If any window marked profile or watchlists dirty, save now
        // with full multi-window context.
        {
            let any_profile_dirty = self.windows.values().any(|pw| pw.chart.profile_dirty);
            let any_watchlists_dirty = self.windows.values().any(|pw| pw.chart.watchlists_dirty);

            if any_profile_dirty {
                // Autosave every window's active preset first.
                for pw in self.windows.values_mut() {
                    pw.chart.autosave_snapshot();
                }

                // Sync OS position/size into ChartApp fields.
                // Skip minimized windows — Windows returns (-32000, -32000).
                for pw in self.windows.values_mut() {
                    if pw.window.is_minimized().unwrap_or(false) {
                        continue;
                    }
                    if let Ok(pos) = pw.window.outer_position() {
                        if pos.x > -30000 && pos.y > -30000 {
                            pw.chart.window_x = Some(pos.x);
                            pw.chart.window_y = Some(pos.y);
                        }
                    }
                    let sz = pw.window.inner_size();
                    if sz.width > 0 && sz.height > 0 {
                        pw.chart.window_width = Some(sz.width);
                        pw.chart.window_height = Some(sz.height);
                    }
                }

                // Collect window states from ALL windows.
                let window_states: Vec<zengeld_chart::WindowState> = self.windows.values()
                    .map(|pw| pw.chart.build_window_state())
                    .collect();

                // Build profile from the preferred (last-focused) window's UI state.
                let preferred_key = self.last_focused
                    .filter(|id| self.windows.contains_key(id))
                    .or_else(|| self.windows.keys().next().copied());

                let mut profile = self.profile.clone();
                profile.windows = window_states;
                profile.connector_enabled = self.app_state.connector_enabled.clone();
                profile.active_theme = self.app_state.theme_preset.clone();
                profile.device_name = self.app_state.device_name.clone();
                profile.app_version = self.app_state.app_version.clone();
                profile.recalc_mode = match self.app_state.recalc_mode {
                    chart_app::RecalcMode::PerTick  => "PerTick".to_string(),
                    chart_app::RecalcMode::PerFrame => "PerFrame".to_string(),
                    chart_app::RecalcMode::PerBar   => "PerBar".to_string(),
                };

                if let Some(key) = preferred_key {
                    if let Some(pw) = self.windows.get(&key) {
                        profile.sidebar_visible = pw.chart.sidebar_state.is_right_open();
                        profile.sidebar_panel = chart_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                        profile.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                        let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                        profile.inline_bar_x = Some(inline.x);
                        profile.inline_bar_y = Some(inline.y);
                        let dock_str = match inline.dock_edge {
                            zengeld_chart::InlineDockEdge::Bottom => "Bottom",
                            zengeld_chart::InlineDockEdge::Top => "Top",
                            zengeld_chart::InlineDockEdge::Free => "Free",
                        };
                        profile.inline_bar_dock = Some(dock_str.to_string());
                    }
                }

                if let Err(e) = zengeld_chart::save_profile(&profile) {
                    eprintln!("[App] Failed to save profile: {}", e);
                } else {
                    self.profile = profile;
                }

                // Clear dirty flags.
                for pw in self.windows.values_mut() {
                    pw.chart.profile_dirty = false;
                }
            }

            if any_watchlists_dirty {
                let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
                if let Err(e) = zengeld_chart::save_json(&watchlists_path, &self.app_state.watchlist_manager) {
                    eprintln!("[App] Failed to save watchlists: {}", e);
                }
                // Clear dirty flags.
                for pw in self.windows.values_mut() {
                    pw.chart.watchlists_dirty = false;
                }
            }
        }
        let _t3 = std::time::Instant::now();

        // ── App shutdown ────────────────────────────────────────────────
        // Chrome X button or Alt+F4 on ANY window → close entire app.
        let shutdown = self.close_all_requested
            || self.windows.values().any(|pw| pw.close_requested);

        if shutdown {
            // Wait for any in-flight GPU frame before saving and exiting.
            // This ensures the GPU thread is not writing to PerWindowState fields
            // (pending_delivery_events, etc.) while we read them in save_all.
            self.wait_for_gpu_frame();
            // Shutdown the GPU render thread cleanly.
            if let Some(ref tx) = self.gpu_cmd_tx {
                let _ = tx.send(GpuCommand::Shutdown);
            }
            self.save_all(&[]);
            event_loop.exit();
            return;
        }

        // ── Single-window close/delete (from chrome context menu) ───────
        // Close window: save state but keep in profile.
        // Delete window: remove from active AND from profile.
        {
            let mut windows_to_close: Vec<(WindowId, bool)> = Vec::new(); // (id, is_delete)
            for (&wid, pw) in self.windows.iter() {
                if pw.delete_window_requested {
                    windows_to_close.push((wid, true));
                } else if pw.close_window_requested {
                    windows_to_close.push((wid, false));
                }
            }

            if !windows_to_close.is_empty() {
                // Autosave closing windows before removal.
                for (wid, _) in &windows_to_close {
                    if let Some(pw) = self.windows.get_mut(wid) {
                        pw.chart.autosave_snapshot();
                    }
                }

                let deleted_ids: Vec<String> = windows_to_close.iter()
                    .filter(|(_, is_delete)| *is_delete)
                    .filter_map(|(wid, _)| self.windows.get(wid).map(|pw| pw.window_id.clone()))
                    .collect();

                self.save_all(&deleted_ids);

                for (wid, _) in windows_to_close {
                    self.windows.remove(&wid);
                }
            }
        }

        // If no windows left after close/delete, exit
        if self.windows.is_empty() {
            event_loop.exit();
            return;
        }

        // Drain new-window spawn requests from any window
        let mut spawn_requests: Vec<(WindowId, usize)> = Vec::new();
        for (&wid, pw) in self.windows.iter_mut() {
            if pw.spawn_new_window {
                pw.spawn_new_window = false;
                spawn_requests.push((wid, 1));
            }
        }
        for (source_wid, _) in spawn_requests {
            self.pending_spawns.push(SpawnRequest {
                cascade_from: Some(source_wid),
            });
        }

        // Drain pending window spawns
        let spawns: Vec<SpawnRequest> = self.pending_spawns.drain(..).collect();
        let spawned_any = !spawns.is_empty();
        for req in spawns {
            self.create_window(event_loop, None, req.cascade_from);
        }
        // New windows must be persisted so they survive a crash/restart.
        if spawned_any {
            for pw in self.windows.values_mut() {
                pw.chart.profile_dirty = true;
            }
        }

        // ── Drain theme changes → AppState → all windows ────────────────
        // If any window switched the theme preset, propagate the change to
        // all windows so they stay visually in sync, then update AppState.
        {
            let new_theme: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.theme_changed.take());
            if let Some(ref preset) = new_theme {
                self.app_state.theme_preset = preset.clone();
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.theme_manager.set_preset(preset);
                }
            }
        }

        // ── Drain recalc_mode changes → AppState → all windows ──────────
        // If any window changed the recalc mode from the User Settings modal,
        // propagate the change to all windows and update AppState.
        {
            let new_mode: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.recalc_mode_changed.take());
            if let Some(ref mode_str) = new_mode {
                self.app_state.recalc_mode = match mode_str.as_str() {
                    "PerTick" => chart_app::RecalcMode::PerTick,
                    "PerBar"  => chart_app::RecalcMode::PerBar,
                    _         => chart_app::RecalcMode::PerFrame,
                };
                let recalc_mode = self.app_state.recalc_mode;
                for pw in self.windows.values_mut() {
                    pw.chart.indicator_manager.recalc_mode = recalc_mode;
                }
                eprintln!("[App] recalc_mode changed to: {}", mode_str);
            }
        }

        // ── Drain server_enabled changes → AppState ──────────────────────
        {
            let server_change: Option<bool> = self.windows.values_mut()
                .find_map(|pw| pw.chart.server_enabled_changed.take());
            if let Some(enabled) = server_change {
                self.app_state.server_enabled = enabled;
                // Sync to all windows
                let status = if enabled { "running" } else { "stopped" };
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_settings_state.server_enabled = enabled;
                    pw.chart.panel_app.user_settings_state.server_status = status.to_string();
                }
                eprintln!("[App] server_enabled changed to: {}", enabled);
                // Note: actual server start/stop requires app restart for now
            }
        }

        // ── Drain api_key changes (legacy single-key hot-reload) ─────────
        // When the UI Regenerate button creates a new master key, register it
        // as an admin key in AgentState and persist immediately.
        {
            let key_change: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.api_key_changed.take());
            if let Some(raw_key) = key_change {
                if !raw_key.is_empty() {
                    if let Some(agent_state) = self.agent_state.clone() {
                        // Remove any previous "master" key, then add the new one
                        agent_state.remove_key("master");
                        let key_hash = zengeld_server::state::hash_key(&raw_key);
                        let entry = zengeld_server::state::ApiKeyEntry {
                            key_hash,
                            label: "master".to_string(),
                            tier: "admin".to_string(),
                            permissions: zengeld_server::state::Permissions::admin(),
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            agent_id: None,
                            source: zengeld_server::state::KeySource::Local,
                        };
                        agent_state.add_key(entry);
                        self.sync_keys_from_agent(&agent_state);
                        eprintln!("[App] Master API key regenerated and persisted");
                    }
                }
            }
        }

        // ── Drain key create requests from UI ────────────────────────────
        {
            let create_req: Option<(String, String)> = self.windows.values_mut()
                .find_map(|pw| pw.chart.key_create_request.take());
            if let Some((label, tier)) = create_req {
                if let Some(ref agent_state) = self.agent_state {
                    use zengeld_server::state::AgentCommand;
                    agent_state.push_command(AgentCommand::CreateKey { label, tier });
                }
            }
        }

        // ── Drain key delete requests from UI ────────────────────────────
        {
            let delete_req: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.key_delete_request.take());
            if let Some(label) = delete_req {
                if let Some(ref agent_state) = self.agent_state {
                    use zengeld_server::state::AgentCommand;
                    agent_state.push_command(AgentCommand::DeleteKey { label });
                }
            }
        }

        // ── Drain clipboard requests ─────────────────────────────────────
        {
            let clip: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.clipboard_text.take());
            if let Some(text) = clip {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Err(e) = cb.set_text(&text) {
                        eprintln!("[App] clipboard copy failed: {}", e);
                    }
                } else {
                    eprintln!("[App] clipboard unavailable");
                }
            }
        }

        // ── Drain open-URL requests ──────────────────────────────────────
        {
            let url: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.pending_open_url.take());
            if let Some(ref url) = url {
                #[cfg(target_os = "windows")]
                {
                    if let Err(e) = std::process::Command::new("cmd")
                        .args(["/c", "start", "", url])
                        .spawn()
                    {
                        eprintln!("[App] failed to open URL {}: {}", url, e);
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
                        eprintln!("[App] failed to open URL {}: {}", url, e);
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
                        eprintln!("[App] failed to open URL {}: {}", url, e);
                    }
                }
            }
        }

        // ── Drain updater command requests ───────────────────────────────
        {
            let cmd: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.pending_updater_cmd.take());
            if let Some(ref cmd_str) = cmd {
                // Mirror mode changes into profile so they persist on next save.
                if cmd_str == "set_connected" {
                    self.user_manager.profile.client_mode =
                        zengeld_chart::user_profile::profile::ClientMode::Connected;
                } else if cmd_str == "set_standalone" {
                    self.user_manager.profile.client_mode =
                        zengeld_chart::user_profile::profile::ClientMode::Standalone;
                }
                if let Some(ref handle) = self.updater_handle {
                    use zengeld_updater::UpdaterCommand;
                    let command = if cmd_str == "logout" {
                        Some(UpdaterCommand::Logout)
                    } else if let Some(provider) = cmd_str.strip_prefix("start_oauth:") {
                        Some(UpdaterCommand::StartOAuth(provider.to_string()))
                    } else if cmd_str == "set_connected" {
                        Some(UpdaterCommand::SetConnectedMode(true))
                    } else if cmd_str == "set_standalone" {
                        Some(UpdaterCommand::SetConnectedMode(false))
                    } else {
                        eprintln!("[App] unknown updater command: {}", cmd_str);
                        None
                    };
                    if let Some(command) = command {
                        if let Err(e) = handle.cmd_tx.send(command) {
                            eprintln!("[App] updater cmd_tx send failed: {}", e);
                        }
                    }
                }
            }
        }

        // ── Poll auth_rx → sync to all windows ───────────────────────────
        {
            if let Some(ref handle) = self.updater_handle {
                if handle.auth_rx.has_changed().unwrap_or(false) {
                    let status = handle.auth_rx.borrow().clone();
                    match &status {
                        zengeld_updater::AuthStatus::LoggedIn { display_name, provider, user_id } => {
                            let dn = display_name.clone();
                            let prov = provider.clone();
                            let uid = *user_id;
                            for pw in self.windows.values_mut() {
                                let s = &mut pw.chart.panel_app.user_settings_state;
                                s.is_logged_in = true;
                                s.auth_display_name = dn.clone();
                                s.auth_provider = prov.clone();
                                s.auth_user_id = uid;
                            }
                            // Mirror auth state into profile so it persists across restarts.
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            self.user_manager.profile.linked_account =
                                Some(zengeld_chart::user_profile::profile::LinkedAccount {
                                    provider: prov.clone(),
                                    provider_user_id: uid.to_string(),
                                    display_name: dn.clone(),
                                    linked_at: now,
                                });
                            // Auto-switch to Connected mode when user logs in.
                            self.user_manager.profile.client_mode =
                                zengeld_chart::user_profile::profile::ClientMode::Connected;
                            // Reflect the mode change in all open windows immediately.
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.client_mode_connected = true;
                            }
                            eprintln!("[App] auth: logged in as {} ({})", dn, prov);
                        }
                        zengeld_updater::AuthStatus::NotLoggedIn => {
                            for pw in self.windows.values_mut() {
                                let s = &mut pw.chart.panel_app.user_settings_state;
                                s.is_logged_in = false;
                                s.auth_display_name = String::new();
                                s.auth_provider = String::new();
                                s.auth_user_id = 0;
                            }
                            // Clear the profile mirror on logout / missing token.
                            self.user_manager.profile.linked_account = None;
                            eprintln!("[App] auth: not logged in");
                        }
                    }
                }
            }
        }

        // ── Poll synced_keys_rx → merge server keys into Agent API registry ─
        {
            let should_merge = self.updater_handle
                .as_ref()
                .map(|h| h.synced_keys_rx.has_changed().unwrap_or(false))
                .unwrap_or(false);

            if should_merge {
                if let (Some(ref handle), Some(ref agent_state)) =
                    (&self.updater_handle, &self.agent_state)
                {
                    let synced = handle.synced_keys_rx.borrow().clone();
                    if !synced.is_empty() {
                        // Merge strategy (source-aware):
                        //   - Keys with source=Local are NEVER removed by cloud sync.
                        //     They were generated locally via CreateKey or the UI.
                        //   - Keys with source=Cloud are fully managed by the server:
                        //     the existing cloud set is replaced with the new synced set.
                        //   - New keys from the server get source=Cloud.

                        let existing = agent_state.list_keys();

                        // Partition existing keys by source.
                        let local_keys: Vec<zengeld_server::state::ApiKeyEntry> = existing
                            .iter()
                            .filter(|k| k.source == zengeld_server::state::KeySource::Local)
                            .cloned()
                            .collect();

                        // Convert synced entries to ApiKeyEntry with source=Cloud.
                        let cloud_keys: Vec<zengeld_server::state::ApiKeyEntry> = synced
                            .iter()
                            .map(|s| {
                                // Preserve created_at from existing entry if we have it,
                                // otherwise use current time as a placeholder.
                                let created_at = existing
                                    .iter()
                                    .find(|k| k.key_hash == s.token_hash)
                                    .map(|k| k.created_at)
                                    .unwrap_or_else(|| {
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs()
                                    });
                                zengeld_server::state::ApiKeyEntry {
                                    key_hash: s.token_hash.clone(),
                                    label: s.label.clone(),
                                    tier: s.tier.clone(),
                                    permissions: zengeld_server::state::Permissions::from_tier(&s.tier),
                                    created_at,
                                    agent_id: None,
                                    source: zengeld_server::state::KeySource::Cloud,
                                }
                            })
                            .collect();

                        // Combine: local keys always kept, cloud keys replaced entirely.
                        let merged: Vec<zengeld_server::state::ApiKeyEntry> = local_keys
                            .into_iter()
                            .chain(cloud_keys)
                            .collect();

                        let merged_count = merged.len();

                        // Replace the registry atomically.
                        if let Ok(mut keys) = agent_state.keys.write() {
                            *keys = merged.clone();
                        }

                        // Mirror into profile so the merged set persists on save.
                        self.app_state.agent_api_keys = merged.iter().map(|k| {
                            zengeld_chart::StoredApiKey {
                                key_hash: k.key_hash.clone(),
                                label: k.label.clone(),
                                tier: k.tier.clone(),
                                created_at: k.created_at,
                                agent_id: k.agent_id.clone(),
                                source: match k.source {
                                    zengeld_server::state::KeySource::Cloud => "cloud".to_string(),
                                    zengeld_server::state::KeySource::Local => "local".to_string(),
                                },
                            }
                        }).collect();

                        eprintln!("[App] Key sync: merged {} key(s) into Agent API registry", merged_count);
                    }
                }
            }
        }

        // ── Telegram test / detect users (async via bridge runtime) ────
        {
            // Check any window for pending Telegram operations
            let mut test_req: Option<(String, Vec<String>)> = None;
            let mut detect_req: Option<String> = None;
            for pw in self.windows.values_mut() {
                let s = &mut pw.chart.panel_app.alert_settings_state;
                if s.tg_test_pending {
                    s.tg_test_pending = false;
                    let active_ids = s.notification_settings.telegram.active_chat_ids()
                        .into_iter()
                        .map(|id| id.to_owned())
                        .collect();
                    test_req = Some((
                        s.notification_settings.telegram.bot_token.clone(),
                        active_ids,
                    ));
                    break;
                }
                if s.tg_detect_pending {
                    s.tg_detect_pending = false;
                    detect_req = Some(s.notification_settings.telegram.bot_token.clone());
                    break;
                }
            }

            if let Some((token, chat_ids)) = test_req {
                let bridge = self.bridge.clone();
                // Fire-and-forget on the bridge runtime — send to all active subscribers.
                bridge.runtime().spawn({
                    let token = token.clone();
                    let chat_ids = chat_ids.clone();
                    async move {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(10))
                            .build()
                            .unwrap_or_default();
                        if chat_ids.is_empty() {
                            eprintln!("[Telegram] Test skipped: no active subscribers");
                            return;
                        }
                        for chat_id in &chat_ids {
                            match alert_delivery::telegram::send_test(&client, &token, chat_id).await {
                                Ok(()) => eprintln!("[Telegram] Test message sent to {}", chat_id),
                                Err(e) => eprintln!("[Telegram] Test failed for {}: {}", chat_id, e),
                            }
                        }
                    }
                });
                // Update status immediately — actual result logged to stderr
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.alert_settings_state.tg_status_message =
                        if chat_ids.is_empty() {
                            "No active subscribers to test.".to_string()
                        } else {
                            format!("Sent to {} subscriber(s)! Check Telegram.", chat_ids.len())
                        };
                }
            }

            if let Some(token) = detect_req {
                // Block on the detect call — it's a one-time user action, ~1s latency is fine.
                let result = self.bridge.runtime().block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .unwrap_or_default();
                    alert_delivery::telegram::get_updates(&client, &token).await
                });
                match result {
                    Ok(chats) if !chats.is_empty() => {
                        let count = chats.len();
                        let status = format!("Found {} user(s). Click Add to subscribe.", count);
                        for pw in self.windows.values_mut() {
                            let s = &mut pw.chart.panel_app.alert_settings_state;
                            // Store detected users for display; user picks which ones to add.
                            s.tg_detected_users = chats.clone();
                            s.tg_status_message = status.clone();
                        }
                    }
                    Ok(_) => {
                        for pw in self.windows.values_mut() {
                            let s = &mut pw.chart.panel_app.alert_settings_state;
                            s.tg_detected_users = Vec::new();
                            s.tg_status_message =
                                "No messages. Send /start to your bot first.".to_string();
                        }
                    }
                    Err(e) => {
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.alert_settings_state.tg_status_message =
                                format!("Error: {}", e);
                        }
                    }
                }
            }

            // When notification settings changed, push to delivery engine + persist to disk.
            let notif_dirty = self.windows.values()
                .any(|pw| pw.chart.panel_app.alert_settings_state.notification_settings_dirty);
            if notif_dirty {
                if let Some(pw) = self.windows.values().next() {
                    let ns = pw.chart.panel_app.alert_settings_state.notification_settings.clone();
                    // Push to live delivery engine
                    if let Some(ref delivery) = self.alert_delivery {
                        delivery.update_settings(ns.clone());
                    }
                    // Persist to profile on disk
                    self.profile.notification_settings = ns;
                    if let Err(e) = zengeld_chart::save_profile(&self.profile) {
                        eprintln!("[App] Failed to persist notification settings: {}", e);
                    }
                }
                // Clear dirty flag on all windows
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.alert_settings_state.notification_settings_dirty = false;
                }
            }
        }

        // ── Drain agent commands every frame ─────────────────────────────
        self.drain_agent_commands();

        // Per-window: tick ALL windows (not just focused) so live data,
        // watchlist updates, and state changes are processed even for
        // background windows that Windows won't deliver WM_PAINT to.
        let _t4 = std::time::Instant::now();
        {
            // ── Conditional sync: only clone when data actually changed ────────
            // Each dirty flag is set when the corresponding AppState field is
            // mutated (drain loops above), and reset here after syncing to all
            // windows.  This avoids O(windows * data_size) allocations per frame
            // when nothing has changed.
            if self.app_state.watchlists_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.sidebar_state.watchlist_manager =
                        self.app_state.watchlist_manager.clone();
                }
                self.app_state.watchlists_dirty = false;
            }
            if self.app_state.connectors_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.sidebar_state.connector_enabled =
                        self.app_state.connector_enabled.clone();
                }
                self.app_state.connectors_dirty = false;
            }
            if self.app_state.presets_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.presets = self.app_state.presets.clone();
                }
                self.app_state.presets_dirty = false;
            }
            if self.app_state.snapshots_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_manager.snapshots =
                        self.app_state.snapshots.clone();
                }
                self.app_state.snapshots_dirty = false;
            }
            if self.app_state.templates_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.template_manager =
                        self.app_state.template_manager.clone();
                }
                self.app_state.templates_dirty = false;
            }

            // Sync recalc_mode to every window's indicator manager.
            // This is a cheap copy (Copy enum) so no dirty flag is needed.
            let recalc_mode = self.app_state.recalc_mode;
            for pw in self.windows.values_mut() {
                pw.chart.indicator_manager.recalc_mode = recalc_mode;
            }

            let frame_time = now_ms();
            for pw in self.windows.values_mut() {
                pw.chart.tick(frame_time);
            }
        }
        let _t5 = std::time::Instant::now();

        // ── Populate performance panel data ─────────────────────────────────
        {
            let ws_connections = self.bridge.ws_task_count_total();
            let window_count = self.windows.len();
            // Use the cached connector count — refreshed once per second below.
            let active_connectors = self.cached_connector_count;
            let fps_limit = self.fps_limit;
            let fps = self.fps_ema;
            let frame_time_ms = self.last_frame_time_ms;
            let recalc_label = format!("{:?}", self.app_state.recalc_mode);
            let msaa_samples = self.msaa_samples;
            let max_bars = self.max_bars;
            let perf_log_enabled = self.perf_log_enabled;
            let render_backend = self.render_backend;
            let vsync_enabled = self.vsync_enabled;

            // System metrics (refreshed 1x/sec in the indicator snapshot timer)
            // global_cpu_usage() is unreliable on Windows — compute the average of per-core values instead.
            let cpus = self.sys.cpus();
            let cpu_usage = if cpus.is_empty() {
                0.0_f32
            } else {
                cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
            };
            let process_cpu = self.sys.process(self.self_pid)
                .map(|p| p.cpu_usage())
                .unwrap_or(0.0);
            // Normalize process CPU: sysinfo reports sum-of-threads (can exceed 100%).
            // Divide by core count to get "fraction of total machine capacity".
            let num_cores = self.sys.cpus().len().max(1) as f32;
            let process_cpu_normalized = process_cpu / num_cores;
            let ram_mb = self.sys.process(self.self_pid)
                .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
                .unwrap_or(0.0);
            let ram_total_mb = self.sys.total_memory() as f64 / (1024.0 * 1024.0);
            let gpu_name = self.gpu_name.clone();
            let gpu_driver = self.gpu_driver.clone();
            let per_core_cpu: Vec<f32> = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
            let scene_build_us = self.cached_scene_us;
            let gpu_render_us = self.cached_gpu_us;

            let mut total_bars_all: u64 = 0;
            for pw in self.windows.values_mut() {
                // Trim bars if max_bars is set
                if max_bars > 0 {
                    for w in pw.chart.panel_app.panel_grid.windows_mut().values_mut() {
                        if w.bars.len() > max_bars {
                            let excess = w.bars.len() - max_bars;
                            w.bars.drain(..excess);
                        }
                    }
                }

                let total_bars: usize = pw.chart.panel_app.panel_grid.windows()
                    .values()
                    .map(|w| w.bars.len())
                    .sum();
                total_bars_all += total_bars as u64;
                let lag_events = pw.chart.lag_event_count;

                let perf = &mut pw.chart.sidebar_state.performance_data;
                perf.fps = fps;
                perf.frame_time_ms = frame_time_ms;
                perf.total_bars = total_bars;
                perf.ws_connections = ws_connections;
                perf.lag_events = lag_events;
                perf.fps_limit = fps_limit;
                perf.msaa_samples = msaa_samples;
                perf.max_bars = max_bars;
                perf.recalc_mode = recalc_label.clone();
                perf.window_count = window_count;
                perf.active_connectors = active_connectors;
                perf.cpu_usage = cpu_usage;
                perf.process_cpu = process_cpu;
                perf.process_cpu_normalized = process_cpu_normalized;
                perf.ram_mb = ram_mb;
                perf.ram_total_mb = ram_total_mb;
                perf.gpu_name = gpu_name.clone();
                perf.gpu_driver = gpu_driver.clone();
                perf.perf_log_enabled = perf_log_enabled;
                perf.render_backend = render_backend;
                perf.vsync_enabled = vsync_enabled;
                perf.per_core_cpu = per_core_cpu.clone();
                perf.scene_build_us = scene_build_us;
                perf.gpu_render_us = gpu_render_us;
            }

            // ── Write live values to telemetry shared atomics ──────────────────
            {
                use std::sync::atomic::Ordering::Relaxed;
                self.telemetry_shared.window_count.store(window_count as u32, Relaxed);
                self.telemetry_shared.avg_fps_bits.store(f32::to_bits(fps as f32), Relaxed);
                self.telemetry_shared.total_bars.store(total_bars_all, Relaxed);
                // Screen size from first available window's monitor.
                if let Some(pw) = self.windows.values().next() {
                    let (sw, sh) = pw.window.current_monitor()
                        .map(|m| { let s = m.size(); (s.width, s.height) })
                        .unwrap_or_else(|| {
                            let s = pw.window.inner_size();
                            (s.width, s.height)
                        });
                    self.telemetry_shared.screen_width.store(sw, Relaxed);
                    self.telemetry_shared.screen_height.store(sh, Relaxed);
                }
            }
        }
        let _t6 = std::time::Instant::now();

        // NOTE: alert delivery events are drained AFTER render_window() below,
        // so that alert screenshots can be captured and attached first.

        // ── Drain toast notifications for UI overlay ──────────────────────────
        if let Some(ref mut rx) = self.toast_rx {
            while let Ok(toast) = rx.try_recv() {
                self.active_toasts.push(toast);
            }
        }
        // Expire old toasts.
        let now_ms_val = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.active_toasts.retain(|t| !t.is_expired(now_ms_val));

        // Per-window: poll cursor, drain screenshots, request redraw
        for pw in self.windows.values_mut() {
            // Poll cursor position when drawing outside window boundary
            #[cfg(target_os = "windows")]
            if pw.drawing_capture {
                if let Some((x, y)) = win32_capture::get_cursor_pos(&pw.window) {
                    if (x, y) != pw.last_mouse_pos {
                        pw.last_mouse_pos = (x, y);
                        let chart_y = (y - chrome::CHROME_HEIGHT).max(0.0);
                        pw.chart.on_mouse_move(x, chart_y);
                    }
                }
            }

            if pw.chart.drain_pending_screenshot() {
                pw.screenshot_pending = true;
            }
        }

        // ── Agent API: update snapshots at most once per second ──────────────
        if self.last_indicator_snapshot.elapsed() >= std::time::Duration::from_secs(1) {
            // Refresh connector metrics once per second — collect_metrics() locks
            // and allocates; calling it every frame at 60 fps is wasteful.
            {
                use std::sync::atomic::Ordering::Relaxed;
                let metrics = self.bridge.collect_metrics();
                self.cached_connector_count = metrics.len(); // streams, used by perf panel
                let ws_total: u32 = metrics.iter().map(|(_, _, ws)| *ws as u32).sum();
                // Count unique exchanges that have active data streams.
                // connector_enabled map is empty by default (all enabled), so we
                // count unique ExchangeIds from active metrics instead.
                let enabled_connectors = {
                    let mut seen = std::collections::HashSet::new();
                    for (eid, _, _) in &metrics {
                        seen.insert(eid.clone());
                    }
                    seen.len() as u32
                };
                self.telemetry_shared.connector_count.store(enabled_connectors, Relaxed);
                self.telemetry_shared.ws_connections.store(ws_total, Relaxed);
            }

            // Refresh CPU/RAM metrics (expensive — once per second).
            self.sys.refresh_cpu_usage();
            self.sys.refresh_memory();
            self.sys.refresh_processes(
                ProcessesToUpdate::Some(&[self.self_pid]),
                false,
            );

            if let Some(ref agent_state) = self.agent_state {
                self.update_indicator_snapshot(agent_state);
                self.update_terminal_snapshot(agent_state);
                self.update_watchlist_snapshot(agent_state);
                self.update_connector_snapshot(agent_state);
            }
            // Sync managed_keys list to all windows for display in the Server tab.
            if let Some(ref agent_state) = self.agent_state {
                let server_keys = agent_state.list_keys();
                let managed: Vec<zengeld_chart::ManagedKeyInfo> = server_keys.iter().map(|k| {
                    zengeld_chart::ManagedKeyInfo {
                        label: k.label.clone(),
                        tier: k.tier.clone(),
                        agent_id: k.agent_id.clone(),
                    }
                }).collect();
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_settings_state.managed_keys = managed.clone();
                }
            }
            // Clock time in the toolbar updates every second — mark all toolbars dirty
            // so the clock string is refreshed on the next frame.
            // Also mark sidebar dirty: performance panel data (fps, frame_time, etc.)
            // updates every second, so the performance panel needs a redraw.
            for pw in self.windows.values_mut() {
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
            }

            self.last_indicator_snapshot = std::time::Instant::now();
        }

        // Propagate chart.sidebar_data_dirty → pw.sidebar_dirty_scene.
        // chart.sidebar_data_dirty is set by tick() after any LiveUpdate and
        // by sidebar panel toggle handlers.  When it fires, the sidebar scene
        // must be rebuilt so the new data is reflected in the vector graphics.
        for pw in self.windows.values_mut() {
            if pw.chart.sidebar_data_dirty {
                pw.sidebar_dirty_scene = true;
            }
        }

        let _t7 = std::time::Instant::now();

        // ── Pipelined rendering ─────────────────────────────────────────────
        //
        // The GPU render thread runs `submit_window_gpu_from_gpu_scene` for
        // all windows while the main thread builds the *next* frame's scenes.
        // This hides GPU latency (~7-13ms) behind the CPU scene-build phase.
        //
        // Protocol:
        //   1. Wait (blocking) for the previous frame's GpuDone (first frame: no wait).
        //   2. Drain alert delivery events — the GPU thread may have attached
        //      screenshots to pending_delivery_events; drain now that it's safe.
        //   3. Swap pw.scene ↔ pw.gpu_scene for each active window.
        //      After the swap: gpu_scene = just-built scene (ready to render),
        //      scene = stale buffer (will be cleared and rebuilt below).
        //   4. Signal the GPU thread to start rendering gpu_scene.
        //   5. Build the next frame's scenes via thread::scope (CPU parallel).
        //      This runs concurrently with step 4 — the GPU thread touches only
        //      gpu_scene/renderer/surface; the main thread touches only scene.
        //
        // Safety invariants (same as the previous implementation plus):
        //   • After the swap and before GpuDone: main thread MUST NOT access
        //     pw.gpu_scene, pw.renderer, or pw.surface.  It only writes to
        //     pw.scene and reads/writes app-level fields (chart, etc.).
        //   • The render_cx pointer passed to the GPU thread remains valid for
        //     the lifetime of App (it lives on the stack of run_app, never
        //     moves, and we never drop it while the GPU thread is running).

        let mut total_scene_us = 0u64;
        let mut total_gpu_us = 0u64;

        // Step 1: wait for the GPU thread to finish the previous frame.
        if self.gpu_frame_pending {
            if let Some(ref done_rx) = self.gpu_done_rx {
                match done_rx.recv() {
                    Ok(done) => {
                        total_gpu_us = done.total_gpu_us;
                        if done.close_all {
                            self.close_all_requested = true;
                        }
                    }
                    Err(_) => {
                        eprintln!("[App] GPU render thread channel closed unexpectedly");
                        self.close_all_requested = true;
                    }
                }
            }
            self.gpu_frame_pending = false;
        }

        // Step 2: drain alert delivery events (screenshots attached by GPU thread).
        if let Some(ref delivery) = self.alert_delivery {
            for pw in self.windows.values_mut() {
                for event in pw.chart.pending_delivery_events.drain(..) {
                    delivery.deliver(event);
                }
            }
        }

        // Collect mutable references to non-minimized windows.
        let mut window_refs: Vec<&mut PerWindowState> = self
            .windows
            .values_mut()
            .filter(|pw| !pw.window.is_minimized().unwrap_or(false))
            .collect();

        let active_toasts = self.active_toasts.clone();
        let frame_time = now_ms();
        let msaa_samples = self.msaa_samples;

        // Sync render_backend from App-level setting into each per-window state so
        // build_window_scene (which only receives &mut PerWindowState) can branch on it.
        let current_backend = self.render_backend;
        for pw in window_refs.iter_mut() {
            pw.render_backend = current_backend;
        }

        // Step 3: swap scene ↔ gpu_scene for each active window so the GPU
        // thread gets the freshly-built scene while the main thread gets a
        // clean buffer for the next build pass.
        for pw in window_refs.iter_mut() {
            std::mem::swap(&mut pw.scene, &mut pw.gpu_scene);
            // pw.scene (formerly gpu_scene) will be reset at the start of
            // build_window_scene, so we don't need to reset it here.
            // Also swap instanced buffers so the GPU thread renders the
            // instances built this frame while the main thread fills new ones.
            std::mem::swap(&mut pw.instanced_commands, &mut pw.gpu_instanced_commands);
            std::mem::swap(&mut pw.cpu_chart_pixels, &mut pw.gpu_cpu_chart_pixels);
            std::mem::swap(&mut pw.cpu_chart_dims, &mut pw.gpu_cpu_chart_dims);
            std::mem::swap(&mut pw.hybrid_ctx, &mut pw.gpu_hybrid_ctx);
        }

        // Step 4: signal the GPU render thread to start rendering gpu_scene.
        let render_cx_addr = &self.render_cx as *const RenderContext as usize;
        let window_addrs: Vec<usize> = window_refs
            .iter()
            .map(|pw| (*pw) as *const PerWindowState as usize)
            .collect();

        if let Some(ref cmd_tx) = self.gpu_cmd_tx {
            if !window_addrs.is_empty() {
                let _ = cmd_tx.send(GpuCommand::Submit {
                    window_addrs,
                    msaa_samples,
                    render_cx_addr,
                });
                self.gpu_frame_pending = true;
            }
        }

        // Step 5: build the next frame's scenes (CPU parallel via thread::scope).
        // Runs concurrently with the GPU thread rendering the previous frame.
        //
        // Safety: each pw_addr points to a distinct live PerWindowState.
        // The GPU thread only touches pw.gpu_scene/renderer/surface;
        // build_window_scene only touches pw.scene (and non-GPU fields).
        let active_toasts_ref: &[alert_delivery::ToastNotification] = &active_toasts;
        let parallel_t0 = std::time::Instant::now();
        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(window_refs.len());
            for pw in window_refs.iter_mut() {
                let pw_addr: usize = (*pw) as *mut PerWindowState as usize;
                let h = s.spawn(move || {
                    // SAFETY: pw_addr is the unique address of a live
                    // PerWindowState; no other thread holds a reference to it.
                    // std::thread::scope guarantees join before scope exits.
                    let pw_ref: &mut PerWindowState =
                        unsafe { &mut *(pw_addr as *mut PerWindowState) };
                    build_window_scene(pw_ref, active_toasts_ref, frame_time)
                });
                handles.push(h);
            }
            for h in handles {
                if let Ok(s_us) = h.join() {
                    if s_us > total_scene_us { total_scene_us = s_us; }
                }
            }
        });
        // Overwrite scene timing with wall-clock of the whole parallel phase.
        total_scene_us = parallel_t0.elapsed().as_micros() as u64;

        // Cache timing values so the next frame's populate block can display them.
        self.cached_scene_us = total_scene_us;
        self.cached_gpu_us = total_gpu_us;

        let _t8 = std::time::Instant::now();

        // ── Timing report every 5 seconds ───────────────────────────────────
        self.frame_count += 1;
        if self.last_timing_report.elapsed() >= std::time::Duration::from_secs(5) {
            if self.perf_log_enabled {
                let total = _t8.duration_since(_t0).as_micros();
                // Collect render sub-timing from the first window (representative sample).
                let (chart_us, toolbar_us, sidebar_us, setup_us) = self.windows.values()
                    .next()
                    .map(|pw| pw.chart.render_timing_us)
                    .unwrap_or((0, 0, 0, 0));
                eprintln!(
                    "[PERF] Frame {} total={:.1}ms | tick_app={:.1}ms drains={:.1}ms persist={:.1}ms sync={:.1}ms tick={:.1}ms perf_pop={:.1}ms agent={:.1}ms render={:.1}ms (scene={:.1}ms gpu={:.1}ms) | breakdown: chart={:.1}ms tb={:.1}ms side={:.1}ms setup={:.1}ms",
                    self.frame_count,
                    total as f64 / 1000.0,
                    _t1.duration_since(_t0).as_micros() as f64 / 1000.0,
                    _t2.duration_since(_t1).as_micros() as f64 / 1000.0,
                    _t3.duration_since(_t2).as_micros() as f64 / 1000.0,
                    _t4.duration_since(_t3).as_micros() as f64 / 1000.0,
                    _t5.duration_since(_t4).as_micros() as f64 / 1000.0,
                    _t6.duration_since(_t5).as_micros() as f64 / 1000.0,
                    _t7.duration_since(_t6).as_micros() as f64 / 1000.0,
                    _t8.duration_since(_t7).as_micros() as f64 / 1000.0,
                    total_scene_us as f64 / 1000.0,
                    total_gpu_us as f64 / 1000.0,
                    chart_us as f64 / 1000.0,
                    toolbar_us as f64 / 1000.0,
                    sidebar_us as f64 / 1000.0,
                    setup_us as f64 / 1000.0,
                );
            }
            self.last_timing_report = std::time::Instant::now();
        }

        // ── Set event loop control flow based on FPS limit ──────────────────
        if self.fps_limit > 0 {
            let target_dt = std::time::Duration::from_secs_f64(1.0 / self.fps_limit as f64);
            let elapsed = self.last_frame_instant.elapsed();
            if elapsed < target_dt {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    std::time::Instant::now() + (target_dt - elapsed),
                ));
            } else {
                // Frame already over budget — don't spin, give the OS a tiny breather.
                // This prevents 100% CPU when frames consistently exceed target_dt.
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(1),
                ));
            }
        } else {
            event_loop.set_control_flow(ControlFlow::Poll);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        // OS-level close (Alt+F4, taskbar close) → shutdown entire app.
        if let WindowEvent::CloseRequested = event {
            if let Some(pw) = self.windows.get_mut(&id) {
                pw.close_requested = true; // triggers app shutdown in about_to_wait
            }
            return;
        }

        // Track focus before borrowing per-window state — updating self.last_focused
        // while holding a &mut to self.windows would be a borrow conflict.
        if let WindowEvent::Focused(true) = event {
            self.last_focused = Some(id);
        }

        // Resize touches pw.surface which the GPU render thread may be using.
        // Wait for the current GPU frame to finish before proceeding so we
        // don't race with submit_window_gpu_from_gpu_scene.
        if let WindowEvent::Resized(_) = event {
            self.wait_for_gpu_frame();
        }

        // All other events need the per-window state
        let Some(pw) = self.windows.get_mut(&id) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => unreachable!(), // handled above

            // ─── Resize ───────────────────────────────────────────────────
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.render_cx
                        .resize_surface(&mut pw.surface, size.width, size.height);
                    // resize_surface recreates target_texture without COPY_SRC;
                    // patch it again so screenshots continue to work after resize.
                    let device = &self.render_cx.devices[pw.surface.dev_id].device;
                    add_copy_src_to_target_texture(&mut pw.surface, device);
                    let chrome_px = (chrome::CHROME_HEIGHT * pw.window.scale_factor()) as u32;
                    pw.chart
                        .resize(size.width, size.height.saturating_sub(chrome_px));

                    // Sync the maximize icon when the window is snapped or
                    // maximized by the OS (e.g. via Win+Arrow keys).
                    pw.chrome_state.is_maximized = pw.window.is_maximized();

                    // Mark dirty so position/size is persisted on next save.
                    pw.chart.profile_dirty = true;
                    // Toolbar and sidebar layout changes on resize — must rebuild both.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;
                }
            }

            // ─── Window moved ─────────────────────────────────────────────
            WindowEvent::Moved(_) => {
                pw.chart.profile_dirty = true;
            }

            // ─── Mouse move ───────────────────────────────────────────────
            WindowEvent::CursorMoved { position, .. } => {
                let x = position.x;
                let y = position.y;
                pw.last_mouse_pos = (x, y);

                // Update context menu hover
                if pw.chrome_state.context_menu.open {
                    chrome::context_menu_hover(&mut pw.chrome_state.context_menu, x, y);
                    // Don't return — let other hover logic run too, it's harmless
                }

                // Update chrome hover state and handle chrome-area cursor/redraw.
                let size = pw.window.inner_size();
                let hit =
                    chrome::hit_test(x, y, size.width as f64, size.height as f64, &pw.chrome_state);
                pw.chrome_state.hovered = hit;

                match hit {
                    chrome::ChromeHit::ResizeTop | chrome::ChromeHit::ResizeBottom => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NsResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeLeft | chrome::ChromeHit::ResizeRight => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::EwResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeTopLeft | chrome::ChromeHit::ResizeBottomRight => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NwseResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeTopRight | chrome::ChromeHit::ResizeBottomLeft => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NeswResize);
                        return;
                    }
                    chrome::ChromeHit::Caption
                    | chrome::ChromeHit::MinimizeButton
                    | chrome::ChromeHit::MaximizeButton
                    | chrome::ChromeHit::CloseButton
                    | chrome::ChromeHit::Tab(_)
                    | chrome::ChromeHit::TabClose(_)
                    | chrome::ChromeHit::NewTabButton
                    | chrome::ChromeHit::SettingsButton
                    | chrome::ChromeHit::NewWindowButton => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::Default);
                        // Do not forward to chart
                        return;
                    }
                    chrome::ChromeHit::None => {}
                }

                // Only forward events in the chart area (below chrome strip).
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y < 0.0 {
                    return;
                }

                // Mark toolbar dirty when the cursor enters or moves within a
                // toolbar band (top/bottom/left/right).  This covers hover-state
                // changes on toolbar buttons without rebuilding on every chart pan.
                {
                    use zengeld_chart::{TOP_TOOLBAR_HEIGHT, BOTTOM_TOOLBAR_HEIGHT,
                                       LEFT_TOOLBAR_WIDTH, RIGHT_TOOLBAR_WIDTH};
                    let chart_w = pw.chart.width as f64;
                    let chart_h = pw.chart.height as f64;
                    let in_toolbar_zone =
                        chart_y < TOP_TOOLBAR_HEIGHT
                        || chart_y > chart_h - BOTTOM_TOOLBAR_HEIGHT
                        || x < LEFT_TOOLBAR_WIDTH
                        || x > chart_w - RIGHT_TOOLBAR_WIDTH;
                    if in_toolbar_zone {
                        pw.toolbar_dirty = true;
                    }

                    // Mark sidebar dirty only when the hovered row changes, not on
                    // every sub-pixel cursor movement within the same row.
                    // Row height is 36 px; the scroll offset shifts which row is
                    // visible so we incorporate it into the calculation.
                    if pw.chart.sidebar_state.is_right_open() {
                        let sidebar_w = pw.chart.sidebar_state.right_sidebar_width;
                        let sidebar_left = chart_w - RIGHT_TOOLBAR_WIDTH - sidebar_w;
                        let sidebar_right = chart_w - RIGHT_TOOLBAR_WIDTH;
                        if x >= sidebar_left && x < sidebar_right {
                            const ROW_HEIGHT: f64 = 8.0;
                            // Content area starts after sidebar header (40 px). The watchlist
                            // panel adds an extra 23 px column header; other panels do not.
                            let extra_header = match pw.chart.sidebar_state.right_panel {
                                sidebar_content::state::RightSidebarPanel::Watchlist => 23.0,
                                _ => 0.0,
                            };
                            let sidebar_top = chrome::CHROME_HEIGHT + 40.0 + extra_header;
                            let scroll_offset = pw.chart.sidebar_state
                                .current_right_scroll()
                                .offset;
                            let row_index = (((y - sidebar_top) + scroll_offset) / ROW_HEIGHT)
                                .max(0.0) as usize;
                            if pw.last_sidebar_hover_row != Some(row_index) {
                                pw.last_sidebar_hover_row = Some(row_index);
                                pw.sidebar_dirty_scene = true;
                            }
                        } else if pw.last_sidebar_hover_row.is_some() {
                            // Cursor left sidebar bounds — clear hover and redraw once.
                            pw.last_sidebar_hover_row = None;
                            pw.sidebar_dirty_scene = true;
                        }
                    } else if pw.last_sidebar_hover_row.is_some() {
                        pw.last_sidebar_hover_row = None;
                    }
                }

                if pw.mouse_pressed {
                    if let Some((last_x, last_y)) = pw.last_drag_pos {
                        let dx = x - last_x;
                        let dy = y - last_y;
                        let last_chart_y = last_y - chrome::CHROME_HEIGHT;
                        pw.chart.on_drag_move(x, chart_y, dx, dy);
                        let _ = last_chart_y; // suppress unused warning
                    }
                    pw.last_drag_pos = Some((x, y));
                } else {
                    pw.chart.on_mouse_move(x, chart_y);
                }

                if pw.chart.is_magnet_snapped() {
                    // Hide system cursor when magnet-locked (crosshair drawn at snapped pos)
                    pw.window.set_cursor_visible(false);
                } else {
                    pw.window.set_cursor_visible(true);
                    pw.window
                        .set_cursor(cursor_style_to_winit(pw.chart.get_cursor(x, chart_y)));
                }
            }

            // ─── Cursor left window ───────────────────────────────────────
            WindowEvent::CursorLeft { .. } => {
                if pw.drawing_capture {
                    return;
                }
                // Hover state clears when cursor leaves — toolbar and sidebar must redraw.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart.on_mouse_leave();
            }

            // ─── Mouse buttons ────────────────────────────────────────────
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = pw.last_mouse_pos;

                // Any click may change toolbar button state (active drawing mode,
                // open dropdown, etc.) so mark the toolbar as dirty.
                // A click in the sidebar area also changes sidebar state (item
                // selection, delete, settings open, scroll) — always mark it dirty.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;

                // Check chrome hit first for left-button press events.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    // If context menu is open, handle click on it first
                    if pw.chrome_state.context_menu.open {
                        if let Some(action) = chrome::context_menu_hit_test(&pw.chrome_state.context_menu, x, y) {
                            pw.chrome_state.context_menu.close();
                            match action {
                                chrome::ChromeMenuAction::CloseWindow => {
                                    pw.close_window_requested = true;
                                }
                                chrome::ChromeMenuAction::DeleteWindow => {
                                    pw.delete_window_requested = true;
                                }
                            }
                            return;
                        } else {
                            // Clicked outside menu → close it
                            pw.chrome_state.context_menu.close();
                            return;
                        }
                    }

                    let size = pw.window.inner_size();
                    let hit = chrome::hit_test(
                        x,
                        y,
                        size.width as f64,
                        size.height as f64,
                        &pw.chrome_state,
                    );
                    match hit {
                        chrome::ChromeHit::Caption => {
                            let _ = pw.window.drag_window();
                            return;
                        }
                        chrome::ChromeHit::MinimizeButton => {
                            pw.window.set_minimized(true);
                            return;
                        }
                        chrome::ChromeHit::MaximizeButton => {
                            let maximized = pw.window.is_maximized();
                            pw.window.set_maximized(!maximized);
                            pw.chrome_state.is_maximized = !maximized;
                            return;
                        }
                        chrome::ChromeHit::CloseButton => {
                            // Chrome X = shutdown entire app (save all + exit in about_to_wait)
                            pw.close_requested = true;
                            return;
                        }
                        chrome::ChromeHit::Tab(idx) => {
                            if let Some(tab) = pw.chrome_state.tabs.get(idx) {
                                let tab_id = tab.id.clone();
                                pw.chart.load_preset(&tab_id);
                            }
                            return;
                        }
                        chrome::ChromeHit::TabClose(idx) => {
                            // Close the tab without deleting the preset.
                            // CloseTab handler in input.rs will switch to an adjacent tab automatically.
                            if let Some(tab) = pw.chrome_state.tabs.get(idx).cloned() {
                                pw.chart.close_tab(&tab.id);
                            }
                            return;
                        }
                        chrome::ChromeHit::NewTabButton => {
                            // Toggle the new_tab_menu dropdown (uses the chart toolbar dropdown system).
                            let ts = &mut pw.chart.panel_app.toolbar_state;
                            if ts.open_dropdown_id.as_deref() == Some("new_tab_menu") {
                                ts.open_dropdown_id = None;
                                ts.open_dropdown_position = None;
                            } else {
                                let btn_x = chrome::new_tab_button_x(&pw.chrome_state);
                                // y=0 in chart-space (chart renders offset by CHROME_HEIGHT,
                                // so 0 here = right below the chrome strip on screen).
                                ts.open_dropdown_id = Some("new_tab_menu".to_string());
                                ts.open_dropdown_position = Some((btn_x, 0.0));
                            }
                            eprintln!(
                                "[Chrome] + clicked, new_tab_menu open={}",
                                pw.chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                            );
                            return;
                        }
                        chrome::ChromeHit::SettingsButton => {
                            pw.chart.open_user_settings();
                            return;
                        }
                        chrome::ChromeHit::NewWindowButton => {
                            // Queue a new window spawn; it will be created in about_to_wait
                            // once pw is no longer borrowed.
                            pw.spawn_new_window = true;
                            return;
                        }
                        chrome::ChromeHit::ResizeTop => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::North,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottom => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::South,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::West,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::East,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeTopLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::NorthWest,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeTopRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::NorthEast,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottomLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::SouthWest,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottomRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::SouthEast,
                            );
                            return;
                        }
                        chrome::ChromeHit::None => {}
                    }
                }

                // Right-click on chrome → open context menu
                if button == MouseButton::Right && state == ElementState::Pressed {
                    if y < chrome::CHROME_HEIGHT {
                        pw.chrome_state.context_menu.open_at(x, y);
                        return;
                    }
                }

                // Only forward to chart when in the chart area (below chrome).
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y < 0.0 {
                    return;
                }

                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        pw.mouse_pressed = true;
                        pw.last_drag_pos = Some((x, y));
                        pw.drag_start_pos = Some((x, y));
                        pw.chart.on_drag_start(x, chart_y);
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        if pw.mouse_pressed {
                            // Detect click vs drag (threshold: 5 pixels)
                            if let Some((sx, sy)) = pw.drag_start_pos {
                                let dist = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
                                if dist < 5.0 {
                                    // Double-click detection (400 ms, 5 px)
                                    let now = std::time::Instant::now();
                                    let is_double_click = if let Some((last_time, last_x, last_y)) =
                                        pw.last_click
                                    {
                                        let elapsed =
                                            now.duration_since(last_time).as_millis();
                                        let dist2 = ((x - last_x).powi(2)
                                            + (y - last_y).powi(2))
                                        .sqrt();
                                        elapsed < 400 && dist2 < 5.0
                                    } else {
                                        false
                                    };

                                    if is_double_click {
                                        pw.last_click = None;
                                        // Check if double-click is on caption — toggle maximize
                                        let size = pw.window.inner_size();
                                        let hit = chrome::hit_test(
                                            x,
                                            y,
                                            size.width as f64,
                                            size.height as f64,
                                            &pw.chrome_state,
                                        );
                                        if hit == chrome::ChromeHit::Caption {
                                            let maximized = pw.window.is_maximized();
                                            pw.window.set_maximized(!maximized);
                                            pw.chrome_state.is_maximized = !maximized;
                                        } else {
                                            pw.chart.on_double_click(x, chart_y);
                                        }
                                    } else {
                                        pw.last_click = Some((now, x, y));
                                        pw.chart.on_click(x, chart_y);
                                    }
                                }
                            }
                            let drag_chart_y = y - chrome::CHROME_HEIGHT;
                            pw.chart.on_drag_end(x, drag_chart_y.max(0.0));
                        }
                        pw.mouse_pressed = false;
                        pw.last_drag_pos = None;
                        pw.drag_start_pos = None;

                        // Track whether the chart is mid-drawing so about_to_wait
                        // can poll GetCursorPos outside the window boundary.
                        #[cfg(target_os = "windows")]
                        {
                            pw.drawing_capture = pw.chart.is_drawing();
                        }
                    }
                    (MouseButton::Right, ElementState::Pressed) => {
                        pw.chart.on_right_click(x, chart_y);
                    }
                    _ => {}
                }
            }

            // ─── Scroll ───────────────────────────────────────────────────
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x as f64 * 20.0, y as f64 * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
                };
                let (x, y) = pw.last_mouse_pos;
                // Only scroll in the chart area; apply y-offset.
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y >= 0.0 {
                    pw.chart.on_scroll(x, chart_y, dx, dy);
                    // Scrolling inside the sidebar changes the visible content —
                    // always mark it dirty so the new scroll offset is rendered.
                    pw.sidebar_dirty_scene = true;
                }
            }

            // ─── Modifier keys ────────────────────────────────────────────
            WindowEvent::ModifiersChanged(new_modifiers) => {
                pw.modifiers = new_modifiers.state();
            }

            // ─── Keyboard ─────────────────────────────────────────────────
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

                if event.state == ElementState::Pressed {
                    // Keyboard actions can change drawing mode (Escape, Delete, etc.)
                    // which is reflected in the left toolbar — mark it dirty.
                    // Delete can remove objects from the object tree — mark sidebar dirty.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;

                    // ── Ctrl shortcuts — use physical_key so layout doesn't matter ──
                    // On a Russian keyboard, Ctrl+С (Cyrillic) still maps to
                    // PhysicalKey::Code(KeyCode::KeyC), matching the physical position.
                    if pw.modifiers.control_key() {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyS) => {
                                pw.screenshot_pending = true;
                                eprintln!("[Screenshot] Capture requested via Ctrl+S");
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyA) => {
                                pw.chart.on_key_press(chart_app::KeyPress::SelectAll);
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyC) => {
                                if let Some(text) = pw.chart.on_copy_selection() {
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        let _ = cb.set_text(text);
                                    }
                                }
                                // No redraw needed — state unchanged.
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyV) => {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    if let Ok(text) = cb.get_text() {
                                        pw.chart.on_key_press(chart_app::KeyPress::Paste(text));
                                    }
                                }
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyZ) => {
                                if pw.modifiers.shift_key() {
                                    pw.chart.on_key_press(chart_app::KeyPress::Redo);
                                } else {
                                    pw.chart.on_key_press(chart_app::KeyPress::Undo);
                                }
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyY) => {
                                pw.chart.on_key_press(chart_app::KeyPress::Redo);
                                return;
                            }
                            // ── Ctrl+B — test: switch to Binance live data ──────
                            PhysicalKey::Code(KeyCode::KeyB) => {
                                eprintln!("[ChartApp] Ctrl+B: switching to Binance");
                                pw.chart.switch_to_exchange(chart_app::ExchangeId::Binance);
                                return;
                            }
                            _ => {}
                        }
                    }

                    let mut handled = true;
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            pw.chart.on_escape();
                            // Escape cancels drawing — clear the polling flag.
                            #[cfg(target_os = "windows")]
                            {
                                pw.drawing_capture = false;
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            pw.chart.on_char_input('\x08');
                        }
                        Key::Named(NamedKey::Enter) => {
                            pw.chart.on_char_input('\n');
                        }
                        Key::Named(NamedKey::Space) => {
                            pw.chart.on_char_input(' ');
                        }
                        Key::Named(NamedKey::Delete) => {
                            pw.chart.on_key_press(chart_app::KeyPress::Delete);
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftLeft);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::ArrowLeft);
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftRight);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::ArrowRight);
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftHome);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::Home);
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftEnd);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::End);
                            }
                        }
                        Key::Character(text) => {
                            // Do NOT forward characters when Ctrl or Alt is held —
                            // any Ctrl+key shortcut that reaches here was not matched
                            // above (e.g. an unhandled Ctrl combo) and must not produce
                            // visible text.  Alt combos (dead keys, AltGr) are kept as
                            // they are needed for some layouts.
                            if !pw.modifiers.control_key() {
                                for ch in text.chars() {
                                    pw.chart.on_char_input(ch);
                                }
                            }
                        }
                        _ => {
                            handled = false;
                        }
                    }
                    let _ = handled;
                }
            }

            // ─── IME commit ───────────────────────────────────────────────
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                for ch in text.chars() {
                    pw.chart.on_char_input(ch);
                }
            }

            _ => {}
        }
    }
}

fn main() {
    // OTA: if launched with --wait-pid, wait for old process to exit first.
    zengeld_updater::wait_for_parent_exit_if_needed();

    println!("chart-app-vello");
    println!("===============");
    println!("Controls:");
    println!("  Left drag : Pan chart");
    println!("  Scroll    : Zoom");
    println!("  Escape    : Cancel / close modal");
    println!("  Ctrl+S    : Screenshot (copied to clipboard + saved to Pictures/Screenshots)");
    println!();

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll); // Will be updated per-frame in about_to_wait()

    // Create DataBridge ONCE — tokio runtime + connector pool shared by all windows.
    // The broadcast receiver (`_live_rx`) is dropped here: we no longer subscribe
    // to it at the app level.  Per-window ChartApp instances each subscribe via
    // `bridge.add_listener()`.  The mpsc `connector_ready_rx` is the lightweight
    // channel used by `tick_app_state` to handle ConnectorReady without touching
    // the broadcast buffer.
    let (bridge, _live_rx, connector_ready_rx) = live_data::DataBridge::new();
    let bridge = std::sync::Arc::new(bridge);

    // Load UserManager (profile + templates + presets + snapshots) once at startup.
    // All windows share this loaded state — no per-window disk reads.
    let mut user_manager = zengeld_chart::UserManager::load();

    // Record this launch (was previously done per-window
    // in load_user_state(); now done once here at the application level).
    user_manager.profile.record_launch(env!("CARGO_PKG_VERSION"));
    user_manager.save_profile();

    let profile = user_manager.profile.clone();
    let saved_windows = profile.windows.clone();

    let symbol = std::env::args().nth(1).unwrap_or_else(|| "BTCUSDT".to_string());
    let mut app = App::new(&symbol, bridge, saved_windows, profile, user_manager, connector_ready_rx);
    event_loop.run_app(&mut app).expect("Event loop error");
}
