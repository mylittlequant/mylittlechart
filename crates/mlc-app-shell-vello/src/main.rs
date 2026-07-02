//! chart-app-vello — minimal winit + vello runner for chart-app
//!
//! Supports multiple windows sharing a single DataBridge (tokio runtime +
//! connector pool).  Each window has its own ChartApp with independent
//! tabs/presets but receives live updates via broadcast channels.
//! Creates windows on demand; closing the last window exits the process.

mod agent;
mod app_state;
mod chrome;
pub mod keychain;
mod platform;
mod render;
mod screenshot;
mod tooltip;
mod window;

#[cfg(target_os = "windows")]
use platform::win32::{win32_border, win32_capture};

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use web_time::SystemTime;
use web_time::UNIX_EPOCH;

// ── Pipelined GPU rendering ──────────────────────────────────────────────────
//
// The render thread owns no GPU resources itself.  Instead it receives a list
// of raw `PerWindowState` pointers (as `usize`) from the main thread and calls
// `submit_window_gpu` on each.  The main thread builds the next frame's scene
// into `pw.scene` while the GPU thread renders the previous frame's scene from
// `pw.gpu_scene`.  A swap (`std::mem::swap(&mut pw.scene, &mut pw.gpu_scene)`)
// happens just before each GPU submit, so the two threads never touch the same
// `Scene` simultaneously.

use render::gpu_thread::{GpuCommand, GpuDone};

use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::{self, PresentMode};
use vello::{AaSupport, Renderer, RendererOptions, Scene};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Icon, Window, WindowId},
};

/// User-defined events sent from background threads to the winit event loop.
enum UserEvent {
    /// Off-thread promote worker finished building all live ChartApp instances.
    PromoteReady(Vec<PromoteWindowResult>),
}

/// Per-window data returned by the off-thread promote worker.
struct PromoteWindowResult {
    winit_id: WindowId,
    chart: Box<mlc_app::ChartApp>,
    window_id: String,
    /// Presets that existed only in the chart (new_window created them) but
    /// were absent from app_state at spawn time — must be merged back on the
    /// main thread.
    new_presets: Vec<(String, mlc_core::preset::preset::ChartPreset)>,
    active_preset_id: String,
    open_tabs: Vec<String>,
}
use sysinfo::{Pid, ProcessesToUpdate, System};

use app_state::AppState;


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
    /// Cached overlay sub-scene — crosshair, axis cursor labels, tooltip,
    /// drawing preview.  Rebuilt when `overlay_dirty` (every mouse move) —
    /// cheap, a few lines + labels — WITHOUT rebuilding the heavy static
    /// `chart_scene`.
    overlay_scene: vello::Scene,
    /// Rebuilt-overlay request.  Set by CursorMoved (hover) instead of
    /// chart_dirty so moving the cursor never rebuilds candles/indicators.
    overlay_dirty: bool,
    // ChartApp — per-window tabs/presets, shared DataBridge via broadcast
    chart: mlc_app::ChartApp,
    // Input state
    last_mouse_pos: (f64, f64),
    mouse_pressed: bool,
    drag_start_pos: Option<(f64, f64)>,
    last_drag_pos: Option<(f64, f64)>,
    last_click: Option<(web_time::Instant, f64, f64)>,
    screenshot_pending: bool,
    /// Pending agent screenshot requests — each entry is `(chart_id, response_tx)`.
    ///
    /// Accumulated by [`drain_agent_commands`] and processed at the end of
    /// each render pass, after the GPU frame has been submitted.
    pending_agent_screenshots: Vec<(u64, tokio::sync::oneshot::Sender<Result<mlc_backend_agent_server::state::ScreenshotData, String>>)>,
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
    /// When this window was created. Used to gate first visibility: we keep a
    /// freshly-promoted (post-login) window hidden until its chart has bars to
    /// draw, so the user never sees an empty chart flash for ~2s while data
    /// loads. A timeout fallback (created_at.elapsed()) shows the window anyway
    /// if data never arrives, so it can't stay invisible forever.
    created_at: web_time::Instant,
    /// When true this window is a skeleton placeholder (shown while vault unlock
    /// or first-run wizard is pending).  Skeleton windows suppress tab/toolbar
    /// rendering and chart content — only chrome window controls are drawn.
    skeleton: bool,
    /// Active render backend for this window — synced from `App.render_backend`
    /// each frame before the parallel scene-build phase.
    render_backend: mlc_app_sidebar_content::state::RenderBackend,
    /// Instanced renderer for the wGPU backend (created lazily).
    instanced_renderer: Option<uzor_render_wgpu_instanced::InstancedRenderer>,
    /// Unified draw command list from the last chart render (instanced backend).
    /// Preserves painter's z-order — later entries draw on top of earlier ones.
    instanced_commands: Vec<uzor_render_wgpu_instanced::DrawCmd>,
    /// GPU-side copy of draw commands (double-buffered like scene/gpu_scene).
    /// The GPU thread consumes this while the main thread fills `instanced_commands`.
    gpu_instanced_commands: Vec<uzor_render_wgpu_instanced::DrawCmd>,
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
    hybrid_ctx: Option<uzor_render_vello_hybrid::VelloHybridRenderContext>,
    /// GPU-side double-buffered copy for the GPU thread.
    gpu_hybrid_ctx: Option<uzor_render_vello_hybrid::VelloHybridRenderContext>,
    /// Whether `window.set_visible(true)` has been called yet.
    ///
    /// Windows are created with `with_visible(false)` to avoid the white-flash
    /// that appears before the first GPU frame is rendered.  After the first
    /// `GpuDone` is received (confirming the frame was presented), the window
    /// is made visible.
    visible_set: bool,
    /// Reference instant used to compute monotonic milliseconds for chrome tooltip timing.
    chrome_tooltip_start: web_time::Instant,
    /// Tooltip state for toolbar button hover tooltips (left/top/right/bottom strips).
    toolbar_tooltip: tooltip::TooltipState,
    /// True when the window was minimized (detected via Resized with 0x0 size).
    /// Cleared on the first non-zero resize after minimization, which triggers
    /// a snap-to-end so the viewport is back at the latest bar after restore.
    was_minimized: bool,
    /// Set when the off-thread promote worker is building this window's live
    /// ChartApp. Drives the animated spinner in the skeleton frame. Cleared
    /// in scene_builder on the first frame that actually composites live chart
    /// content (NOT in finish_promote) — otherwise the spinner vanished a
    /// frame before the chart was built, leaving a blank panel during the
    /// first live tick's event-backlog drain.
    spinner_start: Option<web_time::Instant>,
    /// True from finish_promote until the first live chart scene is built and
    /// composited. While set, the spinner is held even though `skeleton` is
    /// already false, so the spinner masks the first-frame work instead of
    /// blinking out early.
    awaiting_first_live_frame: bool,

    // ── mlc-app-input-shell integration ───────────────────────────────────────
    /// Portable input state (mirror of the scattered fields above).
    /// Written by process_event; read back into PerWindowState fields in event_handler.
    shell_input:   mlc_app_input_shell::InputState,
    /// Dirty flags for this event-loop iteration (synced back to pw fields).
    shell_dirty:   mlc_app_input_shell::DirtyFlags,
    /// Per-event-loop-iteration output from process_event (synced back to pw fields).
    dispatch_out:  mlc_app_input_shell::DispatchOut,
}

/// A pending request to open a new window.
struct SpawnRequest {
    /// The window that spawned this request — used for cascade placement.
    cascade_from: Option<WindowId>,
}


/// Application state — owns all per-window state and the shared render context.
struct App<'s> {
    render_cx: RenderContext,
    windows: HashMap<WindowId, PerWindowState>,
    /// Queue of window spawn requests, drained in about_to_wait.
    pending_spawns: Vec<SpawnRequest>,
    /// Set to true when ALL windows should close.
    close_all_requested: bool,
    /// Live-data runtime — owns the tokio thread-pool for bridge background tasks.
    ///
    /// Kept alive for the lifetime of `App` so that background tasks spawned by
    /// the bridge continue running. Dropped when `App` drops (on process exit).
    live_data_rt: tokio::runtime::Runtime,
    /// Handle into `live_data_rt` — cheap to clone, used for spawning tasks and
    /// as an executor context for blocking calls like Telegram get_updates.
    live_data_handle: tokio::runtime::Handle,
    /// Shared DataBridge — connector pool, created once at startup.
    bridge: std::sync::Arc<mlc_backend_data_bridge::DataBridge>,
    /// Saved window states loaded from profile at startup — used in resumed() to restore windows.
    saved_windows: Vec<mlc_core::WindowState>,
    /// User profile loaded once at startup.
    profile: mlc_core::UserProfile,
    /// ProfileManager — encapsulated profile management layer (presets, templates,
    /// snapshots, vault key). Loaded once at startup and shared across all windows.
    /// Each `new_window()` call clones the relevant fields instead of re-reading
    /// from disk, eliminating redundant disk I/O when multiple windows are opened.
    profile_manager: mlc_app::user_manager::ProfileManager,
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
    app_connector_ready_rx: mlc_backend_data_bridge::ConnectorReadyReceiver,
    _phantom: std::marker::PhantomData<&'s ()>,

    /// Shared state for the internal Agent API server.
    agent_state: Option<std::sync::Arc<mlc_backend_agent_server::AgentState>>,
    /// Wall-clock time of the last per-second maintenance pass (snapshots, sysinfo).
    last_indicator_snapshot: web_time::Instant,
    /// Round-robin cursor for agent snapshot rebuilds. The four snapshots
    /// (indicator/terminal/watchlist/connector) each clone large amounts of
    /// state; building all four in one frame stalls it 20-30ms. We build one
    /// per maintenance pass instead, spreading the cost across frames.
    agent_snapshot_phase: u8,

    /// Alert delivery engine (Telegram, webhook, toast).
    mlc_alert_delivery: Option<mlc_alert_delivery::AlertDelivery>,
    /// Receiver for toast notifications from the delivery engine.
    toast_rx: Option<tokio::sync::mpsc::UnboundedReceiver<mlc_alert_delivery::ToastNotification>>,
    /// Active toast notifications to render as overlays.
    active_toasts: Vec<mlc_alert_delivery::ToastNotification>,

    /// Frame timing — last frame's Instant for FPS calculation.
    last_frame_instant: web_time::Instant,
    /// Rolling FPS average (exponential moving average).
    fps_ema: f64,
    /// Last frame time in ms.
    last_frame_time_ms: f64,
    /// Per-frame interval jitter accumulators (reset each timing report window).
    /// Used by the [PERF] line to expose pacing jitter, not just the average FPS.
    dt_min_ms: f64,
    dt_max_ms: f64,
    dt_samples: u32,
    /// Current FPS limit (0 = unlimited/Poll, otherwise WaitUntil target).
    fps_limit: u32,
    /// Current MSAA sample count (0=off, 4, 8, 16).
    msaa_samples: u8,
    /// Whether frame timing logs are printed to stderr (toggled from Performance panel).
    perf_log_enabled: bool,
    /// Current selected render backend.
    render_backend: mlc_app_sidebar_content::state::RenderBackend,
    /// True if no backend was explicitly saved — triggers auto-detection on first GPU info.
    backend_auto_detect: bool,
    /// Latest CPU/RAM snapshot produced by the background sampler thread.
    ///
    /// The background thread owns its own `sysinfo::System`, refreshes it once
    /// per second, and writes here. The main thread reads the cached values
    /// instead of calling the expensive `refresh_*` syscalls on the frame thread.
    perf_sys_sample: std::sync::Arc<std::sync::Mutex<PerfSysSample>>,
    /// Handle to the background sysinfo sampler thread (kept to avoid joining
    /// before shutdown, though we never explicitly join it — drop is fine).
    _perf_sys_thread: std::thread::JoinHandle<()>,
    /// GPU adapter name (queried once at startup).
    gpu_name: String,
    /// GPU driver info (queried once at startup).
    gpu_driver: String,
    /// Frame counter for timing reports.
    frame_count: u64,
    /// Last time a timing summary was printed.
    last_timing_report: web_time::Instant,
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
    /// Total wall-clock time (µs) spent inside the last about_to_wait call.
    /// Distinct from dt (interval between frames): this is the actual work time.
    /// Small about_to_wait_us + large dt → loop was idle/WaitUntil, not overloaded.
    /// Large about_to_wait_us → genuinely heavy frame.
    cached_about_to_wait_us: u64,
    /// Time (µs) the main thread blocked in Step 1 waiting for the previous
    /// frame's GpuDone. This is an otherwise-invisible stall: it appears in
    /// neither cached_scene_us (CPU build) nor cached_gpu_us (GPU submit). A
    /// large value means the GPU thread is the bottleneck (CPU build finished
    /// first and is now starving on the pipeline).
    cached_gpu_wait_us: u64,
    /// µs in vello render_to_texture (compute encode + submit) — GPU half 1.
    cached_render_tex_us: u64,
    /// µs in get_current_texture + blit + present — GPU half 2 (swapchain).
    cached_present_us: u64,
    /// Startup-freeze diagnostics: the `Instant` at which the previous
    /// about_to_wait pass finished its real work (after the FPS-cap guard). Used
    /// to measure the GAP between consecutive worked frames. A multi-hundred-ms
    /// gap with a tiny atw means the main thread vanished BETWEEN frames — stuck
    /// on a lock, a sync IO, or the promote — not in render. This is the freeze
    /// signature (atw≈0, n=2) the `dt` metric hides. `None` until the first pass.
    last_atw_end: Option<web_time::Instant>,
    /// One-shot: elapsed_ms stamp of promote swap completion. Consumed by the
    /// next about_to_wait pass to log the first-live-frame cost.
    promote_end_ms: Option<u128>,
    /// Amortized post-promote warm-up queue: one background-tab series is
    /// warmed per about_to_wait pass (each ensure_connector blocks ~120ms —
    /// a whole batch at once stalls the main thread right after the chart
    /// appears). Filled when `promote_end_ms` is consumed.
    pending_warm_targets: Vec<(String, String, mlc_core::state::Timeframe, String)>,

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

    /// Proxy for sending `UserEvent` messages from background threads to the
    /// winit event loop. Set once in `App::new` or lazily before first use.
    promote_proxy: Option<EventLoopProxy<UserEvent>>,

    /// True while the off-thread promote worker is running. Prevents a second
    /// promote from starting (double-unlock guard) and drives spinner redraws.
    promote_pending: bool,

    /// True when `profile.json` did not exist at startup (first-run).
    /// Causes the Welcome Wizard overlay to appear on the first window created.
    is_first_run: bool,

    /// True when `salt.hex` exists (encrypted profile) but no vault key has been derived yet.
    /// Causes the Vault Unlock overlay to appear on the first window created.
    needs_vault_unlock: bool,

    /// True when a plaintext profile exists without `salt.hex` — user must set a passphrase
    /// to migrate to encrypted storage.  Shows wizard at page 1 (passphrase).
    needs_migration: bool,

    /// Profile ID to switch to at the end of the current `about_to_wait` pass.
    ///
    /// Set by the `profile_switch` command handler; consumed once by
    /// `execute_profile_switch` which tears down existing windows and rebuilds
    /// the app from the new profile without requiring a restart.
    pending_profile_switch: Option<String>,
    /// Vault key pre-validated during Branch A (pre-switch passphrase check), so
    /// `execute_profile_switch` can inject it directly and skip the unlock screen.
    pending_switch_vault_key: Option<mlc_vault::vault::VaultKey>,
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
    pending_switch_after_recovery: Option<(String, mlc_vault::vault::VaultKey)>,

    /// Master key recovered from `recovery_key.enc` during a `recovery_unlock:` command.
    ///
    /// Stored temporarily so the `set_new_passphrase:` handler can re-derive a new
    /// master key hierarchy and re-encrypt the vault without needing the old passphrase.
    /// Cleared immediately after `set_new_passphrase:` completes (or on any error).
    pending_recovery_master_key: Option<mlc_vault::crypto::MasterKey>,

    /// True when the user confirms they have saved the new recovery key after a
    /// recovery re-key.  Skeleton promote is deferred until this flag is set.
    pending_promote_after_recovery_key: bool,
}

/// Snapshot of system CPU/RAM metrics produced by the background sampler thread.
///
/// The background thread owns its own [`sysinfo::System`] and refreshes it once
/// per second, storing results here. The main thread reads this (cheap) instead
/// of calling the expensive `refresh_*` sysinfo syscalls on the frame thread.
#[derive(Default)]
struct PerfSysSample {
    /// Per-core CPU usage percentages (one entry per logical CPU).
    per_core_cpu: Vec<f32>,
    /// Process CPU usage as reported by sysinfo (sum-of-threads, can exceed 100%).
    process_cpu: f32,
    /// Process RSS memory in bytes.
    process_mem_bytes: u64,
    /// Total system RAM in bytes.
    total_mem_bytes: u64,
}

/// Render toast notifications as semi-transparent overlays in the top-right corner.
///
/// Toasts are stacked from top-right downward. Each toast fades out during its
/// final 20% of display time.  The drawing is done with the flat-rect API from
/// [`uzor::render::RenderContext`] so it works with any backend.
fn render_toasts(
    ctx: &mut dyn uzor::render::RenderContext,
    toasts: &[mlc_alert_delivery::ToastNotification],
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

    let now = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
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

use render::scene_builder::build_window_scene;
use render::gpu_submit::submit_window_gpu_from_gpu_scene;
impl App<'_> {
    fn new(
        bridge: std::sync::Arc<mlc_backend_data_bridge::DataBridge>,
        live_data_rt: tokio::runtime::Runtime,
        live_data_handle: tokio::runtime::Handle,
        saved_windows: Vec<mlc_core::WindowState>,
        profile: mlc_core::UserProfile,
        profile_manager: mlc_app::user_manager::ProfileManager,
        app_connector_ready_rx: mlc_backend_data_bridge::ConnectorReadyReceiver,
        is_first_run: bool,
        needs_vault_unlock: bool,
        needs_migration: bool,
    ) -> Self {
        let app_state = AppState::from_profile(&profile, profile_manager.presets.clone(), profile_manager.snapshots.clone(), profile_manager.template_manager.clone(), profile_manager.vault_key);

        // Start the internal Agent API server on the DataBridge's tokio runtime.
        let agent_state = std::sync::Arc::new(mlc_backend_agent_server::AgentState::new(
            bridge.clone(),
            env!("CARGO_PKG_VERSION").to_string(),
        ));
        // ── Populate indicator catalog (one-time, at startup) ─────────────────
        {
            use mlc_backend_agent_server::state::{
                IndicatorCatalogSnapshot, CatalogIndicator, CatalogParam, CatalogOutput,
            };
            use mlc_backend::indicator::IndicatorManager as TmpMgr;
            use mlc_backend_indicator_engine::{IndicatorParamType, IndicatorParamValue};

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
            use mlc_backend_agent_server::state::{PrimitiveCatalogSnapshot, CatalogPrimitive};
            use mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry;
            use mlc_core::drawing::primitives_v2::{PrimitiveKind, ClickBehavior};

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
            Some(mlc_backend_agent_server::start_server(
                agent_state.clone(),
                &live_data_handle,
                app_state.server_port,
            ))
        } else {
            eprintln!("[App] Agent API server disabled in settings");
            None
        };

        // Initialize alert delivery engine. AlertDelivery::new spawns a tokio
        // task, so we must enter the live-data runtime context first.
        let (alert_delivery_engine, toast_rx) = {
            let _guard = live_data_handle.enter();
            mlc_alert_delivery::AlertDelivery::new(profile.notification_settings.clone())
        };


        // Apply saved language preference from profile.
        {
            let lang = match profile.language.as_str() {
                "ru" => mlc_core::Language::Ru,
                _    => mlc_core::Language::En,
            };
            mlc_core::set_language(lang);
            eprintln!("[App] startup language: {}", profile.language);
        }


        // Load device settings once for App struct initialisation.
        let ds_init = mlc_core::user_profile::DeviceSettings::load();

        // ── Background sysinfo sampler ────────────────────────────────────────
        // sysinfo refresh_* calls cost 15-25 ms on Windows. We move them off the
        // main/render thread into a dedicated 1 s sampler that writes into a shared
        // Mutex<PerfSysSample>. The main thread reads the cached snapshot cheaply.
        let perf_sys_sample_arc: std::sync::Arc<std::sync::Mutex<PerfSysSample>> =
            std::sync::Arc::new(std::sync::Mutex::new(PerfSysSample::default()));
        let perf_sys_thread = {
            let shared = perf_sys_sample_arc.clone();
            let self_pid = Pid::from_u32(std::process::id());
            std::thread::Builder::new()
                .name("perf-sys-sampler".into())
                .spawn(move || {
                    let mut sys = System::new();
                    // Prime the CPU usage baseline (sysinfo needs two samples to
                    // compute a meaningful delta; the first call returns 0 for all cores).
                    sys.refresh_cpu_usage();
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        sys.refresh_cpu_usage();
                        sys.refresh_memory();
                        sys.refresh_processes(
                            ProcessesToUpdate::Some(&[self_pid]),
                            false,
                        );
                        let per_core_cpu: Vec<f32> =
                            sys.cpus().iter().map(|c| c.cpu_usage()).collect();
                        let process_cpu = sys.process(self_pid)
                            .map(|p| p.cpu_usage())
                            .unwrap_or(0.0);
                        let process_mem_bytes = sys.process(self_pid)
                            .map(|p| p.memory())
                            .unwrap_or(0);
                        let total_mem_bytes = sys.total_memory();
                        if let Ok(mut sample) = shared.lock() {
                            sample.per_core_cpu = per_core_cpu;
                            sample.process_cpu = process_cpu;
                            sample.process_mem_bytes = process_mem_bytes;
                            sample.total_mem_bytes = total_mem_bytes;
                        }
                    }
                })
                .expect("perf-sys-sampler thread spawn must succeed")
        };

        Self {
            render_cx: RenderContext::new(),
            windows: HashMap::new(),
            pending_spawns: Vec::new(),
            close_all_requested: false,
            live_data_rt,
            live_data_handle,
            bridge,
            saved_windows,
            profile,
            profile_manager,
            last_focused: None,
            app_state,
            app_connector_ready_rx,
            _phantom: std::marker::PhantomData,
            agent_state: Some(agent_state),
            last_indicator_snapshot: web_time::Instant::now(),
            agent_snapshot_phase: 0,
            mlc_alert_delivery: Some(alert_delivery_engine),
            toast_rx: Some(toast_rx),
            active_toasts: Vec::new(),
            last_frame_instant: web_time::Instant::now(),
            fps_ema: 60.0,
            last_frame_time_ms: 16.0,
            dt_min_ms: f64::INFINITY,
            dt_max_ms: 0.0,
            dt_samples: 0,
            fps_limit: ds_init.fps_limit,
            msaa_samples: ds_init.msaa_samples,
            perf_log_enabled: std::env::var("MLC_PERF_LOG").is_ok(),
            render_backend: {
                use mlc_app_sidebar_content::state::RenderBackend;
                // Only VelloGpu and VelloCpu are exposed in the OSS build.
                // Other persisted variants fall back to VelloCpu so legacy
                // device_settings.json files keep loading without errors.
                match ds_init.render_backend {
                    Some(mlc_core::user_profile::device_settings::RenderBackend::VelloGpu) => RenderBackend::VelloGpu,
                    Some(mlc_core::user_profile::device_settings::RenderBackend::VelloCpu) => RenderBackend::VelloCpu,
                    Some(_) => RenderBackend::VelloCpu,
                    None => RenderBackend::VelloGpu, // will be overridden by auto-detect
                }
            },
            backend_auto_detect: ds_init.render_backend.is_none(),
            perf_sys_sample: perf_sys_sample_arc,
            _perf_sys_thread: perf_sys_thread,
            gpu_name: String::new(),
            gpu_driver: String::new(),
            frame_count: 0,
            last_timing_report: web_time::Instant::now(),
            cached_connector_count: 0,
            cached_scene_us: 0,
            cached_gpu_us: 0,
            cached_about_to_wait_us: 0,
            cached_gpu_wait_us: 0,
            cached_render_tex_us: 0,
            cached_present_us: 0,
            last_atw_end: None,
            promote_end_ms: None,
            pending_warm_targets: Vec::new(),
            gpu_cmd_tx: None,
            gpu_done_rx: None,
            gpu_thread: None,
            gpu_frame_pending: false,
            promote_proxy: None,
            promote_pending: false,
            is_first_run,
            needs_vault_unlock,
            needs_migration,
            pending_profile_switch: None,
            pending_switch_vault_key: None,
            pending_new_profile_id: None,
            pending_skeleton_promote: false,
            pending_switch_after_recovery: None,
            pending_recovery_master_key: None,
            pending_promote_after_recovery_key: false,
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
        restore: Option<&mlc_core::WindowState>,
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

        // Skeleton = loading screen shown while vault is being unlocked /
        // wizard is run.
        //
        // - needs_vault_unlock: the user has a saved profile, we just need
        //   the passphrase. Re-use the saved window geometry so the unlock
        //   prompt appears exactly where the user left the app.
        // - is_first_run / needs_migration: no saved geometry yet, center
        //   a 1200x800 window on the primary monitor.
        let skeleton = self.needs_vault_unlock || self.is_first_run || self.needs_migration;
        if skeleton {
            // For an existing profile prefer the saved geometry over the
            // centered placeholder.
            let restored = if self.needs_vault_unlock {
                self.saved_windows.first().and_then(|ws| {
                    match (ws.x, ws.y, ws.width, ws.height) {
                        (Some(x), Some(y), Some(w), Some(h)) => Some((x, y, w, h)),
                        _ => None,
                    }
                })
            } else {
                None
            };

            if let Some((x, y, w, h)) = restored {
                use winit::dpi::Position;
                attrs = attrs.with_position(Position::Physical(
                    winit::dpi::PhysicalPosition::new(x, y),
                ));
                attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(w, h));
            } else if let Some(monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
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

            // Auto-detect backend based on GPU capabilities (first launch only)
            if self.backend_auto_detect {
                use mlc_app_sidebar_content::state::RenderBackend;
                let recommended = match info.device_type {
                    wgpu::DeviceType::DiscreteGpu => RenderBackend::VelloGpu,
                    wgpu::DeviceType::IntegratedGpu => RenderBackend::VelloGpu,
                    wgpu::DeviceType::VirtualGpu => RenderBackend::VelloCpu,
                    wgpu::DeviceType::Cpu => RenderBackend::VelloCpu,
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
                    RenderBackend::InstancedWgpu => (90,     8),
                    RenderBackend::VelloHybrid   => (90,     8),
                    // TinySkia retained as a variant for serde back-compat
                    // but never selected here; treat as VelloCpu defaults.
                    RenderBackend::TinySkia      => (30,     0),
                };

                // Save auto-detected choice and perf defaults so next launch skips auto-detect.
                {
                    let mut ds = mlc_core::user_profile::DeviceSettings::load();
                    ds.render_backend = Some(match recommended {
                        RenderBackend::VelloGpu => mlc_core::user_profile::device_settings::RenderBackend::VelloGpu,
                        RenderBackend::InstancedWgpu => mlc_core::user_profile::device_settings::RenderBackend::InstancedWgpu,
                        RenderBackend::VelloCpu => mlc_core::user_profile::device_settings::RenderBackend::VelloCpu,
                        RenderBackend::VelloHybrid => mlc_core::user_profile::device_settings::RenderBackend::VelloHybrid,
                        RenderBackend::TinySkia => mlc_core::user_profile::device_settings::RenderBackend::TinySkia,
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
        screenshot::add_copy_src_to_target_texture(&mut surface, device);

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
        let mut chart = mlc_app::ChartApp::new_window(
            self.bridge.clone(),
            window_id.clone(),
            restore,
            &self.profile,
            &self.profile_manager,
            skeleton,
        );
        // Attach this chart's backend to the AgentState so /api/v1/apps
        // routes resolve. Every ChartApp in this process shares one
        // Arc<BackendCore>, so the last writer wins safely.
        if let Some(ref agent_state) = self.agent_state {
            agent_state.attach_core(chart.core.clone());
        }
        // Apply the app-level theme so new windows start with the correct preset.
        if !self.app_state.theme_preset.is_empty() {
            chart.panel_app.theme_manager.set_preset(&self.app_state.theme_preset);
        }
        // Initialise the new window directly from AppState so it has current data
        // without waiting for the next dirty-flag sync pass.
        chart.core.watchlists_mut().replace_all(self.app_state.watchlist_manager.clone());
        chart.watchlist_ui = self.app_state.watchlist_ui.clone();
        chart.sidebar_state.connector_enabled = self.app_state.connector_enabled.clone();
        // Merge AppState presets into window — don't replace, to keep
        // any Untitled preset that new_window() just created.
        {
            let mut presets_mut = chart.core.presets_mut();
            for (k, v) in self.app_state.presets.iter() {
                presets_mut.insert_if_absent(k.clone(), || v.clone());
            }
        }
        // Sync back: if new_window() created an Untitled preset for a fresh profile,
        // propagate it to app_state so chrome tabs and preset list can see it.
        for (k, v) in chart.core.presets().iter() {
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
        chart.core.user_mut().set_snapshots(self.app_state.snapshots.clone());
        // Apply persisted last-used drawing styles to all bubbles' drawing
        // sessions in this new window (per-bubble model — styles live on
        // DataBubble.last_used_style, loaded once per distinct bubble).
        let drawing_styles = self.app_state.snapshots.last_used_drawing_styles.clone();
        let all_bids_styles: Vec<_> = chart.core.bubbles().bubble_ids().collect();
        {
            let mut dreg = chart.core.drawings_mut();
            for bid in all_bids_styles {
                dreg.load_last_styles(bid, &drawing_styles);
            }
        }
        chart.core.templates_mut().replace_all(self.app_state.template_manager.clone());
        // Sync server settings so the User Settings modal shows current state.
        chart.panel_app.user_settings_state.server_enabled = self.app_state.server_enabled;
        chart.panel_app.user_settings_state.server_port = self.app_state.server_port;
        chart.panel_app.user_settings_state.server_status = if self.app_state.server_enabled {
            "running".to_string()
        } else {
            "stopped".to_string()
        };
        // Sync language preference from the loaded profile.
        chart.panel_app.user_settings_state.language =
            self.profile_manager.profile.language.clone();
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
            if let Some(index) = mlc_core::load_profile_index() {
                let profiles_dir = mlc_core::active_profile_data_dir()
                    .parent()
                    .map(|p| p.to_path_buf());
                uss.available_profiles = index.profiles.iter().map(|m| {
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone())
                }).collect();
                uss.profiles_with_vault_status = index.profiles.iter().map(|m| {
                    let has_vault = if let Some(ref pd) = profiles_dir {
                        pd.join(&m.dir_name).join("vault.enc").exists()
                    } else {
                        false
                    };
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone(), has_vault)
                }).collect();
            } else {
                // No index yet — synthesize a single entry from the current profile.
                uss.available_profiles = vec![(
                    uss.profile_id.clone(),
                    uss.profile_display_name.clone(),
                    uss.profile_avatar.clone(),
                )];
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
            use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
            chart.panel_app.user_settings_state.needs_vault_unlock = true;
            chart.panel_app.user_settings_state.show_profile_manager = true;
            chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::ProfileList;
            chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
            chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
            // Populate profile vault status list
            if let Some(index) = mlc_core::load_profile_index() {
                let profiles_dir = mlc_core::active_profile_data_dir()
                    .parent()
                    .map(|p| p.to_path_buf());
                chart.panel_app.user_settings_state.profiles_with_vault_status = index.profiles.iter().map(|m| {
                    let has_vault = if let Some(ref pd) = profiles_dir {
                        pd.join(&m.id).join("vault.enc").exists()
                    } else {
                        false
                    };
                    (m.id.clone(), m.display_name.clone(), m.avatar.clone(), has_vault)
                }).collect();
            }
        }

        // Migration: existing plaintext profile without salt.hex.
        // Show the profile manager create passphrase page so the user sets a passphrase.
        // Their existing data will be encrypted on completion via save_all().
        if self.needs_migration {
            use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
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
            overlay_scene: Scene::new(),
            overlay_dirty: true,
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
            created_at: web_time::Instant::now(),
            skeleton,
            render_backend: mlc_app_sidebar_content::state::RenderBackend::VelloGpu,
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
            chrome_tooltip_start: web_time::Instant::now(),
            toolbar_tooltip: tooltip::TooltipState::with_delay(700.0),
            was_minimized: false,
            spinner_start: None,
            awaiting_first_live_frame: false,
            shell_input:  mlc_app_input_shell::InputState::default(),
            shell_dirty:  mlc_app_input_shell::DirtyFlags::default(),
            dispatch_out: mlc_app_input_shell::DispatchOut::default(),
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
        let profiles_with_vault: Vec<(String, String, String, bool)> = profiles
            .iter()
            .map(|p| (p.id.clone(), p.display_name.clone(), p.avatar.clone(), p.has_vault))
            .collect();
        let available: Vec<(String, String, String)> = profiles
            .iter()
            .map(|p| (p.id.clone(), p.display_name.clone(), p.avatar.clone()))
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
        if let Some(mut index) = mlc_core::load_profile_index() {
            index.active_profile_id = target_id.to_string();
            if let Err(e) = mlc_core::save_profile_index(&index) {
                eprintln!("[App] hot-reload: failed to save index: {}", e);
            }
        }

        // 3. Drop all existing windows.
        //    Removing from the HashMap drops PerWindowState which owns the winit
        //    Window (Arc) and the RenderSurface.  winit will handle OS cleanup.
        //    Block until the GPU thread releases the surfaces — it still holds
        //    raw pointers into target_view from the previous Submit.
        self.wait_for_gpu_frame_blocking();
        let window_ids: Vec<winit::window::WindowId> = self.windows.keys().copied().collect();
        for wid in window_ids {
            self.windows.remove(&wid);
        }
        self.last_focused = None;

        // 4. Determine whether the new profile needs vault unlock.
        //    active_profile_data_dir() now resolves to target_id's directory
        //    because the index was just updated and saved above.
        let new_profile_dir = mlc_core::active_profile_data_dir();
        let has_vault = new_profile_dir.join("vault.enc").exists();
        let has_salt  = new_profile_dir.join("salt.hex").exists();

        // 5. Load the new profile from disk.
        //    active_profile_data_dir() reads the index we just updated, so it
        //    will resolve to target_id's directory.
        self.profile_manager = mlc_app::user_manager::ProfileManager::load(None);
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
            use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
            for pw in self.windows.values_mut() {
                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                pw.chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.profile_id.clone();
                pw.chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.display_name.clone();
            }
        } else if !has_vault && !self.is_first_run {
            // Existing profile with no encryption — offer to set up
            use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
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

        eprintln!(
            "[App] hot-reload: profile switch complete — {} window(s) created",
            self.windows.len()
        );
    }

    /// Kick off the off-thread promote: gathers Send inputs, starts the spinner
    /// on each skeleton window, and spawns a worker thread that builds all live
    /// ChartApp instances. When done the worker sends `UserEvent::PromoteReady`
    /// through the event-loop proxy so the swap runs on the main thread.
    ///
    /// Safe to call when `promote_pending` is false. Ignored if the proxy is
    /// not available (should never happen in practice — the proxy is set before
    /// the first window is created).
    fn start_promote_async(&mut self) {
        let proxy = match self.promote_proxy.take() {
            Some(p) => p,
            None => {
                eprintln!("[App] start_promote_async: no proxy — cannot promote off-thread");
                return;
            }
        };

        let _promote_t0 = mlc_backend_data_bridge::elapsed_ms();
        eprintln!(
            "[App] skeleton → live: spawning worker for {} skeleton window(s) t={}ms (PROMOTE START)",
            self.windows.len(),
            _promote_t0,
        );

        self.promote_pending = true;

        // Start the spinner on every skeleton window AND immediately retire
        // any welcome / profile-manager screen still showing. The spinner is
        // composited on top of `chart_scene` — without retiring the welcome
        // form here, the cached chart_scene keeps drawing passphrase inputs
        // under the spinner ring until `finish_promote` swaps the ChartApp.
        // Owner sees "spinner over still-visible form" which reads as a UI
        // glitch. Force a clean redraw NOW so the user only sees the spinner
        // on the bare background until the live chart appears.
        let spinner_instant = web_time::Instant::now();
        for pw in self.windows.values_mut() {
            if pw.skeleton {
                pw.spinner_start = Some(spinner_instant);
                pw.chart.panel_app.user_settings_state.show_profile_manager = false;
                pw.chart.panel_app.user_settings_state.show_welcome_wizard = false;
                pw.chart_dirty = true;
                pw.window.request_redraw();
            }
        }

        // Collect per-window inputs that are Send + 'static.
        let window_ids: Vec<winit::window::WindowId> = self.windows.keys().copied().collect();
        let saved = self.saved_windows.clone();

        // Capture clones of everything the worker needs to build ChartApp.
        let bridge = self.bridge.clone();
        let profile = self.profile.clone();
        let profile_manager = self.profile_manager.clone();
        // AppState fields needed for the sync block (read-only in the worker).
        let theme_preset = self.app_state.theme_preset.clone();
        let watchlist_manager = self.app_state.watchlist_manager.clone();
        let watchlist_ui = self.app_state.watchlist_ui.clone();
        let connector_enabled = self.app_state.connector_enabled.clone();
        let app_presets = self.app_state.presets.clone();
        let snapshots = self.app_state.snapshots.clone();
        let template_manager = self.app_state.template_manager.clone();
        let server_enabled = self.app_state.server_enabled;
        let server_port = self.app_state.server_port;

        // Capture window sizes now (on the main thread — winit Arc<Window> is !Send).
        let window_sizes: Vec<(u32, u32, f64)> = window_ids.iter().map(|wid| {
            self.windows.get(wid)
                .map(|pw| {
                    let sz = pw.window.inner_size();
                    (sz.width, sz.height, pw.window.scale_factor())
                })
                .unwrap_or((1200, 800, 1.0))
        }).collect();

        std::thread::spawn(move || {
            // `DataBridge` now `enter()`s its captured runtime handle
            // internally on every `spawn_bg(...)` (see crate docs at the top
            // of `mlc-backend-data-bridge/src/bridge.rs`), so this worker thread does
            // NOT need a tokio guard — `bridge.*` calls inside
            // `ChartApp::new_window` self-host their reactor.
            let worker_t0 = web_time::Instant::now();
            let mut results: Vec<PromoteWindowResult> = Vec::with_capacity(window_ids.len());

            for (idx, &wid) in window_ids.iter().enumerate() {
                let restore: Option<&mlc_core::WindowState> = saved.get(idx);

                // Derive window_id the same way create_window does for live windows.
                let window_id = restore
                    .filter(|ws| !ws.window_id.is_empty())
                    .map(|ws| ws.window_id.clone())
                    .unwrap_or_else(|| {
                        format!(
                            "win_{}",
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                        )
                    });

                // Build the live ChartApp (the heavy ~1.7s part).
                let mut chart = mlc_app::ChartApp::new_window(
                    bridge.clone(),
                    window_id.clone(),
                    restore,
                    &profile,
                    &profile_manager,
                    false, // skeleton = false
                );

                // Apply the app-level theme.
                if !theme_preset.is_empty() {
                    chart.panel_app.theme_manager.set_preset(&theme_preset);
                }
                // Sync AppState into the new live window (read-only from captured clones).
                chart.core.watchlists_mut().replace_all(watchlist_manager.clone());
                chart.watchlist_ui = watchlist_ui.clone();
                chart.sidebar_state.connector_enabled = connector_enabled.clone();
                {
                    let mut presets_mut = chart.core.presets_mut();
                    for (k, v) in app_presets.iter() {
                        presets_mut.insert_if_absent(k.clone(), || v.clone());
                    }
                }
                chart.core.user_mut().set_snapshots(snapshots.clone());
                {
                    let drawing_styles = snapshots.last_used_drawing_styles.clone();
                    let all_bids_ls: Vec<_> = chart.core.bubbles().bubble_ids().collect();
                    let mut dreg = chart.core.drawings_mut();
                    for bid in all_bids_ls {
                        dreg.load_last_styles(bid, &drawing_styles);
                    }
                }
                chart.core.templates_mut().replace_all(template_manager.clone());
                chart.panel_app.user_settings_state.server_enabled = server_enabled;
                chart.panel_app.user_settings_state.server_port = server_port;
                chart.panel_app.user_settings_state.server_status = if server_enabled {
                    "running".to_string()
                } else {
                    "stopped".to_string()
                };
                chart.panel_app.user_settings_state.language =
                    profile_manager.profile.language.clone();
                {
                    let uss = &mut chart.panel_app.user_settings_state;
                    uss.profile_id = profile_manager.profile.profile_id.clone();
                    uss.runtime_profile_id = profile_manager.profile.profile_id.clone();
                    uss.profile_display_name = profile_manager.profile.display_name.clone();
                    uss.profile_avatar = profile_manager.profile.avatar.clone();
                    if let Some(index) = mlc_core::load_profile_index() {
                        let profiles_dir = mlc_core::active_profile_data_dir()
                            .parent()
                            .map(|p| p.to_path_buf());
                        uss.available_profiles = index.profiles.iter().map(|m| {
                            (m.id.clone(), m.display_name.clone(), m.avatar.clone())
                        }).collect();
                        uss.profiles_with_vault_status = index.profiles.iter().map(|m| {
                            let has_vault = if let Some(ref pd) = profiles_dir {
                                pd.join(&m.dir_name).join("vault.enc").exists()
                            } else {
                                false
                            };
                            (m.id.clone(), m.display_name.clone(), m.avatar.clone(), has_vault)
                        }).collect();
                    } else {
                        uss.available_profiles = vec![(
                            uss.profile_id.clone(),
                            uss.profile_display_name.clone(),
                            uss.profile_avatar.clone(),
                        )];
                    }
                }

                // Resize using the sizes captured on the main thread.
                let (w, h, scale) = window_sizes.get(idx).copied().unwrap_or((1200, 800, 1.0));
                let chrome_px = (chrome::CHROME_HEIGHT * scale) as u32;
                chart.resize(w, h.saturating_sub(chrome_px));

                // Collect presets that new_window created and that were absent
                // from app_presets — these must be merged back on the main thread.
                let new_presets: Vec<(String, mlc_core::preset::preset::ChartPreset)> = chart
                    .core
                    .presets()
                    .iter()
                    .filter(|(k, _)| !app_presets.contains_key(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                results.push(PromoteWindowResult {
                    winit_id: wid,
                    chart: Box::new(chart),
                    window_id,
                    new_presets,
                    active_preset_id: String::new(), // filled after chart is built
                    open_tabs: Vec::new(),            // filled after chart is built
                });
            }

            // Patch active_preset_id / open_tabs from the built charts.
            for r in results.iter_mut() {
                r.active_preset_id = r.chart.panel_app.active_preset_id.clone();
                r.open_tabs = r.chart.panel_app.open_tabs.clone();
            }

            eprintln!(
                "[App] promote worker: {} ChartApp(s) built in {}ms",
                results.len(),
                worker_t0.elapsed().as_millis(),
            );

            // Restore proxy ownership (we pass it back through the result so the
            // main thread can reuse it for future promotes after finish_promote).
            // Actually we just send the result — the main thread creates a new proxy
            // from the event loop at startup; one promote per session is typical.
            let _ = proxy.send_event(UserEvent::PromoteReady(results));
        });
    }

    /// Swap tail of promote — runs on the main thread in user_event when the
    /// worker delivers `UserEvent::PromoteReady`. Mirrors the old synchronous
    /// swap block exactly: wait_for_gpu_frame_blocking, swap chart in-place,
    /// flip skeleton flag, warm background tabs.
    fn finish_promote(
        &mut self,
        results: Vec<PromoteWindowResult>,
        event_loop: &ActiveEventLoop,
    ) {
        let swap_t0 = mlc_backend_data_bridge::elapsed_ms();

        // Block until any in-flight GPU frame finishes before touching chart state.
        self.wait_for_gpu_frame_blocking();
        let t_gpu_wait = mlc_backend_data_bridge::elapsed_ms();
        eprintln!(
            "[App] finish_promote: gpu-wait {}ms",
            t_gpu_wait.saturating_sub(swap_t0),
        );

        let saved = self.saved_windows.clone();
        let existing_count = self.windows.len();

        for r in results {
            // Write-back: merge new presets and profile fields that the worker
            // could not write because it only had read-only clones.
            for (k, v) in r.new_presets {
                self.app_state.presets.entry(k).or_insert(v);
            }
            if !r.active_preset_id.is_empty() {
                self.profile_manager.profile.active_preset_id = r.active_preset_id.clone();
                self.profile.active_preset_id = r.active_preset_id;
            }
            if !r.open_tabs.is_empty() {
                self.profile_manager.profile.open_tabs = r.open_tabs.clone();
                self.profile.open_tabs = r.open_tabs;
            }

            if let Some(pw) = self.windows.get_mut(&r.winit_id) {
                let t_swap_one = mlc_backend_data_bridge::elapsed_ms();
                // `pw.chart = *r.chart` would drop the OLD skeleton ChartApp
                // inline on the main thread (~144ms measured — Drop chain of
                // BubbleStore + IndicatorManager + AgentSessionManager + Arc
                // refcount fan-out on the bridge), freezing the spinner for
                // that interval. Hand the old chart to a background thread
                // to drop. ChartApp is Send (proven by the promote worker
                // itself), so this is safe.
                let old_chart = std::mem::replace(&mut pw.chart, *r.chart);
                std::thread::spawn(move || {
                    let _t = web_time::Instant::now();
                    drop(old_chart);
                    eprintln!(
                        "[App] finish_promote: off-thread drop of skeleton ChartApp took {}us",
                        _t.elapsed().as_micros(),
                    );
                });
                pw.window_id = r.window_id;
                pw.skeleton = false;
                pw.visible_set = true; // already on screen — don't hide again
                // Keep the spinner running: it is cleared in scene_builder on
                // the first frame that actually composites the live chart, so
                // it masks the first-live-tick event-backlog drain instead of
                // blinking out a frame early onto a blank panel.
                pw.awaiting_first_live_frame = true;
                // Invalidate all render caches so the live content draws next frame.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
                pw.overlay_dirty = true;
                pw.window.request_redraw();
                // Background tab warm-up is DEFERRED to the pass after the
                // first live frame (about_to_wait): warm_background_tabs
                // blocks the main thread ~1.2s (ensure_connector per series)
                // — measured 1206ms of the 1422ms post-spinner freeze. The
                // chart must appear first; the warm-up catches up one frame
                // later.
                eprintln!(
                    "[App] finish_promote: chart-swap (old drop off-thread) {}ms",
                    mlc_backend_data_bridge::elapsed_ms().saturating_sub(t_swap_one),
                );
            }
        }

        // If there are more saved windows than skeleton windows (unusual), create
        // the extras the normal way.
        if saved.len() > existing_count {
            for ws in saved.iter().skip(existing_count) {
                self.create_window(event_loop, Some(ws), None);
            }
        }

        self.promote_pending = false;
        self.promote_end_ms = Some(mlc_backend_data_bridge::elapsed_ms());

        eprintln!(
            "[App] skeleton → live: swap complete, {} window(s) live t={}ms (PROMOTE END — main blocked {}ms)",
            self.windows.len(),
            mlc_backend_data_bridge::elapsed_ms(),
            mlc_backend_data_bridge::elapsed_ms().saturating_sub(swap_t0),
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

    /// Like [`wait_for_gpu_frame`] but blocks unconditionally until the GPU
    /// thread acknowledges the in-flight frame. Use before dropping any
    /// `PerWindowState` (the GPU thread holds raw pointers into surface /
    /// gpu_scene / renderer; dropping while the frame is in flight makes
    /// wgpu fail the TextureView generation check inside `create_bind_group`).
    fn wait_for_gpu_frame_blocking(&mut self) {
        if !self.gpu_frame_pending {
            return;
        }
        if let Some(ref done_rx) = self.gpu_done_rx {
            match done_rx.recv() {
                Ok(done) => {
                    if done.close_all {
                        self.close_all_requested = true;
                    }
                }
                Err(_) => {
                    eprintln!("[App] GPU render thread channel closed (wait_for_gpu_frame_blocking)");
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
                            let mut render_tex_us = 0u64;
                            let mut present_us = 0u64;

                            // SAFETY: each address is a unique live PerWindowState;
                            // the main thread waits for GpuDone before touching
                            // gpu_scene, renderer, or surface on any of them.
                            let render_cx: &RenderContext =
                                unsafe { &*(render_cx_addr as *const RenderContext) };

                            for pw_addr in window_addrs {
                                let pw: &mut PerWindowState =
                                    unsafe { &mut *(pw_addr as *mut PerWindowState) };

                                // submit_window_gpu reads pw.gpu_scene (not pw.scene).
                                let (rt_us, pr_us) = submit_window_gpu_from_gpu_scene(
                                    pw,
                                    render_cx,
                                    &mut close_all,
                                    msaa_samples,
                                );
                                render_tex_us += rt_us;
                                present_us += pr_us;
                                total_gpu_us += rt_us + pr_us;
                            }

                            let _ = done_tx.send(GpuDone {
                                close_all,
                                total_gpu_us,
                                render_tex_us,
                                present_us,
                            });
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
                    mlc_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.app_state.presets.insert(id, preset);
                    }
                    mlc_app::PresetAction::Delete { id } => {
                        self.app_state.presets.remove(&id);
                    }
                    mlc_app::PresetAction::Rename { id, new_name } => {
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
        let window_states: Vec<mlc_core::WindowState> = self.windows.values()
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
            mlc_core::ScaleMode::Auto   => "Auto".to_string(),
            mlc_core::ScaleMode::Focus  => "Focus".to_string(),
            mlc_core::ScaleMode::Manual => "Manual".to_string(),
        };
        profile.server_enabled = self.app_state.server_enabled;
        profile.server_port = self.app_state.server_port;

        // Persist notification settings from the first window's alert settings state.
        if let Some(pw) = self.windows.values().next() {
            profile.notification_settings =
                pw.chart.panel_app.alert_settings_state.notification_settings.clone();
        }

        if let Some(key) = preferred_key {
            if let Some(pw) = self.windows.get(&key) {
                profile.sidebar_visible = pw.chart.sidebar_state.is_right_open();
                profile.sidebar_panel = mlc_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                profile.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                profile.inline_bar_x = Some(inline.x);
                profile.inline_bar_y = Some(inline.y);
                let dock_str = match inline.dock_edge {
                    mlc_core::InlineDockEdge::Bottom => "Bottom",
                    mlc_core::InlineDockEdge::Top => "Top",
                    mlc_core::InlineDockEdge::Free => "Free",
                };
                profile.inline_bar_dock = Some(dock_str.to_string());
            }
        }

        // 5. Save profile.json once.
        let vault_key = self.app_state.vault_key.as_ref();
        if let Err(e) = mlc_core::save_profile(&profile, vault_key) {
            eprintln!("[App] Failed to save profile: {}", e);
        } else {
            // Keep in-memory profile up to date.
            self.profile = profile;
        }

        // 6. Save watchlists.json from AppState (always plaintext — no vault key).
        {
            let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
            if let Err(e) = mlc_core::save_json(&watchlists_path, &self.app_state.watchlist_manager, None) {
                eprintln!("[App] Failed to save watchlists: {}", e);
            }
        }

        // 7. Save settings snapshots from AppState (always plaintext — no vault key).
        {
            let path = mlc_core::active_profile_data_dir().join("settings_snapshots.json");
            if let Err(e) = mlc_core::save_json(&path, &self.app_state.snapshots, None) {
                eprintln!("[App] Failed to save settings snapshots: {}", e);
            }
        }

        // 8. Save templates from AppState (single canonical source of truth).
        if let Err(e) = self.app_state.template_manager.save_to_default_dir(vault_key) {
            eprintln!("[App] Failed to save templates: {:?}", e);
        }

        // 9. Save all presets from AppState (single canonical source of truth).
        for preset in self.app_state.presets.values() {
            if let Err(e) = mlc_core::preset::storage::save_preset(preset, vault_key) {
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
}

impl ApplicationHandler<UserEvent> for App<'_> {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::PromoteReady(results) => {
                self.finish_promote(results, event_loop);
            }
        }
    }

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
                    == mlc_app_sidebar_content::state::RightSidebarPanel::Agents
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

        let _t0 = web_time::Instant::now();
        let _atw_start = _t0; // alias — both measure from same point (after FPS guard return)

        // ── Inter-frame GAP detector (startup-freeze diagnostics) ────────────
        // Measure how long the main thread was AWAY between the previous worked
        // frame and this one. With the FPS cap this is normally ≈ target_dt
        // (the WaitUntil sleep). A gap far larger than the frame budget means
        // the loop was starved — either a heavy single pass (promote, GPU init,
        // a blocking lock on bridge maps) or the OS descheduled us. This is the
        // freeze signature `dt` hides: a 15s stall shows here as gap=15000ms even
        // though atw for the resuming frame is tiny. Env-gated.
        if self.perf_log_enabled {
            if let Some(prev_end) = self.last_atw_end {
                let gap_ms = prev_end.elapsed().as_secs_f64() * 1000.0;
                let budget_ms = if self.fps_limit > 0 {
                    1000.0 / self.fps_limit as f64
                } else {
                    16.7
                };
                // Only shout when the gap is far beyond the expected WaitUntil
                // sleep (3× budget AND ≥ 120ms absolute), to skip normal pacing.
                if gap_ms > budget_ms * 3.0 && gap_ms > 120.0 {
                    eprintln!(
                        "[FRAME-GAP] main thread away {:.0}ms between frames (budget={:.1}ms) t={}ms — loop was STARVED (heavy pass / lock / promote), not idle pacing",
                        gap_ms, budget_ms, mlc_backend_data_bridge::elapsed_ms(),
                    );
                }
            }
        }

        // ── Frame timing ────────────────────────────────────────────────────
        let now = web_time::Instant::now();
        let dt = now.duration_since(self.last_frame_instant);
        self.last_frame_instant = now;
        let dt_ms = dt.as_secs_f64() * 1000.0;
        if dt_ms > 0.1 {
            // Exponential moving average with α = 0.1
            let instant_fps = 1000.0 / dt_ms;
            self.fps_ema = self.fps_ema * 0.9 + instant_fps * 0.1;
            self.last_frame_time_ms = dt_ms;
            // Track pacing jitter for the [PERF] line (min/max interval, not just avg).
            if dt_ms < self.dt_min_ms { self.dt_min_ms = dt_ms; }
            if dt_ms > self.dt_max_ms { self.dt_max_ms = dt_ms; }
            self.dt_samples += 1;
        }

        // ── App-level tick ────────────────────────────────────────────────
        // Process app-level broadcast messages (ConnectorReady → request_symbols).
        self.tick_app_state();
        let _t1 = web_time::Instant::now();

        // ── Drain watchlist actions from all windows → AppState ────────────
        // Windows queue WatchlistAction instead of mutating directly.
        // App applies them to the single AppState watchlist.
        let mut watchlist_had_actions = false;
        for pw in self.windows.values_mut() {
            for action in pw.chart.watchlist_actions.drain(..) {
                watchlist_had_actions = true;
                let active_list_id = self.app_state.watchlist_ui.active_list_id;
                match action {
                    mlc_app::WatchlistAction::Toggle { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange.clone(), symbol.clone(), account_type.parse().unwrap_or_default());
                        let now_in = self.app_state.watchlist_manager.toggle_symbol(active_list_id, &address);
                        eprintln!("[App] watchlist toggle: {}:{}:{} -> in_watchlist={}", symbol, exchange, account_type, now_in);
                        if now_in {
                            if let Some(eid) = mlc_app::ExchangeId::from_str(&exchange) {
                                let enabled = self.app_state.connector_enabled
                                    .get(eid.as_str()).copied().unwrap_or(true);
                                if enabled {
                                    let at = mlc_app::account_type_from_label(&account_type);
                                    self.bridge.ensure_connector(eid);
                                    self.bridge.subscribe_mini_ticker(eid, &symbol, at);
                                }
                            }
                            self.app_state.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                        } else {
                            if let Some(eid) = mlc_app::ExchangeId::from_str(&exchange) {
                                let at = mlc_app::account_type_from_label(&account_type);
                                self.bridge.unsubscribe_mini_ticker(eid, &symbol, at);
                            }
                            if let Some(ref mut snap) = self.app_state.watchlist_ui.list_mut(active_list_id).order_snapshot {
                                snap.retain(|s| s.symbol != symbol);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::Remove { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        self.app_state.watchlist_manager.remove_symbol(active_list_id, &address);
                    }
                    mlc_app::WatchlistAction::ClearOrderSnapshot => {
                        self.app_state.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    }
                    mlc_app::WatchlistAction::Reorder { from_idx, to_idx } => {
                        self.app_state.watchlist_manager.reorder_symbol(active_list_id, from_idx, to_idx);
                    }
                    mlc_app::WatchlistAction::CreateList { name } => {
                        let new_id = self.app_state.watchlist_manager.create_list(name);
                        self.app_state.watchlist_ui.active_list_id = new_id;
                    }
                    mlc_app::WatchlistAction::RenameList { id, new_name } => {
                        if let Some(list) = self.app_state.watchlist_manager.lists.iter_mut().find(|l| l.id == id) {
                            list.name = new_name;
                        }
                    }
                    mlc_app::WatchlistAction::DeleteList { id } => {
                        if self.app_state.watchlist_manager.delete_list(id) {
                            self.app_state.watchlist_ui.remove_list(id);
                            if self.app_state.watchlist_ui.active_list_id == id {
                                self.app_state.watchlist_ui.active_list_id = self.app_state.watchlist_manager.lists.first().map(|l| l.id).unwrap_or(0);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::SetActiveList { id } => {
                        self.app_state.watchlist_ui.active_list_id = id;
                    }
                    mlc_app::WatchlistAction::SetColorFlag { symbol, exchange, account_type, color } => {
                        if let Some(list) = self.app_state.watchlist_manager.find_mut(active_list_id) {
                            let color_str = color.as_deref().unwrap_or("");
                            let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                            list.set_color_flag(&address, color_str);
                        }
                    }
                    mlc_app::WatchlistAction::MoveToGroup { symbol, exchange, account_type, group_name } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        if let Some(list) = self.app_state.watchlist_manager.find_mut(active_list_id) {
                            if let Some(group) = list.groups.iter().find(|g| g.name == group_name) {
                                let group_id = group.id;
                                list.move_to_group(&address, group_id);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::RemoveFromGroup { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        if let Some(list) = self.app_state.watchlist_manager.find_mut(active_list_id) {
                            let mut found: Option<mlc_core::instrument::InstrumentAddress> = None;
                            for g in &mut list.groups {
                                if let Some(pos) = g.symbols.iter().position(|s| *s == address) {
                                    found = Some(g.symbols.remove(pos));
                                    break;
                                }
                            }
                            if let Some(ws) = found {
                                list.ungrouped.push(ws);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::SetSeparatorOffsets { offsets } => {
                        self.app_state.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = Some(offsets);
                    }
                    mlc_app::WatchlistAction::ResetSeparatorOffsets => {
                        self.app_state.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = None;
                    }
                    mlc_app::WatchlistAction::SetSeparatorOffset { index, value } => {
                        let list_ui = self.app_state.watchlist_ui.list_mut(active_list_id);
                        // Count visible columns to get correct separator count.
                        let n_cols = {
                            let c = &list_ui.column_config;
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

                        let needs_init = list_ui.column_config.separator_offsets
                            .as_ref()
                            .map(|o| o.len() != n_seps)
                            .unwrap_or(true);
                        if needs_init {
                            list_ui.column_config.separator_offsets = Some(vec![0.0; n_seps]);
                        }
                        if let Some(ref mut offsets) = list_ui.column_config.separator_offsets {
                            if index < offsets.len() {
                                offsets[index] = value;
                            }
                        }
                    }
                    mlc_app::WatchlistAction::ToggleColumnVisibility { column } => {
                        let list_ui = self.app_state.watchlist_ui.list_mut(active_list_id);
                        match column.as_str() {
                            "exchange"     => list_ui.column_config.show_exchange     = !list_ui.column_config.show_exchange,
                            "last_price"   => list_ui.column_config.show_last_price   = !list_ui.column_config.show_last_price,
                            "change_pct"   => list_ui.column_config.show_change_pct   = !list_ui.column_config.show_change_pct,
                            "change_abs"   => list_ui.column_config.show_change_abs   = !list_ui.column_config.show_change_abs,
                            "volume"       => list_ui.column_config.show_volume       = !list_ui.column_config.show_volume,
                            "high_low"     => list_ui.column_config.show_high_low     = !list_ui.column_config.show_high_low,
                            "account_type" => list_ui.column_config.show_account_type = !list_ui.column_config.show_account_type,
                            "align_columns" => {
                                list_ui.column_config.align_columns = !list_ui.column_config.align_columns;
                                if list_ui.column_config.align_columns {
                                    list_ui.column_config.separator_offsets = None;
                                }
                            }
                            _ => {}
                        }
                        // Reset separator offsets when column visibility changes
                        // (but not for align_columns which manages its own offsets).
                        if column != "align_columns" {
                            list_ui.column_config.separator_offsets = None;
                        }
                    }
                    mlc_app::WatchlistAction::SortCycle => {
                        // Clear order snapshot to trigger re-sort on next render
                        self.app_state.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    }
                    mlc_app::WatchlistAction::ResetSort => {
                        self.app_state.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
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
                    mlc_app::ConnectorAction::ToggleEnabled { exchange_id } => {
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
                    mlc_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.app_state.presets.insert(id.clone(), preset);
                        self.app_state.preset_dirty_ids.insert(id);
                    }
                    mlc_app::PresetAction::Delete { id } => {
                        self.app_state.presets.remove(&id);
                        if let Err(e) = mlc_core::preset::storage::delete_preset(&id) {
                            eprintln!("[App] failed to delete preset file {}: {}", id, e);
                        }
                        self.app_state.preset_dirty_ids.remove(&id);
                    }
                    mlc_app::PresetAction::Rename { id, new_name } => {
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
                    mlc_app::SnapshotAction::ChartSettings(data) => {
                        self.app_state.snapshots.chart_settings = Some(data);
                    }
                    mlc_app::SnapshotAction::PrimitiveSettings { type_id, data } => {
                        self.app_state.snapshots.primitive_settings.insert(type_id, data);
                    }
                    mlc_app::SnapshotAction::IndicatorSettings { type_id, data } => {
                        self.app_state.snapshots.indicator_settings.insert(type_id, data);
                    }
                    mlc_app::SnapshotAction::CompareSettings(data) => {
                        self.app_state.snapshots.compare_settings = Some(data);
                    }
                    mlc_app::SnapshotAction::DrawingStyle { type_id, data } => {
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
                    mlc_app::TemplateAction::AddPrimitive(tmpl) => {
                        let _ = self.app_state.template_manager.add_primitive_template(tmpl);
                    }
                    mlc_app::TemplateAction::RemovePrimitive { id } => {
                        let _ = self.app_state.template_manager.remove_primitive_template(&id);
                    }
                    mlc_app::TemplateAction::AddIndicator(tmpl) => {
                        let _ = self.app_state.template_manager.add_indicator_template(tmpl);
                    }
                    mlc_app::TemplateAction::RemoveIndicator { id } => {
                        let _ = self.app_state.template_manager.remove_indicator_template(&id);
                    }
                    mlc_app::TemplateAction::AddCompare(tmpl) => {
                        let _ = self.app_state.template_manager.add_compare_template(tmpl);
                    }
                    mlc_app::TemplateAction::RemoveCompare { id } => {
                        let _ = self.app_state.template_manager.remove_compare_template(&id);
                    }
                    mlc_app::TemplateAction::AddChart(tmpl) => {
                        let _ = self.app_state.template_manager.add_chart_template(tmpl);
                    }
                    mlc_app::TemplateAction::RemoveChart { id } => {
                        let _ = self.app_state.template_manager.remove_chart_template(&id);
                    }
                    mlc_app::TemplateAction::AddIndicatorSet(set) => {
                        let _ = self.app_state.template_manager.add_indicator_set(set);
                    }
                    mlc_app::TemplateAction::RemoveIndicatorSet { id } => {
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
                    mlc_app::PerfAction::SetFpsLimit(v) => {
                        self.fps_limit = v;
                        let mut ds = mlc_core::user_profile::DeviceSettings::load();
                        ds.fps_limit = v;
                        ds.save();
                        eprintln!("[App] FPS limit → {}", v);
                    }
                    mlc_app::PerfAction::SetMsaa(v) => {
                        self.msaa_samples = v;
                        let mut ds = mlc_core::user_profile::DeviceSettings::load();
                        ds.msaa_samples = v;
                        ds.save();
                        eprintln!("[App] MSAA → {}", v);
                    }
                    mlc_app::PerfAction::SetRecalcMode(ref mode) => {
                        self.app_state.recalc_mode = match mode.as_str() {
                            "PerTick" => mlc_app::RecalcMode::PerTick,
                            "PerBar"  => mlc_app::RecalcMode::PerBar,
                            _         => mlc_app::RecalcMode::PerFrame,
                        };
                        let mut ds = mlc_core::user_profile::DeviceSettings::load();
                        ds.recalc_mode = mode.clone();
                        ds.save();
                        eprintln!("[App] RecalcMode → {:?}", self.app_state.recalc_mode);
                    }
                    mlc_app::PerfAction::TogglePerfLog => {
                        self.perf_log_enabled = !self.perf_log_enabled;
                        eprintln!("[App] Perf logging → {}", self.perf_log_enabled);
                    }
                    mlc_app::PerfAction::SetBackend(ref name) => {
                        use mlc_app_sidebar_content::state::RenderBackend;
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
                            let mut ds = mlc_core::user_profile::DeviceSettings::load();
                            ds.render_backend = Some(match new_backend {
                                RenderBackend::VelloGpu => mlc_core::user_profile::device_settings::RenderBackend::VelloGpu,
                                RenderBackend::InstancedWgpu => mlc_core::user_profile::device_settings::RenderBackend::InstancedWgpu,
                                RenderBackend::VelloCpu => mlc_core::user_profile::device_settings::RenderBackend::VelloCpu,
                                RenderBackend::VelloHybrid => mlc_core::user_profile::device_settings::RenderBackend::VelloHybrid,
                                RenderBackend::TinySkia => mlc_core::user_profile::device_settings::RenderBackend::TinySkia,
                            });
                            ds.save();
                            self.backend_auto_detect = false;
                        }
                    }
                    mlc_app::PerfAction::ToggleVsync => {
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
        let _t2 = web_time::Instant::now();

        // ── Flush dirty presets to disk ──────────────────────────────────
        // Presets are plaintext — always safe to save.
        if !self.app_state.preset_dirty_ids.is_empty() {
            let ids: Vec<String> = self.app_state.preset_dirty_ids.drain().collect();
            let vault_key = self.app_state.vault_key.as_ref();
            for id in ids {
                if let Some(preset) = self.app_state.presets.get(&id) {
                    if let Err(e) = mlc_core::preset::storage::save_preset(preset, vault_key) {
                        eprintln!("[App] failed to save preset {}: {}", id, e);
                    }
                }
            }
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
                let window_states: Vec<mlc_core::WindowState> = self.windows.values()
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
                        profile.sidebar_panel = mlc_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                        profile.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                        let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                        profile.inline_bar_x = Some(inline.x);
                        profile.inline_bar_y = Some(inline.y);
                        let dock_str = match inline.dock_edge {
                            mlc_core::InlineDockEdge::Bottom => "Bottom",
                            mlc_core::InlineDockEdge::Top => "Top",
                            mlc_core::InlineDockEdge::Free => "Free",
                        };
                        profile.inline_bar_dock = Some(dock_str.to_string());
                    }
                }

                let vault_key = self.app_state.vault_key.as_ref();
                if let Err(e) = mlc_core::save_profile(&profile, vault_key) {
                    eprintln!("[App] Failed to save profile: {}", e);
                } else {
                    self.profile = profile;
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

                let window_states: Vec<mlc_core::WindowState> = self.windows.values()
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
                        profile.sidebar_panel = mlc_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                        profile.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                        let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                        profile.inline_bar_x = Some(inline.x);
                        profile.inline_bar_y = Some(inline.y);
                        let dock_str = match inline.dock_edge {
                            mlc_core::InlineDockEdge::Bottom => "Bottom",
                            mlc_core::InlineDockEdge::Top => "Top",
                            mlc_core::InlineDockEdge::Free => "Free",
                        };
                        profile.inline_bar_dock = Some(dock_str.to_string());
                    }
                }

                let vault_key = self.app_state.vault_key.as_ref();
                if let Err(e) = mlc_core::save_profile(&profile, vault_key) {
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
                let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
                // Watchlists are always plaintext — pass None regardless of vault key.
                if let Err(e) = mlc_core::save_json(&watchlists_path, &self.app_state.watchlist_manager, None) {
                    eprintln!("[App] Failed to save watchlists: {}", e);
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
                        mlc_app::PresetAction::Upsert(preset) => {
                            let id = preset.id.clone();
                            self.app_state.presets.insert(id.clone(), preset);
                            self.app_state.preset_dirty_ids.insert(id);
                            second_pass_dirty = true;
                        }
                        mlc_app::PresetAction::Delete { id } => {
                            self.app_state.presets.remove(&id);
                            if let Err(e) = mlc_core::preset::storage::delete_preset(&id) {
                                eprintln!("[App] failed to delete preset file {}: {}", id, e);
                            }
                            self.app_state.preset_dirty_ids.remove(&id);
                        }
                        mlc_app::PresetAction::Rename { id, new_name } => {
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
                        if let Err(e) = mlc_core::preset::storage::save_preset(preset, vault_key) {
                            eprintln!("[App] failed to save preset {}: {}", id, e);
                        }
                    }
                }
            }
        }

        let _t3 = web_time::Instant::now();

        // ── App shutdown ────────────────────────────────────────────────
        // Chrome X button or Alt+F4 on ANY window → close entire app.
        let shutdown = self.close_all_requested
            || self.windows.values().any(|pw| pw.close_requested);

        if shutdown {
            // Block until any in-flight GPU frame finishes before we tear
            // anything down. The GPU thread holds raw pointers into
            // PerWindowState (surface.target_view + the swapchain semaphore
            // owned by the surface); dropping those mid-flight crashes
            // wgpu-hal at `SwapchainAcquireSemaphore … still in use`.
            self.wait_for_gpu_frame_blocking();
            // Shutdown the GPU render thread cleanly, then join it so the
            // thread is fully gone before we drop the windows / surfaces.
            if let Some(tx) = self.gpu_cmd_tx.take() {
                let _ = tx.send(GpuCommand::Shutdown);
            }
            if let Some(handle) = self.gpu_thread.take() {
                let _ = handle.join();
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

                // Block until any in-flight GPU frame is done — dropping a
                // PerWindowState while the GPU thread still holds a raw
                // pointer to its surface.target_view trips wgpu's generation
                // check (vello 0.8 / wgpu-core 28).
                self.wait_for_gpu_frame_blocking();
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
                    "PerTick" => mlc_app::RecalcMode::PerTick,
                    "PerBar"  => mlc_app::RecalcMode::PerBar,
                    _         => mlc_app::RecalcMode::PerFrame,
                };
                let recalc_mode = self.app_state.recalc_mode;
                for pw in self.windows.values_mut() {
                    pw.chart.core.indicators_mut().set_recalc_mode(recalc_mode);
                }
                // Persist to DeviceSettings so the choice survives restarts.
                let mut ds = mlc_core::user_profile::DeviceSettings::load();
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
                    // Toolbar defs (incl. drawing-tool dropdown labels + tooltips) are
                    // built once with the language active at construction time. The
                    // global language has already been switched by now, so rebuild the
                    // toolbar config to pick up the new translations.
                    pw.chart.panel_app.toolbar_config =
                        mlc_core::ToolbarConfig::standalone();
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
        // Consume but ignore — key management via REST API only.
        {
            let _key_change: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.local_agent_key_changed.take());
        }

        // ── Drain key create requests from UI ────────────────────────────
        {
            let create_req: Option<(String, String)> = self.windows.values_mut()
                .find_map(|pw| pw.chart.key_create_request.take());
            if let Some((label, tier)) = create_req {
                if let Some(ref agent_state) = self.agent_state {
                    use mlc_backend_agent_server::state::AgentCommand;
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
                    use mlc_backend_agent_server::state::AgentCommand;
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
                            use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                            self.pending_new_profile_id = Some(meta.id.clone());
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                                pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                                pw.chart.panel_app.user_settings_state.profile_manager_target_id = meta.id.clone();
                                pw.chart.panel_app.user_settings_state.profile_manager_target_name = meta.display_name.clone();
                                {
                                    let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                                }
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
                    // Always create a real profile directory on wizard completion.
                    // On first-run the profile_manager is backed by a `_pending`
                    // placeholder (no directory on disk), so we must materialise
                    // a real profile before rename / vault-derive try to write files.
                    // On non-first-run (adding a second profile) we do the same.
                    match self.profile_manager.create_profile(None, "chart") {
                        Ok(meta) => {
                            eprintln!(
                                "[App] wizard_complete: created profile directory '{}' ({})",
                                wizard_profile_name, meta.id
                            );
                            // Switch active profile to the new one before reload.
                            if let Some(mut index) = mlc_core::load_profile_index() {
                                index.active_profile_id = meta.id.clone();
                                let _ = mlc_core::save_profile_index(&index);
                            }
                            // Reload ProfileManager so active_profile_data_dir()
                            // resolves to the real directory.
                            self.profile_manager = mlc_app::user_manager::ProfileManager::load(None);
                            self.profile = self.profile_manager.profile.clone();
                            if !self.is_first_run {
                                // Adding a second profile — clear stale presets/templates.
                                self.app_state.presets.clear();
                                self.app_state.template_manager =
                                    self.profile_manager.template_manager.clone();
                            }
                        }
                        Err(e) => {
                            eprintln!("[App] wizard_complete: failed to create profile: {}", e);
                            // Fall through — attempt rename/vault on whatever dir we have.
                        }
                    }
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
                            // Sync TextFieldStore recovery key display so it can be selected/copied.
                            let key_str = recovery_key.as_deref().unwrap_or("");
                            {
                                let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                let klen = key_str.chars().count();
                                pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, key_str);
                                if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                    fs.cursor = klen;
                                    fs.selection_start = None;
                                }
                            }
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

                // ── Local vault key derivation (e2e_setup) ──
                if let Some(passphrase) = cmd_str.strip_prefix("e2e_setup:") {
                    // ── New profile vault creation ──
                    // When the user just created a new profile, we show the CreatePassphrase
                    // modal without switching profiles first.  Once the vault is created we
                    // trigger the profile switch so the user lands in the new profile.
                    if let Some(ref new_profile_id) = self.pending_new_profile_id.clone() {
                        let profiles_dir = mlc_core::active_profile_data_dir()
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
                                                use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                {
                                                    let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                                                }
                                                pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
                                                pw.chart.panel_app.user_settings_state.recovery_key_display =
                                                    recovery_key.clone();
                                                // Sync TextFieldStore recovery key display so it can be selected/copied.
                                                let key_str = recovery_key.as_deref().unwrap_or("");
                                                {
                                                    let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                                    let klen = key_str.chars().count();
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, key_str);
                                                    if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                                        fs.cursor = klen;
                                                        fs.selection_start = None;
                                                    }
                                                }
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
                                                {
                                                    let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                                                }
                                                pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
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
                        let profiles_dir = mlc_core::active_profile_data_dir()
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
                                    match mlc_vault::vault::validate_passphrase_at::<mlc_core::user_profile::VaultSecrets>(&salt_path, &vault_path, passphrase) {
                                        Ok(key) => {
                                            eprintln!("[App] pre-switch: passphrase validated for profile {}", target_id);
                                            // Store the pre-validated key so execute_profile_switch
                                            // can inject it directly and skip the unlock screen.
                                            self.pending_switch_vault_key = Some(key);
                                            self.pending_profile_switch = Some(target_id);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                {
                                                    let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                                                }
                                                pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
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
                                    if let Ok(salt) = mlc_vault::vault::load_or_create_salt(&salt_path) {
                                        let key = mlc_vault::vault::derive_key(passphrase, &salt);
                                        let empty_secrets = mlc_core::user_profile::VaultSecrets::default();
                                        if let Ok(()) = mlc_vault::vault::save_encrypted(&key, &vault_path, &empty_secrets) {
                                            eprintln!("[App] pre-switch: vault created, switching to profile {}", target_id);
                                            // Store the derived key so execute_profile_switch
                                            // can inject it directly and skip the unlock screen.
                                            self.pending_switch_vault_key = Some(key);
                                            self.pending_profile_switch = Some(target_id);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                {
                                                    let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                                                }
                                                pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
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
                                self.needs_vault_unlock = false;
                                self.needs_migration = false;

                                let recovery_key = self.profile_manager.pending_recovery_key.clone();
                                if recovery_key.is_some() {
                                    // Show recovery key on skeleton first; promote after user confirms.
                                    for pw in self.windows.values_mut() {
                                        use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                                        pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                                        pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                        pw.chart.panel_app.user_settings_state.recovery_key_display =
                                            recovery_key.clone();
                                        // Sync TextFieldStore recovery key display so it can be selected/copied.
                                        let key_str = recovery_key.as_deref().unwrap_or("");
                                        {
                                            let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                            let klen = key_str.chars().count();
                                            pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, key_str);
                                            if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                                fs.cursor = klen;
                                                fs.selection_start = None;
                                            }
                                        }
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
                        let profiles_dir = mlc_core::active_profile_data_dir()
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
                        Some(mlc_core::active_profile_data_dir())
                    };

                    if let Some(profile_dir) = profile_dir {
                        match mlc_vault::crypto::parse_recovery_key(recovery_key_text) {
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
                                    let salt_result = mlc_vault::crypto::load_or_create_salt(&salt_path);
                                    let enc_blob = std::fs::read(&recovery_enc_path);

                                    match (salt_result, enc_blob) {
                                        (Ok(salt), Ok(blob)) => {
                                            match mlc_vault::crypto::decrypt_master_key_with_recovery(
                                                &blob, &recovery_key_bytes, &salt,
                                            ) {
                                                Ok(master_key) => {
                                                    let vault_key = mlc_vault::crypto::derive_vault_key(&master_key, &salt);
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
                                                        self.needs_vault_unlock = false;

                                                        // Store master_key so set_new_passphrase: can re-key the vault.
                                                        self.pending_recovery_master_key = Some(master_key);

                                                        // Navigate to the mandatory SetNewPassphrase page.
                                                        use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                                                        for pw in self.windows.values_mut() {
                                                            pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                                                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                            pw.chart.panel_app.user_settings_state.set_passphrase_error.clear();
                                                            {
                                                                let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                                                let cp_wid = uzor::WidgetId::from(mlc_app::text_input::CONFIRM_PASSPHRASE);
                                                                let mut ic = pw.chart.input_coordinator.borrow_mut();
                                                                ic.text_fields_mut().clear(&np_wid);
                                                                ic.text_fields_mut().clear(&cp_wid);
                                                                ic.text_fields_mut().blur();
                                                            }
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
                                                            {
                                                                let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                                                pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&rk_wid);
                                                                pw.chart.input_coordinator.borrow_mut().text_fields_mut().blur();
                                                            }
                                                            pw.chart.panel_app.user_settings_state.recovery_key_display_focused = false;
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
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;

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
                                        // Sync TextFieldStore recovery key display so it can be selected/copied.
                                        {
                                            let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                            let klen = new_recovery_key_fmt.chars().count();
                                            pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, &new_recovery_key_fmt);
                                            if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                                fs.cursor = klen;
                                                fs.selection_start = None;
                                            }
                                        }
                                        {
                                            let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                            let cp_wid = uzor::WidgetId::from(mlc_app::text_input::CONFIRM_PASSPHRASE);
                                            let mut ic = pw.chart.input_coordinator.borrow_mut();
                                            ic.text_fields_mut().clear(&np_wid);
                                            ic.text_fields_mut().clear(&cp_wid);
                                            ic.text_fields_mut().blur();
                                        }
                                        pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
                                        pw.chart.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                        pw.chart.panel_app.user_settings_state.set_passphrase_error.clear();
                                        pw.chart.panel_app.user_settings_state.profile_manager_page =
                                            ProfileManagerPage::ShowRecoveryKey;
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
                            if let Some(mut index) = mlc_core::load_profile_index() {
                                index.active_profile_id = meta.id.clone();
                                if let Err(e) = mlc_core::save_profile_index(&index) {
                                    eprintln!(
                                        "[App] vault_skip_to_wizard: failed to update index: {}",
                                        e
                                    );
                                }
                            }
                            // Reload ProfileManager from the new empty profile directory.
                            // No key — no encrypted data yet.
                            self.profile_manager = mlc_app::user_manager::ProfileManager::load(None);
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
                        {
                            let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                            pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                        }
                        pw.chart.panel_app.user_settings_state.new_passphrase_focused = false;
                    }
                    // Treat this as first-run so wizard_complete configures the already-
                    // created profile in-place instead of creating a second duplicate.
                    self.is_first_run = true;
                    eprintln!("[App] vault_skip_to_wizard: dismissed vault lock, showing wizard on fresh profile");
                }
            } // if let Some(ref cmd_str) = cmd
        } // drain updater command requests

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
                // Fire-and-forget on the live-data runtime — send to all active subscribers.
                self.live_data_handle.spawn({
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
                            match mlc_alert_delivery::telegram::send_test(&client, &token, chat_id).await {
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
                let result = self.live_data_rt.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .unwrap_or_default();
                    mlc_alert_delivery::telegram::get_updates(&client, &token).await
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
                    if let Some(ref delivery) = self.mlc_alert_delivery {
                        delivery.update_settings(ns.clone());
                    }
                    // Persist to profile on disk
                    self.profile.notification_settings = ns;
                    if let Err(e) = mlc_core::save_profile(&self.profile, self.app_state.vault_key.as_ref()) {
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
        let _t4 = web_time::Instant::now();
        {
            // ── Conditional sync: only clone when data actually changed ────────
            // Each dirty flag is set when the corresponding AppState field is
            // mutated (drain loops above), and reset here after syncing to all
            // windows.  This avoids O(windows * data_size) allocations per frame
            // when nothing has changed.
            if self.app_state.watchlists_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.core.watchlists_mut().replace_all(self.app_state.watchlist_manager.clone());
                    pw.chart.watchlist_ui = self.app_state.watchlist_ui.clone();
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
                    pw.chart.core.presets_mut().replace_all(self.app_state.presets.clone());
                }
                self.app_state.presets_dirty = false;
            }
            if self.app_state.snapshots_dirty {
                let drawing_styles = self.app_state.snapshots.last_used_drawing_styles.clone();
                for pw in self.windows.values_mut() {
                    pw.chart.core.user_mut().set_snapshots(self.app_state.snapshots.clone());
                    // Apply persisted last-used drawing styles to every bubble's
                    // drawing session so new primitives of each type inherit the
                    // user's last-used style.
                    let all_bids_sd: Vec<_> = pw.chart.core.bubbles().bubble_ids().collect();
                    let mut dreg = pw.chart.core.drawings_mut();
                    for bid in all_bids_sd {
                        dreg.load_last_styles(bid, &drawing_styles);
                    }
                }
                self.app_state.snapshots_dirty = false;
            }
            if self.app_state.templates_dirty {
                for pw in self.windows.values_mut() {
                    pw.chart.core.templates_mut().replace_all(self.app_state.template_manager.clone());
                }
                self.app_state.templates_dirty = false;
            }

            // Sync recalc_mode to every window's indicator manager.
            // This is a cheap copy (Copy enum) so no dirty flag is needed.
            let recalc_mode = self.app_state.recalc_mode;
            let scale_mode = self.app_state.scale_mode;
            for pw in self.windows.values_mut() {
                pw.chart.core.indicators_mut().set_recalc_mode(recalc_mode);
                pw.chart.default_scale_mode = scale_mode;
            }

            let frame_time = screenshot::now_ms();
            for pw in self.windows.values_mut() {
                // Skip tick for minimized windows — no point updating viewport
                // or processing trades for an invisible surface.  On restore,
                // snap_to_end repositions the viewport to the latest bars.
                if pw.was_minimized {
                    continue;
                }
                pw.chart.tick(frame_time);
            }
        }

        // Drain pending reset-cache / reset-storage flags set by the context menu.
        for pw in self.windows.values_mut() {
            if pw.chart.pending_reset_cache || pw.chart.pending_reset_storage {
                let delete_storage = pw.chart.pending_reset_storage;
                pw.chart.pending_reset_cache = false;
                pw.chart.pending_reset_storage = false;

                let active_cid_rc = pw.chart.panel_app.panel_grid.active_container_id();
                let (symbol, exchange, account_type, timeframe, bar_count) = {
                    let sym = active_cid_rc
                        .and_then(|cid| pw.chart.symbol_for_chart(cid))
                        .map(String::from).unwrap_or_default();
                    let exch = active_cid_rc
                        .and_then(|cid| pw.chart.exchange_for_chart(cid))
                        .map(String::from).unwrap_or_default();
                    let at = active_cid_rc
                        .and_then(|cid| pw.chart.account_type_for_chart(cid))
                        .map(String::from).unwrap_or_else(|| "S".to_string());
                    let tf = active_cid_rc
                        .and_then(|cid| pw.chart.timeframe_for_chart(cid))
                        .unwrap_or_else(mlc_core::state::Timeframe::h1);
                    if sym.is_empty() && active_cid_rc.is_none() {
                        continue;
                    }
                    (sym, exch, at, tf, pw.chart.core.user().profile().bar_count as usize)
                };

                // disk persistence removed — delete_storage flag is a no-op now
                let _ = delete_storage;

                // Clear bars on the window and mark pending load.
                if let Some(window) = pw.chart.panel_app.panel_grid.active_container_mut() {
                    {};
                    window.viewport.bar_count = 0;
                    window.viewport.view_start = 0.0;
                    window.pending_symbol_load = true;
                }

                // Reset all other windows in the same sync group that share
                // the same symbol key, when sync_symbol is enabled.
                let group_info = active_cid_rc
                    .and_then(|cid| {
                        let gid = pw.chart.bubble_for_chart(cid)?;
                        Some((cid, gid))
                    });
                if let Some((active_cid, group_id)) = group_info {
                    let sync_symbol_on = pw.chart.with_bubble(group_id, |g| g.sync_flags.sync_symbol).unwrap_or(false);
                    if sync_symbol_on {
                        let peer_ids: Vec<mlc_core::ChartId> = pw.chart.core.tags()
                            .chart_member_ids(group_id)
                            .into_iter()
                            .filter(|&m| m != active_cid)
                            .collect();
                        let peers_matching: Vec<mlc_core::ChartId> = {
                            peer_ids.iter()
                                .filter(|&&pid| {
                                    pw.chart.symbol_for_chart(pid).map_or(false, |s| s == symbol.as_str())
                                    && pw.chart.exchange_for_chart(pid).map_or(false, |e| e == exchange.as_str())
                                    && pw.chart.account_type_for_chart(pid).map_or(false, |a| a == account_type.as_str())
                                })
                                .copied()
                                .collect()
                        };
                        for peer_id in peers_matching {
                            if let Some(peer_window) = pw.chart.panel_app.panel_grid.containers_mut().get_mut(&peer_id) {
                                // Phase X0 / C5c: storage on bubble; window cleared via viewport reset only.
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

                // Request a fresh bar fetch from the exchange.
                if let Some(eid) = mlc_app::ExchangeId::from_str(&exchange) {
                    let at = mlc_app::account_type_from_label(&account_type);
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
            if let Some(w) = pw.chart.panel_app.panel_grid.active_container() {
                self.app_state.scale_mode = w.price_scale.scale_mode;
            }
        }

        let _t5 = web_time::Instant::now();

        // ── Populate performance panel data ─────────────────────────────────
        // Only run when the Performance panel is actually visible — avoids
        // per_core_cpu.clone() + gpu_name.clone() + ~25 field writes every frame.
        let perf_visible = self.windows.values().any(|pw| {
            pw.chart.sidebar_state.is_right_open()
                && pw.chart.sidebar_state.right_panel
                    == mlc_app_sidebar_content::state::RightSidebarPanel::Performance
        });
        if perf_visible {
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

            // System metrics — read the latest snapshot from the background
            // sampler thread (cheap lock + copy, no sysinfo syscalls here).
            // global_cpu_usage() is unreliable on Windows — compute the average
            // of per-core values instead (same logic as before, data from snapshot).
            let (cpu_usage, process_cpu, process_cpu_normalized, ram_mb, ram_total_mb, per_core_cpu) = {
                let sample = self.perf_sys_sample.lock()
                    .unwrap_or_else(|e| e.into_inner());
                let per_core = sample.per_core_cpu.clone();
                let cpu_avg = if per_core.is_empty() {
                    0.0_f32
                } else {
                    per_core.iter().copied().sum::<f32>() / per_core.len() as f32
                };
                let proc_cpu = sample.process_cpu;
                let num_cores = per_core.len().max(1) as f32;
                let proc_cpu_norm = proc_cpu / num_cores;
                let ram = sample.process_mem_bytes as f64 / (1024.0 * 1024.0);
                let ram_total = sample.total_mem_bytes as f64 / (1024.0 * 1024.0);
                (cpu_avg, proc_cpu, proc_cpu_norm, ram, ram_total, per_core)
            };
            let gpu_name = self.gpu_name.clone();
            let gpu_driver = self.gpu_driver.clone();
            let scene_build_us = self.cached_scene_us;
            let gpu_render_us = self.cached_gpu_us;

            for pw in self.windows.values_mut() {
                let total_bars: usize = pw.chart.panel_app.panel_grid.containers()
                    .values()
                    .map(|w| pw.chart.core.bubbles().bars(w.current_bubble_id).len())
                    .sum();
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
                perf.indicator_recalc_count = pw.chart.core.indicators().count() as u32;
                let (incremental_count, full_count) = pw.chart.core.indicators().recalc_stats();
                perf.indicator_incremental_count = incremental_count;
                perf.indicator_full_count = full_count;
                perf.event_process_us = pw.chart.last_event_process_us;
                perf.auto_scale_us = pw.chart.last_auto_scale_us;

                // Detailed profiling — new fields.
                perf.orderbook_panel_us = pw.chart.last_orderbook_panel_us;
                perf.dom_panel_us = pw.chart.last_dom_panel_us;
                perf.l2_panel_us = pw.chart.last_l2_panel_us;
                perf.heatmap_panel_us = pw.chart.last_heatmap_panel_us;
                perf.trade_panel_us = pw.chart.last_trade_panel_us;
                perf.bar_apply_us = pw.chart.last_bar_apply_us;
                perf.ob_event_count = pw.chart.last_ob_event_count;
                perf.trade_event_count = pw.chart.last_trade_event_count;
                perf.composite_us = pw.chart.last_composite_us;
                perf.about_to_wait_us = self.cached_about_to_wait_us;
            }

        } // end if perf_visible
        let _t6 = web_time::Instant::now();

        // NOTE: alert delivery events are drained AFTER render_window() below,
        // so that alert screenshots can be captured and attached first.

        // ── Drain toast notifications for UI overlay ──────────────────────────
        if let Some(ref mut rx) = self.toast_rx {
            while let Ok(toast) = rx.try_recv() {
                self.active_toasts.push(toast);
            }
        }
        // Expire old toasts.
        let now_ms_val = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
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

        // ── Per-second maintenance pass ──────────────────────────────────────
        // Historically this block did ALL of the following synchronously in one
        // frame every second: collect_metrics(), three sysinfo refreshes, and
        // four agent-snapshot rebuilds that clone every indicator's full output
        // series. Measured cost: a 20-30ms (sometimes >1000ms) frame stall once
        // per second — the real source of the "choppy / laggy" rendering, not
        // mouse movement. It is now gated and spread out:
        //   • agent snapshots: only when an agent queried us recently, and only
        //     ONE snapshot per pass (round-robin) so the clone cost is amortised.
        //   • sysinfo (cpu/mem/processes): only when the Performance panel is open.
        //   • collect_metrics + clock/perf dirty: cheap, always.
        if self.last_indicator_snapshot.elapsed() >= std::time::Duration::from_secs(1) {
            self.last_indicator_snapshot = web_time::Instant::now();

            // Connector stream count for the perf panel — cheap, keep every second.
            {
                let metrics = self.bridge.collect_metrics();
                self.cached_connector_count = metrics.len();
            }

            // CPU/RAM metrics are now refreshed by the background perf-sys-sampler
            // thread (1 s interval) and published into self.perf_sys_sample. No
            // sysinfo syscalls on the frame thread; the perf panel reads the
            // cached snapshot cheaply in the populate block above.

            // Agent snapshots: skip entirely when no agent has queried recently
            // (the common case). When active, rebuild ONE snapshot per pass in
            // round-robin order so a single frame never pays for all four clones.
            if let Some(agent_state) = self.agent_state.clone() {
                if agent_state.accessed_within(std::time::Duration::from_secs(5)) {
                    match self.agent_snapshot_phase % 4 {
                        0 => self.update_indicator_snapshot(&agent_state),
                        1 => self.update_terminal_snapshot(&agent_state),
                        2 => self.update_watchlist_snapshot(&agent_state),
                        _ => self.update_connector_snapshot(&agent_state),
                    }
                    self.agent_snapshot_phase = self.agent_snapshot_phase.wrapping_add(1);
                }
            }

            // Clock string + time-based indicator progress refresh once per second.
            // Mark toolbar dirty (clock), sidebar dirty (perf panel data) and chart
            // dirty (current-bar progress). Cheap scene rebuilds (~1ms measured).
            for pw in self.windows.values_mut() {
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
            }
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
        // Sidebar rebuilds event-driven: hover-row change, data tick, resize, PTY drain.
        // The per-second timer above already marks sidebar_dirty_scene for data refresh.
        // No unconditional per-frame force — avoids ~25 String allocs + 137 register
        // calls every frame when the sidebar is open.

        let _t7 = web_time::Instant::now();

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
        let mut gpu_wait_us = 0u64;

        // Step 1: wait for the GPU thread to finish the previous frame.
        if self.gpu_frame_pending {
            if let Some(ref done_rx) = self.gpu_done_rx {
                let wait_t0 = web_time::Instant::now();
                match done_rx.recv() {
                    Ok(done) => {
                        gpu_wait_us = wait_t0.elapsed().as_micros() as u64;
                        total_gpu_us = done.total_gpu_us;
                        self.cached_render_tex_us = done.render_tex_us;
                        self.cached_present_us = done.present_us;
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
            //
            // Order matters:
            //   1. drain_pending_cold_apply — runs the deferred ~1.3-1.8s
            //      `apply_preset_cold` body. We're here ONE frame after
            //      LoadPreset parked the previous state into a placeholder
            //      grid, so the user saw the loading skeleton + spinner
            //      during that frame. Cold body now runs synchronously
            //      under that visible skeleton; when it returns,
            //      load_state flips back to Live and the panel populates.
            //   2. drain_pending_cure_after_preset_load — runs the warm
            //      cure `request_bars` sweep. The loading skeleton is
            //      visible NOW so the 100-220ms request_bars batch can
            //      run without freezing the old chart.
            for pw in self.windows.values_mut() {
                let had_stash = pw.chart.pending_cold_apply.is_some();
                pw.chart.drain_pending_cold_apply();
                if had_stash {
                    // After cold apply the panel_grid is fresh — kick the
                    // renderer or it would sit waiting for next input event.
                    pw.chart_dirty = true;
                    pw.window.request_redraw();
                }
            }
            for pw in self.windows.values_mut() {
                pw.chart.drain_pending_cure_after_preset_load();
            }

            // Background-warm: hydrate one open-tab preset per frame into
            // live_preset_cache so a user click on any chrome-bar tab is
            // always a warm cache hit (zero-flicker swap, ~3ms total).
            // Self-paced via VecDeque — finite, exits when empty.
            for pw in self.windows.values_mut() {
                pw.chart.seed_background_warm_if_idle();
                pw.chart.drain_one_background_warm();
            }

            // Make any newly created window visible — but hold it back until its
            // chart actually has bars to draw, so a freshly promoted (post-login)
            // window doesn't flash an empty chart for ~2s while data loads. A
            // timeout fallback shows it anyway if data never arrives (so it can
            // never stay invisible forever).
            for pw in self.windows.values_mut() {
                if !pw.visible_set {
                    // Skeleton (loading/login) windows have no bars by design —
                    // show them immediately (the loading screen IS their content).
                    // Live windows wait for bars (or a 4s timeout) to avoid the
                    // empty-chart flash.
                    let ready = pw.skeleton
                        || pw.chart
                            .panel_app
                            .panel_grid
                            .active_container()
                            .map(|_w| false)
                            .unwrap_or(false)
                        || pw.created_at.elapsed() >= std::time::Duration::from_secs(4);
                    if ready {
                        pw.window.set_visible(true);
                        pw.visible_set = true;
                    }
                }
            }
        }

        // Step 2: drain alert delivery events (screenshots attached by GPU thread).
        if let Some(ref delivery) = self.mlc_alert_delivery {
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
        let frame_time = screenshot::now_ms();
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
        let active_toasts_ref: &[mlc_alert_delivery::ToastNotification] = &active_toasts;
        let parallel_t0 = web_time::Instant::now();
        // Single window (the common case): build inline. Spawning an OS thread
        // per frame via thread::scope costs ~0.5-2ms on Windows for the spawn+
        // join alone — pure overhead when there's only one scene to build and
        // nothing to parallelize against. Sub-scenes measure ~1.2ms total, so a
        // 6-8ms `total_scene_us` was almost entirely this spawn/join. Only fan
        // out to threads when there are 2+ windows to build concurrently.
        if window_refs.len() <= 1 {
            if let Some(pw) = window_refs.first_mut() {
                // Return value (per-window scene_us) is ignored — total_scene_us
                // is overwritten with parallel_t0 wall-clock below either way.
                let _ = build_window_scene(pw, active_toasts_ref, frame_time);
            }
        } else {
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
        }
        // Overwrite scene timing with wall-clock of the whole build phase.
        total_scene_us = parallel_t0.elapsed().as_micros() as u64;

        // Cache timing values so the next frame's populate block can display them.
        self.cached_scene_us = total_scene_us;
        self.cached_gpu_us = total_gpu_us;
        self.cached_gpu_wait_us = gpu_wait_us;

        // ── Scene-build spike log ────────────────────────────────────────────
        // The scene-build (CPU) phase occasionally explodes to 14-20ms with no
        // obvious cause while chart/sidebar sub-scenes stay ~1ms. Print a per-
        // window breakdown when scene blows past budget so we can see which
        // sub-scene (or the composite/swap itself) cost the time. Gated by env.
        if total_scene_us > 5000 && std::env::var("MLC_PERF_LOG").is_ok() {
            for (i, pw) in self.windows.values().enumerate() {
                let (c, tb, sb, setup) = pw.chart.render_timing_us;
                eprintln!(
                    "[SCENE-SPIKE] total_scene={}us win{}: chart={}us tb={}us side={}us setup={}us (sum of sub-scenes; if <<total, cost is in composite/swap/append)",
                    total_scene_us, i, c, tb, sb, setup,
                );
            }
        }

        let _t8 = web_time::Instant::now();
        // Record actual work time for this about_to_wait invocation.
        self.cached_about_to_wait_us = _atw_start.elapsed().as_micros() as u64;
        // Stamp the end of this worked frame for the next pass's GAP detector.
        self.last_atw_end = Some(web_time::Instant::now());

        // One-shot: time from promote swap completion to the end of the first
        // live frame's scene+submit pass — the unmeasured tail of the spinner
        // freeze. Breakdown shows WHERE the pass went: tick (queued event
        // backlog from the worker phase), scene build (chart/toolbar/sidebar),
        // or the whole atw pass (remainder ≈ GPU dispatch + glue).
        if let Some(end_ms) = self.promote_end_ms.take() {
            let (chart_us, tb_us, side_us, setup_us) = self.windows.values()
                .next()
                .map(|pw| pw.chart.render_timing_us)
                .unwrap_or((0, 0, 0, 0));
            eprintln!(
                "[App] finish_promote: first live frame done {}ms after swap (t={}ms) — atw={}us tick={}us events={}us scene(chart={}us tb={}us side={}us setup={}us)",
                mlc_backend_data_bridge::elapsed_ms().saturating_sub(end_ms),
                mlc_backend_data_bridge::elapsed_ms(),
                self.cached_about_to_wait_us,
                self.windows.values().next().map(|pw| pw.chart.last_tick_us).unwrap_or(0),
                self.windows.values().next().map(|pw| pw.chart.last_event_process_us).unwrap_or(0),
                chart_us, tb_us, side_us, setup_us,
            );
            // Arm the amortized warm-up: one series per pass, so the ~120ms
            // ensure_connector cost never blocks a whole batch at once.
            self.pending_warm_targets = self.windows.values()
                .find(|pw| !pw.skeleton)
                .map(|pw| pw.chart.warm_background_tab_targets())
                .unwrap_or_default();
            if !self.pending_warm_targets.is_empty() {
                eprintln!(
                    "[App] deferred background-tab warm: {} target(s), one per frame",
                    self.pending_warm_targets.len(),
                );
            }
        } else if let Some(target) = self.pending_warm_targets.pop() {
            let t_warm = web_time::Instant::now();
            if let Some(pw) = self.windows.values_mut().find(|pw| !pw.skeleton) {
                pw.chart.warm_one_background_tab(&target);
            }
            eprintln!(
                "[App] warm step {}: {}ms ({} left)",
                target.0,
                t_warm.elapsed().as_millis(),
                self.pending_warm_targets.len(),
            );
        }

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
                let (q_len, n_ticker) = self.windows.values()
                    .next()
                    .map(|pw| (pw.chart.diag_queue_len, pw.chart.diag_mini_ticker))
                    .unwrap_or((0, 0));
                let (ob_panel_us, tr_panel_us, bar_apply_us, ob_ev, tr_ev, composite_us) =
                    self.windows.values()
                    .next()
                    .map(|pw| (
                        pw.chart.last_orderbook_panel_us,
                        pw.chart.last_trade_panel_us,
                        pw.chart.last_bar_apply_us,
                        pw.chart.last_ob_event_count,
                        pw.chart.last_trade_event_count,
                        pw.chart.last_composite_us,
                    ))
                    .unwrap_or((0, 0, 0, 0, 0, 0));
                eprintln!(
                    "[PERF] Frame {} total={:.1}ms atw={:.1}ms dt[min={:.1} max={:.1} n={}] ema_fps={:.0} | tick_app={:.1}ms drains={:.1}ms persist={:.1}ms sync={:.1}ms tick={:.1}ms perf_pop={:.1}ms agent={:.1}ms render={:.1}ms (scene={:.1}ms gpu={:.1}ms[rtex={:.1} pres={:.1}] gpu_wait={:.1}ms) | breakdown: chart={:.1}ms tb={:.1}ms side={:.1}ms setup={:.1}ms | tick-detail: obpanel={}us(n={}) trpanel={}us(n={}) barapply={}us composite={}us | LEAK[queue={} ticker={}]",
                    self.frame_count,
                    total as f64 / 1000.0,
                    self.cached_about_to_wait_us as f64 / 1000.0,
                    self.dt_min_ms,
                    self.dt_max_ms,
                    self.dt_samples,
                    self.fps_ema,
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
                    self.cached_render_tex_us as f64 / 1000.0,
                    self.cached_present_us as f64 / 1000.0,
                    self.cached_gpu_wait_us as f64 / 1000.0,
                    chart_us as f64 / 1000.0,
                    toolbar_us as f64 / 1000.0,
                    sidebar_us as f64 / 1000.0,
                    setup_us as f64 / 1000.0,
                    ob_panel_us, ob_ev,
                    tr_panel_us, tr_ev,
                    bar_apply_us,
                    composite_us,
                    q_len,
                    n_ticker,
                );
            }
            self.last_timing_report = web_time::Instant::now();
            self.dt_min_ms = f64::INFINITY;
            self.dt_max_ms = 0.0;
            self.dt_samples = 0;
        }

        // ── Deferred profile hot-reload ──────────────────────────────────────
        // Must run after ALL command processing and rendering so we don't
        // destroy windows while any iteration over `self.windows` is active.
        if let Some(target_id) = self.pending_profile_switch.take() {
            self.execute_profile_switch(&target_id, event_loop);
        }

        // ── Promote skeleton windows to live ────────────────────────────────
        // After vault unlock / wizard / fresh e2e setup, start the off-thread
        // worker that builds live ChartApp instances. The skeleton window keeps
        // rendering with an animated spinner while the worker runs. The swap
        // itself happens in user_event when PromoteReady arrives.
        if self.pending_skeleton_promote && !self.promote_pending {
            self.pending_skeleton_promote = false;
            self.start_promote_async();
        }

        // Keep spinner windows animating at ~60fps while ANY of:
        //   - the promote worker is running (skeleton spinner)
        //   - awaiting_first_live_frame held flag (post-swap skeleton spinner)
        //   - any per-chart leaf is currently in the loading state
        //     (`last_loading_spinners` non-empty) — needs continuous redraw so
        //     the per-chart spinner overlay animates rather than freezing on
        //     the last drawn frame. The "scene-ready" transition flips this
        //     flag to empty the moment bars land, and the next frame draws
        //     the real chart without a spinner — no extra retire timer.
        for pw in self.windows.values() {
            // Anytime the startup spinner is up, KEEP REDRAWING so the ring
            // keeps spinning. Previously this was gated on
            // `(promote_pending || awaiting_first_live_frame)` — both can
            // flip false briefly in the window between finish_promote
            // returning and the first chart_dirty rebuild, freezing the
            // spinner on its last drawn angle for that interval.
            let spinner_up = pw.spinner_start.is_some();
            let per_chart_loading = !pw.chart.last_loading_spinners.is_empty();
            // Cold preset apply is one-frame-deferred (see
            // `pending_cold_apply`). The renderer must redraw NOW so the
            // skeleton frame appears before `about_to_wait` drains the
            // stash and runs the synchronous cold body.
            let cold_apply_stashed = pw.chart.pending_cold_apply.is_some();
            if spinner_up || per_chart_loading || cold_apply_stashed {
                pw.window.request_redraw();
            }
        }

        // ── Set event loop control flow based on FPS limit ──────────────────
        if self.fps_limit > 0 {
            let target_dt = std::time::Duration::from_secs_f64(1.0 / self.fps_limit as f64);
            let elapsed = self.last_frame_instant.elapsed();
            if elapsed < target_dt {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    web_time::Instant::now() + (target_dt - elapsed),
                ));
            } else {
                // Frame already over budget — don't spin, give the OS a tiny breather.
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    web_time::Instant::now() + std::time::Duration::from_millis(1),
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
        self.handle_window_event(event_loop, id, event);
    }
}

fn main() {
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
    let log_dir = mlc_shell_diagnostics::default_log_dir();
    let _diagnostics = mlc_shell_diagnostics::init(&log_dir, env!("CARGO_PKG_VERSION"));

    // Start memory watchdog — logs warnings via tracing regardless of whether
    // the receiver is consumed. UI toast support can be added later.
    let _mem_warnings = mlc_shell_diagnostics::watchdog::start();

    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event()
        .build()
        .expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll); // Will be updated per-frame in about_to_wait()

    // Build the live-data runtime ONCE. The bridge no longer owns a runtime;
    // the entry point is responsible for creating and keeping it alive.
    // 4 workers: the startup connector warm-up spawns ~21 connect + ~21
    // symbol-fetch tasks in a burst; 4 workers halve tail latency vs 2.
    let live_data_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("live-data")
        .enable_all()
        .build()
        .expect("failed to create tokio runtime for live-data");
    let live_data_handle = live_data_rt.handle().clone();

    // Create DataBridge ONCE — connector pool shared by all windows.
    // Phase 2B step 3: orderbook now lives in dig3-station OrderBookTracker
    // (ob_trackers DashMap inside the bridge). No more shared_orderbook map.
    let (bridge, _live_rx, connector_ready_rx) = live_data_rt.block_on(mlc_backend_data_bridge::DataBridge::new());
    let bridge = std::sync::Arc::new(bridge);

    // Detect startup mode BEFORE loading the user manager.
    //
    // Avoid calling `active_profile_data_dir()` here — that function
    // *seeds* a Default profile on disk when the index is missing,
    // which would mask the first-run state and skip the welcome wizard.
    // Probe the index directly instead, and only resolve to the active
    // profile directory when there really is one.
    let (has_profile, has_salt, has_vault) = match mlc_core::load_profile_index() {
        Some(index) if !index.profiles.is_empty() => {
            let active = index
                .profiles
                .iter()
                .find(|m| m.id == index.active_profile_id)
                .or_else(|| index.profiles.first());
            if let Some(meta) = active {
                let dir = mlc_core::profiles_dir().join(&meta.dir_name);
                (
                    dir.join("profile.json").exists(),
                    dir.join("salt.hex").exists(),
                    dir.join("vault.enc").exists(),
                )
            } else {
                (false, false, false)
            }
        }
        _ => (false, false, false),
    };

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
    let mut profile_manager = mlc_app::user_manager::ProfileManager::load(None);

    // Record this launch (was previously done per-window
    // in load_user_state(); now done once here at the application level).
    profile_manager.profile.record_launch(env!("CARGO_PKG_VERSION"));
    // profile.json is always plaintext — safe to save at startup, but only
    // when the profile directory already exists (i.e. not a first-run where
    // the welcome wizard hasn't created the profile on disk yet).
    // vault.enc is NOT written here (no key yet) — credentials are untouched.
    if !is_first_run {
        if let Err(e) = profile_manager.save_profile() {
            eprintln!("[App] failed to save profile at startup: {}", e);
        }
    }

    let profile = profile_manager.profile.clone();
    let saved_windows = profile.windows.clone();

    let mut app = App::new(bridge, live_data_rt, live_data_handle, saved_windows, profile, profile_manager, connector_ready_rx, is_first_run, needs_vault_unlock, needs_migration);
    // Store the event-loop proxy so the promote worker can send PromoteReady
    // back to the main thread without polling.
    app.promote_proxy = Some(event_loop.create_proxy());
    event_loop.run_app(&mut app).expect("Event loop error");
}
