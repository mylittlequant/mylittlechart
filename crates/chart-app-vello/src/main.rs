//! chart-app-vello — minimal winit + vello runner for chart-app
//!
//! Supports multiple windows sharing a single DataBridge (tokio runtime +
//! connector pool).  Each window has its own ChartApp with independent
//! tabs/presets but receives live updates via broadcast channels.
//! Creates windows on demand; closing the last window exits the process.

mod chrome;
pub mod keychain;

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
                        ScreenToClient(h.hwnd.get(), &mut pt);
                        return Some((pt.x as f64, pt.y as f64));
                    }
                }
            }
        }
        None
    }
}

/// DWM window border color control (Windows 11+).
///
/// Sets the thin colored border that Windows 11 draws around undecorated windows.
/// Silently ignored on Windows 10 and older — `DwmSetWindowAttribute` with
/// `DWMWA_BORDER_COLOR` returns an error on those versions which we discard.
#[cfg(target_os = "windows")]
mod win32_border {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[allow(unused_imports)] // extract_hwnd uses Window, set_dwm_border_color does not
    use winit::window::Window;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const u32,
            cb_attribute: u32,
        ) -> i32;
    }

    /// `DWMWA_BORDER_COLOR` — available since Windows 11 Build 22000.
    const DWMWA_BORDER_COLOR: u32 = 34;

    /// Parse `#RRGGBB` into a Win32 COLORREF (`0x00BBGGRR`).
    fn hex_to_colorref(hex: &str) -> Option<u32> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((b as u32) << 16 | (g as u32) << 8 | r as u32)
    }

    /// Extract HWND from a winit `Window` (must be called on the main thread).
    pub fn extract_hwnd(window: &Window) -> Option<isize> {
        let handle = window.window_handle().ok()?;
        if let RawWindowHandle::Win32(h) = handle.as_ref() {
            Some(h.hwnd.get())
        } else {
            None
        }
    }

    /// Apply the DWM border color using a cached HWND.
    ///
    /// `color` must be a `#RRGGBB` hex string.  Invalid strings or OS versions
    /// that do not support this attribute are silently ignored.
    pub fn set_dwm_border_color(hwnd: isize, color: &str) {
        let Some(colorref) = hex_to_colorref(color) else {
            return;
        };
        // Ignore the return value — non-zero means unsupported (Win10/older), which is fine.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &colorref as *const u32,
                std::mem::size_of::<u32>() as u32,
            );
        }
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
    window::{CursorIcon, Icon, Window, WindowId},
};
use zengeld_chart::CursorStyle;
use sysinfo::{System, Pid, ProcessesToUpdate};

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

/// Decode the embedded 32x32 PNG icon and return a winit [`Icon`].
///
/// The PNG bytes are embedded at compile time so there is no runtime I/O.
/// Returns `None` if decoding fails so a missing icon never crashes the app.
fn load_window_icon() -> Option<Icon> {
    let icon_bytes = include_bytes!("../../../assets/mascot/icon_32.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(icon_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let raw = &buf[..info.buffer_size()];

    // Ensure we have RGBA data; convert RGB → RGBA if needed.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => raw
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255u8])
            .collect(),
        _ => return None,
    };

    Icon::from_rgba(rgba, info.width, info.height).ok()
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
    /// Cached scene for the chart content area (panels, crosshair, drawings).
    /// Rebuilt only when `chart_dirty` is true; otherwise appended as-is.
    /// This avoids the full chart render on every frame when data/input has
    /// not changed — only 2-5 ticks/sec arrive, so 99%+ of frames are no-ops.
    chart_scene: vello::Scene,
    /// True when the chart content needs to be redrawn into `chart_scene`.
    /// Starts true; set on resize, mouse events in chart area, scroll, key
    /// press, tick/data updates, and timer events.  Cleared after rebuild.
    chart_dirty: bool,
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
    /// Cached Win32 HWND (extracted on main thread at window creation).
    #[cfg(target_os = "windows")]
    hwnd: Option<isize>,
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
    /// When true this window is a skeleton placeholder (shown while vault unlock
    /// or first-run wizard is pending).  Skeleton windows suppress tab/toolbar
    /// rendering and chart content — only chrome window controls are drawn.
    skeleton: bool,
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
    /// Whether `window.set_visible(true)` has been called yet.
    ///
    /// Windows are created with `with_visible(false)` to avoid the white-flash
    /// that appears before the first GPU frame is rendered.  After the first
    /// `GpuDone` is received (confirming the frame was presented), the window
    /// is made visible.
    visible_set: bool,
    /// Reference instant used to compute monotonic milliseconds for chrome tooltip timing.
    chrome_tooltip_start: std::time::Instant,
    /// Tooltip state for toolbar button hover tooltips (left/top/right/bottom strips).
    toolbar_tooltip: uzor::TooltipState,
    /// True when the window was minimized (detected via Resized with 0x0 size).
    /// Cleared on the first non-zero resize after minimization, which triggers
    /// a snap-to-end so the viewport is back at the latest bar after restore.
    was_minimized: bool,
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

    /// User's preferred price scale mode (Auto / Focus / Manual).
    /// Applied as the default when windows load bars for the first time.
    scale_mode: zengeld_chart::ScaleMode,

    // ── Agent API server settings ────────────────────────────────────────────
    /// Whether the server is enabled.
    server_enabled: bool,
    /// Port the server listens on.
    server_port: u16,
    /// Registered API keys with permission tiers.
    ///
    /// Canonical source — keys managed via the REST API are reflected here and
    /// persisted to the user profile on the next save_all() call.
    local_agent_keys: Vec<zengeld_chart::StoredLocalAgentKey>,

    /// Encryption key for zero-trust storage. Derived from passphrase at startup.
    /// `None` during migration or when running without a passphrase (plaintext mode).
    vault_key: Option<zengeld_chart::vault::VaultKey>,
}

impl AppState {
    /// Initialise from a loaded watchlist file and user profile.
    fn from_profile(
        profile: &zengeld_chart::UserProfile,
        presets: std::collections::HashMap<String, zengeld_chart::preset::preset::ChartPreset>,
        snapshots: zengeld_chart::user_manager::manager::SettingsSnapshots,
        template_manager: zengeld_chart::templates::manager::TemplateManager,
        vault_key: Option<zengeld_chart::vault::VaultKey>,
    ) -> Self {
        let default_wl = || {
            chart_app::WatchlistManager::new(vec![
                chart_app::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                chart_app::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                chart_app::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                chart_app::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                chart_app::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                chart_app::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
            ])
        };

        let watchlist_manager = {
            let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
            if watchlists_path.exists() {
                // Watchlists are always plaintext — pass None regardless of vault key.
                zengeld_chart::load_json::<chart_app::WatchlistManager>(&watchlists_path, None)
                    .unwrap_or_else(|e| {
                        eprintln!("[AppState] Failed to load watchlists: {}", e);
                        default_wl()
                    })
            } else {
                default_wl()
            }
        };

        // Parse recalc_mode from DeviceSettings (authoritative) with profile as fallback.
        let recalc_mode = {
            let ds = zengeld_chart::user_profile::DeviceSettings::load();
            let src = if ds.recalc_mode == "per_frame" && !profile.recalc_mode.is_empty() {
                // DeviceSettings has the default value — use the profile value if it
                // carries a non-default mode so that existing saves are honoured.
                &profile.recalc_mode
            } else {
                &ds.recalc_mode
            };
            match src.as_str() {
                "PerTick" => chart_app::RecalcMode::PerTick,
                "PerBar"  => chart_app::RecalcMode::PerBar,
                _         => chart_app::RecalcMode::PerFrame,
            }
        };

        // Parse scale_mode from the profile string.
        let scale_mode = match profile.scale_mode.as_str() {
            "Focus"  => zengeld_chart::ScaleMode::Focus,
            "Manual" => zengeld_chart::ScaleMode::Manual,
            _        => zengeld_chart::ScaleMode::Auto, // default / "Auto"
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
            scale_mode,
            server_enabled: profile.server_enabled,
            server_port: profile.server_port,
            local_agent_keys: {
                // Start with whatever the profile already has.
                let mut keys = profile.local_agent_keys.clone();

                // Migrate legacy single-key field if present and no new keys yet.
                if keys.is_empty() && !profile.legacy_single_agent_key.is_empty() {
                    let created_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    keys.push(zengeld_chart::StoredApiKey {
                        key_hash: zengeld_server::state::hash_key(&profile.legacy_single_agent_key),
                        label: "migrated-legacy-key".to_string(),
                        tier: "admin".to_string(),
                        created_at,
                        agent_id: None,
                        source: "local".to_string(),
                    });
                }
                keys
            },
            vault_key,
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

/// Status updates sent from the OAuth device-link poll task to the main thread.
#[derive(Debug)]
enum LinkPollStatus {
    /// Poll responded — waiting for user to click the link.
    Pending,
    /// Successfully linked — carry display name, provider, auth token and user id.
    Linked { display_name: String, provider: String, auth_token: String, user_id: i64 },
    /// Token expired or network error.
    Expired(String),
    /// Display the token/URL so the user can see it in the wizard.
    Init { token: String, link_url: String },
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
    #[allow(dead_code)]
    default_symbol: String,
    /// Shared DataBridge — tokio runtime + connector pool, created once at startup.
    bridge: std::sync::Arc<live_data::DataBridge>,
    /// Saved window states loaded from profile at startup — used in resumed() to restore windows.
    saved_windows: Vec<zengeld_chart::WindowState>,
    /// User profile loaded once at startup.
    profile: zengeld_chart::UserProfile,
    /// ProfileManager — encapsulated profile management layer (presets, templates,
    /// snapshots, vault key). Loaded once at startup and shared across all windows.
    /// Each `new_window()` call clones the relevant fields instead of re-reading
    /// from disk, eliminating redundant disk I/O when multiple windows are opened.
    profile_manager: zengeld_chart::ProfileManager,
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
    /// Present only when the `updater` feature is enabled and `standalone` is not.
    #[cfg(all(feature = "updater", not(feature = "standalone")))]
    updater_handle: Option<zengeld_updater::UpdaterHandle>,
    /// Stub when updater feature is disabled or standalone mode is active.
    #[cfg(not(all(feature = "updater", not(feature = "standalone"))))]
    updater_handle: Option<()>,

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
    /// Whether frame timing logs are printed to stderr (toggled from Performance panel).
    perf_log_enabled: bool,
    /// Current selected render backend.
    render_backend: sidebar_content::state::RenderBackend,
    /// True if no backend was explicitly saved — triggers auto-detection on first GPU info.
    backend_auto_detect: bool,
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

    /// True when BUILD_ATTESTATION was empty at compile time (dev / unofficial build).
    /// Always false in standalone builds (no server connection anyway).
    #[cfg(all(feature = "updater", not(feature = "standalone")))]
    is_unofficial_build: bool,

    /// True when `profile.json` did not exist at startup (first-run).
    /// Causes the Welcome Wizard overlay to appear on the first window created.
    is_first_run: bool,

    /// True when `salt.hex` exists (encrypted profile) but no vault key has been derived yet.
    /// Causes the Vault Unlock overlay to appear on the first window created.
    needs_vault_unlock: bool,

    /// True when a plaintext profile exists without `salt.hex` — user must set a passphrase
    /// to migrate to encrypted storage.  Shows wizard at page 1 (passphrase).
    needs_migration: bool,

    /// Receiver for status updates from the OAuth link-poll task.
    ///
    /// Set when `start_device_auth` spawns a poll loop; `None` when no link
    /// attempt is in progress.  Drained in `about_to_wait` so the wizard UI
    /// reflects current polling state.
    link_poll_rx: Option<tokio::sync::mpsc::UnboundedReceiver<LinkPollStatus>>,

    /// Profile ID to switch to at the end of the current `about_to_wait` pass.
    ///
    /// Set by the `profile_switch` command handler; consumed once by
    /// `execute_profile_switch` which tears down existing windows and rebuilds
    /// the app from the new profile without requiring a restart.
    pending_profile_switch: Option<String>,
    /// Vault key pre-validated during Branch A (pre-switch passphrase check), so
    /// `execute_profile_switch` can inject it directly and skip the unlock screen.
    pending_switch_vault_key: Option<zengeld_chart::vault::VaultKey>,
    /// ID of a newly created profile awaiting vault setup. Set by `profile_create:`
    /// handler; consumed by the `e2e_setup:` handler once the user enters a passphrase
    /// and vault creation succeeds, after which the actual profile switch happens.
    pending_new_profile_id: Option<String>,
    /// When true, skeleton windows are promoted to live: drop all windows, recreate
    /// with `skeleton=false` so they fetch bars, connect exchanges, etc.
    pending_skeleton_promote: bool,
    /// Profile switch deferred until after recovery key is acknowledged.
    ///
    /// Set by the `e2e_setup:` handler for Path C (new profile vault creation) when
    /// a recovery key must be shown before completing the profile switch.  Consumed
    /// by `recovery_key_confirmed` to trigger the actual switch.
    pending_switch_after_recovery: Option<(String, zengeld_chart::vault::VaultKey)>,

    /// Unused — retained to avoid removing struct fields during the OTA simplification.
    /// Previously held a deferred switch until the user chose a sync level in the wizard.
    #[allow(dead_code)]
    pending_switch_after_sync_level: Option<(String, zengeld_chart::vault::VaultKey)>,

    /// Unused — retained alongside `pending_switch_after_sync_level`.
    /// Previously held the sync level chosen by the user on the (now removed) ChooseSyncLevel page.
    pending_switch_sync_level: Option<String>,

    /// Master key recovered from `recovery_key.enc` during a `recovery_unlock:` command.
    ///
    /// Stored temporarily so the `set_new_passphrase:` handler can re-derive a new
    /// master key hierarchy and re-encrypt the vault without needing the old passphrase.
    /// Cleared immediately after `set_new_passphrase:` completes (or on any error).
    pending_recovery_master_key: Option<zengeld_chart::crypto::MasterKey>,

    /// True when the user confirms they have saved the new recovery key after a
    /// recovery re-key.  Skeleton promote is deferred until this flag is set.
    pending_promote_after_recovery_key: bool,

    /// Bar cache persistence service — owns BarStoreHandle + in-memory series map.
    bar_service: bar_service::BarService,
    /// Wall-clock instant of the last periodic bar-cache save.
    last_bar_cache_save: std::time::Instant,
    /// Wall-clock instant of the last periodic bar-cache cleanup run.
    last_cleanup_check: std::time::Instant,
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
    #[cfg(target_os = "windows")]
    let dwm_border_color: String;
    {
        let theme = pw.chart.panel_app.theme_manager.current();
        pw.chrome_state.colors.background = theme.chart.background.clone();
        pw.chrome_state.colors.icon_normal = theme.colors.text_primary.clone();
        pw.chrome_state.colors.icon_hover  = theme.colors.accent.clone();
        pw.chrome_state.colors.button_hover = theme.colors.button_bg_hover.clone();
        pw.chrome_state.colors.close_hover = theme.colors.danger.clone();
        pw.chrome_state.colors.separator   = theme.colors.toolbar_divider.clone();
        pw.chrome_state.colors.tab_accent  = theme.colors.accent.clone();
        pw.chrome_state.colors.tooltip_bg  = theme.colors.button_bg.clone();
        pw.chrome_state.colors.tooltip_text = theme.colors.text_primary.clone();
        #[cfg(target_os = "windows")]
        { dwm_border_color = theme.colors.ui_border.clone(); }
    }
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = pw.hwnd {
        win32_border::set_dwm_border_color(hwnd, &dwm_border_color);
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
        if pw.skeleton {
            // Skeleton: no tabs at all, chrome draws only + and system buttons.
            tabs.clear();
        } else if tabs.is_empty() {
            // Safety net: should not happen after open_tabs is always populated.
            eprintln!("[Chrome] WARNING: tabs empty — injecting fallback Untitled");
            tabs.push(chrome::Tab {
                id: "__fallback__".to_string(),
                name: "Untitled".to_string(),
                active: true,
            });
        } else if !tabs.iter().any(|t| t.active) {
            if let Some(first) = tabs.first_mut() {
                first.active = true;
            }
        }
        pw.chrome_state.tabs = tabs;
    }

    use sidebar_content::state::RenderBackend;
    let is_vello_gpu = pw.render_backend == RenderBackend::VelloGpu;

    if is_vello_gpu {
        // ── VelloGpu: everything renders into pw.scene via vello ──────────────

        // Render chrome strip
        {
            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            let mut chrome_ctx =
                VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state);
            chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active);
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
        // The chart scene is cached: only rebuild when chart_dirty is set.
        if pw.chart_dirty {
            pw.chart_scene.reset();
            let mut render_ctx = VelloGpuRenderContext::new(
                &mut pw.chart_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render(&mut render_ctx, frame_time, true);
            pw.chart_dirty = false;
        }
        pw.scene.append(&pw.chart_scene, None);

        let sidebar_is_open = pw.chart.sidebar_state.is_right_open();

        // In skeleton mode (profile manager / welcome wizard active) the sidebar
        // is hidden — its width is forced to 0 in render_to_scene() and we skip
        // compositing the sidebar scene so no stale sidebar pixels appear.
        let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
            || pw.chart.panel_app.user_settings_state.show_welcome_wizard;

        // Sidebar scene rebuild (after render, within the same input frame)
        if sidebar_is_open && !skeleton_active && pw.sidebar_dirty_scene {
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
        if sidebar_is_open && !skeleton_active {
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

        // Render tooltips (top-most layer): chrome + toolbar
        {
            let mut tooltip_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::render_tooltip(&mut tooltip_ctx, &pw.chrome_state, width as f64, height as f64);
            chrome::render_tooltip_themed(&mut tooltip_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
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
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }

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

                // Render chrome tooltip (top-most layer).
                {
                    let mut tooltip_ctx = instanced_context::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    chrome::render_tooltip(&mut tooltip_ctx, &pw.chrome_state, width as f64, height as f64);
                    chrome::render_tooltip_themed(&mut tooltip_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&tooltip_ctx.inner().draw_commands);
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
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut cpu_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
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
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut cpu_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut cpu_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
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
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut skia_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
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
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut skia_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut skia_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
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
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut hybrid_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
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
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut hybrid_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut hybrid_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
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
                        0  => AaConfig::Area,
                        8  => AaConfig::Msaa8,
                        16 => AaConfig::Msaa16,
                        _  => AaConfig::Msaa8, // safe fallback
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
        Err(e) => {
            eprintln!("[GPU] Surface error: {:?}, reconfiguring", e);
            pw.surface.surface.configure(device, &pw.surface.config);
            return render_tex_us;
        }
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
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
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
        unpadded_bytes_per_row.div_ceil(ALIGNMENT) * ALIGNMENT;

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

/// Rough timeframe string → period in seconds for BarSeries.
fn timeframe_period_secs(tf: &str) -> i64 {
    match tf {
        "1s" => 1,
        "5s" => 5,
        "15s" => 15,
        "30s" => 30,
        "1m" => 60,
        "3m" => 180,
        "5m" => 300,
        "15m" => 900,
        "30m" => 1800,
        "1h" | "1H" => 3600,
        "2h" | "2H" => 7200,
        "4h" | "4H" => 14400,
        "6h" | "6H" => 21600,
        "8h" | "8H" => 28800,
        "12h" | "12H" => 43200,
        "1d" | "1D" => 86400,
        "3d" | "3D" => 259200,
        "1w" | "1W" => 604800,
        "1M" => 2592000,
        _ => {
            // Try to parse numeric prefix as minutes/hours/days/weeks.
            let num_str: String = tf.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = num_str.parse::<i64>() {
                if tf.ends_with('m') { n * 60 }
                else if tf.ends_with('h') || tf.ends_with('H') { n * 3600 }
                else if tf.ends_with('d') || tf.ends_with('D') { n * 86400 }
                else if tf.ends_with('w') || tf.ends_with('W') { n * 604800 }
                else { n * 60 } // default to minutes
            } else {
                60 // fallback: 1 minute
            }
        }
    }
}

/// Collect `(exchange, symbol, timeframe, account_type)` 4-tuples from all windows of all
/// open tabs in the given profile.  Used at startup to pre-load the bar cache from disk.
fn collect_bar_keys_from_presets(
    profile: &zengeld_chart::UserProfile,
    presets: &std::collections::HashMap<String, zengeld_chart::preset::preset::ChartPreset>,
) -> Vec<(String, String, String, String)> {
    let mut keys = std::collections::HashSet::new();
    for ws in &profile.windows {
        for tab_id in &ws.open_tabs {
            if let Some(preset) = presets.get(tab_id) {
                for win in &preset.windows {
                    if !win.symbol.is_empty() && !win.exchange.is_empty() {
                        keys.insert((
                            win.exchange.to_lowercase(),
                            win.symbol.clone(),
                            win.timeframe.name.clone(),
                            win.account_type.clone(), // "S" by default (serde(default) on ChartWindowSnapshot)
                        ));
                    }
                }
            }
        }
    }
    keys.into_iter().collect()
}

impl App<'_> {
    fn new(
        symbol: &str,
        bridge: std::sync::Arc<live_data::DataBridge>,
        shared_series: bar_service::SharedSeriesMap,
        saved_windows: Vec<zengeld_chart::WindowState>,
        profile: zengeld_chart::UserProfile,
        profile_manager: zengeld_chart::ProfileManager,
        app_connector_ready_rx: live_data::ConnectorReadyReceiver,
        is_first_run: bool,
        needs_vault_unlock: bool,
        needs_migration: bool,
    ) -> Self {
        let app_state = AppState::from_profile(&profile, profile_manager.presets.clone(), profile_manager.snapshots.clone(), profile_manager.template_manager.clone(), profile_manager.vault_key);

        // Convert StoredLocalAgentKey entries to LocalAgentKey (the server type).
        let server_keys: Vec<zengeld_server::state::LocalAgentKey> = app_state
            .local_agent_keys
            .iter()
            .map(|k| zengeld_server::state::LocalAgentKey {
                key_hash: k.key_hash.clone(),
                label: k.label.clone(),
                tier: k.tier.clone(),
                permissions: zengeld_server::state::Permissions::from_tier(&k.tier),
                created_at: k.created_at,
                agent_id: k.agent_id.clone(),
                // Treat any stored key as Local unless it was explicitly marked cloud.
                source: if k.source == "cloud" {
                    zengeld_server::state::AgentKeySource::Cloud
                } else {
                    zengeld_server::state::AgentKeySource::Local
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
        // Disabled entirely when the `standalone` feature is active — standalone
        // builds have no cloud connection and no update checks by design.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
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

            // In standalone builds the updater always starts disconnected,
            // regardless of what the profile says.  This permanently disables
            // all server communication in open-source / self-hosted builds.
            #[cfg(feature = "standalone")]
            let connected = false;
            #[cfg(not(feature = "standalone"))]
            let connected = {
                // OTA is controlled solely by the device-level toggle.
                // Profile-level ota_enabled is kept in the struct for backward
                // compatibility but no longer gates the updater.
                let device_settings = zengeld_chart::user_profile::DeviceSettings::load();
                device_settings.ota_enabled
            };

            // Build attestation — embedded at compile time by build.rs.
            // Empty string for dev builds (no RELEASE_SIGNING_KEY set).
            let build_attest = zengeld_updater::BuildAttestation {
                attestation: env!("BUILD_ATTESTATION").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                platform: env!("BUILD_PLATFORM").to_string(),
                timestamp: env!("BUILD_TIMESTAMP").to_string(),
            };

            let handle = zengeld_updater::start(
                bridge.runtime().handle(),
                source,
                connected,
                profile.sync_state.enabled,
                profile.sync_state.synced_items.clone(),
                profile.sync_state.last_synced_checksums.clone(),
                zengeld_chart::active_profile_data_dir(),
                build_attest,
                profile.profile_id.clone(),
                profile.server_port,
            );

            // Seed granular sync toggles from profile into the updater loop.
            // The `start()` function only accepts a single `sync_enabled` bool,
            // so we send the per-category toggles immediately after starting.
            {
                use zengeld_updater::UpdaterCommand;
                let ss = &profile.sync_state;
                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncPresets(ss.sync_presets));
                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncTemplates(ss.sync_templates));
                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncWatchlists(ss.sync_watchlists));
                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncTheme(ss.sync_theme));
            }

            Some(handle)
        };
        // No updater when the crate feature is absent, or when standalone mode is active.
        #[cfg(not(all(feature = "updater", not(feature = "standalone"))))]
        let updater_handle: Option<()> = None;

        // Detect unofficial / dev build (empty attestation = no release signing key).
        // Only meaningful in connected builds; standalone never contacts the server.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        let is_unofficial_build: bool = env!("BUILD_ATTESTATION").is_empty();

        // Apply saved language preference from profile.
        {
            let lang = match profile.language.as_str() {
                "ru" => zengeld_chart::Language::Ru,
                _    => zengeld_chart::Language::En,
            };
            zengeld_chart::set_language(lang);
            eprintln!("[App] startup language: {}", profile.language);
        }

        // ── Bar cache persistence ─────────────────────────────────────────────
        // Create the BarStoreHandle, load persisted bars, and seed the shared series
        // map so that the first symbol switch is instant without a network round-trip.
        // `BarService::with_map` attaches to the same `SharedSeriesMap` that
        // `DataBridge` already holds, so persistence and live-fetch share one map.
        let mut bar_service = {
            let bars_dir = zengeld_chart::app_data_dir().join("bars");
            let bar_store_handle = bar_store::BarStoreHandle::new(bars_dir, bridge.runtime());
            bar_service::BarService::with_map(shared_series, bar_store_handle, bar_service::DEFAULT_CAPACITY)
        };

        // Collect bar keys from presets and load them from disk.
        // Seed both BarService (persistence) and DataBridge cache (runtime).
        {
            let bar_keys = collect_bar_keys_from_presets(&profile, &profile_manager.presets);
            let bar_key_refs: Vec<(&str, &str, &str, &str)> = bar_keys
                .iter()
                .map(|(e, s, t, a)| (e.as_str(), s.as_str(), t.as_str(), a.as_str()))
                .collect();
            let loaded = bar_service.load_many(&bar_key_refs);
            if !loaded.is_empty() {
                eprintln!("[App] pre-loaded {} bar cache entries from disk", loaded.len());
                // Seed BarService series from disk data.
                for (exchange_str, symbol, timeframe, account_type_label, bars) in &loaded {
                    if let Some(eid) = chart_app::ExchangeId::from_str(exchange_str) {
                        let at = chart_app::account_type_from_label(account_type_label);
                        let period_secs = timeframe_period_secs(timeframe);
                        let key = bar_service::BarSeriesKey::new(eid, at, symbol.clone(), timeframe.clone());
                        bar_service.seed_from_disk(key, bars.clone(), period_secs);
                    }
                }
                // Bridge shares the same SharedSeriesMap — no separate seed needed.
            }
        }

        // ── Bar store startup cleanup ─────────────────────────────────────────
        // Run stale-file eviction and LRU size trim on a background thread so
        // app startup is not blocked by disk I/O.
        {
            let bars_dir = bar_service.bars_dir().to_path_buf();
            let max_size_mb = profile.data_load.max_store_size_mb;
            let cleanup_days = profile.data_load.store_cleanup_days;
            std::thread::spawn(move || {
                let cleanup = bar_store::BarStoreCleanup::new(bars_dir);
                cleanup.run_cleanup(max_size_mb, cleanup_days);
            });
        }

        // Load device settings once for App struct initialisation.
        let ds_init = zengeld_chart::user_profile::DeviceSettings::load();

        Self {
            render_cx: RenderContext::new(),
            windows: HashMap::new(),
            pending_spawns: Vec::new(),
            close_all_requested: false,
            default_symbol: symbol.to_string(),
            bridge,
            saved_windows,
            profile,
            profile_manager,
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
            fps_limit: ds_init.fps_limit,
            msaa_samples: ds_init.msaa_samples,
            perf_log_enabled: false,
            render_backend: {
                use sidebar_content::state::RenderBackend;
                match ds_init.render_backend {
                    Some(zengeld_chart::user_profile::device_settings::RenderBackend::VelloGpu) => RenderBackend::VelloGpu,
                    Some(zengeld_chart::user_profile::device_settings::RenderBackend::InstancedWgpu) => RenderBackend::InstancedWgpu,
                    Some(zengeld_chart::user_profile::device_settings::RenderBackend::VelloCpu) => RenderBackend::VelloCpu,
                    Some(zengeld_chart::user_profile::device_settings::RenderBackend::VelloHybrid) => RenderBackend::VelloHybrid,
                    Some(zengeld_chart::user_profile::device_settings::RenderBackend::TinySkia) => RenderBackend::TinySkia,
                    None => RenderBackend::VelloGpu, // will be overridden by auto-detect
                }
            },
            backend_auto_detect: ds_init.render_backend.is_none(),
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
            #[cfg(all(feature = "updater", not(feature = "standalone")))]
            is_unofficial_build,
            telemetry_shared,
            is_first_run,
            needs_vault_unlock,
            needs_migration,
            link_poll_rx: None,
            pending_profile_switch: None,
            pending_switch_vault_key: None,
            pending_new_profile_id: None,
            pending_skeleton_promote: false,
            pending_switch_after_recovery: None,
            pending_switch_after_sync_level: None,
            pending_switch_sync_level: None,
            pending_recovery_master_key: None,
            pending_promote_after_recovery_key: false,
            bar_service,
            last_bar_cache_save: std::time::Instant::now(),
            last_cleanup_check: std::time::Instant::now(),
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
            .with_title("mylittlechart")
            .with_inner_size(winit::dpi::LogicalSize::new(1200u32, 800u32))
            // Prevent squishing the chart area below a degenerate size.
            // 400 px gives room for at least one chart leaf + right toolbar + margins.
            // 300 px ensures enough vertical space for scales and subpanes.
            .with_min_inner_size(winit::dpi::LogicalSize::new(400.0_f64, 300.0_f64))
            .with_decorations(false)
            // Hidden until the first GPU frame completes to eliminate the
            // white-flash that appears before any pixels are drawn.
            .with_visible(false)
            .with_window_icon(load_window_icon());

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_undecorated_shadow(true);
        }

        // Skeleton = loading screen: fixed 1200x800, centered on primary monitor.
        // Live windows use saved geometry from the profile.
        let skeleton = self.needs_vault_unlock || self.is_first_run || self.needs_migration;
        if skeleton {
            // Center on primary monitor
            if let Some(monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
                let screen = monitor.size();
                let scale = monitor.scale_factor();
                let win_w = (1200.0 * scale) as i32;
                let win_h = (800.0 * scale) as i32;
                let cx = (screen.width as i32 - win_w) / 2;
                let cy = (screen.height as i32 - win_h) / 2;
                attrs = attrs.with_position(winit::dpi::Position::Physical(
                    winit::dpi::PhysicalPosition::new(cx, cy),
                ));
            }
        } else if let Some(ws) = restore {
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

        // Populate GPU info unconditionally — adapter is available right after create_surface()
        {
            let info = self.render_cx.devices[dev_id].adapter().get_info();
            self.gpu_name = info.name.clone();
            self.gpu_driver = info.driver_info.clone();
            eprintln!("[App] GPU: {} ({})", self.gpu_name, self.gpu_driver);
            if let Ok(mut g) = self.telemetry_shared.gpu_name.lock() {
                *g = self.gpu_name.clone();
            }

            // Auto-detect backend based on GPU capabilities (first launch only)
            if self.backend_auto_detect {
                use sidebar_content::state::RenderBackend;
                let recommended = match info.device_type {
                    wgpu::DeviceType::DiscreteGpu => RenderBackend::VelloGpu,
                    wgpu::DeviceType::IntegratedGpu => RenderBackend::VelloGpu,
                    wgpu::DeviceType::VirtualGpu => RenderBackend::VelloCpu,
                    wgpu::DeviceType::Cpu => RenderBackend::TinySkia,
                    _ => RenderBackend::VelloGpu,
                };
                if self.render_backend != recommended {
                    self.render_backend = recommended;
                }
                eprintln!("[App] Auto-detected backend → {:?} (device_type={:?})", recommended, info.device_type);

                // Set backend-specific performance defaults.
                let (fps, msaa) = match recommended {
                    RenderBackend::VelloGpu      => (120u32, 8u8),
                    RenderBackend::VelloCpu      => (30,     0),
                    RenderBackend::TinySkia      => (90,     8),
                    RenderBackend::InstancedWgpu => (90,     8),
                    RenderBackend::VelloHybrid   => (90,     8),
                };

                // Save auto-detected choice and perf defaults so next launch skips auto-detect.
                {
                    let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                    ds.render_backend = Some(match recommended {
                        RenderBackend::VelloGpu => zengeld_chart::user_profile::device_settings::RenderBackend::VelloGpu,
                        RenderBackend::InstancedWgpu => zengeld_chart::user_profile::device_settings::RenderBackend::InstancedWgpu,
                        RenderBackend::VelloCpu => zengeld_chart::user_profile::device_settings::RenderBackend::VelloCpu,
                        RenderBackend::VelloHybrid => zengeld_chart::user_profile::device_settings::RenderBackend::VelloHybrid,
                        RenderBackend::TinySkia => zengeld_chart::user_profile::device_settings::RenderBackend::TinySkia,
                    });
                    ds.fps_limit = fps;
                    ds.msaa_samples = msaa;
                    ds.max_bars = 0; // unlimited
                    ds.recalc_mode = "per_frame".to_string();
                    ds.save();
                }
                // Apply to runtime.
                self.fps_limit = fps;
                self.msaa_samples = msaa;
                self.backend_auto_detect = false;
            }
        }

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
            &self.profile_manager,
            skeleton,
        );
        // Apply the app-level theme so new windows start with the correct preset.
        if !self.app_state.theme_preset.is_empty() {
            chart.panel_app.theme_manager.set_preset(&self.app_state.theme_preset);
        }
        // Initialise the new window directly from AppState so it has current data
        // without waiting for the next dirty-flag sync pass.
        chart.sidebar_state.watchlist_manager = self.app_state.watchlist_manager.clone();
        chart.sidebar_state.connector_enabled = self.app_state.connector_enabled.clone();
        // Merge AppState presets into window — don't replace, to keep
        // any Untitled preset that new_window() just created.
        for (k, v) in self.app_state.presets.iter() {
            chart.panel_app.presets.entry(k.clone()).or_insert_with(|| v.clone());
        }
        // Sync back: if new_window() created an Untitled preset for a fresh profile,
        // propagate it to app_state so chrome tabs and preset list can see it.
        for (k, v) in chart.panel_app.presets.iter() {
            self.app_state.presets.entry(k.clone()).or_insert_with(|| v.clone());
        }
        // Also sync open_tabs and active_preset_id back to profile_manager
        // so save_all() persists them.
        if !chart.panel_app.active_preset_id.is_empty() {
            self.profile_manager.profile.active_preset_id = chart.panel_app.active_preset_id.clone();
            self.profile.active_preset_id = chart.panel_app.active_preset_id.clone();
        }
        if !chart.panel_app.open_tabs.is_empty() {
            self.profile_manager.profile.open_tabs = chart.panel_app.open_tabs.clone();
            self.profile.open_tabs = chart.panel_app.open_tabs.clone();
        }
        chart.panel_app.user_manager.snapshots = self.app_state.snapshots.clone();
        // Apply persisted last-used drawing styles to all DrawingManagers in this new window.
        let drawing_styles = &self.app_state.snapshots.last_used_drawing_styles;
        for w in chart.panel_app.panel_grid.windows_mut().values_mut() {
            w.drawing_manager.load_last_styles(drawing_styles);
        }
        chart.panel_app.template_manager = self.app_state.template_manager.clone();
        // Sync server settings so the User Settings modal shows current state.
        chart.panel_app.user_settings_state.server_enabled = self.app_state.server_enabled;
        chart.panel_app.user_settings_state.server_port = self.app_state.server_port;
        chart.panel_app.user_settings_state.server_status = if self.app_state.server_enabled {
            "running".to_string()
        } else {
            "stopped".to_string()
        };
        // Sync connection mode from the authoritative profile (self.profile is
        // the copy serialised by save_all; profile_manager.profile is only the
        // seed used during startup loading).
        chart.panel_app.user_settings_state.client_mode_connected =
            self.profile.cloud_enabled;
        // Sync language preference from the loaded profile.
        chart.panel_app.user_settings_state.language =
            self.profile_manager.profile.language.clone();
        // Sync cloud sync settings from the loaded profile.
        {
            let ss = &self.profile_manager.profile.sync_state;
            let uss = &mut chart.panel_app.user_settings_state;
            uss.sync_enabled = ss.enabled;
            uss.last_sync_timestamp = ss.last_sync_timestamp;
            uss.sync_presets = ss.sync_presets;
            uss.sync_templates = ss.sync_templates;
            uss.sync_watchlists = ss.sync_watchlists;
            uss.sync_theme_toggle = ss.sync_theme;
        }
        {
            let uss = &mut chart.panel_app.user_settings_state;
            uss.ota_enabled = self.profile_manager.profile.ota_enabled;
        }
        // Seed device-level settings into the UI state so the profile manager
        // toggles reflect the persisted device preferences.
        {
            let ds = zengeld_chart::user_profile::DeviceSettings::load();
            let uss = &mut chart.panel_app.user_settings_state;
            uss.device_ota_enabled = ds.ota_enabled;
            uss.device_update_channel = ds.update_channel;
        }
        // Sync profile data into the user settings state.
        {
            let uss = &mut chart.panel_app.user_settings_state;
            uss.profile_id = self.profile_manager.profile.profile_id.clone();
            // runtime_profile_id tracks the ACTUALLY loaded profile for the life of
            // this session.  It is set once here and never changed — even if the user
            // switches the pending active profile, the running profile stays the same
            // until the app is restarted.
            uss.runtime_profile_id = self.profile_manager.profile.profile_id.clone();
            uss.profile_display_name = self.profile_manager.profile.display_name.clone();
            uss.profile_avatar = self.profile_manager.profile.avatar.clone();
            // Load available profiles from the index.
            if let Some(index) = zengeld_chart::load_profile_index() {
                let profiles_dir = zengeld_chart::active_profile_data_dir()
                    .parent()
                    .map(|p| p.to_path_buf());
                uss.available_profiles = index.profiles.iter().map(|m| {
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone(), m.sync_level.clone())
                }).collect();
                uss.profiles_with_vault_status = index.profiles.iter().map(|m| {
                    let has_vault = if let Some(ref pd) = profiles_dir {
                        pd.join(&m.dir_name).join("vault.enc").exists()
                    } else {
                        false
                    };
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone(), m.cloud_enabled, has_vault, m.sync_level.clone())
                }).collect();
            } else {
                // No index yet — synthesize a single entry from the current profile.
                uss.available_profiles = vec![(
                    uss.profile_id.clone(),
                    uss.profile_display_name.clone(),
                    uss.profile_avatar.clone(),
                    "local".to_string(),
                )];
            }
        }
        // API keys are now managed via /api/v1/keys REST endpoint.
        // Show key count in the UI instead of the raw key string.
        chart.panel_app.user_settings_state.local_agent_key_display = format!(
            "{} key(s) registered",
            self.app_state.local_agent_keys.len()
        );
        // Propagate build attestation status to new windows.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        {
            chart.panel_app.user_settings_state.is_unofficial_build = self.is_unofficial_build;
        }
        // Sync auth state from updater watch channel into the new window.
        // `has_changed()` never fires for the initial value, so we read it
        // directly here to restore logged-in state on startup.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        {
            if let Some(ref handle) = self.updater_handle {
                let status = handle.auth_rx.borrow().clone();
                match &status {
                    zengeld_updater::AuthStatus::LoggedIn { display_name, provider, user_id } => {
                        let s = &mut chart.panel_app.user_settings_state;
                        s.is_logged_in = true;
                        s.auth_display_name = display_name.clone();
                        s.auth_provider = provider.clone();
                        s.auth_user_id = *user_id;
                        // Always fetch cloud profiles when entering skeleton while logged in.
                        s.cloud_profiles_loading = true;
                        s.cloud_profiles_error.clear();
                        let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::ListCloudProfiles);
                    }
                    zengeld_updater::AuthStatus::NotLoggedIn => {}
                }
            }
        }

        // Show the welcome wizard on the first window when this is a first-run launch.
        // The wizard is non-closeable until the user makes a mode choice.
        if self.is_first_run {
            chart.panel_app.user_settings_state.show_welcome_wizard = true;
        }

        // Show the profile list when vault needs unlocking (returning user with encrypted data).
        // User picks their profile from the list, then gets the passphrase form.
        if self.needs_vault_unlock {
            use zengeld_chart::ui::modal_settings::ProfileManagerPage;
            chart.panel_app.user_settings_state.needs_vault_unlock = true;
            chart.panel_app.user_settings_state.show_profile_manager = true;
            chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::ProfileList;
            chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
            chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
            // Populate profile vault status list
            if let Some(index) = zengeld_chart::load_profile_index() {
                let profiles_dir = zengeld_chart::active_profile_data_dir()
                    .parent()
                    .map(|p| p.to_path_buf());
                chart.panel_app.user_settings_state.profiles_with_vault_status = index.profiles.iter().map(|m| {
                    let has_vault = if let Some(ref pd) = profiles_dir {
                        pd.join(&m.id).join("vault.enc").exists()
                    } else {
                        false
                    };
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone(), m.cloud_enabled, has_vault, m.sync_level.clone())
                }).collect();
            }
        }

        // Migration: existing plaintext profile without salt.hex.
        // Show the profile manager create passphrase page so the user sets a passphrase.
        // Their existing data will be encrypted on completion via save_all().
        if self.needs_migration {
            use zengeld_chart::ui::modal_settings::ProfileManagerPage;
            chart.panel_app.user_settings_state.needs_vault_unlock = true;
            chart.panel_app.user_settings_state.show_profile_manager = true;
            chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
            chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
            chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
        }

        let chrome_px = (chrome::CHROME_HEIGHT * window.scale_factor()) as u32;
        chart.resize(size.width, size.height.saturating_sub(chrome_px));

        let win_id = window.id();
        #[cfg(target_os = "windows")]
        let cached_hwnd = win32_border::extract_hwnd(&window);
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
            chart_scene: Scene::new(),
            chart_dirty: true,
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
            #[cfg(target_os = "windows")]
            hwnd: cached_hwnd,
            close_requested: false,
            spawn_new_window: false,
            window_id,
            close_window_requested: false,
            delete_window_requested: false,
            last_sidebar_hover_row: None,
            skeleton,
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
            visible_set: false,
            chrome_tooltip_start: std::time::Instant::now(),
            toolbar_tooltip: uzor::TooltipState::with_delay(700.0),
            was_minimized: false,
        };

        self.windows.insert(win_id, pw);

        // Spawn the GPU render thread on the first window creation.
        // The thread is persistent across all frames so we avoid per-frame
        // spawn overhead.
        if self.gpu_cmd_tx.is_none() {
            self.spawn_gpu_thread();
        }
    }

    /// Propagate the current profile list from `profile_manager` into all open
    /// windows so the user-settings modal always shows up-to-date data.
    fn sync_profiles_to_windows(&mut self) {
        let profiles = self.profile_manager.available_profiles();
        let profiles_with_vault: Vec<(String, String, String, bool, bool, String)> = profiles
            .iter()
            .map(|p| (p.id.clone(), p.display_name.clone(), p.avatar.clone(), p.cloud_enabled, p.has_vault, p.sync_level.clone()))
            .collect();
        let available: Vec<(String, String, String, String)> = profiles
            .iter()
            .map(|p| (p.id.clone(), p.display_name.clone(), p.avatar.clone(), p.sync_level.clone()))
            .collect();
        for pw in self.windows.values_mut() {
            pw.chart.panel_app.user_settings_state.available_profiles = available.clone();
            pw.chart.panel_app.user_settings_state.profiles_with_vault_status = profiles_with_vault.clone();
        }
    }

    /// Perform a hot-reload profile switch without restarting the process.
    ///
    /// Saves the current profile, updates the index, drops all existing windows,
    /// loads the new profile, and recreates windows from the new profile's saved state.
    /// Called at the end of `about_to_wait` once `pending_profile_switch` is set.
    fn execute_profile_switch(&mut self, target_id: &str, event_loop: &ActiveEventLoop) {
        eprintln!("[App] hot-reload: switching to profile {}", target_id);

        // 1. Save current profile state (windows, presets, watchlist).
        self.save_all(&[]);

        // 2. Update the index so the new profile is active on next load too.
        if let Some(mut index) = zengeld_chart::load_profile_index() {
            index.active_profile_id = target_id.to_string();
            if let Err(e) = zengeld_chart::save_profile_index(&index) {
                eprintln!("[App] hot-reload: failed to save index: {}", e);
            }
        }

        // 3. Drop all existing windows.
        //    Removing from the HashMap drops PerWindowState which owns the winit
        //    Window (Arc) and the RenderSurface.  winit will handle OS cleanup.
        let window_ids: Vec<winit::window::WindowId> = self.windows.keys().copied().collect();
        for wid in window_ids {
            self.windows.remove(&wid);
        }
        self.last_focused = None;

        // 4. Determine whether the new profile needs vault unlock.
        //    active_profile_data_dir() now resolves to target_id's directory
        //    because the index was just updated and saved above.
        let new_profile_dir = zengeld_chart::active_profile_data_dir();
        let has_vault = new_profile_dir.join("vault.enc").exists();
        let has_salt  = new_profile_dir.join("salt.hex").exists();

        // 5. Load the new profile from disk.
        //    active_profile_data_dir() reads the index we just updated, so it
        //    will resolve to target_id's directory.
        self.profile_manager = zengeld_chart::ProfileManager::load(None);
        let profile = self.profile_manager.profile.clone();
        let saved_windows = profile.windows.clone();

        // 6. Rebuild app-level state from the new profile.
        self.app_state = AppState::from_profile(
            &profile,
            self.profile_manager.presets.clone(),
            self.profile_manager.snapshots.clone(),
            self.profile_manager.template_manager.clone(),
            self.profile_manager.vault_key,
        );
        self.profile = profile;
        self.saved_windows = saved_windows.clone();
        self.needs_vault_unlock = has_vault && has_salt;

        // 6b. If the passphrase was pre-validated in Branch A, inject the key now
        //     so the user goes straight into the app without seeing the unlock screen.
        if let Some(key) = self.pending_switch_vault_key.take() {
            self.profile_manager.set_vault_key(key);
            self.profile_manager.vault_key = Some(key);
            if let Err(e) = self.profile_manager.load_vault_secrets() {
                eprintln!("[App] hot-reload: failed to load vault secrets after pre-switch: {}", e);
            }
            self.profile = self.profile_manager.profile.clone();
            self.app_state.vault_key = Some(key);
            self.needs_vault_unlock = false;
            eprintln!("[App] hot-reload: pre-validated vault key injected — skipping unlock screen");
        }

        // 6c. Apply deferred sync level to the freshly loaded profile.
        if let Some(level) = self.pending_switch_sync_level.take() {
            eprintln!("[App] hot-reload: applying deferred sync_level={}", level);
            let p = &mut self.profile_manager.profile;
            match level.as_str() {
                "local" => {
                    p.ota_enabled = false;
                    p.sync_state.enabled = false;
                    p.cloud_enabled = false;
                }
                "connected" => {
                    p.ota_enabled = true;
                    p.sync_state.enabled = false;
                    p.cloud_enabled = false;
                }
                "cloud" => {
                    p.ota_enabled = true;
                    p.sync_state.enabled = true;
                    p.sync_state.sync_presets = true;
                    p.sync_state.sync_templates = true;
                    p.sync_state.sync_watchlists = true;
                    p.sync_state.sync_theme = true;
                    p.sync_state.sync_vault = true;
                    p.sync_state.sync_recovery_key = true;
                    p.cloud_enabled = true;
                }
                _ => {
                    eprintln!("[App] hot-reload: unknown sync level '{}' — skipping", level);
                }
            }
            p.sync_level = level.clone();
            // Persist sync level to profile.json and index.
            if let Err(e) = self.profile_manager.save_profile() {
                eprintln!("[App] hot-reload: failed to save profile after sync level: {}", e);
            }
            let _ = zengeld_chart::set_profile_sync_level(
                &self.profile_manager.profile.profile_id,
                self.profile_manager.profile.cloud_enabled,
                &level,
            );
            // Re-sync self.profile from the updated profile_manager.
            self.profile = self.profile_manager.profile.clone();
        }

        self.is_first_run = false;

        // 7. Recreate windows from the new profile's saved state.
        if saved_windows.is_empty() {
            eprintln!("[App] hot-reload: no saved windows — creating default");
            self.create_window(event_loop, None, None);
        } else {
            for ws in &saved_windows {
                eprintln!(
                    "[App] hot-reload: restoring window tabs={} active={}",
                    ws.open_tabs.len(),
                    ws.active_preset_id
                );
                self.create_window(event_loop, Some(ws), None);
            }
        }

        if has_vault && has_salt && self.needs_vault_unlock {
            // Profile has vault and key was NOT pre-injected — need unlock
            use zengeld_chart::ui::modal_settings::ProfileManagerPage;
            for pw in self.windows.values_mut() {
                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                pw.chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
                pw.chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
            }
        } else if !has_vault && !self.is_first_run {
            // Existing profile with no encryption — offer to set up
            use zengeld_chart::ui::modal_settings::ProfileManagerPage;
            self.needs_vault_unlock = true;
            for pw in self.windows.values_mut() {
                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                pw.chart.panel_app.user_settings_state.needs_vault_unlock = true;
                pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                pw.chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
                pw.chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
                pw.chart.panel_app.user_settings_state.is_open = false;
            }
            eprintln!("[App] hot-reload: profile has no vault — showing passphrase creation");
        }

        // Notify the updater of the new data directory and sync settings.
        // The updater loop holds its own copy of these values; after a profile
        // switch it must read from the new profile's directory and respect the
        // new profile's per-category sync toggles.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        if let Some(ref handle) = self.updater_handle {
            use zengeld_updater::UpdaterCommand;
            let new_data_dir = zengeld_chart::active_profile_data_dir();
            let _ = handle.cmd_tx.send(UpdaterCommand::SetDataDir(new_data_dir));
            let _ = handle.cmd_tx.send(UpdaterCommand::SetProfileId(
                self.profile_manager.profile.profile_id.clone(),
            ));
            // Sync OTA/cloud mode from the new profile so the updater
            // starts or stops OTA checks accordingly.
            let _ = handle.cmd_tx.send(UpdaterCommand::SetCloudEnabled(
                self.profile_manager.profile.ota_enabled,
            ));
            let ss = &self.profile_manager.profile.sync_state;
            let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncEnabled(ss.enabled));
            let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncPresets(ss.sync_presets));
            let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncTemplates(ss.sync_templates));
            let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncWatchlists(ss.sync_watchlists));
            let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncTheme(ss.sync_theme));
            eprintln!("[App] sent SetDataDir + sync toggles + SetCloudEnabled to updater after profile switch");
        }

        eprintln!(
            "[App] hot-reload: profile switch complete — {} window(s) created",
            self.windows.len()
        );
    }

    /// Promote skeleton windows to live: drop all windows and recreate with `skeleton=false`
    /// so they actually fetch bars, connect to exchanges, subscribe to tickers, etc.
    ///
    /// Called after vault unlock, wizard completion, or fresh E2E setup.
    /// The profile is already fully loaded in memory (vault secrets decrypted, app_state rebuilt).
    fn promote_skeleton(&mut self, event_loop: &ActiveEventLoop) {
        eprintln!(
            "[App] skeleton → live: dropping {} skeleton window(s)",
            self.windows.len()
        );

        // Drop all skeleton windows.
        let window_ids: Vec<winit::window::WindowId> = self.windows.keys().copied().collect();
        for wid in window_ids {
            self.windows.remove(&wid);
        }
        self.last_focused = None;

        // Recreate from saved window state (or default).
        let saved = self.saved_windows.clone();
        if saved.is_empty() {
            self.create_window(event_loop, None, None);
        } else {
            for ws in &saved {
                self.create_window(event_loop, Some(ws), None);
            }
        }

        eprintln!(
            "[App] skeleton → live: {} live window(s) created",
            self.windows.len()
        );
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
        // 1. Autosave every window's active preset (skip skeleton windows).
        for pw in self.windows.values_mut() {
            if !pw.skeleton {
                pw.chart.autosave_snapshot();
            }
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

        // Sync profile_manager mutations back into self.profile before saving.
        // All toggle handlers (set_sync_enabled, set_ota_enabled, e2e salt, etc.)
        // write to profile_manager.profile, not self.profile. Without this copy
        // those changes would be lost on the next save_all() call.
        self.profile.sync_state = self.profile_manager.profile.sync_state.clone();
        self.profile.ota_enabled = self.profile_manager.profile.ota_enabled;
        self.profile.cloud_enabled = self.profile_manager.profile.cloud_enabled;

        let mut profile = self.profile.clone();
        profile.windows = window_states;

        // Use AppState as the canonical source for connector_enabled, theme, and
        // device identity (replaces per-window copies that were previously written here).
        profile.connector_enabled = self.app_state.connector_enabled.clone();
        profile.active_theme = self.app_state.theme_preset.clone();
        profile.device_name = self.app_state.device_name.clone();
        profile.app_version = self.app_state.app_version.clone();
        // recalc_mode is now persisted to DeviceSettings, not UserProfile.
        profile.scale_mode = match self.app_state.scale_mode {
            zengeld_chart::ScaleMode::Auto   => "Auto".to_string(),
            zengeld_chart::ScaleMode::Focus  => "Focus".to_string(),
            zengeld_chart::ScaleMode::Manual => "Manual".to_string(),
        };
        profile.server_enabled = self.app_state.server_enabled;
        profile.server_port = self.app_state.server_port;
        // Persist the current key registry (managed via the REST API).
        // The legacy `legacy_single_agent_key` field is kept empty after migration so
        // we don't double-migrate on the next load.
        profile.local_agent_keys = self.app_state.local_agent_keys.clone();
        profile.legacy_single_agent_key = String::new();

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
        let vault_key = self.app_state.vault_key.as_ref();
        if let Err(e) = zengeld_chart::save_profile(&profile, vault_key) {
            eprintln!("[App] Failed to save profile: {}", e);
        } else {
            // Keep in-memory profile up to date.
            self.profile = profile;
        }

        // 6. Save watchlists.json from AppState (always plaintext — no vault key).
        {
            let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
            if let Err(e) = zengeld_chart::save_json(&watchlists_path, &self.app_state.watchlist_manager, None) {
                eprintln!("[App] Failed to save watchlists: {}", e);
            }
        }

        // 7. Save settings snapshots from AppState (always plaintext — no vault key).
        {
            let path = zengeld_chart::active_profile_data_dir().join("settings_snapshots.json");
            if let Err(e) = zengeld_chart::save_json(&path, &self.app_state.snapshots, None) {
                eprintln!("[App] Failed to save settings snapshots: {}", e);
            }
        }

        // 8. Save templates from AppState (single canonical source of truth).
        if let Err(e) = self.app_state.template_manager.save_to_default_dir(vault_key) {
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
            if let Err(e) = zengeld_chart::preset::storage::save_preset(preset, vault_key) {
                eprintln!("[App] failed to save preset {}: {}", preset.id, e);
            }
        }

        // 10. Flush bar cache to disk.
        eprintln!("[App] save_all: flushing {} bar series to disk", self.bar_service.series_count());
        self.bar_service.flush_dirty();
        self.bar_service.flush_sync();
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
                    window_id, chart_id, symbol, exchange, timeframe, account_type,
                } => {
                    eprintln!(
                        "[AgentCommand] SwitchSymbol: window={}, chart={}, symbol={}/{}/{} acct={}",
                        window_id, chart_id, exchange, symbol, timeframe, account_type,
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
        let server_keys = agent_state.list_local_keys();
        self.app_state.local_agent_keys = server_keys.iter().map(|k| {
            zengeld_chart::StoredLocalAgentKey {
                key_hash: k.key_hash.clone(),
                label: k.label.clone(),
                tier: k.tier.clone(),
                created_at: k.created_at,
                agent_id: k.agent_id.clone(),
                source: match k.source {
                    zengeld_server::state::AgentKeySource::Cloud => "cloud".to_string(),
                    zengeld_server::state::AgentKeySource::Local => "local".to_string(),
                },
            }
        }).collect();
        // Persist profile with updated keys
        let mut profile = self.profile.clone();
        profile.local_agent_keys = self.app_state.local_agent_keys.clone();
        profile.legacy_single_agent_key = String::new();
        let vault_key = self.app_state.vault_key.as_ref();
        if let Err(e) = zengeld_chart::save_profile(&profile, vault_key) {
            eprintln!("[App] Failed to persist keys: {}", e);
        } else {
            self.profile = profile;
            eprintln!("[App] Keys persisted to profile ({} keys)", self.app_state.local_agent_keys.len());
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

        let skeleton = self.needs_vault_unlock || self.is_first_run || self.needs_migration;
        if skeleton {
            // Skeleton = one default-sized window centered by OS, no profile geometry.
            eprintln!("[App] Skeleton mode — creating single loading window");
            self.create_window(event_loop, None, None);
        } else if self.saved_windows.is_empty() {
            eprintln!("[App] No saved windows — creating default");
            self.create_window(event_loop, None, None);
        } else {
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
        // ── Agents panel: drain PTY events (non-blocking, before FPS cap) ───
        // PTY data arrives on a background broadcast channel. We drain it
        // every wake-up so bytes don't pile up, but rendering is still
        // governed by the global FPS cap below — no bypass needed.
        for pw in self.windows.values_mut() {
            if pw.chart.sidebar_state.is_right_open()
                && pw.chart.sidebar_state.right_panel
                    == sidebar_content::state::RightSidebarPanel::Agents
            {
                if pw.chart.agent.drain_events() {
                    pw.chart.sidebar_data_dirty = true;
                    pw.sidebar_dirty_scene = true;
                    pw.window.request_redraw();
                }
            }
        }

        // ── FPS cap guard — must be the very first check ─────────────────────
        // winit wakes the event loop on every mouse event (CursorMoved at
        // 125-500 Hz), which preempts the WaitUntil timer set at the end of
        // this method.  We exit early here so no scene work or GPU submission
        // happens until the target frame interval has actually elapsed.
        if self.fps_limit > 0 {
            let target_dt = std::time::Duration::from_secs_f64(1.0 / self.fps_limit as f64);
            if self.last_frame_instant.elapsed() < target_dt {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    self.last_frame_instant + target_dt,
                ));
                return;
            }
        }

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

        // ── OTA updater status check ─────────────────────────────────────────
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        if let Some(ref mut handle) = self.updater_handle {
            if handle.status_rx.has_changed().unwrap_or(false) {
                let status = handle.status_rx.borrow_and_update().clone();
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
                    zengeld_updater::UpdateStatus::RestartPending => {
                        eprintln!("[Updater] RestartPending — saving all state before restart");
                        // CRITICAL: save_all() MUST run before process::exit()
                        // so that agents, presets, and profile are persisted.
                        // Previously spawn_and_exit() was called from the async
                        // updater task, bypassing save_all() entirely and causing
                        // data loss (agent leaves, slot layouts, etc.).
                    }
                    zengeld_updater::UpdateStatus::Error(e) => {
                        eprintln!("[Updater] Error: {}", e);
                    }
                    _ => {}
                }
            }
        }

        // ── OTA restart: save state then spawn new process ──────────────
        // Runs AFTER the status_rx borrow is released to avoid holding
        // an immutable ref to updater_handle during save_all (which needs &mut self).
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        {
            let restart_pending = self.updater_handle.as_ref()
                .map(|h| matches!(*h.status_rx.borrow(), zengeld_updater::UpdateStatus::RestartPending))
                .unwrap_or(false);
            if restart_pending {
                self.save_all(&[]);
                eprintln!("[Updater] State saved — spawning new process");
                let port = self.app_state.server_port;
                if let Err(e) = zengeld_updater::replace::spawn_and_exit(Some(port)) {
                    eprintln!("[Updater] spawn_and_exit failed: {} — continuing", e);
                }
                // spawn_and_exit calls process::exit(0), so we only reach here on error.
            }
        }

        // ── App-level tick ────────────────────────────────────────────────
        // Process app-level broadcast messages (ConnectorReady → request_symbols).
        self.tick_app_state();
        let _t1 = std::time::Instant::now();

        // ── Periodic bar cache save (every 5 minutes) ─────────────────────────
        if self.last_bar_cache_save.elapsed() >= std::time::Duration::from_secs(300) {
            self.last_bar_cache_save = std::time::Instant::now();
            // BarService already has all data from tick() events — just flush.
            self.bar_service.flush_dirty();
        }

        // ── Event-driven bar cache flush (backfill / scroll) ─────────────────
        // BarService tracks dirty state internally via merge_rest_batch /
        // apply_trade.  Also check per-window flag for backward compat
        // (BackfillComplete / ScrollBarsLoaded set it).
        let any_bars_dirty = self.bar_service.has_any_dirty()
            || self.windows.values().any(|pw| pw.chart.bars_cache_dirty);
        if any_bars_dirty {
            for pw in self.windows.values_mut() {
                pw.chart.bars_cache_dirty = false;
            }
            self.bar_service.flush_dirty();
        }

        // ── Periodic bar store cleanup (every hour) ───────────────────────────
        if self.last_cleanup_check.elapsed() >= std::time::Duration::from_secs(3600) {
            self.last_cleanup_check = std::time::Instant::now();
            let bars_dir = self.bar_service.bars_dir().to_path_buf();
            let max_size_mb = self.profile.data_load.max_store_size_mb;
            let cleanup_days = self.profile.data_load.store_cleanup_days;
            std::thread::spawn(move || {
                let cleanup = bar_store::BarStoreCleanup::new(bars_dir);
                cleanup.run_cleanup(max_size_mb, cleanup_days);
            });
        }

        // ── Accumulator for event-driven sync push ───────────────────────────
        // Collects blob categories that were flushed to disk this frame.  A
        // single SyncPushChanged command is sent after all flush blocks so the
        // updater can immediately push changed items to the server.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        let mut sync_changed_categories: Vec<String> = Vec::new();

        // ── Drain watchlist actions from all windows → AppState ────────────
        // Windows queue WatchlistAction instead of mutating directly.
        // App applies them to the single AppState watchlist.
        let mut watchlist_had_actions = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.watchlist_actions.drain(..) {
                watchlist_had_actions = true;
                match action {
                    chart_app::WatchlistAction::Toggle { symbol, exchange, account_type } => {
                        let now_in = self.app_state.watchlist_manager.toggle_symbol(&symbol, &exchange, &account_type);
                        eprintln!("[App] watchlist toggle: {}:{}:{} -> in_watchlist={}", symbol, exchange, account_type, now_in);
                        if now_in {
                            if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                                let enabled = self.app_state.connector_enabled
                                    .get(eid.as_str()).copied().unwrap_or(true);
                                if enabled {
                                    let at = chart_app::account_type_from_label(&account_type);
                                    self.bridge.ensure_connector(eid);
                                    self.bridge.subscribe_mini_ticker(eid, &symbol, at);
                                }
                            }
                            if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                                list.order_snapshot = None;
                            }
                        } else {
                            if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                                let at = chart_app::account_type_from_label(&account_type);
                                self.bridge.unsubscribe_mini_ticker(eid, &symbol, at);
                            }
                            if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                                if let Some(ref mut snap) = list.order_snapshot {
                                    snap.retain(|s| s.symbol != symbol);
                                }
                            }
                        }
                    }
                    chart_app::WatchlistAction::Remove { symbol, exchange, account_type } => {
                        self.app_state.watchlist_manager.remove_symbol(&symbol, &exchange, &account_type);
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
                    chart_app::WatchlistAction::SetColorFlag { symbol, exchange, account_type, color } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            let color_str = color.as_deref().unwrap_or("");
                            list.set_color_flag(&symbol, &exchange, &account_type, color_str);
                        }
                    }
                    chart_app::WatchlistAction::MoveToGroup { symbol, exchange, account_type, group_name } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            if let Some(group) = list.groups.iter().find(|g| g.name == group_name) {
                                let group_id = group.id;
                                list.move_to_group(&symbol, &exchange, &account_type, group_id);
                            }
                        }
                    }
                    chart_app::WatchlistAction::RemoveFromGroup { symbol, exchange, account_type } => {
                        if let Some(list) = self.app_state.watchlist_manager.active_list_mut() {
                            let mut found: Option<sidebar_content::watchlist::WatchlistSymbol> = None;
                            for g in &mut list.groups {
                                if let Some(pos) = g.symbols.iter().position(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == account_type) {
                                    found = Some(g.symbols.remove(pos));
                                    break;
                                }
                            }
                            if let Some(ws) = found {
                                list.ungrouped.push(ws);
                            }
                        }
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
                            // Count visible columns to get correct separator count.
                            let n_cols = {
                                let c = &list.column_config;
                                let mut n: usize = 1; // Symbol always on
                                if c.show_exchange     { n += 1; }
                                if c.show_account_type { n += 1; }
                                if c.show_last_price   { n += 1; }
                                if c.show_change_pct   { n += 1; }
                                if c.show_change_abs   { n += 1; }
                                if c.show_high_low     { n += 2; }
                                if c.show_volume       { n += 1; }
                                n
                            };
                            let n_seps = n_cols.saturating_sub(1);

                            let needs_init = list.column_config.separator_offsets
                                .as_ref()
                                .map(|o| o.len() != n_seps)
                                .unwrap_or(true);
                            if needs_init {
                                list.column_config.separator_offsets = Some(vec![0.0; n_seps]);
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
                                "exchange"     => list.column_config.show_exchange     = !list.column_config.show_exchange,
                                "last_price"   => list.column_config.show_last_price   = !list.column_config.show_last_price,
                                "change_pct"   => list.column_config.show_change_pct   = !list.column_config.show_change_pct,
                                "change_abs"   => list.column_config.show_change_abs   = !list.column_config.show_change_abs,
                                "volume"       => list.column_config.show_volume       = !list.column_config.show_volume,
                                "high_low"     => list.column_config.show_high_low     = !list.column_config.show_high_low,
                                "account_type" => list.column_config.show_account_type = !list.column_config.show_account_type,
                                "align_columns" => {
                                    list.column_config.align_columns = !list.column_config.align_columns;
                                    if list.column_config.align_columns {
                                        list.column_config.separator_offsets = None;
                                    }
                                }
                                _ => {}
                            }
                            // Reset separator offsets when column visibility changes
                            // (but not for align_columns which manages its own offsets).
                            if column != "align_columns" {
                                list.column_config.separator_offsets = None;
                            }
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
                    chart_app::SnapshotAction::DrawingStyle { type_id, data } => {
                        self.app_state.snapshots.last_used_drawing_styles.insert(type_id, data);
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
            #[cfg(all(feature = "updater", not(feature = "standalone")))]
            {
                sync_changed_categories.push("template_primitive".to_string());
                sync_changed_categories.push("template_indicator".to_string());
            }
        }

        // ── Drain performance actions from all windows ─────────────────────
        let mut reset_instanced_renderer = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.perf_actions.drain(..) {
                match action {
                    chart_app::PerfAction::SetFpsLimit(v) => {
                        self.fps_limit = v;
                        let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                        ds.fps_limit = v;
                        ds.save();
                        eprintln!("[App] FPS limit → {}", v);
                    }
                    chart_app::PerfAction::SetMsaa(v) => {
                        self.msaa_samples = v;
                        let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                        ds.msaa_samples = v;
                        ds.save();
                        eprintln!("[App] MSAA → {}", v);
                    }
                    chart_app::PerfAction::SetRecalcMode(ref mode) => {
                        self.app_state.recalc_mode = match mode.as_str() {
                            "PerTick" => chart_app::RecalcMode::PerTick,
                            "PerBar"  => chart_app::RecalcMode::PerBar,
                            _         => chart_app::RecalcMode::PerFrame,
                        };
                        let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                        ds.recalc_mode = mode.clone();
                        ds.save();
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
                        // Persist to device_settings.json
                        {
                            let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                            ds.render_backend = Some(match new_backend {
                                RenderBackend::VelloGpu => zengeld_chart::user_profile::device_settings::RenderBackend::VelloGpu,
                                RenderBackend::InstancedWgpu => zengeld_chart::user_profile::device_settings::RenderBackend::InstancedWgpu,
                                RenderBackend::VelloCpu => zengeld_chart::user_profile::device_settings::RenderBackend::VelloCpu,
                                RenderBackend::VelloHybrid => zengeld_chart::user_profile::device_settings::RenderBackend::VelloHybrid,
                                RenderBackend::TinySkia => zengeld_chart::user_profile::device_settings::RenderBackend::TinySkia,
                            });
                            ds.save();
                            self.backend_auto_detect = false;
                        }
                    }
                    chart_app::PerfAction::ToggleVsync => {
                        // VSync is hardcoded to AutoNoVsync in the surface config — no-op.
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
        // Presets are plaintext — always safe to save.
        if !self.app_state.preset_dirty_ids.is_empty() {
            let ids: Vec<String> = self.app_state.preset_dirty_ids.drain().collect();
            let vault_key = self.app_state.vault_key.as_ref();
            for id in ids {
                if let Some(preset) = self.app_state.presets.get(&id) {
                    if let Err(e) = zengeld_chart::preset::storage::save_preset(preset, vault_key) {
                        eprintln!("[App] failed to save preset {}: {}", id, e);
                    }
                }
            }
            #[cfg(all(feature = "updater", not(feature = "standalone")))]
            sync_changed_categories.push("preset".to_string());
        }

        // ── Dirty-flag persistence ──────────────────────────────────────
        // profile.json and watchlists.json are always plaintext — they can
        // be written regardless of vault lock status.
        // Only preset/template writes require the vault key (guarded above).
        {
            let any_profile_dirty = self.windows.values().any(|pw| pw.chart.profile_dirty);
            let any_geometry_dirty = self.windows.values().any(|pw| pw.chart.profile_geometry_dirty);
            let any_watchlists_dirty = self.windows.values().any(|pw| pw.chart.watchlists_dirty);

            if any_profile_dirty {
                // Autosave every window's active preset first (skip skeleton).
                for pw in self.windows.values_mut() {
                    if !pw.skeleton {
                        pw.chart.autosave_snapshot();
                    }
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
                // recalc_mode is now persisted to DeviceSettings, not UserProfile.

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

                let vault_key = self.app_state.vault_key.as_ref();
                if let Err(e) = zengeld_chart::save_profile(&profile, vault_key) {
                    eprintln!("[App] Failed to save profile: {}", e);
                } else {
                    self.profile = profile;
                    #[cfg(all(feature = "updater", not(feature = "standalone")))]
                    sync_changed_categories.push("profile".to_string());
                }

                // Clear dirty flags.
                for pw in self.windows.values_mut() {
                    pw.chart.profile_dirty = false;
                }
            }

            // Geometry-only changes: save locally but skip cloud sync.
            // This handles window move/resize — position persists across restarts
            // but doesn't flood the sync pipeline (~180 events per 3s drag).
            if any_geometry_dirty && !any_profile_dirty {
                // Sync OS position/size into ChartApp fields.
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

                let window_states: Vec<zengeld_chart::WindowState> = self.windows.values()
                    .map(|pw| pw.chart.build_window_state())
                    .collect();

                let preferred_key = self.last_focused
                    .filter(|id| self.windows.contains_key(id))
                    .or_else(|| self.windows.keys().next().copied());

                let mut profile = self.profile.clone();
                profile.windows = window_states;
                profile.connector_enabled = self.app_state.connector_enabled.clone();
                profile.active_theme = self.app_state.theme_preset.clone();
                profile.device_name = self.app_state.device_name.clone();
                profile.app_version = self.app_state.app_version.clone();
                // recalc_mode is now persisted to DeviceSettings, not UserProfile.

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

                let vault_key = self.app_state.vault_key.as_ref();
                if let Err(e) = zengeld_chart::save_profile(&profile, vault_key) {
                    eprintln!("[App] Failed to save profile (geometry): {}", e);
                } else {
                    self.profile = profile;
                    // NOTE: No sync_changed_categories push — geometry is local-only.
                }
            }

            // Clear geometry dirty flags (always, whether or not content was also dirty).
            if any_geometry_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.profile_geometry_dirty = false;
                }
            }

            if any_watchlists_dirty {
                let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
                // Watchlists are always plaintext — pass None regardless of vault key.
                if let Err(e) = zengeld_chart::save_json(&watchlists_path, &self.app_state.watchlist_manager, None) {
                    eprintln!("[App] Failed to save watchlists: {}", e);
                } else {
                    #[cfg(all(feature = "updater", not(feature = "standalone")))]
                    sync_changed_categories.push("watchlist".to_string());
                }
                // Clear dirty flags.
                for pw in self.windows.values_mut() {
                    pw.chart.watchlists_dirty = false;
                }
            }
        }

        // ── Second-pass: autosave_snapshot() inside profile_dirty may have
        //    pushed new PresetAction::Upsert that missed the first drain.
        //    Drain + flush them now so they don't wait until next frame. ──────
        {
            let mut second_pass_dirty = false;
            for pw in self.windows.values_mut() {
                for action in pw.chart.preset_actions.drain(..) {
                    match action {
                        chart_app::PresetAction::Upsert(preset) => {
                            let id = preset.id.clone();
                            self.app_state.presets.insert(id.clone(), preset);
                            self.app_state.preset_dirty_ids.insert(id);
                            second_pass_dirty = true;
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
                                second_pass_dirty = true;
                            }
                        }
                    }
                }
            }
            if second_pass_dirty && !self.app_state.preset_dirty_ids.is_empty() {
                let ids: Vec<String> = self.app_state.preset_dirty_ids.drain().collect();
                let vault_key = self.app_state.vault_key.as_ref();
                for id in ids {
                    if let Some(preset) = self.app_state.presets.get(&id) {
                        if let Err(e) = zengeld_chart::preset::storage::save_preset(preset, vault_key) {
                            eprintln!("[App] failed to save preset {}: {}", id, e);
                        }
                    }
                }
                #[cfg(all(feature = "updater", not(feature = "standalone")))]
                sync_changed_categories.push("preset".to_string());
            }
        }

        // ── Event-driven sync push ────────────────────────────────────────────
        // Fire one SyncPushChanged after all dirty-flag flushes so the updater
        // can immediately push to the server rather than waiting for the 5-min
        // interval tick.  The updater's do_cloud_sync reads all files and dedupes
        // via checksums — no extra work is done for unchanged items.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        if !sync_changed_categories.is_empty() {
            if let Some(ref handle) = self.updater_handle {
                let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::SyncPushChanged(sync_changed_categories));
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
            // Shutdown the updater background task cleanly.
            #[cfg(all(feature = "updater", not(feature = "standalone")))]
            if let Some(ref handle) = self.updater_handle {
                let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::Shutdown);
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
                // Autosave closing windows before removal (skip skeleton).
                for (wid, _) in &windows_to_close {
                    if let Some(pw) = self.windows.get_mut(wid) {
                        if !pw.skeleton { pw.chart.autosave_snapshot(); }
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
                // Persist to DeviceSettings so the choice survives restarts.
                let mut ds = zengeld_chart::user_profile::DeviceSettings::load();
                ds.recalc_mode = mode_str.clone();
                ds.save();
                eprintln!("[App] recalc_mode changed to: {}", mode_str);
            }
        }

        // ── Drain language changes → profile + all windows ───────────────
        {
            let new_lang: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.language_changed.take());
            if let Some(ref lang_code) = new_lang {
                // Propagate to all windows
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_settings_state.language = lang_code.clone();
                }
                // Save to active profile
                self.profile.language = lang_code.clone();
                self.profile_manager.profile.language = lang_code.clone();
                eprintln!("[App] language changed to: {}", lang_code);
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

        // ── Drain local_agent_key changes (legacy single-key hot-reload) ─────────
        // When the UI Regenerate button creates a new master key, register it
        // as an admin key in AgentState and persist immediately.
        {
            let key_change: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.local_agent_key_changed.take());
            if let Some(raw_key) = key_change {
                if !raw_key.is_empty() {
                    if let Some(agent_state) = self.agent_state.clone() {
                        // Remove any previous "master" key, then add the new one
                        agent_state.remove_key("master");
                        let key_hash = zengeld_server::state::hash_agent_key(&raw_key);
                        let entry = zengeld_server::state::LocalAgentKey {
                            key_hash,
                            label: "master".to_string(),
                            tier: "admin".to_string(),
                            permissions: zengeld_server::state::Permissions::admin(),
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            agent_id: None,
                            source: zengeld_server::state::AgentKeySource::Local,
                        };
                        agent_state.add_local_key(entry);
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
                // ClientMode is immutable — set at profile creation in the wizard.
                // set_connected/set_standalone only affect the updater's sync behavior,
                // NOT the profile's mode.

                // ── Profile commands (all build configs) ─────────────────────
                if let Some(rest) = cmd_str.strip_prefix("profile_rename:") {
                    // Format: "profile_rename:{id}:{new_name}"
                    let (target_id, new_name) = if let Some(idx) = rest.find(':') {
                        (rest[..idx].to_string(), rest[idx + 1..].to_string())
                    } else {
                        // Fallback for old format "profile_rename:{name}"
                        (self.profile_manager.profile.profile_id.clone(), rest.to_string())
                    };
                    let is_active = target_id == self.profile_manager.profile.profile_id;
                    match self.profile_manager.rename_profile(&target_id, &new_name) {
                        Ok(()) => {
                            if is_active {
                                self.profile.display_name = new_name.clone();
                                for pw in self.windows.values_mut() {
                                    pw.chart.panel_app.user_settings_state.profile_display_name =
                                        new_name.clone();
                                }
                            }
                            self.sync_profiles_to_windows();
                            eprintln!("[App] profile renamed: id={} name={}", target_id, new_name);
                        }
                        Err(e) => eprintln!("[App] profile_rename failed: {}", e),
                    }
                } else if let Some(avatar) = cmd_str.strip_prefix("profile_set_avatar:") {
                    let active_id = self.profile_manager.profile.profile_id.clone();
                    match self.profile_manager.set_avatar(&active_id, avatar) {
                        Ok(()) => {
                            self.profile.avatar = avatar.to_string();
                            self.sync_profiles_to_windows();
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.profile_avatar =
                                    avatar.to_string();
                            }
                            eprintln!("[App] profile avatar set to: {}", avatar);
                        }
                        Err(e) => eprintln!("[App] profile_set_avatar failed: {}", e),
                    }
                } else if let Some(rest) = cmd_str.strip_prefix("profile_create:") {
                    // Format: "profile_create:{name}" — cloud_enabled is always false at creation.
                    let name_opt = if rest.trim().is_empty() { None } else { Some(rest) };
                    match self.profile_manager.create_profile(name_opt, "chart") {
                        Ok(meta) => {
                            eprintln!(
                                "[App] profile created: {} ({})",
                                meta.display_name, meta.id
                            );
                            self.sync_profiles_to_windows();
                            // Show passphrase setup in the current window — no profile switch yet.
                            // The switch happens only after the user successfully creates the vault.
                            use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                            self.pending_new_profile_id = Some(meta.id.clone());
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                                pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                                pw.chart.panel_app.user_settings_state.profile_manager_target_id = meta.id.clone();
                                pw.chart.panel_app.user_settings_state.profile_manager_target_name = meta.display_name.clone();
                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                            }
                        }
                        Err(e) => eprintln!("[App] profile_create failed: {}", e),
                    }
                } else if let Some(id) = cmd_str.strip_prefix("profile_switch:") {
                    // Guard: if the requested profile is already the running one, do nothing.
                    if id == self.profile_manager.profile.profile_id.as_str() {
                        eprintln!("[App] profile_switch: already on this profile, ignoring");
                    } else {
                        let exists = self.profile_manager.available_profiles().iter().any(|p| p.id == id);
                        if exists {
                            eprintln!("[App] profile_switch: scheduling hot-reload for profile {}", id);
                            self.pending_profile_switch = Some(id.to_string());
                        } else {
                            eprintln!("[App] profile_switch: profile {} not found", id);
                        }
                    }
                } else if let Some(id) = cmd_str.strip_prefix("profile_delete:") {
                    match self.profile_manager.delete_profile(id) {
                        Ok(()) => {
                            self.sync_profiles_to_windows();
                            eprintln!("[App] profile deleted: {}", id);
                        }
                        Err(e) => eprintln!("[App] profile_delete failed: {}", e),
                    }
                }

                // ── Wizard complete: name + passphrase in a single command ──
                // Format: "wizard_complete:{name}:{passphrase}"
                if let Some(rest) = cmd_str.strip_prefix("wizard_complete:") {
                    let (wizard_profile_name, passphrase) = if let Some(colon) = rest.find(':') {
                        (rest[..colon].to_string(), &rest[colon + 1..])
                    } else {
                        // Legacy format without name — treat entire remainder as passphrase.
                        ("Default".to_string(), rest)
                    };
                    if !self.is_first_run {
                        // Creating a NEW profile from settings wizard.
                        // The current profile is immutable — create a fresh one.
                        match self.profile_manager.create_profile(None, "chart") {
                            Ok(meta) => {
                                eprintln!(
                                    "[App] wizard_complete: created new profile '{}' ({})",
                                    wizard_profile_name, meta.id
                                );
                                // Apply the user-chosen name immediately.
                                let _ = self.profile_manager.rename_profile(&meta.id, &wizard_profile_name);
                                // Switch active profile to the new one.
                                if let Some(mut index) = zengeld_chart::load_profile_index() {
                                    index.active_profile_id = meta.id.clone();
                                    let _ = zengeld_chart::save_profile_index(&index);
                                }
                                // Reload ProfileManager from the new empty profile directory.
                                self.profile_manager = zengeld_chart::ProfileManager::load(None);
                                self.profile = self.profile_manager.profile.clone();
                                // Clear presets/templates for fresh profile.
                                self.app_state.presets.clear();
                                self.app_state.template_manager =
                                    self.profile_manager.template_manager.clone();
                            }
                            Err(e) => {
                                eprintln!("[App] wizard_complete: failed to create profile: {}", e);
                                // Fall through — configure existing profile as fallback.
                            }
                        }
                    }
                    // First run — no profile creation needed, configure in-place.
                    // Apply the user-chosen profile name.
                    let active_id = self.profile_manager.profile.profile_id.clone();
                    let _ = self.profile_manager.rename_profile(&active_id, &wizard_profile_name);
                    self.profile.display_name = wizard_profile_name.clone();
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.user_settings_state.profile_display_name =
                            wizard_profile_name.clone();
                    }

                    // Derive vault key via ProfileManager and sync to app_state.
                    match self.profile_manager.derive_and_set_vault_key(passphrase) {
                        Ok(key) => {
                            self.app_state.vault_key = Some(key);
                            self.profile_manager.vault_key = Some(key);
                            self.app_state.template_manager.vault_key = Some(key);
                            eprintln!("[App] wizard_complete: vault key derived, saving");
                        }
                        Err(e) => {
                            eprintln!("[App] wizard_complete: failed to derive vault key: {}", e);
                        }
                    }

                    // Save profile + vault.
                    self.save_all(&[]);
                    self.is_first_run = false;
                    self.needs_vault_unlock = false;
                    self.needs_migration = false;

                    // If a recovery key was generated, show it inside the wizard (page 3).
                    let recovery_key = self.profile_manager.pending_recovery_key.clone();
                    if recovery_key.is_some() {
                        // Stay in the wizard at page 3 so the user sees the key before closing.
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                            pw.chart.panel_app.user_settings_state.recovery_key_display =
                                recovery_key.clone();
                            // Sync read-only display editing state so it can be selected/copied.
                            let key_str = recovery_key.as_deref().unwrap_or("");
                            pw.chart.panel_app.user_settings_state.recovery_key_display_editing.text = key_str.to_string();
                            pw.chart.panel_app.user_settings_state.recovery_key_display_editing.cursor = key_str.chars().count();
                            pw.chart.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            // Navigate to wizard page 3 (recovery key) — wizard stays open
                            pw.chart.panel_app.user_settings_state.wizard_page = 3;
                            pw.chart.panel_app.user_settings_state.show_welcome_wizard = true;
                        }
                    } else {
                        // No recovery key — promote immediately.
                        eprintln!("[App] wizard_complete — promoting skeleton to live");
                        self.pending_skeleton_promote = true;
                    }
                }

                // ── Local vault key derivation (e2e_setup — all build configs) ──
                // This runs BEFORE the updater-handle block so that we can call
                // save_all() without holding an immutable borrow of updater_handle.
                if let Some(passphrase) = cmd_str.strip_prefix("e2e_setup:") {
                    // ── New profile vault creation ──
                    // When the user just created a new profile, we show the CreatePassphrase
                    // modal without switching profiles first.  Once the vault is created we
                    // trigger the profile switch so the user lands in the new profile.
                    if let Some(ref new_profile_id) = self.pending_new_profile_id.clone() {
                        let profiles_dir = zengeld_chart::active_profile_data_dir()
                            .parent()
                            .map(|p| p.to_path_buf());
                        if let Some(pd) = profiles_dir {
                            let target_meta = self.profile_manager.available_profiles()
                                .iter()
                                .find(|p| p.id == *new_profile_id)
                                .cloned();
                            if let Some(meta) = target_meta {
                                let target_dir = pd.join(&meta.dir_name);
                                match self.profile_manager.derive_and_set_vault_key_for_dir(passphrase, &target_dir) {
                                    Ok(key) => {
                                        eprintln!("[App] new profile vault created, switching to {}", new_profile_id);

                                        let recovery_key = self.profile_manager.pending_recovery_key.clone();
                                        if recovery_key.is_some() {
                                            // Show recovery key before completing the profile switch.
                                            self.pending_switch_after_recovery = Some((new_profile_id.clone(), key));
                                            self.pending_new_profile_id = None;
                                            for pw in self.windows.values_mut() {
                                                use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                                pw.chart.panel_app.user_settings_state.recovery_key_display =
                                                    recovery_key.clone();
                                                // Sync read-only display editing state so it can be selected/copied.
                                                let key_str = recovery_key.as_deref().unwrap_or("");
                                                pw.chart.panel_app.user_settings_state.recovery_key_display_editing.text = key_str.to_string();
                                                pw.chart.panel_app.user_settings_state.recovery_key_display_editing.cursor = key_str.chars().count();
                                                pw.chart.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                                                pw.chart.panel_app.user_settings_state.profile_manager_page =
                                                    ProfileManagerPage::ShowRecoveryKey;
                                                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                                            }
                                        } else {
                                            // No recovery key — switch immediately.
                                            self.pending_new_profile_id = None;
                                            self.pending_switch_vault_key = Some(key);
                                            self.pending_profile_switch = Some(new_profile_id.clone());
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                                pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[App] new profile: failed to create vault: {}", e);
                                        for pw in self.windows.values_mut() {
                                            pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                Some(format!("Failed to create vault: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                    } else
                    // ── Pre-switch validation: unlocking a DIFFERENT profile ──
                    // If the target profile differs from the running profile, validate
                    // the passphrase against the target's vault WITHOUT hot-reloading.
                    // Only switch after successful validation.
                    {
                    let target_id = self.windows.values().next()
                        .map(|pw| pw.chart.panel_app.user_settings_state.profile_manager_target_id.clone())
                        .unwrap_or_default();
                    let current_id = self.profile_manager.profile.profile_id.clone();
                    if !target_id.is_empty() && target_id != current_id {
                        // Find the target profile's data dir
                        let profiles_dir = zengeld_chart::active_profile_data_dir()
                            .parent()
                            .map(|p| p.to_path_buf());
                        if let Some(pd) = profiles_dir {
                            let target_meta = self.profile_manager.available_profiles()
                                .iter()
                                .find(|p| p.id == target_id)
                                .cloned();
                            if let Some(meta) = target_meta {
                                let target_dir = pd.join(&meta.dir_name);
                                let salt_path = target_dir.join("salt.hex");
                                let vault_path = target_dir.join("vault.enc");
                                if salt_path.exists() && vault_path.exists() {
                                    // Validate passphrase against the target vault
                                    match zengeld_chart::vault::validate_passphrase_at(&salt_path, &vault_path, passphrase) {
                                        Ok(key) => {
                                            eprintln!("[App] pre-switch: passphrase validated for profile {}", target_id);
                                            // Store the pre-validated key so execute_profile_switch
                                            // can inject it directly and skip the unlock screen.
                                            self.pending_switch_vault_key = Some(key);
                                            self.pending_profile_switch = Some(target_id);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                                pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                                                pw.chart.panel_app.user_settings_state.is_open = false;
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("[App] pre-switch: wrong passphrase for profile {} ({})", target_id, e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some("Wrong passphrase — please try again".to_string());
                                                pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                            }
                                        }
                                    }
                                } else {
                                    // Target profile has no vault — create vault with this passphrase
                                    eprintln!("[App] pre-switch: creating vault for unencrypted profile {}", target_id);
                                    if let Ok(salt) = zengeld_chart::vault::load_or_create_salt(&salt_path) {
                                        let key = zengeld_chart::vault::derive_key(passphrase, &salt);
                                        let empty_secrets = zengeld_chart::user_profile::VaultSecrets::default();
                                        if let Ok(()) = zengeld_chart::vault::save_encrypted(&key, &vault_path, &empty_secrets) {
                                            eprintln!("[App] pre-switch: vault created, switching to profile {}", target_id);
                                            // Store the derived key so execute_profile_switch
                                            // can inject it directly and skip the unlock screen.
                                            self.pending_switch_vault_key = Some(key);
                                            self.pending_profile_switch = Some(target_id);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                                                pw.chart.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                                pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                                                pw.chart.panel_app.user_settings_state.is_open = false;
                                            }
                                        } else {
                                            eprintln!("[App] pre-switch: failed to create vault.enc");
                                        }
                                    } else {
                                        eprintln!("[App] pre-switch: failed to create salt");
                                    }
                                }
                            }
                        }
                    } else if self.needs_vault_unlock {
                        match self.profile_manager.validate_passphrase(passphrase) {
                            Ok(key) => {
                                eprintln!("[App] vault passphrase validated OK — promoting skeleton");
                                self.profile_manager.set_vault_key(key);
                                if let Err(e) = self.profile_manager.load_vault_secrets() {
                                    eprintln!("[App] failed to load vault secrets: {}", e);
                                }
                                self.app_state.vault_key = Some(key);
                                self.profile_manager.vault_key = Some(key);
                                self.profile = self.profile_manager.profile.clone();
                                self.app_state.local_agent_keys =
                                    self.profile_manager.profile.local_agent_keys.clone();
                                self.needs_vault_unlock = false;
                                // Drop skeleton windows and recreate with live data.
                                self.pending_skeleton_promote = true;
                            }
                            Err(e) => {
                                eprintln!("[App] vault unlock REJECTED: wrong passphrase ({})", e);
                                for pw in self.windows.values_mut() {
                                    pw.chart.panel_app.user_settings_state.needs_vault_unlock = true;
                                    pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                        Some("Wrong passphrase — please try again".to_string());
                                    pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                }
                            }
                        }
                    } else {
                        // Fresh E2E setup — derive and store the vault key.
                        match self.profile_manager.derive_and_set_vault_key(passphrase) {
                            Ok(key) => {
                                self.app_state.vault_key = Some(key);
                                self.profile_manager.vault_key = Some(key);
                                self.app_state.template_manager.vault_key = Some(key);
                                eprintln!("[App] vault key derived and set — promoting skeleton");
                                self.app_state.local_agent_keys =
                                    self.profile_manager.profile.local_agent_keys.clone();
                                self.needs_vault_unlock = false;
                                self.needs_migration = false;

                                let recovery_key = self.profile_manager.pending_recovery_key.clone();
                                if recovery_key.is_some() {
                                    // Show recovery key on skeleton first; promote after user confirms.
                                    for pw in self.windows.values_mut() {
                                        use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                                        pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                                        pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                        pw.chart.panel_app.user_settings_state.recovery_key_display =
                                            recovery_key.clone();
                                        // Sync read-only display editing state so it can be selected/copied.
                                        let key_str = recovery_key.as_deref().unwrap_or("");
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.text = key_str.to_string();
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.cursor = key_str.chars().count();
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                                        pw.chart.panel_app.user_settings_state.profile_manager_page =
                                            ProfileManagerPage::ShowRecoveryKey;
                                        pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                                    }
                                } else {
                                    // No recovery key — promote immediately.
                                    self.pending_skeleton_promote = true;
                                }
                            }
                            Err(e) => eprintln!("[App] vault salt error: {}", e),
                        }
                    }
                    } // end else (not a new-profile vault creation)
                }

                // ── Recovery key unlock ──────────────────────────────────────────────────────
                // Emitted when the user enters a recovery key to restore vault access.
                if let Some(recovery_key_text) = cmd_str.strip_prefix("recovery_unlock:") {
                    let target_id = self.windows.values().next()
                        .map(|pw| pw.chart.panel_app.user_settings_state.profile_manager_target_id.clone())
                        .unwrap_or_default();
                    let current_id = self.profile_manager.profile.profile_id.clone();

                    // Determine which profile dir to use
                    let profile_dir = if !target_id.is_empty() && target_id != current_id && !self.needs_vault_unlock {
                        // Unlocking a different profile
                        let profiles_dir = zengeld_chart::active_profile_data_dir()
                            .parent()
                            .map(|p| p.to_path_buf());
                        profiles_dir.and_then(|pd| {
                            self.profile_manager.available_profiles()
                                .iter()
                                .find(|p| p.id == target_id)
                                .map(|meta| pd.join(&meta.dir_name))
                        })
                    } else {
                        // Unlocking the active profile (needs_vault_unlock)
                        Some(zengeld_chart::active_profile_data_dir())
                    };

                    if let Some(profile_dir) = profile_dir {
                        match zengeld_chart::crypto::parse_recovery_key(recovery_key_text) {
                            Ok(recovery_key_bytes) => {
                                let salt_path = profile_dir.join("salt.hex");
                                let recovery_enc_path = profile_dir.join("recovery_key.enc");

                                if !recovery_enc_path.exists() {
                                    eprintln!("[App] recovery_unlock: no recovery_key.enc found in {:?}", profile_dir);
                                    for pw in self.windows.values_mut() {
                                        pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                            Some("No recovery data found for this profile".to_string());
                                    }
                                } else {
                                    let salt_result = zengeld_chart::crypto::load_or_create_salt(&salt_path);
                                    let enc_blob = std::fs::read(&recovery_enc_path);

                                    match (salt_result, enc_blob) {
                                        (Ok(salt), Ok(blob)) => {
                                            match zengeld_chart::crypto::decrypt_master_key_with_recovery(
                                                &blob, &recovery_key_bytes, &salt,
                                            ) {
                                                Ok(master_key) => {
                                                    let vault_key = zengeld_chart::crypto::derive_vault_key(&master_key, &salt);
                                                    eprintln!("[App] recovery_unlock: master key recovered — vault key derived");

                                                    if self.needs_vault_unlock {
                                                        // Active profile unlock via recovery — vault is
                                                        // now readable but the user must set a new
                                                        // passphrase before we promote the skeleton.
                                                        self.profile_manager.set_vault_key(vault_key);
                                                        if let Err(e) = self.profile_manager.load_vault_secrets() {
                                                            eprintln!("[App] failed to load vault secrets: {}", e);
                                                        }
                                                        self.app_state.vault_key = Some(vault_key);
                                                        self.profile_manager.vault_key = Some(vault_key);
                                                        self.profile = self.profile_manager.profile.clone();
                                                        self.app_state.local_agent_keys =
                                                            self.profile_manager.profile.local_agent_keys.clone();
                                                        self.needs_vault_unlock = false;

                                                        // Store master_key so set_new_passphrase: can re-key the vault.
                                                        self.pending_recovery_master_key = Some(master_key);

                                                        // Navigate to the mandatory SetNewPassphrase page.
                                                        use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                                                        for pw in self.windows.values_mut() {
                                                            pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                                                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                            pw.chart.panel_app.user_settings_state.set_passphrase_error.clear();
                                                            pw.chart.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                                                            pw.chart.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                                                            pw.chart.panel_app.user_settings_state.confirm_passphrase_editing.text.clear();
                                                            pw.chart.panel_app.user_settings_state.confirm_passphrase_editing.cursor = 0;
                                                            pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
                                                            pw.chart.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                                            pw.chart.panel_app.user_settings_state.profile_manager_page =
                                                                ProfileManagerPage::SetNewPassphrase;
                                                        }
                                                        eprintln!("[App] recovery_unlock: navigating to SetNewPassphrase");
                                                    } else {
                                                        // Pre-switch recovery for a different profile
                                                        self.pending_switch_vault_key = Some(vault_key);
                                                        self.pending_profile_switch = Some(target_id.clone());
                                                        for pw in self.windows.values_mut() {
                                                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                            pw.chart.panel_app.user_settings_state.recovery_key_editing.text.clear();
                                                            pw.chart.panel_app.user_settings_state.recovery_key_editing.cursor = 0;
                                                            pw.chart.panel_app.user_settings_state.recovery_key_focused = false;
                                                            pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                                                            pw.chart.panel_app.user_settings_state.is_open = false;
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("[App] recovery_unlock: decryption failed: {}", e);
                                                    for pw in self.windows.values_mut() {
                                                        pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                            Some("Wrong recovery key — decryption failed".to_string());
                                                        pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                                    }
                                                }
                                            }
                                        }
                                        (Err(e), _) => {
                                            eprintln!("[App] recovery_unlock: failed to read salt: {}", e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some(format!("Failed to read vault data: {}", e));
                                            }
                                        }
                                        (_, Err(e)) => {
                                            eprintln!("[App] recovery_unlock: failed to read recovery_key.enc: {}", e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some(format!("Failed to read recovery data: {}", e));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[App] recovery_unlock: invalid recovery key format: {}", e);
                                for pw in self.windows.values_mut() {
                                    pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                        Some(format!("Invalid recovery key format: {}", e));
                                }
                            }
                        }
                    }
                }

                // ── Recovery key confirmed ───────────────────────────────────────────────────
                // Emitted when the user clicks "I have written it down" on the ShowRecoveryKey page.
                // Clears the pending recovery key from memory and completes the profile switch.
                if cmd_str == "recovery_key_confirmed" {
                    self.profile_manager.clear_pending_recovery_key();
                    if let Some((profile_id, vault_key)) = self.pending_switch_after_recovery.take() {
                        // Recovery key was shown for a NEW profile — complete the switch immediately.
                        // Sync level is no longer chosen in a wizard step; new profiles default to
                        // device-level OTA control.
                        eprintln!("[App] recovery key confirmed — completing switch for new profile {}", profile_id);
                        self.pending_switch_vault_key = Some(vault_key);
                        self.pending_profile_switch = Some(profile_id);
                    } else if self.pending_promote_after_recovery_key {
                        // Recovery key was shown after a vault re-key (post-recovery passphrase reset).
                        eprintln!("[App] recovery key confirmed (post re-key) — promoting skeleton to live");
                        self.pending_promote_after_recovery_key = false;
                        self.pending_skeleton_promote = true;
                    } else {
                        // Recovery key was shown during wizard / first-run — promote skeleton.
                        eprintln!("[App] recovery key confirmed — promoting skeleton to live");
                        // Don't patch windows — skeleton promote will recreate them.
                        self.pending_skeleton_promote = true;
                    }
                }

                // ── Set new passphrase after recovery key unlock ─────────────────────────────
                // Emitted by the SetNewPassphrase page after the user enters matching passphrases.
                // Re-keys the vault: new passphrase + same salt → new master_key + vault_key,
                // re-encrypts vault.enc, generates a fresh recovery key, then shows ShowRecoveryKey.
                if let Some(new_passphrase) = cmd_str.strip_prefix("set_new_passphrase:") {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;

                    match self.pending_recovery_master_key.take() {
                        None => {
                            // This should never happen — guard state is invalid.
                            eprintln!("[App] set_new_passphrase: no pending_recovery_master_key — ignoring");
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.set_passphrase_error =
                                    "Internal error: no recovery context. Please restart.".to_string();
                            }
                        }
                        Some(_old_master_key) => {
                            // Re-key the vault using the profile_manager helper.
                            match self.profile_manager.rekey_vault(new_passphrase) {
                                Ok(new_recovery_key_fmt) => {
                                    eprintln!("[App] set_new_passphrase: vault re-keyed successfully");

                                    // Update app-level vault key reference.
                                    if let Some(new_vk) = self.profile_manager.vault_key {
                                        self.app_state.vault_key = Some(new_vk);
                                    }

                                    // Mark that skeleton promote should happen after recovery key is confirmed.
                                    self.pending_promote_after_recovery_key = true;

                                    // Navigate to ShowRecoveryKey so user can record the new key.
                                    for pw in self.windows.values_mut() {
                                        pw.chart.panel_app.user_settings_state.recovery_key_display =
                                            Some(new_recovery_key_fmt.clone());
                                        // Sync read-only display editing state so it can be selected/copied.
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.text = new_recovery_key_fmt.clone();
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.cursor = new_recovery_key_fmt.chars().count();
                                        pw.chart.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                                        pw.chart.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                                        pw.chart.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                                        pw.chart.panel_app.user_settings_state.confirm_passphrase_editing.text.clear();
                                        pw.chart.panel_app.user_settings_state.confirm_passphrase_editing.cursor = 0;
                                        pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
                                        pw.chart.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                        pw.chart.panel_app.user_settings_state.set_passphrase_error.clear();
                                        pw.chart.panel_app.user_settings_state.profile_manager_page =
                                            ProfileManagerPage::ShowRecoveryKey;
                                    }

                                    // ── Upload re-keyed vault params to server ──
                                    let salt_path = zengeld_chart::active_profile_data_dir().join("salt.hex");
                                    let vault_salt = std::fs::read_to_string(&salt_path)
                                        .unwrap_or_default()
                                        .trim()
                                        .to_string();

                                    if !vault_salt.is_empty() {
                                        let token = zengeld_updater::token_store::load_token();
                                        if let Some(tok) = token.filter(|_| self.profile.cloud_enabled) {
                                            let client = reqwest::Client::builder()
                                                .timeout(std::time::Duration::from_secs(15))
                                                .build()
                                                .unwrap_or_default();
                                            let server_url = "https://chart.example".to_string();
                                            let token_str = tok.token.clone();
                                            let salt_hex_for_spawn = vault_salt;

                                            let encrypted_master_key_for_spawn =
                                                self.profile_manager.take_encrypted_master_key();

                                            let build_attest_for_spawn = zengeld_updater::BuildAttestation {
                                                attestation: env!("BUILD_ATTESTATION").to_string(),
                                                version: env!("CARGO_PKG_VERSION").to_string(),
                                                platform: env!("BUILD_PLATFORM").to_string(),
                                                timestamp: env!("BUILD_TIMESTAMP").to_string(),
                                            };
                                            let profile_id_for_spawn = self.profile_manager.profile.profile_id.clone();
                                            let device_id_for_spawn = zengeld_updater::telemetry::get_or_create_device_id();
                                            let iterations = zengeld_updater::vault_params::PBKDF2_ITERATIONS as i32;
                                            self.bridge.runtime().spawn(async move {
                                                match zengeld_updater::vault_params::upload_vault_params(
                                                    &client,
                                                    &server_url,
                                                    &token_str,
                                                    &salt_hex_for_spawn,
                                                    iterations,
                                                    encrypted_master_key_for_spawn.as_deref(),
                                                    &build_attest_for_spawn,
                                                    &profile_id_for_spawn,
                                                    &device_id_for_spawn,
                                                )
                                                .await {
                                                    Ok(_) => eprintln!("[App] Re-keyed vault params uploaded to server"),
                                                    Err(e) => eprintln!("[App] Re-keyed vault params upload failed: {}", e),
                                                }
                                            });
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[App] set_new_passphrase: rekey_vault failed: {}", e);
                                    for pw in self.windows.values_mut() {
                                        pw.chart.panel_app.user_settings_state.set_passphrase_error =
                                            format!("Failed to re-key vault: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Vault unlock → new profile wizard ────────────────────────────────────────
                // Emitted by ChartApp when the user clicks "Forgot passphrase?" after 3 failures.
                // Dismiss the vault lock overlay and show the welcome wizard so the user can
                // create a fresh profile from scratch.
                if cmd_str == "vault_skip_to_wizard" {
                    // Create a brand-new profile so the wizard operates on fresh data,
                    // not the old encrypted profile the user can no longer unlock.
                    match self.profile_manager.create_profile(None, "chart") {
                        Ok(meta) => {
                            eprintln!(
                                "[App] vault_skip_to_wizard: created new profile {} ({})",
                                meta.display_name, meta.id
                            );
                            // Switch the active profile in the index so that
                            // active_profile_data_dir() resolves to the new empty directory.
                            if let Some(mut index) = zengeld_chart::load_profile_index() {
                                index.active_profile_id = meta.id.clone();
                                if let Err(e) = zengeld_chart::save_profile_index(&index) {
                                    eprintln!(
                                        "[App] vault_skip_to_wizard: failed to update index: {}",
                                        e
                                    );
                                }
                            }
                            // Reload ProfileManager from the new empty profile directory.
                            // No key — no encrypted data yet.
                            self.profile_manager = zengeld_chart::ProfileManager::load(None);
                            self.profile = self.profile_manager.profile.clone();
                            // Reset shared app state from the old profile.
                            self.app_state.vault_key = None;
                            self.app_state.presets.clear();
                            self.app_state.preset_dirty_ids.clear();
                            self.app_state.presets_dirty = true;
                        }
                        Err(e) => {
                            eprintln!(
                                "[App] vault_skip_to_wizard: failed to create new profile: {}",
                                e
                            );
                            // Proceed anyway — the wizard will at least let the user set a
                            // passphrase, even if it ends up re-encrypting the old profile.
                        }
                    }

                    self.needs_vault_unlock = false;
                    self.sync_profiles_to_windows();
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                        pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                        pw.chart.panel_app.user_settings_state.vault_unlock_attempts = 0;
                        pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                        pw.chart.panel_app.user_settings_state.show_welcome_wizard = true;
                        pw.chart.panel_app.user_settings_state.wizard_page = 0;
                        // Clear the passphrase field so the wizard starts clean.
                        pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.text.clear();
                        pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.cursor = 0;
                        pw.chart.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                        pw.chart.panel_app.user_settings_state.e2e_passphrase_focused = false;
                    }
                    // Treat this as first-run so wizard_complete configures the already-
                    // created profile in-place instead of creating a second duplicate.
                    self.is_first_run = true;
                    eprintln!("[App] vault_skip_to_wizard: dismissed vault lock, showing wizard on fresh profile");
                }

                // Flag set inside the `handle` borrow below; acted on after the borrow ends.
                #[cfg(all(feature = "updater", not(feature = "standalone")))]
                let mut sync_level_changed = false;

                #[cfg(all(feature = "updater", not(feature = "standalone")))]
                if let Some(ref handle) = self.updater_handle {
                    use zengeld_updater::UpdaterCommand;
                    let command = if cmd_str.starts_with("wizard_complete:") {
                        // Handled entirely in the wizard_complete block above; do not
                        // forward to the updater (it would log "unknown updater command").
                        None
                    } else if cmd_str.starts_with("profile_switch:")
                        || cmd_str == "vault_skip_to_wizard"
                        || cmd_str == "recovery_key_confirmed"
                        || cmd_str.starts_with("sync_level_chosen:")
                        || cmd_str.starts_with("recovery_unlock:")
                        || cmd_str.starts_with("set_new_passphrase:")
                        || cmd_str.starts_with("profile_create:")
                        || cmd_str.starts_with("profile_rename:")
                        || cmd_str == "profile_delete"
                        || cmd_str.starts_with("profile_avatar:")
                    {
                        // App-level profile commands — handled above, not updater commands.
                        None
                    } else if cmd_str.starts_with("e2e_setup:") && self.needs_vault_unlock {
                        // Vault unlock attempt with wrong passphrase — local handler already
                        // rejected it.  Do NOT forward to updater (would set up E2E on server
                        // with the wrong salt/key combo).
                        None
                    } else if cmd_str == "logout" {
                        Some(UpdaterCommand::Logout)
                    } else if let Some(provider) = cmd_str.strip_prefix("start_oauth:") {
                        Some(UpdaterCommand::StartOAuth(provider.to_string()))
                    } else if cmd_str == "set_connected"
                        || cmd_str == "set_connected_upload"
                        || cmd_str == "set_connected_download"
                    {
                        // All three connect variants enable Connected mode.
                        // Upload/download also trigger an immediate sync cycle so
                        // local data is pushed / server data is pulled right away.
                        if cmd_str == "set_connected_upload" || cmd_str == "set_connected_download" {
                            // Send SetConnectedMode first, then ForceSync.
                            // SetConnectedMode is sent below via the `command` variable;
                            // ForceSync is queued here as a second message.
                            let _ = handle.cmd_tx.send(UpdaterCommand::SetCloudEnabled(true));
                            let _ = handle.cmd_tx.send(UpdaterCommand::ForceSync);
                            // Skip the generic send below (already sent).
                            None
                        } else {
                            Some(UpdaterCommand::SetCloudEnabled(true))
                        }
                    } else if cmd_str == "device_ota:connected" {
                        // Device-level Connected mode toggle — enable network activity.
                        Some(UpdaterCommand::SetCloudEnabled(true))
                    } else if cmd_str == "device_ota:standalone" {
                        // Device-level Standalone mode toggle — disable all network activity.
                        Some(UpdaterCommand::SetCloudEnabled(false))
                    } else if let Some(ch) = cmd_str.strip_prefix("set_channel:") {
                        Some(UpdaterCommand::SetChannel(ch.to_string()))
                    } else if cmd_str == "set_standalone" {
                        Some(UpdaterCommand::SetCloudEnabled(false))
                    // ── Sync level commands ────────────────────────────────────
                    } else if let Some(level) = cmd_str.strip_prefix("set_sync_level:") {
                        let p = &mut self.profile_manager.profile;
                        let cloud = match level {
                            "local" => {
                                p.ota_enabled = false;
                                p.sync_state.enabled = false;
                                p.cloud_enabled = false;
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetCloudEnabled(false));
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncEnabled(false));
                                false
                            }
                            "connected" => {
                                p.ota_enabled = true;
                                p.sync_state.enabled = false;
                                p.cloud_enabled = false;
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetCloudEnabled(true));
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncEnabled(false));
                                false
                            }
                            "cloud" => {
                                p.ota_enabled = true;
                                p.sync_state.enabled = true;
                                p.sync_state.sync_presets = true;
                                p.sync_state.sync_templates = true;
                                p.sync_state.sync_watchlists = true;
                                p.sync_state.sync_theme = true;
                                // Cloud = always ZT: vault + recovery key always synced
                                p.sync_state.sync_vault = true;
                                p.sync_state.sync_recovery_key = true;
                                p.cloud_enabled = true;
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetCloudEnabled(true));
                                let _ = handle.cmd_tx.send(UpdaterCommand::SetSyncEnabled(true));
                                true
                            }
                            _ => {
                                eprintln!("[App] unknown sync_level: {}", level);
                                false
                            }
                        };
                        self.profile.cloud_enabled = cloud;
                        // Write sync_level into the profile itself (source of truth).
                        self.profile_manager.profile.sync_level = level.to_string();
                        let _ = zengeld_chart::set_profile_sync_level(&self.profile.profile_id, cloud, level);
                        eprintln!("[App] sync_level = {}", level);
                        // Persist immediately — OTA restart may kill the process before save_all().
                        if let Err(e) = self.profile_manager.save_profile() {
                            eprintln!("[App] failed to save profile after sync_level change: {}", e);
                        }
                        // Defer profile-list refresh to after the handle borrow ends
                        // (sync_profiles_to_windows needs &mut self which conflicts with
                        // the immutable `handle` borrow active through this entire block).
                        sync_level_changed = true;
                        None
                    } else if cmd_str == "force_sync" {
                        Some(UpdaterCommand::ForceSync)
                    } else if cmd_str == "start_device_auth" {
                        // OAuth device-link flow:
                        // 1. POST /api/auth/link/init  → get token + link_url
                        // 2. Open link_url in browser
                        // 3. Poll /api/auth/link/poll?token=... every 2 s
                        // 4. Send status updates via an mpsc channel to about_to_wait
                        eprintln!("[App] start_device_auth requested — starting link flow");

                        let device_id = zengeld_updater::telemetry::get_or_create_device_id();
                        let device_name = self.app_state.device_name.clone();
                        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<LinkPollStatus>();
                        self.link_poll_rx = Some(rx);

                        // Update wizard UI immediately to show "Connecting…"
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.wizard_linking_status =
                                "Connecting to server\u{2026}".to_string();
                        }

                        self.bridge.runtime().spawn(async move {
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(15))
                                .build()
                                .unwrap_or_default();

                            // Step 1: POST /api/auth/link/init
                            let init_resp = client
                                .post("https://chart.example/api/auth/link/init")
                                .json(&serde_json::json!({
                                    "device_name": device_name,
                                    "device_id": device_id,
                                }))
                                .send()
                                .await;

                            let (token, link_url) = match init_resp {
                                Ok(r) if r.status().is_success() => {
                                    match r.json::<serde_json::Value>().await {
                                        Ok(body) => {
                                            let tok = body["token"].as_str().unwrap_or("").to_string();
                                            let url = body["link_url"].as_str().unwrap_or("").to_string();
                                            if tok.is_empty() || url.is_empty() {
                                                let _ = tx.send(LinkPollStatus::Expired(
                                                    "Server returned empty token.".to_string(),
                                                ));
                                                return;
                                            }
                                            (tok, url)
                                        }
                                        Err(e) => {
                                            let _ = tx.send(LinkPollStatus::Expired(
                                                format!("Failed to parse server response: {}", e),
                                            ));
                                            return;
                                        }
                                    }
                                }
                                Ok(r) => {
                                    let status = r.status();
                                    let _ = tx.send(LinkPollStatus::Expired(
                                        format!("Server error: {}", status),
                                    ));
                                    return;
                                }
                                Err(e) => {
                                    let _ = tx.send(LinkPollStatus::Expired(
                                        format!("Network error: {}", e),
                                    ));
                                    return;
                                }
                            };

                            // Step 2: send init data (token + link_url) to UI, open browser
                            let _ = tx.send(LinkPollStatus::Init {
                                token: token.clone(),
                                link_url: link_url.clone(),
                            });

                            // Step 3: poll every 2 seconds for up to 10 minutes
                            let deadline = std::time::Instant::now()
                                + std::time::Duration::from_secs(10 * 60);
                            loop {
                                if std::time::Instant::now() >= deadline {
                                    let _ = tx.send(LinkPollStatus::Expired(
                                        "Link expired. Try again.".to_string(),
                                    ));
                                    return;
                                }

                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                                let poll_url = format!(
                                    "https://chart.example/api/auth/link/poll?token={}",
                                    token
                                );
                                let poll_result = client.get(&poll_url).send().await;

                                match poll_result {
                                    Ok(r) if r.status().is_success() => {
                                        match r.json::<serde_json::Value>().await {
                                            Ok(body) => {
                                                let status_str = body["status"].as_str().unwrap_or("pending");
                                                match status_str {
                                                    "linked" => {
                                                        let display_name = body["display_name"]
                                                            .as_str()
                                                            .unwrap_or("User")
                                                            .to_string();
                                                        let provider = body["provider"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string();
                                                        let auth_token = body["auth_token"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string();
                                                        let user_id = body["user_id"]
                                                            .as_i64()
                                                            .unwrap_or(0);
                                                        let _ = tx.send(LinkPollStatus::Linked {
                                                            display_name,
                                                            provider,
                                                            auth_token,
                                                            user_id,
                                                        });
                                                        return;
                                                    }
                                                    "expired" => {
                                                        let _ = tx.send(LinkPollStatus::Expired(
                                                            "Link expired. Try again.".to_string(),
                                                        ));
                                                        return;
                                                    }
                                                    _ => {
                                                        // "pending" — send Pending so UI stays updated
                                                        let _ = tx.send(LinkPollStatus::Pending);
                                                    }
                                                }
                                            }
                                            Err(_) => {
                                                let _ = tx.send(LinkPollStatus::Pending);
                                            }
                                        }
                                    }
                                    Ok(r) if r.status() == 404 => {
                                        // Token not found or expired
                                        let _ = tx.send(LinkPollStatus::Expired(
                                            "Link expired. Try again.".to_string(),
                                        ));
                                        return;
                                    }
                                    _ => {
                                        // Network error — keep polling
                                        let _ = tx.send(LinkPollStatus::Pending);
                                    }
                                }
                            }
                        });
                        None
                    } else if cmd_str == "resolve_all:keep_local"
                        || cmd_str == "resolve_all:keep_cloud"
                    {
                        // Resolve every pending conflict in bulk by sending individual
                        // ResolveConflict messages — ResolveAllConflicts variant does not
                        // exist in the updater yet.
                        let resolution = if cmd_str == "resolve_all:keep_local" {
                            zengeld_updater::ConflictResolution::KeepLocal
                        } else {
                            zengeld_updater::ConflictResolution::KeepCloud
                        };
                        eprintln!("[App] resolve_all: {:?}", cmd_str);
                        // Borrow the current sync status to extract the conflict list.
                        let conflicts: Vec<String> = {
                            match &*handle.sync_status_rx.borrow() {
                                zengeld_updater::SyncStatus::ConflictsDetected(list) => {
                                    list.iter().map(|c| c.sync_id.clone()).collect()
                                }
                                _ => Vec::new(),
                            }
                        };
                        for sync_id in conflicts {
                            let _ = handle.cmd_tx.send(UpdaterCommand::ResolveConflict {
                                sync_id,
                                resolution: resolution.clone(),
                            });
                        }
                        None
                    } else if let Some(rest) = cmd_str.strip_prefix("resolve_conflict:") {
                        if let Some((sync_id, resolution_str)) = rest.rsplit_once(':') {
                            let resolution = if resolution_str == "keep_local" {
                                zengeld_updater::ConflictResolution::KeepLocal
                            } else {
                                zengeld_updater::ConflictResolution::KeepCloud
                            };
                            Some(UpdaterCommand::ResolveConflict {
                                sync_id: sync_id.to_string(),
                                resolution,
                            })
                        } else {
                            eprintln!("[App] malformed resolve_conflict command: {}", cmd_str);
                            None
                        }
                    } else if cmd_str.starts_with("e2e_setup:") {
                        // Vault unlock: upload vault salt + encrypted_master_key to the server
                        // so that vault recovery is possible on another device.
                        //
                        // CloudSync items are NOT encrypted client-side.  This upload is solely
                        // for vault recovery (salt + encrypted_master_key stored server-side).
                        //
                        // NOTE: Local vault key derivation (save_all) is done OUTSIDE this
                        // block to avoid holding the `handle` borrow while calling save_all.

                        // Read the vault salt from disk.  This salt was created when the vault
                        // was first set up; it is the ground-truth salt for all key derivation.
                        let salt_path = zengeld_chart::active_profile_data_dir().join("salt.hex");
                        let vault_salt = std::fs::read_to_string(&salt_path)
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                        if vault_salt.is_empty() {
                            eprintln!("[App] e2e_setup: salt.hex not found or empty — skipping vault recovery upload");
                        } else {
                            // Upload vault salt + encrypted_master_key to the server so that
                            // vault recovery is possible on another device.  This call is
                            // idempotent — the server accepts repeated uploads of the same salt.
                            // CloudSync items are NOT encrypted client-side; this upload is
                            // solely for vault recovery purposes.
                            let token = zengeld_updater::token_store::load_token();
                            if let Some(tok) = token.filter(|_| self.profile.cloud_enabled) {
                                let client = reqwest::Client::builder()
                                    .timeout(std::time::Duration::from_secs(15))
                                    .build()
                                    .unwrap_or_default();
                                let server_url = "https://chart.example".to_string();
                                let token_str = tok.token.clone();
                                let salt_hex_for_spawn = vault_salt;

                                let encrypted_master_key_for_spawn =
                                    self.profile_manager.take_encrypted_master_key();

                                let build_attest_for_spawn = zengeld_updater::BuildAttestation {
                                    attestation: env!("BUILD_ATTESTATION").to_string(),
                                    version: env!("CARGO_PKG_VERSION").to_string(),
                                    platform: env!("BUILD_PLATFORM").to_string(),
                                    timestamp: env!("BUILD_TIMESTAMP").to_string(),
                                };
                                let profile_id_for_spawn = self.profile_manager.profile.profile_id.clone();
                                let device_id_for_spawn = zengeld_updater::telemetry::get_or_create_device_id();
                                let iterations = zengeld_updater::vault_params::PBKDF2_ITERATIONS as i32;
                                self.bridge.runtime().spawn(async move {
                                    match zengeld_updater::vault_params::upload_vault_params(
                                        &client,
                                        &server_url,
                                        &token_str,
                                        &salt_hex_for_spawn,
                                        iterations,
                                        encrypted_master_key_for_spawn.as_deref(),
                                        &build_attest_for_spawn,
                                        &profile_id_for_spawn,
                                        &device_id_for_spawn,
                                    )
                                    .await {
                                        Ok(_) => eprintln!("[App] E2E setup on server succeeded (salt + recovery key uploaded)"),
                                        Err(e) => eprintln!("[App] E2E setup on server failed: {}", e),
                                    }
                                });
                            } else {
                                eprintln!("[App] e2e_setup: not logged in or cloud disabled — vault recovery upload deferred");
                            }
                        }
                        None
                    } else if cmd_str == "list_cloud_profiles" {
                        Some(UpdaterCommand::ListCloudProfiles)
                    } else if let Some(profile_id) = cmd_str.strip_prefix("restore_cloud_profile:") {
                        Some(UpdaterCommand::RestoreCloudProfile {
                            profile_id: profile_id.to_string(),
                        })
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
                // ── Deferred: refresh profile list after sync_level change ────
                // The `handle` borrow above is now released, so &mut self is safe.
                if sync_level_changed {
                    self.profile_manager.refresh_index();
                    self.sync_profiles_to_windows();
                    // Push updated toggle states to all windows.
                    let ota = self.profile_manager.profile.ota_enabled;
                    let sync_en = self.profile_manager.profile.sync_state.enabled;
                    for pw in self.windows.values_mut() {
                        let us = &mut pw.chart.panel_app.user_settings_state;
                        us.ota_enabled = ota;
                        us.sync_enabled = sync_en;
                    }
                }
            }
        }

        // ── Poll auth_rx → sync to all windows ───────────────────────────
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        {
            if let Some(ref mut handle) = self.updater_handle {
                if handle.auth_rx.has_changed().unwrap_or(false) {
                    let status = handle.auth_rx.borrow_and_update().clone();
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
                            self.profile_manager.profile.linked_account =
                                Some(zengeld_chart::user_profile::profile::LinkedAccount {
                                    provider: prov.clone(),
                                    provider_user_id: uid.to_string(),
                                    display_name: dn.clone(),
                                    linked_at: now,
                                });
                            // Auto-switch to Connected mode when user logs in.
                            self.profile_manager.profile.cloud_enabled = true;
                            self.profile.cloud_enabled = true;
                            // Reflect the mode change in all open windows immediately.
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.client_mode_connected = true;
                            }
                            // Mirror the bearer token into the OS keychain as a
                            // secondary encrypted store.  This is best-effort: a
                            // failure here is non-fatal since auth_token.json is
                            // still the primary storage.
                            if let Some(stored) = zengeld_updater::token_store::load_token() {
                                if let Err(e) = keychain::store_auth_token(&stored.token) {
                                    eprintln!("[App] keychain: failed to mirror auth token: {}", e);
                                }
                            }
                            // Wizard: update linking status but do NOT close the wizard.
                            // The user must still enter a passphrase (mandatory zero-trust).
                            for pw in self.windows.values_mut() {
                                let uss = &mut pw.chart.panel_app.user_settings_state;
                                if uss.show_welcome_wizard {
                                    uss.wizard_linking_status = format!("Linked as {}", dn);
                                }
                            }
                            eprintln!("[App] auth: logged in as {} ({})", dn, prov);
                            // Auto-fetch cloud profiles on login so skeleton shows them immediately.
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.cloud_profiles_loading = true;
                                pw.chart.panel_app.user_settings_state.cloud_profiles_error.clear();
                            }
                            let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::ListCloudProfiles);
                        }
                        zengeld_updater::AuthStatus::NotLoggedIn => {
                            for pw in self.windows.values_mut() {
                                let s = &mut pw.chart.panel_app.user_settings_state;
                                s.is_logged_in = false;
                                s.auth_display_name = String::new();
                                s.auth_provider = String::new();
                                s.auth_user_id = 0;
                                s.cloud_profiles.clear();
                                s.cloud_profiles_loading = false;
                                s.cloud_profiles_error.clear();
                            }
                            // Clear the profile mirror on logout / missing token.
                            self.profile_manager.profile.linked_account = None;
                            // Remove the keychain copy on logout (best-effort).
                            if let Err(e) = keychain::delete_auth_token() {
                                eprintln!("[App] keychain: failed to delete auth token: {}", e);
                            }
                            eprintln!("[App] auth: not logged in");
                        }
                    }
                }
            }
        }

        // ── Drain link-poll task updates → update wizard UI ──────────────
        {
            // Collect all pending messages from the link poll task (non-blocking).
            let mut link_done = false;
            if self.link_poll_rx.is_some() {
                use tokio::sync::mpsc::error::TryRecvError;
                let mut last_status: Option<LinkPollStatus> = None;
                loop {
                    match self.link_poll_rx.as_mut().unwrap().try_recv() {
                        Ok(status) => { last_status = Some(status); }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            link_done = true;
                            break;
                        }
                    }
                }
                if let Some(status) = last_status {
                    match status {
                        LinkPollStatus::Init { token, link_url } => {
                            // Truncate token for display (first 8 chars)
                            let token_display = if token.len() > 8 {
                                token[..8].to_string()
                            } else {
                                token.clone()
                            };
                            for pw in self.windows.values_mut() {
                                let uss = &mut pw.chart.panel_app.user_settings_state;
                                uss.wizard_device_code = token_display.clone();
                                uss.wizard_linking_status =
                                    "Waiting for confirmation\u{2026}".to_string();
                                // Open the link URL in the browser
                                pw.chart.pending_open_url = Some(link_url.clone());
                            }
                            eprintln!("[App] link flow: opened {}", link_url);
                        }
                        LinkPollStatus::Pending => {
                            // Keep showing "Waiting…" — no change needed unless UI is stale
                        }
                        LinkPollStatus::Linked { display_name, provider, auth_token, user_id } => {
                            eprintln!("[App] link flow: linked as {} ({}) auth_token_len={} user_id={}", display_name, provider, auth_token.len(), user_id);

                            // Persist auth token to disk
                            if !auth_token.is_empty() {
                                let stored = zengeld_updater::token_store::StoredToken {
                                    token: auth_token.clone(),
                                    provider: provider.clone(),
                                    display_name: display_name.clone(),
                                    saved_at: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    user_id,
                                };
                                if let Err(e) = zengeld_updater::token_store::save_token(&stored) {
                                    eprintln!("[App] failed to save auth token: {}", e);
                                } else {
                                    eprintln!("[App] auth token saved to disk");
                                }
                            }

                            // Mirror auth state into profile so it persists across restarts.
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            self.profile_manager.profile.linked_account =
                                Some(zengeld_chart::user_profile::profile::LinkedAccount {
                                    provider: provider.clone(),
                                    provider_user_id: user_id.to_string(),
                                    display_name: display_name.clone(),
                                    linked_at: now,
                                });

                            for pw in self.windows.values_mut() {
                                let uss = &mut pw.chart.panel_app.user_settings_state;
                                uss.wizard_linking_status = "Linked!".to_string();
                                uss.is_logged_in = true;
                                uss.auth_display_name = display_name.clone();
                                uss.auth_provider = provider.clone();
                                uss.auth_user_id = user_id;
                                // Auto-fetch cloud profiles immediately after link auth.
                                uss.cloud_profiles_loading = true;
                                uss.cloud_profiles_error.clear();
                                // Do NOT close wizard on link — passphrase still required (zero-trust).
                                // Just update linking status for visual feedback.
                            }
                            // Mirror cloud_enabled into profile.
                            self.profile_manager.profile.cloud_enabled = true;
                            self.profile.cloud_enabled = true;
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.client_mode_connected = true;
                            }
                            // Mirror the bearer token into the OS keychain (best-effort).
                            if let Some(stored) = zengeld_updater::token_store::load_token() {
                                if let Err(e) = keychain::store_auth_token(&stored.token) {
                                    eprintln!("[App] keychain: failed to mirror auth token: {}", e);
                                }
                            }
                            // Trigger cloud profiles fetch via updater.
                            #[cfg(all(feature = "updater", not(feature = "standalone")))]
                            if let Some(ref handle) = self.updater_handle {
                                let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::ListCloudProfiles);
                            }
                            link_done = true;
                        }
                        LinkPollStatus::Expired(msg) => {
                            eprintln!("[App] link flow expired: {}", msg);
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.wizard_linking_status =
                                    msg.clone();
                            }
                            link_done = true;
                        }
                    }
                }
            }
            if link_done {
                self.link_poll_rx = None;
            }
        }

        // ── Poll sync_status_rx → update all windows' user_settings_state ─
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        if let Some(ref mut handle) = self.updater_handle {
            if handle.sync_status_rx.has_changed().unwrap_or(false) {
                let sync_status = handle.sync_status_rx.borrow_and_update().clone();

                let (label, color, is_active, has_error) =
                    match &sync_status {
                        zengeld_updater::SyncStatus::Idle => (
                            "Idle".to_string(),
                            "#888888".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::Syncing => (
                            "Syncing\u{2026}".to_string(),
                            "#f0ad4e".to_string(),
                            true,
                            false,
                        ),
                        zengeld_updater::SyncStatus::Completed { pushed, pulled } => {
                            let lbl = if *pushed == 0 && *pulled == 0 {
                                "Synced \u{2014} no changes".to_string()
                            } else {
                                format!("Synced \u{2014} \u{2191}{} \u{2193}{}", pushed, pulled)
                            };
                            (lbl, "#5cb85c".to_string(), false, false)
                        }
                        zengeld_updater::SyncStatus::Error(msg) => {
                            let truncated = if msg.len() > 60 {
                                let safe_end = msg.char_indices()
                                    .take_while(|(i, _)| *i <= 57)
                                    .last()
                                    .map(|(i, c)| i + c.len_utf8())
                                    .unwrap_or(msg.len());
                                format!("{}\u{2026}", &msg[..safe_end])
                            } else {
                                msg.clone()
                            };
                            (
                                format!("Error: {}", truncated),
                                "#d9534f".to_string(),
                                false,
                                true,
                            )
                        }
                        zengeld_updater::SyncStatus::NeedsSetup => (
                            "Cloud data found".to_string(),
                            "#f0ad4e".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::ConflictsDetected(conflicts) => (
                            format!("{} conflict(s)", conflicts.len()),
                            "#e67e22".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::CloudProfilesLoaded(_) => (
                            "Idle".to_string(),
                            "#888888".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::CloudProfilesError(msg) => (
                            format!("Cloud profiles error: {}", msg),
                            "#d9534f".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::ProfileRestored { .. } => (
                            "Idle".to_string(),
                            "#888888".to_string(),
                            false,
                            false,
                        ),
                        zengeld_updater::SyncStatus::ProfileRestoreError(msg) => (
                            format!("Restore error: {}", msg),
                            "#d9534f".to_string(),
                            false,
                            false,
                        ),
                    };

                let is_completed = matches!(
                    &sync_status,
                    zengeld_updater::SyncStatus::Completed { .. }
                );
                let now_ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                // Pre-extract cloud profile state changes before the windows loop
                // so we can call methods that need &mut self after the loop.
                let cloud_profiles_loaded: Option<Vec<zengeld_chart::ui::modal_settings::CloudProfileEntry>> =
                    if let zengeld_updater::SyncStatus::CloudProfilesLoaded(ref profiles) = sync_status {
                        Some(profiles.iter().map(|p| zengeld_chart::ui::modal_settings::CloudProfileEntry {
                            profile_id: p.profile_id.clone(),
                            display_name: p.display_name.clone(),
                            item_count: p.item_count,
                            total_bytes: p.total_bytes,
                            last_modified: p.last_modified,
                            has_vault: p.has_vault,
                            has_recovery_key: p.has_recovery_key,
                        }).collect())
                    } else {
                        None
                    };
                let cloud_profiles_error: Option<String> =
                    if let zengeld_updater::SyncStatus::CloudProfilesError(ref msg) = sync_status {
                        Some(msg.clone())
                    } else {
                        None
                    };
                let profile_restored: Option<String> =
                    if let zengeld_updater::SyncStatus::ProfileRestored { ref profile_id } = sync_status {
                        Some(profile_id.clone())
                    } else {
                        None
                    };
                let profile_restore_error: Option<String> =
                    if let zengeld_updater::SyncStatus::ProfileRestoreError(ref msg) = sync_status {
                        Some(msg.clone())
                    } else {
                        None
                    };

                for pw in self.windows.values_mut() {
                    let uss = &mut pw.chart.panel_app.user_settings_state;
                    uss.sync_status_label = label.clone();
                    uss.sync_status_color = color.clone();
                    uss.sync_is_active = is_active;

                    if is_completed {
                        uss.last_sync_timestamp = now_ts;
                    }

                    // ── Cloud profile restore state updates ───────────────────
                    if let Some(ref profiles) = cloud_profiles_loaded {
                        uss.cloud_profiles = profiles.clone();
                        uss.cloud_profiles_loading = false;
                        uss.cloud_profiles_error.clear();
                    }
                    if let Some(ref msg) = cloud_profiles_error {
                        uss.cloud_profiles_loading = false;
                        uss.cloud_profiles_error = msg.clone();
                    }
                    if profile_restored.is_some() {
                        uss.restoring_profile_id = None;
                    }
                    if let Some(ref msg) = profile_restore_error {
                        uss.restoring_profile_id = None;
                        uss.cloud_profiles_error = msg.clone();
                    }

                    // Reset attestation_rejected on any non-error status
                    if !has_error {
                        uss.attestation_rejected = false;
                    }
                    if has_error {
                        // Check error message for attestation failures
                        if let zengeld_updater::SyncStatus::Error(ref msg) = sync_status {
                            if msg.contains("build attestation") || msg.contains("attestation failed") {
                                uss.attestation_rejected = true;
                            }
                        }
                    }

                }

                // ── ProfileRestored: refresh the local profile index ──────────
                // Must be done after the windows loop because sync_profiles_to_windows
                // needs &mut self which conflicts with the pw borrow above.
                if let Some(ref restored_id) = profile_restored {
                    eprintln!("[App] ProfileRestored: refreshing index for profile {}", restored_id);
                    self.profile_manager.refresh_index();
                    self.sync_profiles_to_windows();
                    // Remove the restored profile from cloud_profiles in all windows
                    // (it now exists locally, no need to offer restore again).
                    let rid = restored_id.clone();
                    for pw in self.windows.values_mut() {
                        let uss = &mut pw.chart.panel_app.user_settings_state;
                        uss.cloud_profiles.retain(|cp| cp.profile_id != rid);
                    }
                    // Re-fetch the cloud list so the server-side view is also current.
                    if let Some(ref handle) = self.updater_handle {
                        let _ = handle.cmd_tx.send(zengeld_updater::UpdaterCommand::ListCloudProfiles);
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.cloud_profiles_loading = true;
                        }
                    }
                }

                eprintln!("[App] sync_status: {}", label);
            }
        }

        // ── Poll sync_checksums_rx → persist last_synced_checksums to profile ─
        //
        // After each successful sync cycle the updater broadcasts the updated
        // checksum map so it survives the next restart.  Without this, an empty
        // map after restart causes every locally-modified item to look like a
        // conflict on the first sync of the new session.
        #[cfg(all(feature = "updater", not(feature = "standalone")))]
        if let Some(ref mut handle) = self.updater_handle {
            if handle.sync_checksums_rx.has_changed().unwrap_or(false) {
                let checksums = handle.sync_checksums_rx.borrow_and_update().clone();
                if !checksums.is_empty() {
                    self.profile_manager
                        .profile
                        .sync_state
                        .last_synced_checksums = checksums;
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
                    if let Err(e) = zengeld_chart::save_profile(&self.profile, self.app_state.vault_key.as_ref()) {
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
                let drawing_styles = &self.app_state.snapshots.last_used_drawing_styles;
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_manager.snapshots =
                        self.app_state.snapshots.clone();
                    // Apply persisted last-used drawing styles to every DrawingManager
                    // so new primitives of each type inherit the user's last-used style.
                    for w in pw.chart.panel_app.panel_grid.windows_mut().values_mut() {
                        w.drawing_manager.load_last_styles(drawing_styles);
                    }
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
            let scale_mode = self.app_state.scale_mode;
            for pw in self.windows.values_mut() {
                pw.chart.indicator_manager.recalc_mode = recalc_mode;
                pw.chart.default_scale_mode = scale_mode;
            }

            let frame_time = now_ms();
            let bar_svc = &mut self.bar_service;
            for pw in self.windows.values_mut() {
                // Skip tick for minimized windows — no point updating viewport
                // or processing trades for an invisible surface.  On restore,
                // snap_to_end repositions the viewport to the latest bars.
                if pw.was_minimized {
                    continue;
                }
                pw.chart.tick(frame_time, bar_svc);
            }
        }

        // Drain pending reset-cache / reset-storage flags set by the context menu.
        for pw in self.windows.values_mut() {
            if pw.chart.pending_reset_cache || pw.chart.pending_reset_storage {
                let delete_storage = pw.chart.pending_reset_storage;
                pw.chart.pending_reset_cache = false;
                pw.chart.pending_reset_storage = false;

                // Collect window info before borrowing bar_service / bridge.
                let (symbol, exchange, account_type, timeframe, bar_count) =
                    if let Some(window) = pw.chart.panel_app.panel_grid.active_window() {
                        (
                            window.symbol.clone(),
                            window.exchange.clone(),
                            window.account_type.clone(),
                            window.timeframe.clone(),
                            pw.chart.panel_app.user_manager.profile.bar_count as usize,
                        )
                    } else {
                        continue;
                    };

                if delete_storage {
                    self.bar_service.delete_file(&exchange, &symbol, &timeframe.name, &account_type);
                }

                // Remove the in-memory series so the next fetch starts clean.
                if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                    let at = chart_app::account_type_from_label(&account_type);
                    let key = bar_service::BarSeriesKey::new(eid, at, symbol.clone(), timeframe.name.clone());
                    self.bar_service.remove_series(&key);
                }

                // Clear bars on the window and mark pending load.
                if let Some(window) = pw.chart.panel_app.panel_grid.active_window_mut() {
                    window.bars.clear();
                    window.viewport.bar_count = 0;
                    window.viewport.view_start = 0.0;
                    window.pending_symbol_load = true;
                }

                // Reset all other windows in the same sync group that share
                // the same symbol key, when sync_symbol is enabled.
                let active_cid = pw.chart.panel_app.panel_grid.active_chart_id();
                let group_info = active_cid
                    .and_then(|cid| {
                        let w = pw.chart.panel_app.panel_grid.active_window()?;
                        let gid = w.group_id?;
                        Some((cid, gid))
                    });
                if let Some((active_cid, group_id)) = group_info {
                    let sync_symbol_on = pw.chart.panel_app.tag_manager
                        .group(group_id)
                        .map(|g| g.sync_flags.sync_symbol)
                        .unwrap_or(false);
                    if sync_symbol_on {
                        let peer_ids: Vec<zengeld_chart::ChartId> = pw.chart.panel_app.tag_manager
                            .chart_members(group_id)
                            .into_iter()
                            .filter(|&m| m != active_cid)
                            .collect();
                        for peer_id in peer_ids {
                            if let Some(peer_window) = pw.chart.panel_app.panel_grid.windows_mut().get_mut(&peer_id) {
                                if peer_window.symbol == symbol
                                    && peer_window.exchange == exchange
                                    && peer_window.account_type == account_type
                                {
                                    peer_window.bars.clear();
                                    peer_window.viewport.bar_count = 0;
                                    peer_window.viewport.view_start = 0.0;
                                    peer_window.pending_symbol_load = true;
                                    eprintln!(
                                        "[App] Reset cache propagated to peer {:?} ({}:{}:{})",
                                        peer_id, symbol, exchange, account_type
                                    );
                                }
                            }
                        }
                    }
                }

                // Request a fresh bar fetch from the exchange.
                if let Some(eid) = chart_app::ExchangeId::from_str(&exchange) {
                    let at = chart_app::account_type_from_label(&account_type);
                    self.bridge.request_bars(eid, &symbol, &timeframe, at, None, Some(bar_count), true);
                }

                eprintln!(
                    "[App] Reset {} for {}:{} {} ({})",
                    if delete_storage { "storage+cache" } else { "cache" },
                    symbol, exchange, timeframe.name, account_type
                );
            }
        }

        // Capture the active window's scale mode back to AppState so it
        // persists across sessions.  Reading after tick() ensures any toggle
        // from this frame is captured.
        if let Some(pw) = self.last_focused
            .and_then(|id| self.windows.get(&id))
            .or_else(|| self.windows.values().next())
        {
            if let Some(w) = pw.chart.panel_app.panel_grid.active_window() {
                self.app_state.scale_mode = w.price_scale.scale_mode;
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
            let perf_log_enabled = self.perf_log_enabled;
            let render_backend = self.render_backend;

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
                perf.per_core_cpu = per_core_cpu.clone();
                perf.scene_build_us = scene_build_us;
                perf.gpu_render_us = gpu_render_us;

                // Internal CPU profiling fields — read directly from ChartApp.
                perf.tick_us = pw.chart.last_tick_us;
                perf.indicator_recalc_us = pw.chart.last_indicator_recalc_us;
                perf.indicator_recalc_count = pw.chart.indicator_manager.instances_count() as u32;
                perf.indicator_incremental_count = pw.chart.indicator_manager.last_incremental_count;
                perf.indicator_full_count = pw.chart.indicator_manager.last_full_count;
                perf.event_process_us = pw.chart.last_event_process_us;
                perf.auto_scale_us = pw.chart.last_auto_scale_us;
                perf.moving_avg_us = pw.chart.last_moving_avg_us;
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

            // Tooltip tick — fires when cursor is stationary (no CursorMoved events)
            {
                let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                let chrome_was = pw.chrome_state.tooltip.is_visible();
                chrome::update_tooltip(&mut pw.chrome_state, pw.last_mouse_pos.0, pw.last_mouse_pos.1, time_ms);
                let toolbar_was = pw.toolbar_tooltip.is_visible();
                pw.toolbar_tooltip.update(pw.toolbar_tooltip.hovered_widget().cloned(), time_ms);
                if (!chrome_was && pw.chrome_state.tooltip.is_visible())
                    || (!toolbar_was && pw.toolbar_tooltip.is_visible())
                {
                    pw.window.request_redraw();
                }
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
                        seen.insert(*eid);
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
                let managed: Vec<zengeld_chart::LocalAgentKeyInfo> = server_keys.iter().map(|k| {
                    zengeld_chart::LocalAgentKeyInfo {
                        label: k.label.clone(),
                        tier: k.tier.clone(),
                        agent_id: k.agent_id.clone(),
                    }
                }).collect();
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_settings_state.local_agent_keys_ui = managed.clone();
                }
            }
            // Clock time in the toolbar updates every second — mark all toolbars dirty
            // so the clock string is refreshed on the next frame.
            // Also mark sidebar dirty: performance panel data (fps, frame_time, etc.)
            // updates every second, so the performance panel needs a redraw.
            // Mark chart dirty: time-based indicators (current-bar progress, etc.) update.
            for pw in self.windows.values_mut() {
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
            }

            self.last_indicator_snapshot = std::time::Instant::now();
        }

        // Propagate chart.sidebar_data_dirty → pw.sidebar_dirty_scene.
        // chart.sidebar_data_dirty is set by tick() after any LiveUpdate and
        // by sidebar panel toggle handlers.  When it fires, the sidebar scene
        // must be rebuilt so the new data is reflected in the vector graphics.
        // New bar data also means chart panels need redrawing (new candle, price
        // labels shift, indicators update) — mark chart dirty too.
        for pw in self.windows.values_mut() {
            if pw.chart.sidebar_data_dirty {
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
            }
        }
        // Sidebar redraws at FPS-cap rate — same cadence as chart.
        // The FPS cap is already paid for; no reason to throttle sidebar
        // separately.  This ensures cursor blink, hover highlights and
        // agent PTY output are rendered within one frame of the event.
        for pw in self.windows.values_mut() {
            pw.sidebar_dirty_scene = true;
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

            // After GpuDone the previous frame has been presented on screen.
            // Make any newly created windows visible now so they appear with
            // their first rendered frame rather than a blank white rectangle.
            for pw in self.windows.values_mut() {
                if !pw.visible_set {
                    pw.window.set_visible(true);
                    pw.visible_set = true;
                }
            }
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

        // ── Deferred profile hot-reload ──────────────────────────────────────
        // Must run after ALL command processing and rendering so we don't
        // destroy windows while any iteration over `self.windows` is active.
        if let Some(target_id) = self.pending_profile_switch.take() {
            self.execute_profile_switch(&target_id, event_loop);
        }

        // ── Promote skeleton windows to live ────────────────────────────────
        // After vault unlock / wizard / fresh e2e setup, the skeleton windows
        // (no bar data, no connector activity) are replaced with fully-live
        // windows that fetch data.
        if self.pending_skeleton_promote {
            self.pending_skeleton_promote = false;
            self.promote_skeleton(event_loop);
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
        _event_loop: &ActiveEventLoop,
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

                    // Preventive sidebar guard: on resize, ensure the sidebar
                    // doesn't push the chart area below its minimum.
                    if pw.chart.sidebar_state.is_right_open() {
                        use zengeld_chart::RIGHT_TOOLBAR_WIDTH;
                        use sidebar_content::state::{MIN_SIDEBAR_WIDTH, RightSidebarPanel};
                        let window_w = pw.chart.width as f64;
                        let right_toolbar_left_x = window_w - RIGHT_TOOLBAR_WIDTH;
                        let min_chart_w = pw.chart.panel_app.panel_grid
                            .min_sidebar_chart_width() as f64;
                        if right_toolbar_left_x < MIN_SIDEBAR_WIDTH + min_chart_w {
                            // Window too narrow for sidebar + chart — close sidebar.
                            pw.chart.sidebar_state
                                .set_right_panel(RightSidebarPanel::None);
                        } else {
                            // Clamp sidebar width so chart area stays >= min_chart_w.
                            let max_sidebar = right_toolbar_left_x - min_chart_w;
                            let cur = pw.chart.sidebar_state.right_sidebar_width;
                            if cur > max_sidebar {
                                if max_sidebar < MIN_SIDEBAR_WIDTH {
                                    pw.chart.sidebar_state
                                        .set_right_panel(RightSidebarPanel::None);
                                } else {
                                    pw.chart.sidebar_state.set_right_width(max_sidebar);
                                }
                            }
                        }
                    }

                    // Sync the maximize icon when the window is snapped or
                    // maximized by the OS (e.g. via Win+Arrow keys).
                    pw.chrome_state.is_maximized = pw.window.is_maximized();

                    // Mark dirty so position/size is persisted on next save
                    // (skip skeleton — it's a loading screen, nothing to persist).
                    if !pw.skeleton {
                        pw.chart.profile_geometry_dirty = true;
                    }
                    // Toolbar and sidebar layout changes on resize — must rebuild both.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;
                    pw.chart_dirty = true;

                    // Restore from minimize: tick was skipped while minimized,
                    // so viewport stayed at the pre-minimize position.  For
                    // Follow/Auto modes, snap ALL windows to end so the chart
                    // shows the latest bars.  Manual mode keeps user's position.
                    if pw.was_minimized {
                        pw.was_minimized = false;
                        for window in pw.chart.panel_app.panel_grid.windows_mut().values_mut() {
                            if !window.bars.is_empty()
                                && (window.price_scale.scale_mode.is_follow()
                                    || window.price_scale.scale_mode == zengeld_chart::ScaleMode::Auto)
                            {
                                window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                            }
                        }
                    }
                } else {
                    // Window minimized — size collapses to 0x0 on Windows.
                    pw.was_minimized = true;
                }
            }

            // ─── Window moved ─────────────────────────────────────────────
            WindowEvent::Moved(_) => {
                if !pw.skeleton {
                    pw.chart.profile_geometry_dirty = true;
                }
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
                // In skeleton mode, suppress hover for chrome buttons that are blocked.
                let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                    || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                pw.chrome_state.hovered = if skeleton_active {
                    match hit {
                        chrome::ChromeHit::NewTabButton
                        | chrome::ChromeHit::MenuButton
                        | chrome::ChromeHit::MascotButton
                        | chrome::ChromeHit::NewWindowButton
                        | chrome::ChromeHit::Tab(_)
                        | chrome::ChromeHit::TabClose(_) => chrome::ChromeHit::None,
                        other => other,
                    }
                } else {
                    hit
                };

                // Update chrome tooltip based on the (possibly skeleton-filtered) hover.
                {
                    let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                    chrome::update_tooltip(&mut pw.chrome_state, x, y, time_ms);
                }

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
                    | chrome::ChromeHit::CloseWindowButton
                    | chrome::ChromeHit::MascotButton
                    | chrome::ChromeHit::MenuButton
                    | chrome::ChromeHit::Tab(_)
                    | chrome::ChromeHit::TabClose(_)
                    | chrome::ChromeHit::NewTabButton
                    | chrome::ChromeHit::NewWindowButton => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::Default);
                        // Cursor is on chrome — clear toolbar tooltip
                        pw.toolbar_tooltip.clear();
                        // Do not forward to chart
                        return;
                    }
                    chrome::ChromeHit::None => {
                        // Cursor is below the chrome strip — clear tooltip.
                        pw.chrome_state.tooltip.clear();
                    }
                }

                // Only forward events in the chart area (below chrome strip).
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y < 0.0 {
                    return;
                }

                // Crosshair, hover highlights and price labels update with every
                // mouse move inside the chart area — mark the chart scene dirty.
                pw.chart_dirty = true;

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

                    // Dropdowns extend below the toolbar zone — always rebuild toolbar
                    // scene when any dropdown is open so hover highlights update.
                    if pw.chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                        || pw.chart.panel_app.toolbar_state.open_inline_style_dropdown
                        || pw.chart.panel_app.toolbar_state.open_inline_width_dropdown
                    {
                        pw.toolbar_dirty = true;
                    }

                    // Inline bar dragging moves the toolbar — always rebuild while drag is active.
                    if pw.chart.panel_app.toolbar_state.floating_inline_bar.dragging {
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
                        // Sidebar separator drag: mark sidebar + toolbar dirty so
                        // the cached scenes rebuild every frame during resize.
                        if pw.chart.is_sidebar_separator_dragging() {
                            pw.sidebar_dirty_scene = true;
                            pw.toolbar_dirty = true;
                        }
                    }
                    pw.last_drag_pos = Some((x, y));
                } else {
                    pw.chart.on_mouse_move(x, chart_y);

                    // Auto-focus agent PTY terminal on hover.
                    if pw.chart.check_agent_hover(x, chart_y) {
                        pw.sidebar_dirty_scene = true;
                    }

                    // Update toolbar tooltip based on hovered toolbar button
                    let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                    let hovered_id = pw.chart.panel_app.toolbar_state.hovered_top_toolbar_id.as_deref()
                        .or(pw.chart.panel_app.toolbar_state.hovered_left_toolbar_id.as_deref())
                        .or(pw.chart.panel_app.toolbar_state.hovered_right_toolbar_id.as_deref())
                        .or(pw.chart.panel_app.toolbar_state.hovered_bottom_toolbar_id.as_deref());
                    if let Some(btn_id) = hovered_id {
                        let wid = uzor::WidgetId::new(format!("toolbar:{}", btn_id));
                        pw.toolbar_tooltip.update(Some(wid.clone()), time_ms);
                        if let Some(text) = zengeld_chart::toolbar::find_toolbar_tooltip(btn_id) {
                            pw.toolbar_tooltip.request_tooltip(wid, text.to_string(), (x, y), time_ms);
                        }
                    } else {
                        // No toolbar button hovered — check sidebar agent buttons
                        let mut sidebar_tip = false;
                        if pw.chart.sidebar_state.is_right_open()
                            && pw.chart.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Agents
                        {
                            if let Some(ref sr) = pw.chart.last_sidebar_result {
                                for (wid_str, wrect) in &sr.item_rects {
                                    // item_rects are in chart-space (rendered with
                                    // translate(0, CHROME_HEIGHT)), so compare
                                    // against chart_y, not window y.
                                    if x >= wrect.x && x < wrect.x + wrect.width
                                        && chart_y >= wrect.y && chart_y < wrect.y + wrect.height
                                    {
                                        if let Some(tip_text) = sidebar_content::render::find_agent_tooltip(wid_str) {
                                            let wid = uzor::WidgetId::new(&wid_str[..]);
                                            pw.toolbar_tooltip.update(Some(wid.clone()), time_ms);
                                            // Tooltip renders in window-space (no
                                            // translate), so pass window y for position.
                                            pw.toolbar_tooltip.request_tooltip(wid, tip_text.to_string(), (x, y), time_ms);
                                            sidebar_tip = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if !sidebar_tip {
                            pw.toolbar_tooltip.update(None, time_ms);
                        }
                    }
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
                // Hover state clears when cursor leaves — toolbar, sidebar and chart must redraw.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
                pw.chrome_state.tooltip.clear();
                pw.toolbar_tooltip.clear();
                pw.chart.on_mouse_leave();
                // Clear agent PTY hover focus when cursor leaves the window.
                pw.chart.agent_pty_hover_focused = false;
            }

            // ─── Mouse buttons ────────────────────────────────────────────
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = pw.last_mouse_pos;

                // Any click may change toolbar button state (active drawing mode,
                // open dropdown, etc.) so mark the toolbar as dirty.
                // A click in the sidebar area also changes sidebar state (item
                // selection, delete, settings open, scroll) — always mark it dirty.
                // A click in the chart area can create/select drawings — mark chart dirty.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;

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
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                if let Some(tab) = pw.chrome_state.tabs.get(idx) {
                                    let tab_id = tab.id.clone();
                                    pw.chart.load_preset(&tab_id);
                                }
                            }
                            return;
                        }
                        chrome::ChromeHit::TabClose(idx) => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                // Close the tab without deleting the preset.
                                // CloseTab handler in input.rs will switch to an adjacent tab automatically.
                                if let Some(tab) = pw.chrome_state.tabs.get(idx).cloned() {
                                    pw.chart.close_tab(&tab.id);
                                }
                            }
                            return;
                        }
                        chrome::ChromeHit::NewTabButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
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
                            }
                            return;
                        }
                        chrome::ChromeHit::MenuButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                pw.chart.open_user_settings();
                            }
                            return;
                        }
                        chrome::ChromeHit::CloseWindowButton => {
                            pw.close_window_requested = true;
                            return;
                        }
                        chrome::ChromeHit::MascotButton => {
                            eprintln!("[Chrome] Mascot clicked — future modal");
                            return;
                        }
                        chrome::ChromeHit::NewWindowButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                // Queue a new window spawn; it will be created in about_to_wait
                                // once pw is no longer borrowed.
                                pw.spawn_new_window = true;
                            }
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
                if button == MouseButton::Right && state == ElementState::Pressed
                    && y < chrome::CHROME_HEIGHT {
                        pw.chrome_state.context_menu.open_at(x, y);
                        return;
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
                        let dismissed = pw.chart.on_drag_start(x, chart_y);
                        if dismissed {
                            // Popup was dismissed — synthetic drag-end cleans up
                            // ui_drag_active, drag_dismissed_popup, text_input state.
                            pw.chart.on_drag_end(x, chart_y);
                            pw.mouse_pressed = false;
                            pw.last_drag_pos = None;
                            pw.drag_start_pos = None;
                        }
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
                    pw.chart.on_scroll(x, chart_y, dx, dy, pw.modifiers.control_key());
                    // Scrolling inside the sidebar changes the visible content —
                    // always mark it dirty so the new scroll offset is rendered.
                    pw.sidebar_dirty_scene = true;
                    // Scrolling pans/zooms the chart — bars shift, price scale updates.
                    pw.chart_dirty = true;
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
                    // Keyboard can also modify chart state (drawings, mode) — mark chart dirty.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;
                    pw.chart_dirty = true;

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
                                // Chat-first: if the focused leaf is a Chat leaf with an
                                // active selection, copy selected message lines to clipboard.
                                {
                                    let chat_text = pw.chart.chat_selection_text();
                                    if let Some((leaf_id, text)) = chat_text {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(text);
                                        }
                                        pw.chart.sidebar_state.agent_chat_selections.remove(&leaf_id);
                                        pw.sidebar_dirty_scene = true;
                                        return;
                                    }
                                }
                                // PTY-second: if there's a host-side PTY selection,
                                // copy it to clipboard and clear. Otherwise send \x03
                                // to the running CLI.
                                if pw.chart.is_agent_pty_focused() {
                                    let sel_text = pw.chart.pty_selection_text();
                                    if !sel_text.is_empty() {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(sel_text);
                                        }
                                        pw.chart.clear_pty_selection();
                                    } else {
                                        pw.chart.on_key_press(chart_app::KeyPress::CtrlC);
                                    }
                                    return;
                                }
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
                                        if pw.chart.is_agent_pty_focused() {
                                            pw.chart.paste_to_pty(&text);
                                        } else {
                                            pw.chart.on_key_press(chart_app::KeyPress::Paste(text));
                                        }
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

                    // ── PTY-first key routing ─────────────────────────────
                    // If the Agent PTY owns focus, translate named keys directly
                    // to KeyPress variants so TIM emits raw PTY bytes.
                    if pw.chart.is_agent_pty_focused() {
                        let pty_key = match &event.logical_key {
                            Key::Named(NamedKey::Escape) => Some(chart_app::KeyPress::Escape),
                            Key::Named(NamedKey::Enter) => Some(chart_app::KeyPress::Enter),
                            Key::Named(NamedKey::Tab) => Some(chart_app::KeyPress::Tab),
                            Key::Named(NamedKey::Backspace) => Some(chart_app::KeyPress::Backspace),
                            Key::Named(NamedKey::Delete) => Some(chart_app::KeyPress::Delete),
                            Key::Named(NamedKey::ArrowLeft) => Some(chart_app::KeyPress::ArrowLeft),
                            Key::Named(NamedKey::ArrowRight) => Some(chart_app::KeyPress::ArrowRight),
                            Key::Named(NamedKey::ArrowUp) => Some(chart_app::KeyPress::ArrowUp),
                            Key::Named(NamedKey::ArrowDown) => Some(chart_app::KeyPress::ArrowDown),
                            Key::Named(NamedKey::Home) => Some(chart_app::KeyPress::Home),
                            Key::Named(NamedKey::End) => Some(chart_app::KeyPress::End),
                            Key::Named(NamedKey::PageUp) => Some(chart_app::KeyPress::PageUp),
                            Key::Named(NamedKey::PageDown) => Some(chart_app::KeyPress::PageDown),
                            _ => None,
                        };
                        if let Some(k) = pty_key {
                            pw.chart.on_key_press(k);
                            return;
                        }
                        // Space + printable chars still go via on_char_input below.
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
                        Key::Named(NamedKey::Tab) => {
                            pw.chart.on_char_input('\x09');
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
    // Disabled in standalone builds — no OTA update process to wait for.
    #[cfg(all(feature = "updater", not(feature = "standalone")))]
    zengeld_updater::wait_for_parent_exit_if_needed();

    // ── Single-instance guard (Windows named mutex) ──────────────────
    // If another mylittlechart process is already running, exit immediately
    // instead of spawning a zombie that retries port binding forever.
    #[cfg(target_os = "windows")]
    let _single_instance_mutex = {
        extern "system" {
            fn CreateMutexW(
                lp_mutex_attributes: *const std::ffi::c_void,
                b_initial_owner: i32,
                lp_name: *const u16,
            ) -> isize;
            fn GetLastError() -> u32;
        }
        const ERROR_ALREADY_EXISTS: u32 = 183;

        let name: Vec<u16> = "Local\\mylittlechart_single_instance\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        let is_ota_restart = std::env::args().any(|a| a == "--wait-pid");
        if !is_ota_restart && handle != 0 && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            eprintln!("[App] another instance is already running — exiting.");
            std::process::exit(0);
        }
        if handle == 0 {
            eprintln!("[App] WARNING: CreateMutexW failed (error {}), continuing without single-instance guard", unsafe { GetLastError() });
        }
        handle // keep alive for process lifetime
    };

    eprintln!("[App] chart-app-vello v{}", env!("CARGO_PKG_VERSION"));
    println!("chart-app-vello v{}", env!("CARGO_PKG_VERSION"));
    println!("===============");
    println!("Controls:");
    println!("  Left drag : Pan chart");
    println!("  Scroll    : Zoom");
    println!("  Escape    : Cancel / close modal");
    println!("  Ctrl+S    : Screenshot (copied to clipboard + saved to Pictures/Screenshots)");
    println!();

    // Initialize diagnostics (logging, crash reporting, memory watchdog).
    // The guard must live until the end of main() to keep the log file writer alive.
    let log_dir = diagnostics::default_log_dir();
    let _diagnostics = diagnostics::init(&log_dir, env!("CARGO_PKG_VERSION"));

    // Start memory watchdog — logs warnings via tracing regardless of whether
    // the receiver is consumed. UI toast support can be added later.
    let _mem_warnings = diagnostics::watchdog::start();

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll); // Will be updated per-frame in about_to_wait()

    // Create the shared series map BEFORE the bridge so both share the same
    // Arc-backed registry.  DataBridge holds a clone for its async fetch tasks;
    // App::new passes it to BarService so persistence + live data share one map.
    let shared_series: bar_service::SharedSeriesMap =
        std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));

    // Create DataBridge ONCE — tokio runtime + connector pool shared by all windows.
    // The broadcast receiver (`_live_rx`) is dropped here: we no longer subscribe
    // to it at the app level.  Per-window ChartApp instances each subscribe via
    // `bridge.add_listener()`.  The mpsc `connector_ready_rx` is the lightweight
    // channel used by `tick_app_state` to handle ConnectorReady without touching
    // the broadcast buffer.
    let (bridge, _live_rx, connector_ready_rx) = live_data::DataBridge::new(shared_series.clone());
    let bridge = std::sync::Arc::new(bridge);

    // Detect startup mode BEFORE loading the user manager.
    let profile_dir = zengeld_chart::active_profile_data_dir();
    let has_profile = profile_dir.join("profile.json").exists();
    let has_salt = profile_dir.join("salt.hex").exists();
    let has_vault = profile_dir.join("vault.enc").exists();

    // Startup scenarios:
    // 1. First run: no profile.json → show full wizard (page 0)
    // 2. vault.enc exists: profile.json readable, vault unlock overlay for credentials
    // 3. No vault.enc, no salt: plaintext-only install or migration needed
    let is_first_run = !has_profile;

    // Show vault unlock overlay when vault.enc exists (credentials need decryption).
    // The app is fully functional without it (presets, tabs, watchlists are plaintext),
    // but API keys and exchange credentials will be unavailable until unlocked.
    let needs_vault_unlock = has_vault && has_salt;
    // has_salt && !has_vault = incomplete profile (created but passphrase never set)
    // !has_salt && !has_vault = old plaintext profile (pre-ZT)
    let needs_migration = has_profile && !has_vault;

    if is_first_run {
        eprintln!("[App] first-run detected — welcome wizard will be shown");
    }
    if needs_migration {
        eprintln!("[App] plaintext profile detected — migration wizard will be shown");
    }

    // Load ProfileManager (profile + templates + presets + snapshots) once at startup.
    // All windows share this loaded state — no per-window disk reads.
    let mut profile_manager = zengeld_chart::ProfileManager::load(None);

    // Record this launch (was previously done per-window
    // in load_user_state(); now done once here at the application level).
    profile_manager.profile.record_launch(env!("CARGO_PKG_VERSION"));
    // profile.json is always plaintext — safe to save at startup.
    // vault.enc is NOT written here (no key yet) — credentials are untouched.
    if let Err(e) = profile_manager.save_profile() {
        eprintln!("[App] failed to save profile at startup: {}", e);
    }

    let profile = profile_manager.profile.clone();
    let saved_windows = profile.windows.clone();

    let symbol = std::env::args().nth(1).unwrap_or_else(|| "BTCUSDT".to_string());
    let mut app = App::new(&symbol, bridge, shared_series, saved_windows, profile, profile_manager, connector_ready_rx, is_first_run, needs_vault_unlock, needs_migration);
    event_loop.run_app(&mut app).expect("Event loop error");
}
