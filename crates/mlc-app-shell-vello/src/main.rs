//! chart-app-vello — minimal winit + vello runner for chart-app
//!
//! Supports multiple windows sharing a single DataBridge (tokio runtime +
//! connector pool).  Each window has its own ChartApp with independent
//! tabs/presets but receives live updates via broadcast channels.
//! Creates windows on demand; closing the last window exits the process.
//!
//! No console window on a release Windows launch — logs go through
//! `mlc_shell_diagnostics`'s tracing file sink instead of a console the user
//! never asked for. Debug builds keep the console (`cargo run` convenience).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent;
mod app_state;
mod chrome;
pub mod keychain;
mod platform;
mod render;
mod screenshot;
mod telemetry;
mod tooltip;
mod window;

#[cfg(target_os = "windows")]
use platform::win32::{win32_border, win32_capture};

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
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
    /// Set when `ChartApp::drain_pending_screenshot()` fires, consumed by the
    /// very next `build_window_scene` call (screenshot-is-chart-only arc,
    /// 2026-09-05).
    ///
    /// The GPU renderer is pipelined one frame behind scene build (see
    /// `render/gpu_submit.rs`'s doc comment on `submit_window_gpu_from_gpu_
    /// scene`): the scene the GPU thread renders+captures this tick was
    /// BUILT last tick, before the click that requested the screenshot was
    /// even processed. Setting `screenshot_pending` (below) immediately
    /// would make the capture read a stale, already-swapped-in scene that
    /// was never built with overlays suppressed. Arming instead defers the
    /// capture by exactly one tick: `build_window_scene` sees `screenshot_
    /// armed`, calls `ChartApp::set_capturing_screenshot(true)` for that one
    /// build pass (suppressing overlay/modal/tooltip painters), then sets
    /// `screenshot_pending = true` so the freshly-built, overlay-free scene
    /// is the one the GPU thread swaps in and captures next tick.
    screenshot_armed: bool,
    /// Last-seen value of `toolbar_state.screenshot_active()`, compared each
    /// tick against the current value so the toolbar strip is marked dirty
    /// exactly once when the camera button's ~1s active phase flips off
    /// (owner ruling 2026-09-05). The toolbar strip is cached behind
    /// `toolbar_dirty`, so without this the strip would keep painting the
    /// active state after the phase actually expired, with no click or
    /// other dirty-triggering event to force a rebuild.
    screenshot_active_prev: bool,
    /// Last-seen value of `chart.unread_count() > 0`, compared each tick so
    /// the toolbar strip is marked dirty exactly once when the system-event
    /// log's unread badge flips (owner scope §8, 2026-09-05) — same cached-
    /// strip problem as `screenshot_active_prev` above: nothing else drives
    /// `toolbar_dirty` when an event arrives from a background path with no
    /// accompanying click.
    unread_events_prev: bool,
    /// Pending agent screenshot requests — each entry is `(chart_id, response_tx)`.
    ///
    /// Accumulated by [`drain_agent_commands`] and processed at the end of
    /// each render pass, after the GPU frame has been submitted.
    pending_agent_screenshots: Vec<(u64, tokio::sync::oneshot::Sender<Result<mlc_backend_shell_http::state::ScreenshotData, String>>)>,
    /// A `ScreenshotDestination::ShareLink` capture's PNG bytes plus the
    /// title to upload it under, set by `gpu_submit`'s capture branch (which
    /// has no `UpdaterHandle` — it runs on the GPU render thread/from a
    /// per-window borrow, not `App`). Drained once per frame in `about_to_
    /// wait`, which is where `self.updater_handle` is reachable.
    pending_share_screenshot: Option<(String, Vec<u8>)>,
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
    /// `AppSession` id returned by `core.register_session` when this window
    /// was created. Used to `deregister_session`/`release_all_presets_for_app`
    /// on window close (shared-BackendCore arc Phase F,
    /// `docs/mlc/plans/shared-backend-core-2026-07-03.md` §4.1/§5.3.5).
    app_id: String,
    /// Set when context menu "Close Window" is selected; drained in about_to_wait.
    close_window_requested: bool,
    /// The chrome's update control was pressed while an installed update
    /// was waiting. Drained in `about_to_wait`: save everything, then hand
    /// over to the new binary.
    restart_requested: bool,
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

/// What `about_to_wait`'s forum-v2 drain does with the next value the
/// single, process-wide `forum_v2_rx` channel settles into.
///
/// Every window shares ONE `mlc_updater` background task, unlike the web
/// shell's own per-tab-instance mailbox — so at most one forum-v2 action is
/// ever in flight system-wide (`App::forum2_pending` tracks it), and a
/// multi-step action (create/reply/delete/moderate, none of which return
/// the thread's own content) is driven across TWO settles: the write's own
/// ack, then a follow-up `LoadThread` this chain fires once that ack lands.
#[cfg(feature = "cloud")]
enum Forum2Chain {
    /// `Boards` / `Thread` (an `OpenThread` click or a passive background
    /// refresh) — apply directly; each `ForumV2State` variant is already
    /// self-describing.
    Direct,
    /// `Search` (`ForumV2Request::Search`) is in flight — either the
    /// header's inline quick-search (Enter-fire via `text_action_forum_search`
    /// or the debounced due-check in [`App::drive_forum2`],
    /// `ForumV2State::quick_search_pending` says which) or the full-page
    /// "More results" navigation. `q` is the query text the answer belongs
    /// to — `ForumV2State::SearchResults` itself never echoes it back
    /// (unlike the web shell's own per-spawn `FlowResult::Forum2Search { q,
    /// .. }`), and it is needed to populate `ForumV2State::
    /// quick_search_query` on the quick-search landing.
    Search { q: String },
    /// `LoadBoard` — `append` distinguishes "Load more" from a fresh open
    /// or a passive due-refresh (both replace the page in place).
    BoardThreads { append: bool },
    /// `CreateThread`/`Reply`/`DeleteReply`/`Moderate` is in flight.
    /// `thread_id_hint` is the thread to reload once the write's ack
    /// lands — known up front for `Reply`/`DeleteReply`/`Moderate` (read
    /// from the window's own open thread before firing), `None` for
    /// `CreateThread` (the new thread's id is the ack's own payload).
    AwaitingWriteAck { thread_id_hint: Option<i64> },
    /// The write's own follow-up `LoadThread` is in flight — the `Thread`
    /// this settles into is applied UNCONDITIONALLY (forces the view,
    /// invalidates the board's thread list, clears the compose/reply
    /// fields), unlike the tolerant [`Self::Direct`] apply for a plain
    /// `OpenThread`/background refresh.
    ReloadAfterWrite,
    /// `Me` is in flight — never reaches `apply_forum2_result`: the settle
    /// drain intercepts `ForumV2State::Me` and broadcasts it to every
    /// window directly (same idiom `DmState::Identity` uses for its own
    /// account-level fact), clearing `forum2_pending` itself. Exists here
    /// only so `drive_forum2`'s passive-fetch fire has a chain value to
    /// record while the request is in flight.
    Me,
}

/// Same single-in-flight idiom as [`Forum2Chain`], for the shared `dm_rx`
/// channel. `DmRequest::Open` is a two-step action (`GetIdentity` then
/// `OpenConversation`, neither call the server exposes as one route); every
/// other request is one wire call.
#[cfg(feature = "cloud")]
enum DmChain {
    /// `PublishIdentity` (the broadcast case, applied before the tracked
    /// window is even looked up — see `apply_dm_result`'s own doc) /
    /// `ListConversations` / `MarkRead` — apply directly.
    Direct,
    /// `LoadMessages` is in flight — `page` carries the same cursor
    /// direction the request was sent with, so the landed answer knows
    /// whether to replace/append/prepend (`mlc_app_layout::ui::
    /// modal_settings::merge_dm_messages`) — same split
    /// [`Forum2Chain::BoardThreads`] draws for its own pagination.
    LoadingMessages { page: mlc_app_layout::ui::modal_settings::DmMessagePage },
    /// A `Send` is in flight — `plaintext` rides along so the settled
    /// `Sent { id, created_at }` can be turned into an optimistic message
    /// bubble without a second round trip to re-fetch what was just sent.
    Sending { plaintext: String },
    /// `Open{with_label}` step 1: `GetIdentity` is in flight for the peer.
    OpeningPeerLookup,
    /// `Open{with_label}` step 2: `OpenConversation` is in flight — the
    /// peer's public key (decoded from step 1's answer) is carried along
    /// so it can be cached under the conversation id once this settles.
    OpeningConversation { peer_public_key: [u8; 32] },
    /// Backfilling a LIST-sourced conversation's peer fingerprint/key
    /// (`GetIdentity` for a conversation reached via `ListConversations`,
    /// which does not carry either) — `conversation_id` names the row to
    /// update once the answer lands.
    BackfillPeerKey { conversation_id: i64 },
    /// The recipient-search dropdown's debounced `Lookup { q }` is in
    /// flight — `q` rides along so the applied `DmState::lookup_query`
    /// always shows the query that was actually asked, never a guess
    /// reconstructed from whatever the field holds when the answer lands.
    Lookup { q: String },
}


/// Minimum interval between two `templates/*` disk writes triggered by the
/// per-frame dirty drain. Template writes are heavier than a single-preset
/// write (encryption + several files under `templates/`); this keeps rapid
/// successive edits (e.g. dragging a color picker that upserts a template
/// per tick) from re-encrypting the whole set every frame while still
/// guaranteeing a write happens shortly after the last edit.
const TEMPLATES_WRITE_DEBOUNCE_MS: u128 = 2_000;

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
    ///
    /// `Arc`-wrapped so the same runtime can also be handed to `core` via
    /// `BackendCore::new_with_runtime` — `core.native_rt()` then resolves for
    /// gate4agent PTY / blocking input handlers instead of always returning
    /// `None` (native_rt was never wired for the vello shell pre-arc).
    live_data_rt: std::sync::Arc<tokio::runtime::Runtime>,
    /// Handle into `live_data_rt` — cheap to clone, used for spawning tasks and
    /// as an executor context for blocking calls like Telegram get_updates.
    live_data_handle: tokio::runtime::Handle,
    /// Shared DataBridge — connector pool, created once at startup.
    ///
    /// Chart-type registry arc Step 12: NOT dead weight from the facade
    /// migration. `ChartApp::new_window` (mlc-app public API) takes
    /// `Arc<DataBridge>` as its own constructor parameter — every window
    /// spawn (both the synchronous path and the background-thread restore
    /// path) needs an owned clone to hand it, independent of `core`. Method
    /// calls that DO have a `BackendCore` facade go through `self.core.*`;
    /// this field stays for the handful that don't (`subscribe_mini_ticker`,
    /// `ws_task_count_total`) and for `ChartApp::new_window`'s own
    /// `Arc<DataBridge>` parameter.
    bridge: std::sync::Arc<mlc_backend_data_bridge::DataBridge>,
    /// Shared backend core — one instance per process, handed to every
    /// `ChartApp::new_window` call so all windows read/write the same
    /// domain managers (bubbles, tags, indicators, alerts, drawings,
    /// presets, templates, watchlists, user profile, sessions).
    core: std::sync::Arc<mlc_backend::BackendCore>,
    /// Saved window states loaded from profile at startup — used in resumed() to restore windows.
    saved_windows: Vec<mlc_core::WindowState>,
    /// User profile loaded once at startup.
    profile: mlc_core::UserProfile,
    /// ProfileManager — disk-load/vault-derivation bookkeeping (loading the
    /// profile once at startup, deriving/rotating the vault key, listing
    /// available profiles). Loaded once, read by `new_window`/the promote
    /// worker for non-domain fields (language, display name, available
    /// profiles list). Domain data (presets/templates/watchlists/snapshots)
    /// is seeded from here into `core` ONCE via `seed_core_from_profile` —
    /// windows read that shared copy, not a per-window clone of this struct
    /// (shared-BackendCore arc Phase B).
    profile_manager: mlc_app::user_manager::ProfileManager,
    /// Window that last received focus — used by save_all() to pick a deterministic
    /// source for sidebar/toolbar/preset state instead of relying on HashMap iteration order.
    last_focused: Option<WindowId>,
    /// Application-level shared state — theme + read-only-after-startup
    /// process identity. Domain data (watchlist, presets, templates,
    /// snapshots, connector preferences, recalc/scale mode, server
    /// settings, vault key) lives on `core` now (shared-BackendCore arc
    /// Phase B/C) — see `app_state.rs` module doc.
    app_state: AppState,
    /// Preset ids modified this frame but not yet flushed to disk.
    ///
    /// App-level (not domain) bookkeeping — batches disk writes so a preset
    /// mutated N times in one frame is written once, not N times. Not part
    /// of `AppState`: it tracks "what needs saving", not "what the current
    /// value is" (that lives on `core.presets()`, one shared copy per
    /// Phase A/B — nothing left here to mirror).
    preset_dirty_ids: std::collections::HashSet<String>,
    /// `true` when `core.user().snapshots()` changed since the last
    /// `settings_snapshots.json` write. Same disk-write-trigger class as
    /// `preset_dirty_ids` — drained every frame in `about_to_wait`, cleared
    /// after a successful write. Closes the crash-loses-data gap `save_all`
    /// alone left (settings_snapshots.json previously wrote only on exit/
    /// close/switch/wizard).
    snapshots_disk_dirty: bool,
    /// `true` when any template CRUD (primitive/indicator/compare/chart/
    /// indicator-set) or a vault-key change touched `core.templates()`
    /// since the last on-disk write. Same class as `snapshots_disk_dirty`.
    /// Template writes are heavier (encryption + multiple files under
    /// `templates/`), so the drain below debounces: it only writes when at
    /// least `TEMPLATES_WRITE_DEBOUNCE_MS` elapsed since the previous write,
    /// deferring (not dropping) the flag otherwise.
    templates_disk_dirty: bool,
    /// Wall-clock time of the last `templates` disk write, used by the
    /// debounce check above. `None` before the first write this session.
    templates_last_write_at: Option<web_time::Instant>,
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
    agent_state: Option<std::sync::Arc<mlc_backend_shell_http::AgentState>>,

    /// `JoinHandle` for the currently-running Agent API listener task,
    /// `None` while stopped. Owned by the sign-in/`server_enabled` edge in
    /// `about_to_wait` (agent-api-auth-and-secret-store plan, piece 4): the
    /// listener starts when `account_signed_in && server_enabled()` becomes
    /// true and is `.abort()`ed + cleared on the reverse transition (sign-out
    /// or the settings toggle turning the server off). Replaces the old
    /// boot-time-only start, which had no stop path at all.
    listener_handle: Option<tokio::task::JoinHandle<()>>,

    /// OTA / telemetry / auth background task, started once in `App::new`
    /// after the `DataBridge` exists. `cmd_tx` forwards user-triggered
    /// commands (logout, force-check, ...); `status_rx`/`auth_rx` are polled
    /// by UI code that needs to react to update/auth state.
    updater_handle: mlc_updater::UpdaterHandle,
    /// Edge-detector for the cloud Download overlay: whether any window had
    /// it open as of the previous frame. Flips false→true drives exactly one
    /// `UpdaterCommand::ForceCheck` (so opening the overlay never shows a
    /// stale check) — never sent again while it stays open.
    #[cfg(feature = "cloud")]
    download_overlay_was_open: bool,
    /// Set when that edge fires a `ForceCheck`, cleared when the updater's
    /// status leaves `Idle`. While it stands, `Idle` is reported as
    /// "Checking" rather than "up to date": the updater sits idle both
    /// before and after a check, so between asking and being answered the
    /// only honest thing the overlay can say is that the question is open.
    #[cfg(feature = "cloud")]
    download_check_inflight: bool,
    /// Whether the "restart to finish" toast has already been shown for the
    /// update currently waiting. One notice per installed update: the state
    /// persists until the app restarts, so a toast driven by the state alone
    /// would reappear on every frame it is checked.
    /// Mailbox for the News overlay's one fetch of the marketing site's blog
    /// feed. `Sender` is cloned into the fire-and-forget task spawned on
    /// `live_data_handle`; `Receiver` is polled (non-blocking `try_recv`) in
    /// the same per-frame drain that started the fetch.
    #[cfg(feature = "cloud")]
    news_feed_tx: std::sync::mpsc::Sender<
        Result<Vec<mlc_app_layout::ui::modal_settings::CloudNewsPost>, String>,
    >,
    #[cfg(feature = "cloud")]
    news_feed_rx: std::sync::mpsc::Receiver<
        Result<Vec<mlc_app_layout::ui::modal_settings::CloudNewsPost>, String>,
    >,
    /// Thread-safe telemetry snapshot handed to the updater as
    /// `Arc<dyn TelemetrySource>` — see `telemetry.rs` for why `App` itself
    /// cannot implement the trait. Refreshed once a second below and on
    /// resize/GPU-init; read by the updater's background task on its own
    /// hourly clock.
    telemetry: std::sync::Arc<telemetry::AppTelemetry>,
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
    /// Receiver for per-attempt Telegram delivery outcomes (alerts-account-
    /// wide-and-telegram-delivery-2026-09-06 plan §4) — `deliver()` itself
    /// is fire-and-forget, so this is the only way the drain loop learns
    /// whether a send actually reached Telegram. Drained unconditionally
    /// every frame (not gated on this tick having fired a new alert): a
    /// previous tick's `deliver()` call resolves asynchronously, often
    /// several frames later.
    alert_outcome_rx: Option<tokio::sync::mpsc::UnboundedReceiver<mlc_alert_delivery::DeliveryOutcome>>,
    /// RAM-only `SecretStore` sidecar (`alert_secrets.enc`) holding the
    /// Telegram bot token + subscribers + webhook URL — same construction
    /// and same seal/unlock lockstep as `agent_state.agent_key_store` (see
    /// `build_secret_sidecar`, `seal_secret_sidecars`), just for native
    /// alert delivery instead of the Agent API. Always constructed (never
    /// `Option`): unlike the agent-key store it is not conditional on the
    /// Agent API being wired.
    alert_secrets_store: std::sync::Arc<mlc_vault::SecretStore>,
    /// Set once a delivery event fires while `alert_secrets_store` is
    /// sealed, so the "unlock the vault to deliver alerts" system event
    /// fires once per seal rather than once per tick; cleared the moment
    /// the store is next observed unlocked. See the drain loop in
    /// `about_to_wait`.
    alert_delivery_seal_notice_shown: bool,
    /// Last APPLIED entitlements answer `(paid, paid_until)` — compared
    /// against each edge-triggered `entitlements_rx` change to detect a
    /// paid⇄free transition (system-event-bus arc, 2026-09-05, owner scope
    /// `docs/mlc/plans/users-paid-features-scope-2026-09-05.md` §8). `None`
    /// means no answer has landed yet THIS session — the first real answer
    /// then falls back to `mlc_updater::tier_store::load_tier()` (same
    /// identity only) as the baseline, so a tier change applied while the
    /// app was closed still fires once instead of silently becoming the new
    /// baseline (owner ask 2026-09-05, mirrors the web shell's
    /// `cloud_session.rs::TIER_KEY`, commit fe955099).
    #[cfg(feature = "cloud")]
    prev_entitlements: Option<(bool, Option<String>)>,

    // ── Forum v2 (public boards) ────────────────────────────────────────
    /// Which window (and what to do with the answer) the single shared
    /// `forum_v2_rx` channel's next settled value belongs to — see
    /// [`Forum2Chain`]'s own doc. `None` means no forum-v2 request is in
    /// flight; both the passive fetch and the explicit-action drain in
    /// `about_to_wait` gate on this being `None` before sending another.
    #[cfg(feature = "cloud")]
    forum2_pending: Option<(WindowId, Forum2Chain)>,

    // ── Direct messages (paid, end-to-end encrypted) ────────────────────
    /// This profile's DM identity keypair — deterministically derived from
    /// the profile's `VaultKey` (owner ruling 2026-09-09:
    /// `mlc_vault::dm::DmIdentity::from_vault_key`, never a separate secret
    /// to generate or persist). Refreshed at boot (if the vault is already
    /// unlocked), on every vault unlock, and on profile switch; dropped
    /// (`None`) whenever the vault seals — see [`Self::refresh_dm_identity`]
    /// and [`Self::seal_secret_sidecars`].
    #[cfg(feature = "cloud")]
    dm_identity: Option<mlc_vault::dm::DmIdentity>,
    /// The public half of the [`mlc_vault::dm::DmIdentity`] this process has
    /// already attempted to publish (`PUT /api/dm/identity`) since the last
    /// unlock or sign-in edge — `None` means no attempt has been made yet
    /// this epoch. Guards [`Self::drive_dm`]'s auto-publish against sending
    /// the request again on every frame while the async answer is still in
    /// flight; reset to `None` by [`Self::refresh_dm_identity`] (a fresh
    /// unlock/profile-switch gets its own attempt) and by the sign-in edge
    /// in `about_to_wait` (so a session that signs in after the vault was
    /// already unlocked still gets one attempt, even if an earlier one this
    /// same unlock failed while signed out).
    #[cfg(feature = "cloud")]
    dm_identity_publish_attempted_for: Option<[u8; 32]>,
    /// Peer public keys, cached per conversation id — populated by
    /// `DmRequest::Open`'s `GetIdentity` step and by the list-page
    /// backfill (`dm_backfill_queue`) for a conversation reached via
    /// `GET /api/dm/conversations` instead, which does not carry the
    /// peer's key. Needed to derive `conversation_key` for both sending
    /// and decrypting.
    #[cfg(feature = "cloud")]
    dm_peer_keys: std::collections::HashMap<i64, [u8; 32]>,
    /// Same single-in-flight tracking as `forum2_pending`, for the shared
    /// `dm_rx` channel — see [`DmChain`]'s own doc.
    #[cfg(feature = "cloud")]
    dm_pending: Option<(WindowId, DmChain)>,
    /// Conversations loaded via `ListConversations` (the list endpoint,
    /// which never carries the peer's key or fingerprint) whose peer key
    /// is not yet cached — `(conversation_id, peer_label)`, drained one at
    /// a time via `DmChain::BackfillPeerKey` whenever `dm_pending` is free.
    #[cfg(feature = "cloud")]
    dm_backfill_queue: std::collections::VecDeque<(i64, String)>,
    /// Memory-watchdog warning receiver (`mlc_shell_diagnostics::watchdog::start`,
    /// started once in `main()` before `App::new`). Drained every frame into
    /// `Memory` system events — previously bound as `_mem_warnings` and
    /// never consumed (warnings only reached `tracing`).
    mem_warnings_rx: std::sync::mpsc::Receiver<mlc_shell_diagnostics::watchdog::MemoryWarning>,

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

    /// Raw share token/URL/deep-link parsed from argv at startup (native-
    /// share-viewer arc, 2026-09-06) — drained once the first window exists
    /// (`ChartApp::request_open_share` needs a live window to queue onto).
    /// See `main()`'s own comment for the parse rule and the single-instance
    /// gap this leaves open.
    pending_deep_link_share: Option<String>,
    /// Bare token of the most recent `UpdaterCommand::OpenShare` this shell
    /// sent — `mlc_updater::ShareOpenState::Opened` carries no token of its
    /// own (the mailbox is one-shot, mirroring `ShareState`), so this is
    /// where the apply block reads it back to fill
    /// `share_viewer::SharedChartMeta::token`. Only ever one open request in
    /// flight at a time, same assumption `ShareOpenState`'s own doc makes.
    pending_open_share_token: Option<String>,
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

/// Default watchlist seed — one ticker per crypto venue, used when no
/// `watchlists.json` exists yet (first run). Existing files are merged via
/// `WatchlistManager::apply_seed_migration`.
fn default_watchlist() -> mlc_app::WatchlistManager {
    mlc_app::WatchlistManager::default_seed()
}

/// One fetch of the marketing site's blog feed for the cloud News overlay.
///
/// Hand-parsed from `serde_json::Value` rather than a derived struct pair:
/// this crate depends on `serde_json` directly but not on `serde` with the
/// `derive` feature, and pulling that in for one small shape is not worth a
/// new dependency edge. `body` is optional in the feed (defaults to empty —
/// the reading view falls back to the summary already); `kind` defaults to
/// `"post"` and `version` to absent, same as the wasm side's serde defaults,
/// so a feed served before either field existed still parses; `next_before`
/// is ignored — this fetch has no "load more".
#[cfg(feature = "cloud")]
async fn fetch_news_feed(
) -> Result<Vec<mlc_app_layout::ui::modal_settings::CloudNewsPost>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let body: serde_json::Value = client
        .get("https://chart.example/api/posts?limit=100")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let posts = body
        .get("posts")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "blog feed: missing 'posts' array".to_string())?;
    let str_field = |v: &serde_json::Value, key: &str| -> String {
        v.get(key).and_then(|f| f.as_str()).unwrap_or_default().to_string()
    };
    Ok(posts
        .iter()
        .map(|p| mlc_app_layout::ui::modal_settings::CloudNewsPost {
            id: str_field(p, "id"),
            kind: p
                .get("kind")
                .and_then(|f| f.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "post".to_string()),
            version: p.get("version").and_then(|f| f.as_str()).map(|s| s.to_string()),
            date: str_field(p, "date"),
            title: str_field(p, "title"),
            summary: str_field(p, "summary"),
            body: str_field(p, "body"),
            url: str_field(p, "url"),
        })
        .collect())
}

// ── Forum wire → UI mapping (native) ────────────────────────────────────
// `mlc-app-layout` cannot depend on `mlc-updater` (the layout crate must not
// depend on the updater — same split `CloudNewsPost`/`BlogPost` already
// has), so this shell is where an updater row becomes the flat mirror the
// overlay renders. Mirrors `mlc-app-shell-web`'s own `map_forum_summary`/
// `map_forum_thread` (`cloud_session.rs`) field-for-field.

fn map_forum_summary_native(
    s: &mlc_updater::ForumThreadSummary,
) -> mlc_app_layout::ui::modal_settings::CloudForumThreadSummary {
    mlc_app_layout::ui::modal_settings::CloudForumThreadSummary {
        id: s.id,
        title: s.title.clone(),
        author: s.author.clone(),
        author_is_me: s.author_is_me,
        created_at: s.created_at.clone(),
        last_reply_at: s.last_reply_at.clone(),
        reply_count: s.reply_count,
        excerpt: s.excerpt.clone(),
    }
}

fn map_forum_thread_native(
    t: &mlc_updater::ForumThread,
) -> mlc_app_layout::ui::modal_settings::CloudForumThread {
    mlc_app_layout::ui::modal_settings::CloudForumThread {
        id: t.id,
        title: t.title.clone(),
        author: t.author.clone(),
        author_is_me: t.author_is_me,
        body: t.body.clone(),
        created_at: t.created_at.clone(),
        replies: t
            .replies
            .iter()
            .map(|r| mlc_app_layout::ui::modal_settings::CloudForumReply {
                id: r.id,
                author: r.author.clone(),
                author_is_me: r.author_is_me,
                body: r.body.clone(),
                created_at: r.created_at.clone(),
            })
            .collect(),
    }
}

// ── Forum v2 wire → UI mapping (native) ─────────────────────────────────
// Same split as the v1 mapping above, mirroring `mlc-app-shell-web`'s own
// `map_forum_board`/`map_forum_thread_v2_summary`/etc. (`cloud_session.rs`)
// field-for-field.

/// Same ranking `mlc-app-shell-web`'s own `forum2_class_rank` uses — full
/// ladder `public < user < paid < trusted < moderator < admin` (`trusted` =
/// "paid with standing", owner ruling 2026-09-08, rank 3).
#[cfg(feature = "cloud")]
fn forum2_class_rank_native(class: &str) -> u8 {
    match class {
        "public" => 0,
        "user" => 1,
        "paid" => 2,
        "trusted" => 3,
        "moderator" => 4,
        "admin" => 5,
        _ => 0,
    }
}

#[cfg(feature = "cloud")]
fn forum2_can_write_native(write_class: &str, caller_classes: &[String]) -> bool {
    let caller_rank = caller_classes.iter().map(|c| forum2_class_rank_native(c)).max().unwrap_or(0);
    caller_rank >= forum2_class_rank_native(write_class)
}

#[cfg(feature = "cloud")]
fn map_forum2_board_native(
    b: mlc_updater::ForumBoard,
    caller_classes: &[String],
) -> mlc_app_layout::ui::modal_settings::BoardUi {
    let can_write = forum2_can_write_native(&b.write_class, caller_classes);
    mlc_app_layout::ui::modal_settings::BoardUi {
        slug: b.slug,
        title: b.title,
        description: b.description,
        read_class: b.read_class,
        write_class: b.write_class,
        can_write,
        thread_count: b.thread_count,
        reply_count: b.reply_count,
        last_post_at: b.last_post_at,
        last_thread_title: b.last_thread_title,
    }
}

#[cfg(feature = "cloud")]
fn map_forum2_thread_summary_native(
    s: mlc_updater::ForumThreadV2Summary,
) -> mlc_app_layout::ui::modal_settings::ForumThreadV2Ui {
    mlc_app_layout::ui::modal_settings::ForumThreadV2Ui {
        id: s.id,
        title: s.title,
        author: s.author,
        author_is_me: s.author_is_me,
        author_rank: s.author_rank,
        created_at: s.created_at,
        updated_at: s.updated_at,
        last_reply_at: s.last_reply_at,
        reply_count: s.reply_count,
        pinned: s.pinned,
        locked: s.locked,
    }
}

#[cfg(feature = "cloud")]
fn map_forum2_reply_node_native(
    r: mlc_updater::ForumReplyNode,
) -> mlc_app_layout::ui::modal_settings::ForumReplyV2Ui {
    mlc_app_layout::ui::modal_settings::ForumReplyV2Ui {
        id: r.id,
        parent_reply_id: r.parent_reply_id,
        depth: r.depth,
        author: r.author,
        author_is_me: r.author_is_me,
        author_rank: r.author_rank,
        body: r.body,
        created_at: r.created_at,
        hidden: r.hidden,
        children: r.children.into_iter().map(map_forum2_reply_node_native).collect(),
    }
}

/// Maps only the flat header fields — unlike the web transport's
/// `ForumThreadV2Response{thread, replies}` split, native's own
/// `mlc_updater::ForumThreadV2` already merges `replies` into itself (see
/// that type's own doc); the caller maps `.replies` separately with
/// [`map_forum2_reply_node_native`] after reading this.
#[cfg(feature = "cloud")]
fn map_forum2_thread_detail_native(
    t: &mlc_updater::ForumThreadV2,
) -> mlc_app_layout::ui::modal_settings::ForumThreadV2Detail {
    mlc_app_layout::ui::modal_settings::ForumThreadV2Detail {
        id: t.id,
        board_id: t.board_id,
        title: t.title.clone(),
        author: t.author.clone(),
        author_is_me: t.author_is_me,
        author_rank: t.author_rank.clone(),
        body: t.body.clone(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        pinned: t.pinned,
        locked: t.locked,
        hidden: t.hidden,
    }
}

#[cfg(feature = "cloud")]
fn map_forum2_search_hit_native(
    h: mlc_updater::ForumSearchHit,
) -> mlc_app_layout::ui::modal_settings::ForumSearchHitUi {
    mlc_app_layout::ui::modal_settings::ForumSearchHitUi {
        kind: h.kind,
        board_id: h.board_id,
        thread_id: h.thread_id,
        reply_id: h.reply_id,
        title: h.title,
        snippet: h.snippet,
        author: h.author,
        created_at: h.created_at,
    }
}

// ── Direct-message crypto helpers (native) ──────────────────────────────
// The shell owns the crypto end to end (`mlc_vault::dm`'s own module doc);
// this crate is where it is actually called from — `mlc-app-layout`'s
// `DmMessageUi::body` (`DmMessageBody`) never carries a key — only
// plaintext, raw wire ciphertext (pre-unlock, or before a peer key backfill
// lands), or the permanent "could not decrypt" marker.

/// AAD binding for one DM message's seal/open — conversation id plus the
/// SENDING identity's own key fingerprint (not the server's `public_id`:
/// native's stored auth token carries no `public_id` field to seal with
/// before the message round-trips even once — see this crate's own report
/// for why). Both sides always land on the same value for the same
/// message: whichever side sealed it used their own identity's
/// fingerprint, and any later reader — the peer, or the sender re-reading
/// their own history — resolves "whose key signed this" from the same
/// fixed pair of identities the conversation already pins (their own, and
/// the cached peer key), never from a value that has to round-trip through
/// the server first.
#[cfg(feature = "cloud")]
fn dm_aad(conversation_id: i64, sender_fingerprint: &str) -> Vec<u8> {
    format!("{}|{}", conversation_id, sender_fingerprint).into_bytes()
}

#[cfg(feature = "cloud")]
fn dm_decode_public_key(public_key_b64: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(public_key_b64).ok()?;
    bytes.try_into().ok()
}

#[cfg(feature = "cloud")]
fn dm_decode_nonce(nonce_b64: &str) -> Option<[u8; 12]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(nonce_b64).ok()?;
    bytes.try_into().ok()
}

#[cfg(feature = "cloud")]
fn dm_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decrypt one wire message into a
/// [`DmMessageBody`](mlc_app_layout::ui::modal_settings::DmMessageBody) —
/// see this module's own crypto-helpers doc. `ctx = (conversation_key,
/// my_public, peer_public)`, `None` when either this profile's identity or
/// the conversation's cached peer key is missing (sealed vault, or the peer
/// key has not been resolved/backfilled yet) — that case returns
/// `Ciphertext` (retryable: [`redecrypt_open_conversation`] re-runs this
/// exact function once new key material lands), never the permanent
/// `Undecryptable`.
#[cfg(feature = "cloud")]
fn decrypt_dm_message_native(
    ctx: Option<&([u8; 32], [u8; 32], [u8; 32])>,
    conversation_id: i64,
    sender_is_me: bool,
    nonce_b64: &str,
    ciphertext_b64: &str,
) -> mlc_app_layout::ui::modal_settings::DmMessageBody {
    use mlc_app_layout::ui::modal_settings::DmMessageBody;
    let Some((key, my_public, peer_public)) = ctx else {
        return DmMessageBody::Ciphertext {
            nonce_b64: nonce_b64.to_string(),
            ciphertext_b64: ciphertext_b64.to_string(),
        };
    };
    let Some(nonce) = dm_decode_nonce(nonce_b64) else {
        return DmMessageBody::Undecryptable;
    };
    let Some(ciphertext) = base64_decode(ciphertext_b64) else {
        return DmMessageBody::Undecryptable;
    };
    let sender_public = if sender_is_me { my_public } else { peer_public };
    let aad = dm_aad(conversation_id, &mlc_vault::dm::fingerprint(sender_public));
    let sealed = mlc_vault::dm::DmCiphertext { nonce, ciphertext };
    match mlc_vault::dm::open_message(key, &sealed, &aad) {
        Ok(plaintext) => DmMessageBody::Plain(String::from_utf8_lossy(&plaintext).into_owned()),
        Err(_) => DmMessageBody::Undecryptable,
    }
}

/// Re-derive `body` for every still-`Ciphertext` message in `win_id`'s
/// currently open conversation, from each message's own stored
/// `nonce_b64`/`ciphertext_b64` — no refetch. Idempotent (a no-op once
/// nothing is left to retry), so [`App::drive_dm`] calls this
/// unconditionally every frame rather than edge-detecting the two things
/// that can unblock a message: the vault unlocking (`DmState::available`
/// flipping true) and a peer-key backfill landing for the open conversation
/// (owner ruling 2026-09-10: ciphertext shows before unlock, decrypts in
/// place after — the same principle covers a backfill resolving mid-view).
/// A free function (not a method) so it borrows only `windows`/`dm_identity`/
/// `dm_peer_keys` — never all of `self` — and can be called from inside a
/// loop already borrowing `self.windows` mutably.
#[cfg(feature = "cloud")]
fn redecrypt_open_conversation(
    windows: &mut HashMap<WindowId, PerWindowState>,
    win_id: WindowId,
    identity: Option<&mlc_vault::dm::DmIdentity>,
    peer_keys: &HashMap<i64, [u8; 32]>,
) {
    use mlc_app_layout::ui::modal_settings::{DmMessageBody, DmView};
    let Some(pw) = windows.get_mut(&win_id) else { return };
    let DmView::Conversation { id, messages, .. } = &mut pw.chart.panel_app.community_state.dm.view else {
        return;
    };
    let conversation_id = *id;
    let ctx = match (identity, peer_keys.get(&conversation_id)) {
        (Some(identity), Some(peer_public)) => {
            let key: [u8; 32] =
                *mlc_vault::dm::conversation_key(&identity.secret, peer_public, &conversation_id.to_string());
            Some((key, identity.public, *peer_public))
        }
        _ => None,
    };
    let Some(ctx) = ctx else { return };
    for m in messages.iter_mut() {
        if let DmMessageBody::Ciphertext { nonce_b64, ciphertext_b64 } = &m.body {
            m.body = decrypt_dm_message_native(Some(&ctx), conversation_id, m.mine, nonce_b64, ciphertext_b64);
        }
    }
}

#[cfg(feature = "cloud")]
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Seed `core`'s domain managers (presets, templates, watchlists, snapshots,
/// connector_enabled, recalc_mode, server_enabled, server_port,
/// vault_key) from disk-loaded profile state.
///
/// Replaces `AppState::from_profile`'s domain half (shared-BackendCore arc,
/// Phase B+C): under the Phase A shared `Arc<BackendCore>` there is exactly
/// ONE copy of each of these managers for the whole process, so seeding
/// happens here, once, instead of being threaded through a per-process
/// `AppState` mirror that used to get cloned into every window each frame.
///
/// Called both at startup (`App::new`) and on profile hot-reload
/// (`execute_profile_switch`, after every window has been dropped and
/// before new ones are created — `core`'s domain data must reflect the
/// NEWLY loaded profile before any window reads it).
fn seed_core_from_profile(
    core: &mlc_backend::BackendCore,
    profile_manager: &mlc_app::user_manager::ProfileManager,
) {
    let profile = &profile_manager.profile;

    let mut watchlist_manager = {
        let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
        if watchlists_path.exists() {
            // Watchlists are always plaintext — pass None regardless of vault key.
            mlc_core::load_json::<mlc_app::WatchlistManager>(&watchlists_path, None)
                .unwrap_or_else(|e| {
                    warn!("[App] Failed to load watchlists: {}", e);
                    default_watchlist()
                })
        } else {
            default_watchlist()
        }
    };
    let watchlists_migrated = watchlist_manager.apply_seed_migration();
    core.watchlists_mut().replace_all(watchlist_manager);
    if watchlists_migrated {
        let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
        let snap = core.watchlists().snapshot();
        if let Err(e) = mlc_core::save_json(&watchlists_path, &snap, None) {
            warn!("[App] Failed to persist migrated watchlists: {}", e);
        }
    }
    core.presets_mut().replace_all(profile_manager.presets.clone());
    core.templates_mut().replace_all(profile_manager.template_manager.clone());
    core.user_mut().set_snapshots(profile_manager.snapshots.clone());

    // Parse recalc_mode from DeviceSettings (authoritative) with profile as fallback.
    let recalc_mode = {
        let ds = mlc_core::user_profile::DeviceSettings::load();
        let src = if ds.recalc_mode == "per_frame" && !profile.domain.recalc_mode.is_empty() {
            // DeviceSettings has the default value — use the profile value if it
            // carries a non-default mode so that existing saves are honoured.
            &profile.domain.recalc_mode
        } else {
            &ds.recalc_mode
        };
        match src.as_str() {
            "PerTick" => mlc_app::RecalcMode::PerTick,
            "PerBar"  => mlc_app::RecalcMode::PerBar,
            _         => mlc_app::RecalcMode::PerFrame,
        }
    };

    let mut user = core.user_mut();
    user.set_connector_enabled(profile.domain.connector_enabled.clone());
    user.set_recalc_mode(recalc_mode);
    user.set_server_enabled(profile.domain.server_enabled);
    user.set_server_port(profile.domain.server_port);
    drop(user);
    core.user_mut().set_vault_key(profile_manager.vault_key);
}

/// Build one RAM-only, vault-keyed `SecretStore` sidecar and, if the main
/// vault is already unlocked this boot (`initial_key`), unlock it off that
/// SAME key immediately — shared by the agent-key store and the
/// alert-secrets store so their construction never drifts apart (both use
/// `with_ttls`, idle:none, absolute:24h; see `mlc_backend_shell_http::
/// auth::agent_keys_paths`'s doc for why the salt file is always the
/// profile's own `salt.hex`). `label` only decorates the startup-failure
/// log line.
fn build_secret_sidecar(
    salt_path: std::path::PathBuf,
    vault_path: std::path::PathBuf,
    initial_key: Option<&mlc_vault::vault::VaultKey>,
    label: &str,
) -> std::sync::Arc<mlc_vault::SecretStore> {
    let store = std::sync::Arc::new(mlc_vault::SecretStore::with_ttls(
        salt_path,
        vault_path,
        None,
        std::time::Duration::from_secs(24 * 60 * 60),
    ));
    if let Some(vault_key) = initial_key {
        if let Err(e) = store.unlock_with_key(vault_key) {
            warn!("[App] {label} store: failed to unlock at startup: {}", e);
        }
    }
    store
}

/// Decode `store`'s live payload into [`mlc_alert_delivery::
/// NotificationSecrets`] — `None` while sealed (or never written, which
/// unlocks to `b"{}"` and decodes to the all-default value, itself `Some`,
/// not `None` — see [`mlc_vault::SecretStore::unlock`]'s doc). Shared by
/// every read site in this file (`AlertDelivery::new`'s initial secrets,
/// the delivery drain loop's sealed check) so the decode step lives once.
fn alert_secrets_snapshot(store: &mlc_vault::SecretStore) -> Option<mlc_alert_delivery::NotificationSecrets> {
    let bytes = store.get().ok()?;
    serde_json::from_slice(&bytes).ok()
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
        mem_warnings_rx: std::sync::mpsc::Receiver<mlc_shell_diagnostics::watchdog::MemoryWarning>,
    ) -> Self {
        let app_state = AppState::from_profile(&profile);

        // Single `Arc<BackendCore>` for the whole process — every window's
        // `ChartApp` shares this one instance (Phase A, shared-backend-core
        // arc). Seed the exchange_symbols cache once here instead of per
        // window (previously each `new_window`/`new_empty` call re-seeded
        // its own fresh core from the bridge's session-long catalogue).
        // `seed_exchange_symbols_from_bridge` walks the station's zero-clone
        // `for_each_symbol` directly into lean `CatalogueSymbol`s — the old
        // `bridge.symbols_snapshot()` deep-cloned every `SymbolInfo`
        // (including its raw `extra` JSON) for every connected exchange.
        let live_data_rt = std::sync::Arc::new(live_data_rt);
        let core = mlc_backend::BackendCore::new_with_runtime(bridge.clone(), None, live_data_rt.clone());
        core.seed_exchange_symbols_from_bridge();
        seed_core_from_profile(&core, &profile_manager);

        // Agent-key `SecretStore` — ONE instance for the whole process,
        // bound to the active profile's `agent_keys.enc` sidecar (same
        // `VaultKey` as `vault.enc`, its own 24h-absolute/no-idle TTL — see
        // `mlc_backend_shell_http::auth::agent_keys_paths`'s doc). Handed to
        // `AgentState` below AND to every window's `ChartApp` via
        // `set_agent_key_store` (`create_window`/`start_promote_async`), so
        // the HTTP-facing copy and the render thread's mint/revoke path
        // never drift onto two different stores. `seed_core_from_profile`
        // above already retained whatever `VaultKey` the boot flow derived —
        // `build_secret_sidecar` unlocks off it immediately if present, so a
        // running session does not start sealed.
        let vault_key_at_boot = core.user().vault_key();
        let (agent_keys_salt_path, agent_keys_vault_path) = mlc_backend_shell_http::auth::agent_keys_paths(
            &mlc_core::active_profile_data_dir(),
        );
        let agent_key_store = build_secret_sidecar(
            agent_keys_salt_path,
            agent_keys_vault_path,
            vault_key_at_boot.as_ref(),
            "agent key",
        );

        // Alert-secrets `SecretStore` sidecar (`alert_secrets.enc`) — same
        // shape as `agent_key_store` above, holding
        // `mlc_alert_delivery::NotificationSecrets` (Telegram bot token,
        // subscribers, webhook URL) instead of agent-key records. Handed to
        // every window's `ChartApp` via `set_alert_secrets_store`, same
        // wiring call-outs as `agent_key_store` (see
        // `input/credentials.rs::vault_key_sidecars`).
        let alert_secrets_dir = mlc_core::active_profile_data_dir();
        let alert_secrets_store = build_secret_sidecar(
            alert_secrets_dir.join("salt.hex"),
            alert_secrets_dir.join("alert_secrets.enc"),
            vault_key_at_boot.as_ref(),
            "alert secrets",
        );

        // Start the internal Agent API server on the DataBridge's tokio runtime.
        let agent_state = std::sync::Arc::new(mlc_backend_shell_http::AgentState::with_agent_key_store(
            bridge.clone(),
            env!("CARGO_PKG_VERSION").to_string(),
            agent_key_store,
        ));
        // ── Populate indicator catalog (one-time, at startup) ─────────────────
        {
            use mlc_backend_shell_http::state::{
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
                            IndicatorParamType::Venues => ("venues".to_string(), None, None),
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
                            IndicatorParamValue::StringList(v) => serde_json::Value::Array(
                                v.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                            ),
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

            info!("[App] Indicator catalog populated: {} types", indicators.len());
            if let Ok(mut cat) = agent_state.indicator_catalog.write() {
                *cat = IndicatorCatalogSnapshot { indicators };
            }
        }

        // ── Populate primitive catalog (one-time, at startup) ─────────────────
        {
            use mlc_backend_shell_http::state::{PrimitiveCatalogSnapshot, CatalogPrimitive};
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
                    ClickBehavior::SingleClick      => "SingleClick",
                    ClickBehavior::TwoPoint         => "TwoPoint",
                    ClickBehavior::ThreePoint       => "ThreePoint",
                    ClickBehavior::FourPoint        => "FourPoint",
                    ClickBehavior::MultiPoint(_)    => "MultiPoint",
                    ClickBehavior::MultiPointFixed(_) => "MultiPointFixed",
                    ClickBehavior::ClickDrag        => "ClickDrag",
                    ClickBehavior::FreehandDrag     => "FreehandDrag",
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

            info!("[App] Primitive catalog populated: {} types", primitives.len());
            if let Ok(mut cat) = agent_state.primitive_catalog.write() {
                *cat = PrimitiveCatalogSnapshot { primitives };
            }
        }

        // The Agent API listener no longer starts here — it starts (and
        // stops) off `account_signed_in && server_enabled()` in
        // `about_to_wait`'s per-frame edge (agent-api-auth-and-secret-store
        // plan, piece 4): unauthenticated boot must never bind the port,
        // and a settings-toggle flip must take effect without a restart.
        // See `App::listener_handle`'s own doc.

        // ── Start the OTA / telemetry / auth background task ──────────────────
        // Runs on `live_data_handle` — the same multi-thread runtime `bridge`
        // and the agent server above already use; no second runtime is built
        // here. Must come after `core`/`bridge` exist (server_port, data_dir
        // and profile_id below all read from them).
        let telemetry = std::sync::Arc::new(telemetry::AppTelemetry::new(bridge.clone()));
        let updater_handle = mlc_updater::start(
            &live_data_handle,
            telemetry.clone(),
            // OTA reaches every install. Owner's ruling, 2026-08-31: an
            // opt-out update channel is legacy — someone who does not want
            // updates does not install the app. Deliberately NOT wired to the
            // profile's `cloud_enabled`, which governs cloud sync.
            true,
            // Cloud sync is no longer a flag this shell gets to set. It
            // follows the account: no OAuth, no cloud; OAuth, cloud; paid
            // account, more room on the server (owner's ruling 2026-09-02).
            // `mlc_updater` seeds it from the token store and flips it on
            // sign-in and sign-out.
            //
            // What used to be here was a hardcoded `false` justified by the
            // server enforcing build attestation. Measured 2026-09-02:
            // `BUILD_ATTEST_ENFORCE` defaults to false and is not set on the
            // box, and an unattested request answers 200.
            profile.domain.sync_state.synced_items.clone(),
            profile.domain.sync_state.last_synced_checksums.clone(),
            mlc_core::active_profile_data_dir(),
            // A real attestation needs `build.rs` to sign
            // `BUILD_ATTESTATION_V1:{version}:{timestamp}:{platform}` with the
            // release signing key. This shell's build.rs does not do that yet
            // (see `crates/mlc-app-shell-vello/build.rs`), so the default
            // (empty) attestation is passed — fine for OTA/telemetry, and the
            // reason cloud sync is hardcoded off above.
            mlc_updater::BuildAttestation::default(),
            profile.domain.profile_id.clone(),
            core.user().server_port(),
        );

        // Initialize alert delivery engine. AlertDelivery::new spawns a tokio
        // task, so we must enter the live-data runtime context first.
        // Secrets now come from `alert_secrets_store` (unlocked above, off
        // `vault_key_at_boot`, if the vault was already open this boot) —
        // `None` (sealed, or the vault was still locked at boot) becomes an
        // empty snapshot, which is fine: delivery already no-ops on an empty
        // bot token/URL.
        let (alert_delivery_engine, toast_rx, alert_outcome_rx) = {
            let _guard = live_data_handle.enter();
            mlc_alert_delivery::AlertDelivery::new(
                profile.domain.notification_settings.clone(),
                alert_secrets_snapshot(&alert_secrets_store).unwrap_or_default(),
            )
        };


        // Apply saved language preference from profile.
        {
            let lang = match profile.ui.language.as_str() {
                "ru" => mlc_core::Language::Ru,
                _    => mlc_core::Language::En,
            };
            mlc_core::set_language(lang);
            info!("[App] startup language: {}", profile.ui.language);
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

        // Mailbox for the cloud News overlay's blog-feed fetch — see the
        // field doc on `news_feed_tx`/`news_feed_rx`.
        #[cfg(feature = "cloud")]
        let (news_feed_tx, news_feed_rx) = std::sync::mpsc::channel();

        Self {
            render_cx: RenderContext::new(),
            windows: HashMap::new(),
            pending_spawns: Vec::new(),
            close_all_requested: false,
            live_data_rt,
            live_data_handle,
            bridge,
            core,
            saved_windows,
            profile,
            profile_manager,
            last_focused: None,
            app_state,
            preset_dirty_ids: std::collections::HashSet::new(),
            snapshots_disk_dirty: false,
            templates_disk_dirty: false,
            templates_last_write_at: None,
            app_connector_ready_rx,
            _phantom: std::marker::PhantomData,
            agent_state: Some(agent_state),
            listener_handle: None,
            updater_handle,
            #[cfg(feature = "cloud")]
            download_overlay_was_open: false,
            #[cfg(feature = "cloud")]
            download_check_inflight: false,
            #[cfg(feature = "cloud")]
            news_feed_tx,
            #[cfg(feature = "cloud")]
            news_feed_rx,
            telemetry,
            last_indicator_snapshot: web_time::Instant::now(),
            agent_snapshot_phase: 0,
            mlc_alert_delivery: Some(alert_delivery_engine),
            toast_rx: Some(toast_rx),
            alert_outcome_rx: Some(alert_outcome_rx),
            alert_secrets_store,
            alert_delivery_seal_notice_shown: false,
            #[cfg(feature = "cloud")]
            prev_entitlements: None,
            #[cfg(feature = "cloud")]
            forum2_pending: None,
            #[cfg(feature = "cloud")]
            dm_identity: vault_key_at_boot.as_ref().map(mlc_vault::dm::DmIdentity::from_vault_key),
            #[cfg(feature = "cloud")]
            dm_identity_publish_attempted_for: None,
            #[cfg(feature = "cloud")]
            dm_peer_keys: std::collections::HashMap::new(),
            #[cfg(feature = "cloud")]
            dm_pending: None,
            #[cfg(feature = "cloud")]
            dm_backfill_queue: std::collections::VecDeque::new(),
            mem_warnings_rx,
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
            pending_profile_switch: None,
            pending_switch_vault_key: None,
            pending_new_profile_id: None,
            pending_skeleton_promote: false,
            pending_switch_after_recovery: None,
            pending_recovery_master_key: None,
            pending_promote_after_recovery_key: false,
            pending_deep_link_share: None,
            pending_open_share_token: None,
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

        // A first run has no saved profile geometry to restore, so its window
        // is centred at 1200x800 on the primary monitor. That is a LAYOUT
        // decision and the only thing first-run still changes about a window.
        //
        // It used to also make the window a skeleton — a spinner with no
        // chart behind it, held until something promoted it. Nothing does
        // that any more (owner's ruling 2026-09-02: the app starts
        // immediately, a skeleton may only spin a spinner, never withhold the
        // app). `main()` now creates the profile before the first window is
        // built, so there is a real chart to draw from the first frame and
        // the wizard is an ordinary modal over it.
        if self.is_first_run {
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

        // A profile nobody has chosen a theme for takes the OS's own
        // light/dark preference for its first frames, then never asks again
        // — from here on the user's file decides (owner's ruling
        // 2026-09-02, matching what the web shell does with
        // `prefers-color-scheme`).
        //
        // Read off the window rather than poking the registry or
        // `AppleInterfaceStyle`: winit already resolves the system setting
        // per platform, and reading it here — before the first paint — is
        // what keeps a dark-desktop user from being shown a white flash.
        //
        // Only the two base themes are ever inferred. High Contrast, Wizard
        // Hat, Surf, Vaporwave and Terminal are deliberate picks (owner's
        // ruling 2026-08-31), and guessing one from an OS bit would be
        // inventing an opinion the OS never expressed.
        if self.app_state.theme_preset.is_empty() {
            let detected = match window.theme() {
                Some(winit::window::Theme::Light) => "light",
                _ => "dark",
            };
            info!("[App] no saved theme — following the system: {detected}");
            self.app_state.theme_preset = detected.to_string();
        }

        let size = window.inner_size();
        // Telemetry's screen_width/screen_height: seeded here at creation,
        // kept current by the same Resized handler that already drives GPU
        // resize (see `window/event_handler.rs`) — no separate event path.
        self.telemetry.set_screen_size(size.width, size.height);

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
            self.telemetry.set_gpu_name(&self.gpu_name);
            info!("[App] GPU: {} ({})", self.gpu_name, self.gpu_driver);

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
                info!("[App] Auto-detected backend → {:?} (device_type={:?})", recommended, info.device_type);

                // Set backend-specific performance defaults.
                let (fps, msaa) = match recommended {
                    RenderBackend::VelloGpu      => (120u32, 8u8),
                    RenderBackend::VelloCpu      => (30,     0),
                    RenderBackend::InstancedWgpu => (90,     8),
                    RenderBackend::VelloHybrid   => (90,     8),
                    // TinySkia retained as a variant for serde back-compat
                    // but never selected here; treat as VelloCpu defaults.
                    RenderBackend::TinySkia      => (30,     0),
                    // Canvas2d is wasm-only, unreachable on native auto-detect.
                    RenderBackend::Canvas2d      => unreachable!("Canvas2d is wasm-only"),
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
                        RenderBackend::Canvas2d => unreachable!("Canvas2d is wasm-only"),
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
            self.core.clone(),
            self.bridge.clone(),
            window_id.clone(),
            restore,
            &self.profile,
            &self.profile_manager,
            // Never a skeleton at creation: see the first-run note above.
            false,
        );
        // Attach this chart's backend to the AgentState so /api/v1/apps
        // routes resolve, and wire the SAME agent-key `SecretStore` into
        // this window's ChartApp (agent-api-auth-and-secret-store plan,
        // piece 4) — the seal/unlock call-outs in `credentials.rs` are all
        // `if let Some(store) = &self.agent_key_store`, a no-op until a
        // host calls this. Every ChartApp in this process shares one
        // Arc<BackendCore>, so the last writer wins safely.
        if let Some(ref agent_state) = self.agent_state {
            agent_state.attach_core(chart.core.clone());
            chart.set_agent_key_store(agent_state.agent_key_store());
        }
        // Same wiring for the alert-secrets sidecar (see `alert_secrets_store`'s
        // own doc) — `refresh_alert_secrets_mirror` re-seeds this window's
        // `AlertSettingsState` from the store now that it is wired, since
        // `new_window()`'s own `apply_profile_state` call ran before this
        // point and could only seed from the pre-sidecar `VaultSecrets` copy.
        chart.set_alert_secrets_store(self.alert_secrets_store.clone());
        chart.refresh_alert_secrets_mirror();
        // Apply the app-level theme so new windows start with the correct preset.
        if !self.app_state.theme_preset.is_empty() {
            chart.panel_app.theme_manager.set_preset(&self.app_state.theme_preset);
        }
        // Domain data (watchlists, presets, templates, snapshots) needs no
        // sync here anymore — `chart.core` IS `self.core` (Phase A, shared
        // Arc<BackendCore>), already seeded once at startup / profile-switch
        // by `seed_core_from_profile`. `new_window()`'s own "Untitled 1"
        // fresh-profile logic already read/wrote through this same shared
        // `core.presets()`, so there is nothing left to merge or sync back.
        chart.sidebar_state.connector_enabled = chart.core.user().connector_enabled().clone();
        // Also sync open_tabs and active_preset_id back to profile_manager
        // so save_all() persists them.
        if !chart.panel_app.active_preset_id.is_empty() {
            self.profile_manager.profile.ui.active_preset_id = chart.panel_app.active_preset_id.clone();
            self.profile.ui.active_preset_id = chart.panel_app.active_preset_id.clone();
        }
        if !chart.panel_app.open_tabs.is_empty() {
            self.profile_manager.profile.ui.open_tabs = chart.panel_app.open_tabs.clone();
            self.profile.ui.open_tabs = chart.panel_app.open_tabs.clone();
        }
        // Apply persisted last-used drawing styles to all bubbles' drawing
        // sessions in this new window (per-bubble model — styles live on
        // DataBubble.last_used_style, loaded once per distinct bubble).
        let drawing_styles = chart.core.user().snapshots().last_used_drawing_styles.clone();
        let all_bids_styles: Vec<_> = chart.core.bubbles().bubble_ids().collect();
        {
            let mut dreg = chart.core.drawings_mut();
            for bid in all_bids_styles {
                dreg.load_last_styles(bid, &drawing_styles);
            }
        }
        // Sync server settings so the User Settings modal shows current state.
        let server_enabled = chart.core.user().server_enabled();
        chart.panel_app.user_settings_state.server_enabled = server_enabled;
        chart.panel_app.user_settings_state.server_port = chart.core.user().server_port();
        chart.panel_app.user_settings_state.server_status = if server_enabled {
            "running".to_string()
        } else {
            "stopped".to_string()
        };
        // Sync language preference from the loaded profile.
        chart.panel_app.user_settings_state.language =
            self.profile_manager.profile.ui.language.clone();
        // Sync profile data into the user settings state.
        {
            let uss = &mut chart.panel_app.user_settings_state;
            uss.profile_id = self.profile_manager.profile.domain.profile_id.clone();
            // runtime_profile_id tracks the ACTUALLY loaded profile for the life of
            // this session.  It is set once here and never changed — even if the user
            // switches the pending active profile, the running profile stays the same
            // until the app is restarted.
            uss.runtime_profile_id = self.profile_manager.profile.domain.profile_id.clone();
            uss.profile_display_name = self.profile_manager.profile.ui.display_name.clone();
            uss.profile_avatar = self.profile_manager.profile.ui.avatar.clone();
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
        // The wizard shows once in a profile's LIFE, not once per process
        // and not once per window (owner's ruling 2026-09-02). The flag is
        // persisted on the profile and set by any ending — finished or
        // dismissed — so a second window opened during that same first
        // session does not raise it again, which is exactly what keying off
        // the in-memory `is_first_run` used to do.
        //
        // Read through `core`, NOT `self.profile`: the latter is a snapshot
        // cloned at startup, while `mark_welcome_wizard_seen` writes to the
        // shared `BackendCore` user profile. Off the snapshot, a second
        // window opened after the wizard was closed would still see `false`
        // and raise it again — the exact bug this is fixing.
        // ...and only when there was no profile on disk to begin with.
        // `is_first_run` is exactly that: "profile.json did not exist at
        // startup". Web has always ANDed the flag with this
        // (`mlc-app-shell-web/src/lib.rs`); native kept only the flag, and the
        // flag was added long after these profiles were written, so
        // `serde(default)` hands every one of them `false` and a first-run
        // screen forever.
        //
        // On a legacy profile that was not cosmetic. The wizard raised, and
        // finishing it took the `wizard_complete` else-branch — the one that
        // exists to ADD a profile — which created a fresh empty profile,
        // rewrote `active_profile_id` to it and switched. The owner lost the
        // twelve tabs he had open and landed on "Untitled" with no presets,
        // three launches running (2026-09-03). The gate is what stops that
        // whole chain: no raise, no else-branch, no phantom profile.
        let show_wizard =
            self.is_first_run && !self.core.user().profile().ui.welcome_wizard_seen;
        if show_wizard {
            chart.panel_app.user_settings_state.show_welcome_wizard = true;
        }

        // Whether the active profile's vault is locked this session — no
        // longer forces the profile manager open (owner's ruling 2026-08-31,
        // docs/mlc/audits/2026-08-30-entry-lock-vault-and-user-persistence.md
        // §10). Kept as truth so that when the user opens the profile
        // manager themselves and clicks their own profile row, the click
        // handler (`panel_click.rs`, "profile_mgr:select:") knows to route
        // to the UnlockPassphrase page instead of treating the vault as
        // already open. `profiles_with_vault_status` for the picker is
        // already populated unconditionally above.
        chart.panel_app.user_settings_state.needs_vault_unlock = self.needs_vault_unlock;

        // Physical-px convention: the whole shell renders 1:1 physical px and
        // the steady-state resize path (dispatch.rs) subtracts CHROME_HEIGHT
        // unscaled — boot must match, or on scale!=1 the chart starts short.
        let chrome_px = chrome::CHROME_HEIGHT as u32;
        chart.resize(size.width, size.height.saturating_sub(chrome_px));

        let win_id = window.id();
        #[cfg(target_os = "windows")]
        let cached_hwnd = win32_border::extract_hwnd(&window);
        // Register this window as an AppSession on the shared core (Phase F,
        // §4.1) — makes AppSession a real 1:1 mirror of open OS windows
        // instead of dead scaffolding. `app_id` is also the identity the
        // open_presets claim registry (§5.3.5) keys park/unpark ownership
        // by (ChartApp::window_id, threaded in as the claim's app_id param).
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let app_id = self.core.register_session(Some(chart.window_id.clone()), now_secs);
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
            screenshot_armed: false,
            screenshot_active_prev: false,
            unread_events_prev: false,
            pending_agent_screenshots: Vec::new(),
            pending_share_screenshot: None,
            modifiers: winit::keyboard::ModifiersState::default(),
            drawing_capture: false,
            chrome_state: chrome::ChromeState::new("chart-app-vello"),
            #[cfg(target_os = "windows")]
            hwnd: cached_hwnd,
            close_requested: false,
            spawn_new_window: false,
            window_id,
            app_id,
            close_window_requested: false,
            restart_requested: false,
            delete_window_requested: false,
            last_sidebar_hover_row: None,
            created_at: web_time::Instant::now(),
            // Never a skeleton at creation: see the first-run note above.
            // The flag survives for the promote path, which rebuilds windows
            // after a vault unlock.
            skeleton: false,
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
        info!("[App] hot-reload: switching to profile {}", target_id);

        // 1. Save current profile state (windows, presets, watchlist).
        self.save_all(&[]);

        // 2. Update the index so the new profile is active on next load too.
        if let Some(mut index) = mlc_core::load_profile_index() {
            index.active_profile_id = target_id.to_string();
            if let Err(e) = mlc_core::save_profile_index(&index) {
                error!("[App] hot-reload: failed to save index: {}", e);
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
            if let Some(pw) = self.windows.remove(&wid) {
                // `core` is the SAME shared Arc<BackendCore> across the
                // switch (only its domain data is re-seeded below) — every
                // session/claim registered under the OLD profile's windows
                // must be released here, since these windows never go
                // through the normal close path that would otherwise do it.
                self.core.deregister_session(&pw.app_id);
                self.core.release_all_presets_for_app(&pw.app_id);
            }
        }
        self.last_focused = None;

        // 3b. Seal + drop every secret sidecar bound to the OLD profile
        //     before anything below touches the new one. `seal_secret_sidecars`
        //     (one-list idiom, `vault_key_sidecars`) wipes both RAM-only
        //     `SecretStore`s AND drops `self.dm_identity` (it is derived from
        //     the vault key, never persisted — see `refresh_dm_identity`'s
        //     own doc) — the NEW profile's identity gets re-derived either
        //     by the pre-validated-key injection below (`sync_secret_sidecars`)
        //     or, if the new profile still needs an unlock screen, by
        //     whichever `e2e_setup:`/`cred_unlock:`/`recovery_unlock:` handler
        //     lands its key.
        self.seal_secret_sidecars();

        // 4. Determine whether the new profile needs vault unlock.
        //    active_profile_data_dir() now resolves to target_id's directory
        //    because the index was just updated and saved above.
        let new_profile_dir = mlc_core::active_profile_data_dir();
        let has_vault = new_profile_dir.join("vault.enc").exists();
        let has_salt  = new_profile_dir.join("salt.hex").exists();

        // 4a. Rebuild `alert_secrets_store` fresh against `new_profile_dir`
        //     (sealed — nothing has confirmed a key for the NEW profile
        //     yet). Every window recreated in step 7 below wires its own
        //     `ChartApp` off this field's CURRENT value (`create_window`'s
        //     own `set_alert_secrets_store` call), so rebuilding it here,
        //     before step 7 runs, is sufficient to reach every window —
        //     there is no live window left to walk at this point (step 3
        //     already dropped all of them). `agent_key_store` gets the same
        //     treatment now that `AgentState::agent_key_store` is
        //     re-pointable (`AgentState::set_agent_key_store`): a fresh,
        //     sealed store bound to the NEW profile's `agent_keys.enc`,
        //     swapped into the shared `AgentState` so every subsequent
        //     Agent API request — and every window recreated in step 7,
        //     via `create_window`'s own `agent_state.agent_key_store()`
        //     read — sees the switched-to profile's keys, not the one
        //     active at process boot (or the previously switched-to one).
        self.alert_secrets_store = build_secret_sidecar(
            new_profile_dir.join("salt.hex"),
            new_profile_dir.join("alert_secrets.enc"),
            None,
            "alert secrets",
        );
        if let Some(ref agent_state) = self.agent_state {
            let (agent_keys_salt_path, agent_keys_vault_path) =
                mlc_backend_shell_http::auth::agent_keys_paths(&new_profile_dir);
            let new_agent_key_store = build_secret_sidecar(
                agent_keys_salt_path,
                agent_keys_vault_path,
                None,
                "agent key",
            );
            agent_state.set_agent_key_store(new_agent_key_store);
        }

        // 5. Load the new profile from disk.
        //    active_profile_data_dir() reads the index we just updated, so it
        //    will resolve to target_id's directory.
        self.profile_manager = mlc_app::user_manager::ProfileManager::load(None);
        let profile = self.profile_manager.profile.clone();
        let saved_windows = profile.ui.windows.clone();

        // 6. Rebuild app-level state from the new profile.
        self.app_state = AppState::from_profile(&profile);
        // 6a. Re-seed the shared core's domain managers from the NEW profile.
        // Every window was just dropped above (step 3) — nothing reads
        // `self.core` between here and the `create_window` calls below, so
        // this is safe. Without this, the new windows would come up showing
        // the OLD profile's presets/templates/watchlists/snapshots — `core`
        // is one shared instance now, not rebuilt per window.
        seed_core_from_profile(&self.core, &self.profile_manager);
        self.profile = profile;
        self.saved_windows = saved_windows.clone();
        self.needs_vault_unlock = has_vault && has_salt;

        // 6b. If the passphrase was pre-validated in Branch A, inject the key now
        //     so the user goes straight into the app without seeing the unlock screen.
        if let Some(key) = self.pending_switch_vault_key.take() {
            self.profile_manager.set_vault_key(key);
            self.profile_manager.vault_key = Some(key);
            if let Err(e) = self.profile_manager.load_vault_secrets() {
                warn!("[App] hot-reload: failed to load vault secrets after pre-switch: {}", e);
            }
            self.profile = self.profile_manager.profile.clone();
            self.core.user_mut().set_vault_key(Some(key));
            // Every `set_vault_key(Some(..))` site must also unlock the
            // sidecars off the same key (agent-api-auth-and-secret-store
            // plan, follow-up) — this one previously left them sealed
            // through an entire profile switch. One-list idiom via
            // `sync_secret_sidecars`/`vault_key_sidecars`, mirroring
            // `ChartApp::sync_vault_sidecars`.
            //
            // `alert_secrets_store` was rebuilt against `new_profile_dir` in
            // 4a above, and `agent_key_store` was re-pointed at a fresh
            // store against the same directory in that same step (via
            // `AgentState::set_agent_key_store`) — so this now correctly
            // unlocks BOTH sidecars' SWITCHED-TO copies, not a stale
            // original-profile store.
            self.sync_secret_sidecars(&key);
            self.needs_vault_unlock = false;
            info!("[App] hot-reload: pre-validated vault key injected — skipping unlock screen");
        }

        self.is_first_run = false;

        // 7. Recreate windows from the new profile's saved state.
        if saved_windows.is_empty() {
            info!("[App] hot-reload: no saved windows — creating default");
            self.create_window(event_loop, None, None);
        } else {
            for ws in &saved_windows {
                debug!(
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
                pw.chart.panel_app.user_settings_state.profile_manager_target_id = self.profile.domain.profile_id.clone();
                pw.chart.panel_app.user_settings_state.profile_manager_target_name = self.profile.ui.display_name.clone();
            }
        }
        // A switched-to profile with no vault used to land here and get the
        // profile manager forced open on CreatePassphrase — the same fossil
        // as `needs_migration` (see `main()`), one layer down. Switching to a
        // profile that has never been given a vault is not an error and not a
        // migration: it has no secrets, and if the user wants one they open
        // the Vault tab and press Set up. Owner's ruling 2026-09-02: the
        // vault is opened where a secret is used, never in front of the
        // chart.

        info!(
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
                warn!("[App] start_promote_async: no proxy — cannot promote off-thread");
                return;
            }
        };

        let _promote_t0 = mlc_backend_data_bridge::elapsed_ms();
        info!(
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
                pw.chart.close_profile_manager();
                pw.chart.panel_app.user_settings_state.show_welcome_wizard = false;
                pw.chart_dirty = true;
                pw.window.request_redraw();
            }
        }

        // Collect per-window inputs that are Send + 'static.
        let window_ids: Vec<winit::window::WindowId> = self.windows.keys().copied().collect();
        let saved = self.saved_windows.clone();

        // Capture clones of everything the worker needs to build ChartApp.
        let core = self.core.clone();
        let bridge = self.bridge.clone();
        let profile = self.profile.clone();
        let profile_manager = self.profile_manager.clone();
        // AppState fields needed for the presentation sync block below
        // (read-only in the worker). Domain data (watchlists/presets/
        // templates/snapshots/connector_enabled/server_enabled/server_port)
        // needs no capture — `chart.core` built inside the worker IS
        // `self.core` (Phase A shared Arc<BackendCore>), already seeded
        // once by `seed_core_from_profile`; the worker reads it directly
        // via `core.user()` instead of capturing separate clones.
        let theme_preset = self.app_state.theme_preset.clone();
        // Agent-key `SecretStore` (agent-api-auth-and-secret-store plan,
        // piece 4) — same `Arc` every other window already carries, cloned
        // for the worker thread so a promoted window's ChartApp is wired
        // exactly like one built through `create_window`.
        let agent_key_store = self.agent_state.as_ref().map(|s| s.agent_key_store());
        // Alert-secrets `SecretStore` — same `Arc` every other window
        // already carries (see `alert_secrets_store`'s own doc), cloned for
        // the worker thread so a promoted window's ChartApp is wired
        // exactly like one built through `create_window`.
        let alert_secrets_store = self.alert_secrets_store.clone();

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
                    core.clone(),
                    bridge.clone(),
                    window_id.clone(),
                    restore,
                    &profile,
                    &profile_manager,
                    false, // skeleton = false
                );
                if let Some(ref store) = agent_key_store {
                    chart.set_agent_key_store(store.clone());
                }
                chart.set_alert_secrets_store(alert_secrets_store.clone());
                chart.refresh_alert_secrets_mirror();

                // Apply the app-level theme.
                if !theme_preset.is_empty() {
                    chart.panel_app.theme_manager.set_preset(&theme_preset);
                }
                chart.sidebar_state.connector_enabled = chart.core.user().connector_enabled().clone();
                {
                    let drawing_styles = chart.core.user().snapshots().last_used_drawing_styles.clone();
                    let all_bids_ls: Vec<_> = chart.core.bubbles().bubble_ids().collect();
                    let mut dreg = chart.core.drawings_mut();
                    for bid in all_bids_ls {
                        dreg.load_last_styles(bid, &drawing_styles);
                    }
                }
                let server_enabled = chart.core.user().server_enabled();
                chart.panel_app.user_settings_state.server_enabled = server_enabled;
                chart.panel_app.user_settings_state.server_port = chart.core.user().server_port();
                chart.panel_app.user_settings_state.server_status = if server_enabled {
                    "running".to_string()
                } else {
                    "stopped".to_string()
                };
                chart.panel_app.user_settings_state.language =
                    profile_manager.profile.ui.language.clone();
                {
                    let uss = &mut chart.panel_app.user_settings_state;
                    uss.profile_id = profile_manager.profile.domain.profile_id.clone();
                    uss.runtime_profile_id = profile_manager.profile.domain.profile_id.clone();
                    uss.profile_display_name = profile_manager.profile.ui.display_name.clone();
                    uss.profile_avatar = profile_manager.profile.ui.avatar.clone();
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

                // Resize using the sizes captured on the main thread. Scale is
                // captured but intentionally unused: physical-px convention,
                // CHROME_HEIGHT is subtracted unscaled everywhere (dispatch.rs).
                let (w, h, _scale) = window_sizes.get(idx).copied().unwrap_or((1200, 800, 1.0));
                let chrome_px = chrome::CHROME_HEIGHT as u32;
                chart.resize(w, h.saturating_sub(chrome_px));

                results.push(PromoteWindowResult {
                    winit_id: wid,
                    chart: Box::new(chart),
                    window_id,
                    active_preset_id: String::new(), // filled after chart is built
                    open_tabs: Vec::new(),            // filled after chart is built
                });
            }

            // Patch active_preset_id / open_tabs from the built charts.
            for r in results.iter_mut() {
                r.active_preset_id = r.chart.panel_app.active_preset_id.clone();
                r.open_tabs = r.chart.panel_app.open_tabs.clone();
            }

            debug!(
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
        debug!(
            "[App] finish_promote: gpu-wait {}ms",
            t_gpu_wait.saturating_sub(swap_t0),
        );

        let saved = self.saved_windows.clone();
        let existing_count = self.windows.len();

        for r in results {
            // Write-back: profile fields the worker could not write because
            // it only had read-only clones. Preset write-back is no longer
            // needed — `chart.core` built inside the worker IS `self.core`
            // (Phase A shared Arc<BackendCore>), so any preset `new_window`
            // created is already visible in `self.core.presets()`.
            if !r.active_preset_id.is_empty() {
                self.profile_manager.profile.ui.active_preset_id = r.active_preset_id.clone();
                self.profile.ui.active_preset_id = r.active_preset_id;
            }
            if !r.open_tabs.is_empty() {
                self.profile_manager.profile.ui.open_tabs = r.open_tabs.clone();
                self.profile.ui.open_tabs = r.open_tabs;
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
                    debug!(
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
                debug!(
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

        info!(
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
                    error!("[App] GPU render thread channel closed (wait_for_gpu_frame)");
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
                    error!("[App] GPU render thread channel closed (wait_for_gpu_frame_blocking)");
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

    /// Sync OS window position/size into `ChartApp` fields for every open,
    /// non-minimized window. Shared by `save_all` and the two `about_to_wait`
    /// dirty-drain paths — all three read the live OS geometry into
    /// `ChartApp` right before building a `WindowState` snapshot from it.
    /// Minimized windows are skipped (Windows reports `(-32000, -32000)` for
    /// iconic state) so the pre-minimized position/size is preserved.
    fn sync_window_geometry_from_os(&mut self) {
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
    }

    /// Build a `UserProfile` ready to save, starting from `self.profile` and
    /// overlaying the fields that always come from live app/core state:
    /// window list, connector_enabled, theme/device/app_version, and the
    /// preferred (last-focused, or first available) window's sidebar/inline
    /// bar UI state.
    ///
    /// This is the ~30-line assembly that used to be duplicated verbatim in
    /// `save_all` and both `about_to_wait` dirty-drain branches (profile-dirty
    /// and geometry-only). Callers needing additional domain fields (scale
    /// mode, server settings, notification settings — currently only
    /// `save_all`) set them on the returned profile afterward; this helper
    /// intentionally does NOT set them, so the lighter-weight geometry/
    /// profile-dirty drains keep writing exactly the same field set as
    /// before (no new fields added to those paths by this refactor).
    ///
    /// `window_states` — already-collected, exclusion-filtered window
    /// snapshots (callers differ on whether excluded/deleted windows are
    /// filtered, so collection stays at the call site).
    /// The single window whose `ChartApp::system_events()` log records
    /// process-level facts (tier transitions, sign-in/out, sync, memory,
    /// update, alert deliveries) — same "one window speaks for the profile"
    /// choice `build_profile_for_save` makes for `preferred_key` below
    /// (last-focused, falling back to any open window). Owner ask
    /// 2026-09-05: ONE place for everything the system told the user, not
    /// one independent copy per OS window.
    fn system_event_window_mut(&mut self) -> Option<&mut PerWindowState> {
        let key = self.last_focused
            .filter(|id| self.windows.contains_key(id))
            .or_else(|| self.windows.keys().next().copied())?;
        self.windows.get_mut(&key)
    }

    /// Every RAM-only vault-keyed `SecretStore` sidecar this HOST holds —
    /// mirrors `ChartApp::vault_key_sidecars` (`input/credentials.rs`,
    /// `mlc-app` crate) one level up, so a host-level seal or re-unlock
    /// never forgets one. `agent_key_store` reads through
    /// `AgentState::agent_key_store()` (a cheap `Arc` clone under
    /// `AgentState`'s own internal `RwLock`) rather than a direct field
    /// borrow — it CAN be re-pointed at a different `SecretStore` mid-session
    /// now (`AgentState::set_agent_key_store`, called from
    /// `execute_profile_switch`'s step 4a), so returning owned `Arc`s here
    /// rather than references is what makes that safe: nothing here can end
    /// up holding a reference into a store a concurrent re-point already
    /// swapped out from under it.
    fn vault_key_sidecars(&self) -> [Option<std::sync::Arc<mlc_vault::SecretStore>>; 2] {
        [
            self.agent_state.as_ref().map(|s| s.agent_key_store()),
            Some(self.alert_secrets_store.clone()),
        ]
    }

    /// Seal every store in [`Self::vault_key_sidecars`] — from the two
    /// edges that must drop both together without going through any live
    /// window's `ChartApp` (sign-out, `vault_skip_to_wizard`'s fresh-profile
    /// reset, a profile hot-reload switch). Per-window seal/unlock lockstep
    /// for the SAME two stores is
    /// `input/credentials.rs::seal_vault_sidecars`/`sync_vault_sidecars`
    /// (`mlc-app` crate) — this is the host-level counterpart for the
    /// call-outs that run before or without one.
    ///
    /// Also drops the DM identity and its publish-attempt guard (owner
    /// ruling 2026-09-09: the identity is derived from the vault key, never
    /// persisted, so sealing the vault must drop the derived copy in RAM
    /// too — nothing else clears it on this edge).
    fn seal_secret_sidecars(&mut self) {
        for store in self.vault_key_sidecars().into_iter().flatten() {
            store.seal();
        }
        #[cfg(feature = "cloud")]
        {
            self.dm_identity = None;
            self.dm_identity_publish_attempted_for = None;
        }
    }

    /// Refresh every store in [`Self::vault_key_sidecars`] off the SAME
    /// `VaultKey`, mirroring `ChartApp::sync_vault_sidecars`'s one-list
    /// idiom at the host level — called wherever `core.user_mut().
    /// set_vault_key(Some(..))` lands here (today: the profile-switch
    /// pre-validated-key injection in `execute_profile_switch`). Also
    /// re-derives the DM identity off the same key — see
    /// [`Self::refresh_dm_identity`].
    fn sync_secret_sidecars(&mut self, vault_key: &mlc_vault::vault::VaultKey) {
        for store in self.vault_key_sidecars().into_iter().flatten() {
            if let Err(e) = store.unlock_with_key(vault_key) {
                warn!("[App] secret store: failed to unlock alongside vault: {}", e);
            }
        }
        #[cfg(feature = "cloud")]
        self.refresh_dm_identity();
    }

    /// Re-derive `self.dm_identity` from whatever `VaultKey`
    /// `self.core.user()` currently holds (`None` while sealed) —
    /// deterministic (`mlc_vault::dm::DmIdentity::from_vault_key`), so this
    /// is cheap and safe to call on every vault unlock, at every one of the
    /// `e2e_setup:`/`cred_unlock:`/`recovery_unlock:` command handlers that
    /// land a fresh key, and after a profile switch — there is no sidecar
    /// file to race against another caller, and no failure mode: the same
    /// vault key always derives the same keypair. Also resets
    /// [`Self::dm_identity_publish_attempted_for`], so the freshly
    /// (re)derived identity gets its own auto-publish attempt in
    /// [`Self::drive_dm`] rather than inheriting a stale guard from
    /// whichever identity (or profile) was active before this call.
    #[cfg(feature = "cloud")]
    fn refresh_dm_identity(&mut self) {
        let vault_key = self.core.user().vault_key();
        self.dm_identity = vault_key.as_ref().map(mlc_vault::dm::DmIdentity::from_vault_key);
        self.dm_identity_publish_attempted_for = None;
    }

    /// Applies the single, process-wide `forum_v2_rx` channel's next
    /// settled value — see [`Forum2Chain`]'s own doc for why one channel
    /// shared across every window needs a tracked in-flight marker instead
    /// of the web shell's own per-tab-instance mailbox.
    #[cfg(feature = "cloud")]
    fn apply_forum2_result(&mut self, win_id: WindowId, chain: Forum2Chain, state: mlc_updater::ForumV2State) {
        use mlc_app_layout::ui::modal_settings::ForumV2View;

        let Some(pw) = self.windows.get_mut(&win_id) else {
            // The window closed while the request was in flight — nothing
            // to apply to; the request itself already landed server-side.
            return;
        };

        let mut reload_after: Option<i64> = None;
        let mut write_failure: Option<String> = None;

        {
            let ov = &mut pw.chart.panel_app.community_state;
            ov.forum2.boards_refreshing = false;
            ov.forum2.board_threads_refreshing = false;
            ov.forum2.thread_refreshing = false;
            // Only ever `true` while a `Forum2Chain::Search` is the chain
            // in flight (set right before firing it, by either
            // `text_action_forum_search`'s Enter-fire or the debounced
            // due-check below) — taken unconditionally here so it can never
            // survive past the answer it was armed for, success or failure
            // alike (mirrors the web shell's own `Forum2Search` apply arm,
            // `cloud_session.rs`).
            let quick = std::mem::take(&mut ov.forum2.quick_search_pending);

            match (chain, state) {
                (_, mlc_updater::ForumV2State::Idle) | (_, mlc_updater::ForumV2State::Working) => {}
                (Forum2Chain::Direct, mlc_updater::ForumV2State::Boards(boards)) => {
                    let caller_classes = ov.forum2.caller_classes.clone();
                    ov.forum2.boards = boards
                        .into_iter()
                        .map(|b| map_forum2_board_native(b, &caller_classes))
                        .collect();
                    ov.forum2.mark_boards_fetched();
                }
                (Forum2Chain::BoardThreads { append }, mlc_updater::ForumV2State::BoardThreads { threads, next_before }) => {
                    let mapped: Vec<_> = threads.into_iter().map(map_forum2_thread_summary_native).collect();
                    if let ForumV2View::BoardThreads { threads: existing, next_before: nb, .. } = &mut ov.forum2.view {
                        if append {
                            existing.extend(mapped);
                        } else {
                            *existing = mapped;
                        }
                        *nb = next_before;
                        ov.forum2.mark_board_threads_fetched();
                    }
                    // Else: the view moved on before this landed — dropped,
                    // same tolerance an interrupted v1 fetch gets.
                }
                (Forum2Chain::Direct, mlc_updater::ForumV2State::Thread(t)) => {
                    let detail = map_forum2_thread_detail_native(&t);
                    let id = detail.id;
                    let replies_tree: Vec<_> = t.replies.into_iter().map(map_forum2_reply_node_native).collect();
                    if let ForumV2View::Thread { id: existing_id, thread, replies_tree: rt, .. } = &mut ov.forum2.view {
                        if *existing_id == id {
                            *thread = Some(detail);
                            *rt = replies_tree;
                            ov.forum2.mark_thread_fetched();
                        }
                        // Else: landed for a thread the caller already
                        // navigated away from — dropped.
                    }
                }
                (Forum2Chain::Search { q }, mlc_updater::ForumV2State::SearchResults(hits)) => {
                    let mapped: Vec<_> = hits.into_iter().map(map_forum2_search_hit_native).collect();
                    if quick {
                        ov.forum2.quick_search_query = Some(q);
                        ov.forum2.quick_search_results = mapped;
                    } else if let ForumV2View::Search { hits: h, .. } = &mut ov.forum2.view {
                        *h = mapped;
                    }
                }
                (Forum2Chain::AwaitingWriteAck { thread_id_hint: _ }, mlc_updater::ForumV2State::ThreadCreated { id }) => {
                    ov.forum2.invalidate_board_threads();
                    reload_after = Some(id);
                }
                (Forum2Chain::AwaitingWriteAck { thread_id_hint }, mlc_updater::ForumV2State::ReplyPosted { .. })
                | (Forum2Chain::AwaitingWriteAck { thread_id_hint }, mlc_updater::ForumV2State::ReplyDeleted)
                | (Forum2Chain::AwaitingWriteAck { thread_id_hint }, mlc_updater::ForumV2State::Moderated) => {
                    ov.forum2.invalidate_board_threads();
                    reload_after = thread_id_hint;
                }
                (Forum2Chain::ReloadAfterWrite, mlc_updater::ForumV2State::Thread(t)) => {
                    let detail = map_forum2_thread_detail_native(&t);
                    let id = detail.id;
                    let replies_tree: Vec<_> = t.replies.into_iter().map(map_forum2_reply_node_native).collect();
                    let reply_to = match &ov.forum2.view {
                        ForumV2View::Thread { id: existing_id, reply_to, .. } if *existing_id == id => *reply_to,
                        _ => None,
                    };
                    ov.forum2.view = ForumV2View::Thread { id, thread: Some(detail), replies_tree, reply_to };
                    ov.forum2.mark_thread_fetched();
                    // Compose/reply forms are gone — clear the shared
                    // fields, mirroring v1's own `ForumThreadCreated`/
                    // `ForumThreadRefreshed` arms (empty clears to empty,
                    // harmless for a `DeleteReply`/`Moderate` write).
                    let mut coord = pw.chart.input_coordinator.borrow_mut();
                    let tf = coord.text_fields_mut();
                    for field in [mlc_app::text_input::FORUM_TITLE, mlc_app::text_input::FORUM_BODY, mlc_app::text_input::FORUM_REPLY] {
                        let wid = uzor::WidgetId::from(field);
                        if !tf.text(&wid).is_empty() {
                            tf.clear(&wid);
                        }
                    }
                }
                (chain, mlc_updater::ForumV2State::Failed { message })
                    if message == mlc_updater::forum_v2::NICK_REQUIRED_SENTINEL =>
                {
                    // Caller lost the nick between the proactive
                    // `nick_required()` gate and this write landing (owner
                    // mandate 2026-09-09) — re-ask `Me` rather than show the
                    // sentinel; the composer's own gate picks the prompt
                    // back up from `caller_nick` on the next frame.
                    ov.forum2.invalidate_me();
                    let _ = chain;
                }
                (chain, mlc_updater::ForumV2State::Failed { message }) => {
                    ov.forum2.last_error = Some(message.clone());
                    if matches!(chain, Forum2Chain::AwaitingWriteAck { .. } | Forum2Chain::ReloadAfterWrite) {
                        write_failure = Some(message);
                    }
                }
                // Any other (chain, state) pairing should not occur given
                // the strict single-in-flight sequencing above — dropped
                // rather than panicking on a defect elsewhere.
                _ => {}
            }
        }

        if let Some(id) = reload_after {
            let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::ForumV2(
                mlc_updater::ForumV2Request::LoadThread { id },
            ));
            self.forum2_pending = Some((win_id, Forum2Chain::ReloadAfterWrite));
        }
        if let Some(message) = write_failure {
            pw.chart.push_system_event(mlc_core::SystemEventKind::Info, "Forum post failed", message, true);
        }
        pw.overlay_dirty = true;
        pw.window.request_redraw();
    }

    /// Applies the single, process-wide `dm_rx` channel's next settled
    /// value — same in-flight-tracking reason as [`Self::apply_forum2_result`].
    /// Never called with `DmState::Identity`: that variant is an
    /// account-level fact `drive_dm` broadcasts to every window directly,
    /// before this per-window lookup is even reachable (same as
    /// `forum_entitled`/`is_paid` elsewhere in this file).
    #[cfg(feature = "cloud")]
    fn apply_dm_result(&mut self, win_id: WindowId, chain: DmChain, state: mlc_updater::DmState) {
        use mlc_app_layout::ui::modal_settings::{
            dm_has_older, merge_dm_messages, DmConversationUi, DmLookupHitUi, DmMessageBody, DmMessagePage,
            DmMessageUi, DmView,
        };

        let Some(pw) = self.windows.get_mut(&win_id) else {
            return;
        };

        let mut write_failure: Option<String> = None;
        let mut new_backfill_candidates: Vec<(i64, String)> = Vec::new();
        let mut open_conversation_target: Option<(String, [u8; 32])> = None;
        let mut opened: Option<(mlc_updater::DmConversation, [u8; 32])> = None;
        let mut backfilled: Option<(i64, [u8; 32], String)> = None;

        {
            let ov = &mut pw.chart.panel_app.community_state;
            ov.dm.conversations_refreshing = false;
            ov.dm.messages_refreshing = false;

            match (chain, state) {
                (_, mlc_updater::DmState::Idle) | (_, mlc_updater::DmState::Working) => {}

                (DmChain::Direct, mlc_updater::DmState::Conversations(conversations)) => {
                    let mut mapped: Vec<DmConversationUi> = Vec::with_capacity(conversations.len());
                    for c in conversations {
                        new_backfill_candidates.push((c.id, c.peer.label.clone()));
                        mapped.push(DmConversationUi {
                            id: c.id,
                            peer_label: c.peer.label,
                            peer_fingerprint: c.peer.fingerprint,
                            last_message_at: c.last_message_at,
                            unread_count: c.unread,
                        });
                    }
                    ov.dm.conversations = mapped;
                    ov.dm.mark_conversations_fetched();
                }
                (DmChain::OpeningPeerLookup, mlc_updater::DmState::PeerIdentity(identity)) => {
                    match dm_decode_public_key(&identity.public_key_b64) {
                        Some(peer_public_key) => {
                            open_conversation_target = Some((identity.public_id, peer_public_key));
                        }
                        None => {
                            ov.dm.last_error = Some("That account's messaging key is malformed".to_string());
                        }
                    }
                }
                (DmChain::BackfillPeerKey { conversation_id }, mlc_updater::DmState::PeerIdentity(identity)) => {
                    if let Some(key) = dm_decode_public_key(&identity.public_key_b64) {
                        backfilled = Some((conversation_id, key, identity.fingerprint));
                    }
                    // A malformed key here just leaves the row without a
                    // fingerprint/cached key — Send/decrypt refuse locally
                    // until a later attempt succeeds.
                }
                (DmChain::OpeningConversation { peer_public_key }, mlc_updater::DmState::ConversationOpened(conversation)) => {
                    opened = Some((conversation, peer_public_key));
                }
                (DmChain::LoadingMessages { page }, mlc_updater::DmState::Messages { messages, next_after }) => {
                    let view_id = match &ov.dm.view {
                        DmView::Conversation { id, .. } => Some(*id),
                        _ => None,
                    };
                    if let Some(conv_id) = view_id {
                        // `key` is deref'd out of `conversation_key`'s
                        // `Zeroizing` wrapper immediately (`[u8; 32]` is
                        // `Copy`) so this crate never needs to spell the
                        // `zeroize` crate's own type name — see the module
                        // doc's own note on what `Zeroizing` does and does
                        // not cover for exactly this class of stack copy.
                        let crypto_ctx: Option<([u8; 32], [u8; 32], [u8; 32])> =
                            match (&self.dm_identity, self.dm_peer_keys.get(&conv_id)) {
                                (Some(identity), Some(peer_public)) => {
                                    let key: [u8; 32] =
                                        *mlc_vault::dm::conversation_key(&identity.secret, peer_public, &conv_id.to_string());
                                    Some((key, identity.public, *peer_public))
                                }
                                _ => None,
                            };
                        let mapped: Vec<DmMessageUi> = messages
                            .into_iter()
                            .map(|m| {
                                let body = decrypt_dm_message_native(
                                    crypto_ctx.as_ref(),
                                    conv_id,
                                    m.sender_is_me,
                                    &m.nonce_b64,
                                    &m.ciphertext_b64,
                                );
                                DmMessageUi {
                                    id: m.id,
                                    mine: m.sender_is_me,
                                    body,
                                    created_at: m.created_at,
                                    read_at: m.read_at,
                                }
                            })
                            .collect();
                        let page_len = mapped.len();
                        let available = ov.dm.available;
                        let has_error = ov.dm.last_error.is_some();
                        let key_changed = ov.dm.has_peer_key_changed(conv_id);
                        if let DmView::Conversation { id: existing_id, messages: msgs, has_older, .. } = &mut ov.dm.view {
                            if *existing_id == conv_id {
                                // "Load older" (`Before`) must keep the
                                // viewport anchored on whatever message was
                                // at the top before the page landed — the
                                // exact height the page added is the only
                                // way to do that without hardcoding bubble
                                // geometry a second time here, so read it
                                // off `dm_overlay`'s own sizing function
                                // before AND after the merge (owner review
                                // 2026-09-23, `forum-messenger-realtime.md`
                                // item 1).
                                let is_before = matches!(page, DmMessagePage::Before(_));
                                let old_h = is_before.then(|| {
                                    mlc_app_layout::layout::modals::dm_overlay::conversation_scroll_h(
                                        available, has_error, key_changed, *has_older, msgs,
                                    )
                                });
                                merge_dm_messages(msgs, mapped, page);
                                *has_older = dm_has_older(page, page_len, *has_older);
                                if let Some(old_h) = old_h {
                                    let new_h = mlc_app_layout::layout::modals::dm_overlay::conversation_scroll_h(
                                        available, has_error, key_changed, *has_older, msgs,
                                    );
                                    ov.scroll.nudge((new_h - old_h).max(0.0));
                                }
                                ov.dm.mark_messages_fetched();
                            }
                        }
                        let _ = next_after;
                    }
                }
                (DmChain::Sending { plaintext }, mlc_updater::DmState::Sent { id, created_at }) => {
                    if let DmView::Conversation { messages, .. } = &mut ov.dm.view {
                        messages.push(DmMessageUi {
                            id,
                            mine: true,
                            body: DmMessageBody::Plain(plaintext),
                            created_at,
                            read_at: None,
                        });
                    }
                    // Right after a send (owner ruling 2026-09-10, item 1):
                    // the list is stale the instant this conversation's
                    // `last_message_at` moves — drop the freshness stamp so
                    // the very next `drive_dm` frame's passive fetch
                    // (paid-gated, not tab-gated) picks it up immediately
                    // instead of waiting out `FORUM_REFRESH_MS`.
                    ov.dm.invalidate_conversations();
                }
                (DmChain::Direct, mlc_updater::DmState::Read) => {
                    if let DmView::Conversation { id, .. } = &ov.dm.view {
                        let id = *id;
                        if let Some(row) = ov.dm.conversations.iter_mut().find(|c| c.id == id) {
                            row.unread_count = 0;
                        }
                    }
                }
                (DmChain::Lookup { q }, mlc_updater::DmState::LookupResults(hits)) => {
                    ov.dm.lookup_query = Some(q);
                    ov.dm.lookup_results = hits
                        .into_iter()
                        .map(|h| DmLookupHitUi { public_id: h.public_id, nick: h.nick, label: h.label })
                        .collect();
                }
                (chain, mlc_updater::DmState::Failed { message })
                    if message == mlc_updater::forum_v2::NICK_REQUIRED_SENTINEL =>
                {
                    // Same "nick_required" gate as the forum composer
                    // (owner mandate 2026-09-09) — `caller_nick` is a
                    // shared account-level fact, so a Me refetch there
                    // re-drives the DM composer's own gate too.
                    ov.forum2.invalidate_me();
                    let _ = chain;
                }
                (chain, mlc_updater::DmState::Failed { message }) => {
                    ov.dm.last_error = Some(message.clone());
                    if matches!(chain, DmChain::Sending { .. }) {
                        write_failure = Some(message);
                    }
                }
                _ => {}
            }
        }

        for (id, label) in new_backfill_candidates {
            if !self.dm_peer_keys.contains_key(&id) {
                self.dm_backfill_queue.push_back((id, label));
            }
        }

        if let Some((public_id, peer_public_key)) = open_conversation_target {
            let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                mlc_updater::DmRequest::OpenConversation { with: public_id },
            ));
            self.dm_pending = Some((win_id, DmChain::OpeningConversation { peer_public_key }));
        }

        if let Some((conversation, peer_public_key)) = opened {
            self.dm_peer_keys.insert(conversation.id, peer_public_key);
            let ov = &mut pw.chart.panel_app.community_state;
            let row = DmConversationUi {
                id: conversation.id,
                peer_label: conversation.peer.label,
                peer_fingerprint: conversation.peer.fingerprint,
                last_message_at: conversation.last_message_at,
                unread_count: conversation.unread,
            };
            if let Some(existing) = ov.dm.conversations.iter_mut().find(|c| c.id == row.id) {
                *existing = row;
            } else {
                ov.dm.conversations.push(row);
            }
            ov.dm.open_conversation(conversation.id);
        }

        if let Some((conversation_id, key, fingerprint)) = backfilled {
            self.dm_peer_keys.insert(conversation_id, key);
            let ov = &mut pw.chart.panel_app.community_state;
            // Owner review 2026-09-23, `forum-messenger-realtime.md` item 2:
            // this cache used to overwrite silently — a peer's key rotating
            // (vault reset, reinstall, different profile) made every
            // message sealed under the old key permanently `Undecryptable`
            // with no explanation. `Some(old) != Some(new)` only (never
            // fires on the FIRST ever fingerprint for this row); read
            // before the mutable lookup below so the two `find`s never
            // overlap borrows.
            let old_fingerprint =
                ov.dm.conversations.iter().find(|c| c.id == conversation_id).and_then(|c| c.peer_fingerprint.clone());
            if old_fingerprint.is_some() && old_fingerprint.as_deref() != Some(fingerprint.as_str()) {
                ov.dm.mark_peer_key_changed(conversation_id);
            }
            if let Some(row) = ov.dm.conversations.iter_mut().find(|c| c.id == conversation_id) {
                row.peer_fingerprint = Some(fingerprint);
            }
        }

        if let Some(message) = write_failure {
            pw.chart.push_system_event(mlc_core::SystemEventKind::Info, "Message not sent", message, true);
        }

        pw.overlay_dirty = true;
        pw.window.request_redraw();
    }

    /// Per-frame drive for the Forum-v2 tab: applies the shared
    /// `forum_v2_rx` channel's settled answer (if any), then — only when
    /// nothing is in flight — drains one explicit user action or fires one
    /// passive fetch. Called from `about_to_wait`, right where v1's own
    /// Forum block already lives.
    #[cfg(feature = "cloud")]
    fn drive_forum2(&mut self) {
        use mlc_app_layout::ui::modal_settings::{
            CommunityTab, ForumV2Request as UiForumV2Request, ForumV2View, UserSettingsTab,
        };

        if self.updater_handle.forum_v2_rx.has_changed().unwrap_or(false) {
            let state = self.updater_handle.forum_v2_rx.borrow_and_update().clone();
            match state {
                mlc_updater::ForumV2State::Idle | mlc_updater::ForumV2State::Working => {}
                // Account-level fact — broadcast to every window directly,
                // before `apply_forum2_result` (the single tracked window)
                // is even reached, same idiom `DmState::Identity` uses for
                // its own broadcast fact.
                mlc_updater::ForumV2State::Me { classes, roles, nick } => {
                    for pw in self.windows.values_mut() {
                        let ov = &mut pw.chart.panel_app.community_state;
                        ov.forum2.caller_roles = roles.clone();
                        // Recompute `can_write` on whatever boards this
                        // window already has loaded — a `Me` answer landing
                        // after `Boards` must not wait for a second boards
                        // fetch to reflect it.
                        for b in ov.forum2.boards.iter_mut() {
                            b.can_write = forum2_can_write_native(&b.write_class, &classes);
                        }
                        ov.forum2.caller_classes = classes.clone();
                        ov.forum2.caller_nick = nick.clone();
                        ov.forum2.mark_me_fetched();
                        // Account tab's own mirrors of the same fetch — see
                        // `UserSettingsState::account_classes`'s own doc for
                        // why this crate carries a second copy instead of
                        // reading `CommunityState` directly.
                        pw.chart.panel_app.user_settings_state.account_classes = classes.clone();
                        pw.chart.panel_app.user_settings_state.account_roles = roles.clone();
                        pw.overlay_dirty = true;
                    }
                    self.forum2_pending = None;
                }
                settled => {
                    if let Some((win_id, chain)) = self.forum2_pending.take() {
                        self.apply_forum2_result(win_id, chain, settled);
                    }
                }
            }
        }

        if self.forum2_pending.is_some() {
            return;
        }

        // Explicit user action — drained from the first window that has one
        // armed (one shared channel across every window — see
        // `Forum2Chain`'s own doc for why only one action is ever in flight
        // at a time).
        let drained = self.windows.iter_mut().find_map(|(id, pw)| {
            pw.chart.panel_app.community_state.forum2.request.take().map(|r| (*id, r))
        });
        if let Some((win_id, req)) = drained {
            // Optimistic view pre-set, and the thread-id inference
            // `DeleteReply` needs (it carries no thread_id of its own) —
            // both read/written before firing.
            let mut thread_id_hint = None;
            if let Some(pw) = self.windows.get_mut(&win_id) {
                let ov = &mut pw.chart.panel_app.community_state;
                match &req {
                    UiForumV2Request::LoadBoard { slug, before } if before.is_none() => {
                        ov.forum2.view = ForumV2View::BoardThreads {
                            slug: slug.clone(),
                            threads: Vec::new(),
                            next_before: None,
                        };
                    }
                    UiForumV2Request::OpenThread { id } => {
                        ov.forum2.view = ForumV2View::Thread {
                            id: *id,
                            thread: None,
                            replies_tree: Vec::new(),
                            reply_to: None,
                        };
                    }
                    _ => {}
                }
                thread_id_hint = match &ov.forum2.view {
                    ForumV2View::Thread { id, .. } => Some(*id),
                    _ => None,
                };
            }

            let (cmd, chain) = match req {
                UiForumV2Request::LoadBoards => (mlc_updater::ForumV2Request::ListBoards, Forum2Chain::Direct),
                UiForumV2Request::LoadBoard { slug, before } => {
                    let append = before.is_some();
                    (
                        mlc_updater::ForumV2Request::ListBoardThreads { slug, before },
                        Forum2Chain::BoardThreads { append },
                    )
                }
                UiForumV2Request::OpenThread { id } => {
                    (mlc_updater::ForumV2Request::LoadThread { id }, Forum2Chain::Direct)
                }
                UiForumV2Request::CreateThread { slug, title, body } => (
                    mlc_updater::ForumV2Request::CreateThread { slug, title, body },
                    Forum2Chain::AwaitingWriteAck { thread_id_hint: None },
                ),
                UiForumV2Request::Reply { thread_id, body, parent_reply_id } => (
                    mlc_updater::ForumV2Request::Reply { thread_id, body, parent_reply_id },
                    Forum2Chain::AwaitingWriteAck { thread_id_hint: Some(thread_id) },
                ),
                UiForumV2Request::DeleteReply { id } => (
                    mlc_updater::ForumV2Request::DeleteReply { id },
                    Forum2Chain::AwaitingWriteAck { thread_id_hint },
                ),
                UiForumV2Request::Search { q, board } => {
                    (mlc_updater::ForumV2Request::Search { q: q.clone(), board }, Forum2Chain::Search { q })
                }
                UiForumV2Request::Moderate { thread_id, action, board } => (
                    mlc_updater::ForumV2Request::Moderate { thread_id, action, board },
                    Forum2Chain::AwaitingWriteAck { thread_id_hint: Some(thread_id) },
                ),
            };
            let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::ForumV2(cmd));
            self.forum2_pending = Some((win_id, chain));
            return;
        }

        // Debounced inline quick-search (owner review 2026-09-09, item d) —
        // mirrors the web shell's own `drive()` (`cloud_session.rs`): every
        // keystroke into the header's compact search box while it is open
        // (re)arms `quick_search_due_at` via `ForumV2State::
        // arm_quick_search_debounce` (`text_action_forum_search`); once the
        // timer elapses this fires the same `ForumV2Request::Search` Enter
        // already sends immediately (that function's own `fire`). Two-pass
        // like the passive scan below, for the same reason.
        let due_window = self.windows.iter().find_map(|(win_id, pw)| {
            pw.chart.panel_app.community_state.forum2.quick_search_due().then_some(*win_id)
        });
        if let Some(win_id) = due_window {
            if let Some(pw) = self.windows.get_mut(&win_id) {
                let q = {
                    let coord = pw.chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::FORUM_SEARCH))
                        .trim()
                        .to_string()
                };
                let ov = &mut pw.chart.panel_app.community_state.forum2;
                ov.quick_search_due_at = None;
                if q.is_empty() {
                    ov.quick_search_query = None;
                    ov.quick_search_results = Vec::new();
                } else {
                    ov.quick_search_pending = true;
                    ov.last_error = None;
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::ForumV2(
                        mlc_updater::ForumV2Request::Search { q: q.clone(), board: None },
                    ));
                    self.forum2_pending = Some((win_id, Forum2Chain::Search { q }));
                }
            }
            return;
        }

        // Passive: boards / a board's threads / the open thread — first
        // window found needing one. Two-pass (scan read-only, then mutate
        // + send) rather than acting inside the scan loop, so nothing here
        // has to reason about mutating `self.forum2_pending`/sending on
        // `self.updater_handle` while `self.windows` is mid-iteration.
        enum Passive {
            Me,
            Boards,
            BoardThreads(String),
            Thread(i64),
        }
        let mut found: Option<(WindowId, Passive)> = None;
        'scan: for (win_id, pw) in self.windows.iter() {
            let ov = &pw.chart.panel_app.community_state;
            let forum2_active = ov.is_open && ov.tab == CommunityTab::Forum;
            // User Settings' Account tab shows the same tier/role line off
            // this fetch (`UserSettingsState::account_classes`'s own doc) —
            // it must fire even when the Community modal itself is closed,
            // so the tab's line fills the moment it's looked at rather than
            // only after the Forum tab happens to have been opened once
            // (owner ruling 2026-09-10).
            let account_tab_visible = pw.chart.panel_app.user_settings_state.is_open
                && pw.chart.panel_app.user_settings_state.active_tab == UserSettingsTab::Account;
            // Once per tab-open (and again after a sign-in/sign-out edge
            // invalidates it — see the `auth_rx` edge block in
            // `about_to_wait`) — no refresh cadence of its own, see
            // `ForumV2State::me_fetched_at`'s own doc.
            if (forum2_active || account_tab_visible) && ov.forum2.me_fetched_at.is_none() {
                found = Some((*win_id, Passive::Me));
                break 'scan;
            }
            if !forum2_active {
                continue;
            }
            let boards_needs_fetch = ov.forum2.boards.is_empty() && ov.forum2.boards_fetched_at.is_none();
            if boards_needs_fetch || (!ov.forum2.boards.is_empty() && ov.forum2.boards_due_for_refresh()) {
                found = Some((*win_id, Passive::Boards));
                break 'scan;
            }
            if let ForumV2View::BoardThreads { slug, threads, .. } = &ov.forum2.view {
                let stale = threads.is_empty() && ov.forum2.board_threads_fetched_at.is_none();
                if stale || ov.forum2.board_threads_due_for_refresh() {
                    found = Some((*win_id, Passive::BoardThreads(slug.clone())));
                    break 'scan;
                }
            }
            if let ForumV2View::Thread { id, thread: Some(_), .. } = &ov.forum2.view {
                if ov.forum2.thread_due_for_refresh() {
                    found = Some((*win_id, Passive::Thread(*id)));
                    break 'scan;
                }
            }
        }
        if let Some((win_id, passive)) = found {
            if let Some(pw) = self.windows.get_mut(&win_id) {
                let ov = &mut pw.chart.panel_app.community_state;
                match &passive {
                    Passive::Me => ov.forum2.me_refreshing = true,
                    Passive::Boards => ov.forum2.boards_refreshing = true,
                    Passive::BoardThreads(_) => ov.forum2.board_threads_refreshing = true,
                    Passive::Thread(_) => ov.forum2.thread_refreshing = true,
                }
            }
            let (cmd, chain) = match passive {
                Passive::Me => (mlc_updater::ForumV2Request::Me, Forum2Chain::Me),
                Passive::Boards => (mlc_updater::ForumV2Request::ListBoards, Forum2Chain::Direct),
                Passive::BoardThreads(slug) => (
                    mlc_updater::ForumV2Request::ListBoardThreads { slug, before: None },
                    Forum2Chain::BoardThreads { append: false },
                ),
                Passive::Thread(id) => (mlc_updater::ForumV2Request::LoadThread { id }, Forum2Chain::Direct),
            };
            let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::ForumV2(cmd));
            self.forum2_pending = Some((win_id, chain));
        }
    }

    /// Per-frame drive for Direct Messages: `dm.available`/`dm.last_error`
    /// on every window, identity derive/publish, redecrypt-in-place, the
    /// shared `dm_rx` channel's settled answer, then — only when nothing
    /// is in flight — the passive conversations-list fetch (paid-gated,
    /// not tied to any window/tab), and, only for whichever window (if
    /// any) is actually showing Messages: the recipient-search debounce,
    /// the open conversation's passive message poll, one explicit user
    /// action, or the peer-key backfill queue. Same structure as
    /// [`Self::drive_forum2`].
    ///
    /// Until 2026-09-10 this whole function returned immediately unless
    /// SOME window had the Messages tab open, and again if the vault was
    /// sealed — the root cause of the owner's report that a recipient
    /// never saw an incoming DM until they searched the sender by nick:
    /// the conversations list only ever refetched while already looking
    /// at it. `dm.available` itself also never checked `paid` at all on
    /// this shell (a second, independent bug fixed in the same pass) —
    /// see `dm.available`'s own stamping loop below.
    #[cfg(feature = "cloud")]
    fn drive_dm(&mut self) {
        use mlc_app_layout::ui::modal_settings::{
            CommunityTab, DmMessagePage, DmRequest as UiDmRequest, DmUnavailableReason, DmView, DM_MESSAGE_PAGE_LIMIT,
        };

        // Account-level facts — neither needs a specific window/tab on
        // screen. `messenger_ok` is the single ladder-class predicate
        // (`ForumV2State::messenger_available` — owner ruling E-14,
        // 2026-09-23: class and role, not payment) the Messages tab itself
        // gates on; deliberately NOT `entitlements_rx`/`forum_entitled`,
        // which answer a payment question the ladder no longer asks here.
        // `caller_classes`/`caller_signed_in` are broadcast identically to
        // every window on each `Me` answer (`apply_forum2_result`'s own
        // `Me` arm), so any one window speaks for the account — same
        // "one candidate answers for all" idiom `confirmed_fingerprint`
        // below already relies on.
        let messenger_ok = self
            .windows
            .values()
            .next()
            .is_some_and(|pw| pw.chart.panel_app.community_state.forum2.messenger_available());
        let vault_key = self.core.user().vault_key();

        // Stamp `dm.available`/`dm.last_error` on every window, every
        // frame — cheap (two field writes each). Vault gates identity
        // (send/decrypt); `messenger_ok` gates the feature outright; neither
        // gates the LIST or the ciphertext fetch below (owner ruling
        // 2026-09-10: both must render — the list, and messages as raw
        // ciphertext — before the vault is even unlocked).
        for pw in self.windows.values_mut() {
            let comm = &mut pw.chart.panel_app.community_state;
            let messages_active = comm.is_open && comm.tab == CommunityTab::Messages;
            if vault_key.is_none() {
                comm.dm.available = false;
                comm.dm.unavailable_reason =
                    if messenger_ok { DmUnavailableReason::VaultLocked } else { DmUnavailableReason::NotPaid };
                if messages_active {
                    comm.dm.last_error = Some("Unlock the vault to use messages".to_string());
                }
            } else if !messenger_ok {
                comm.dm.available = false;
                comm.dm.unavailable_reason = DmUnavailableReason::NotPaid;
                if messages_active {
                    comm.dm.last_error = Some("Messages need a paid account".to_string());
                }
            } else {
                comm.dm.available = true;
                comm.dm.unavailable_reason = DmUnavailableReason::None;
            }
        }

        if let Some(key) = &vault_key {
            if self.dm_identity.is_none() {
                self.dm_identity = Some(mlc_vault::dm::DmIdentity::from_vault_key(key));
            }
        }

        // Keep already-fetched ciphertext decrypting in place as new key
        // material lands (the vault unlocking, or a peer-key backfill
        // landing) — see `redecrypt_open_conversation`'s own doc. Cheap
        // no-op once nothing is left to retry, so this runs unconditionally
        // rather than edge-detecting either trigger.
        let conv_windows: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, pw)| {
                matches!(pw.chart.panel_app.community_state.dm.view, DmView::Conversation { .. })
            })
            .map(|(id, _)| *id)
            .collect();
        for win_id in conv_windows {
            redecrypt_open_conversation(&mut self.windows, win_id, self.dm_identity.as_ref(), &self.dm_peer_keys);
        }

        if self.updater_handle.dm_rx.has_changed().unwrap_or(false) {
            let state = self.updater_handle.dm_rx.borrow_and_update().clone();
            match state {
                mlc_updater::DmState::Idle | mlc_updater::DmState::Working => {}
                // Account-level fact — broadcast to every window regardless
                // of `dm_pending` (the auto-publish below never sets it at
                // all), rather than routed like every other answer.
                mlc_updater::DmState::Identity { fingerprint } => {
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.community_state.dm.identity_fingerprint = Some(fingerprint.clone());
                        pw.overlay_dirty = true;
                    }
                }
                settled => {
                    if let Some((win_id, chain)) = self.dm_pending.take() {
                        self.apply_dm_result(win_id, chain, settled);
                    }
                }
            }
        }

        // Auto-publish this profile's key once signed in + paid — the ONLY
        // publish path (owner ruling 2026-09-09: no user-facing button; the
        // identity itself is derived from the vault key, so there is
        // nothing to generate or remember). Fires at most once per unlock
        // (`dm_identity_publish_attempted_for` is cleared by
        // `refresh_dm_identity` on every fresh unlock/profile switch) and
        // again after a sign-in edge (cleared in `about_to_wait`'s
        // `newly_signed_in_as` handling) — the guard stops this from
        // resending on every frame while an earlier attempt's answer is
        // still in flight. Triggers whenever the server has not confirmed a
        // MATCHING fingerprint yet, covering both "never published" (no
        // confirmation at all) and "the confirmed key is stale" (this
        // identity rotated — e.g. a different vault key now — since the
        // server's last confirmed answer).
        if self.dm_pending.is_none() {
            if let Some(identity) = &self.dm_identity {
                // Every window mirrors the same account-level fact (the
                // `Identity` broadcast above writes it to all of them), so
                // any one that already has an answer speaks for the
                // account — no window is `messages_window`-only here
                // (this fires happily with the Messages tab closed
                // everywhere, or no window at all yet).
                let confirmed_fingerprint = self
                    .windows
                    .values()
                    .find_map(|pw| pw.chart.panel_app.community_state.dm.identity_fingerprint.clone());
                let my_fingerprint = mlc_vault::dm::fingerprint(&identity.public);
                let already_confirmed = confirmed_fingerprint.as_deref() == Some(my_fingerprint.as_str());
                let already_attempted = self.dm_identity_publish_attempted_for == Some(identity.public);
                if messenger_ok && !already_confirmed && !already_attempted {
                    let public_key_b64 = dm_encode(&identity.public);
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::PublishIdentity { public_key_b64 },
                    ));
                    self.dm_identity_publish_attempted_for = Some(identity.public);
                    // No window to route the answer to — `Identity` is
                    // broadcast unconditionally above regardless of
                    // `dm_pending`, so this is left `None`; a failure lands
                    // in `dm.last_error` the same (best-effort) way an
                    // explicit action's does, via `apply_dm_result`'s
                    // `Failed` arm, only if nothing else claims `dm_pending`
                    // first — unchanged from this shell's existing
                    // non-guarantee for the auto-publish path.
                }
            }
        }

        if self.dm_pending.is_some() {
            return;
        }

        // Explicit user action.
        let drained = self.windows.iter_mut().find_map(|(id, pw)| {
            pw.chart.panel_app.community_state.dm.request.take().map(|r| (*id, r))
        });
        if let Some((win_id, req)) = drained {
            match req {
                UiDmRequest::LoadConversations => {
                    if let Some(pw) = self.windows.get_mut(&win_id) {
                        pw.chart.panel_app.community_state.dm.conversations_refreshing = true;
                    }
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::ListConversations,
                    ));
                    self.dm_pending = Some((win_id, DmChain::Direct));
                }
                UiDmRequest::Open { with_label } => {
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::GetIdentity { label: with_label },
                    ));
                    self.dm_pending = Some((win_id, DmChain::OpeningPeerLookup));
                }
                UiDmRequest::LoadMessages { id, page } => {
                    if let Some(pw) = self.windows.get_mut(&win_id) {
                        pw.chart.panel_app.community_state.dm.messages_refreshing = true;
                    }
                    let (after, before) = match page {
                        DmMessagePage::Newest => (None, None),
                        DmMessagePage::After(a) => (Some(a), None),
                        DmMessagePage::Before(b) => (None, Some(b)),
                    };
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::ListMessages { id, after, before, limit: DM_MESSAGE_PAGE_LIMIT },
                    ));
                    self.dm_pending = Some((win_id, DmChain::LoadingMessages { page }));
                }
                UiDmRequest::Send { id, plaintext } => {
                    let Some(identity) = &self.dm_identity else {
                        if let Some(pw) = self.windows.get_mut(&win_id) {
                            pw.chart.panel_app.community_state.dm.last_error =
                                Some("Unlock the vault to send messages".to_string());
                        }
                        return;
                    };
                    let Some(peer_public) = self.dm_peer_keys.get(&id).copied() else {
                        if let Some(pw) = self.windows.get_mut(&win_id) {
                            pw.chart.panel_app.community_state.dm.last_error =
                                Some("Open this conversation again to send".to_string());
                        }
                        return;
                    };
                    let key: [u8; 32] = *mlc_vault::dm::conversation_key(&identity.secret, &peer_public, &id.to_string());
                    let my_fingerprint = mlc_vault::dm::fingerprint(&identity.public);
                    let aad = dm_aad(id, &my_fingerprint);
                    let sealed = mlc_vault::dm::seal_message(&key, plaintext.as_bytes(), &aad);
                    let nonce_b64 = dm_encode(&sealed.nonce);
                    let ciphertext_b64 = dm_encode(&sealed.ciphertext);
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::Send { id, nonce_b64, ciphertext_b64 },
                    ));
                    self.dm_pending = Some((win_id, DmChain::Sending { plaintext }));
                }
                UiDmRequest::MarkRead { id, up_to } => {
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::MarkRead { id, up_to },
                    ));
                    self.dm_pending = Some((win_id, DmChain::Direct));
                }
            }
            return;
        }

        if self.dm_pending.is_some() {
            return;
        }

        // Passive: conversations list — independent of the vault and of
        // which window/tab is open (this function's own doc: the owner
        // report's root cause). One at a time across every window, same
        // "found = one candidate, fire it" idiom `drive_forum2`'s own
        // boards/threads scan uses; also what a 30 s (`FORUM_REFRESH_MS`)
        // background poll and the envelope badge lighting with the modal
        // closed both come from for free, since this is not gated on
        // `messages_window` at all.
        if messenger_ok {
            let due_win = self.windows.iter().find_map(|(id, pw)| {
                let dm = &pw.chart.panel_app.community_state.dm;
                (dm.conversations_fetched_at.is_none() || dm.conversations_due_for_refresh()).then_some(*id)
            });
            if let Some(win_id) = due_win {
                if let Some(pw) = self.windows.get_mut(&win_id) {
                    pw.chart.panel_app.community_state.dm.conversations_refreshing = true;
                }
                let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                    mlc_updater::DmRequest::ListConversations,
                ));
                self.dm_pending = Some((win_id, DmChain::Direct));
                return;
            }
        }

        // Everything below needs an actual window showing Messages, ladder-
        // available (`messenger_ok`'s own per-window mirror) — a stale
        // `tab == Messages` surviving a sign-out or a role change mid-
        // session must not keep firing the recipient-search/backfill
        // fetches below (same "stale click target" reasoning the render
        // side's `active_tab()` fallback already applies).
        let messages_window: Option<WindowId> = self.windows.iter().find_map(|(id, pw)| {
            let ov = &pw.chart.panel_app.community_state;
            (ov.is_open && ov.tab == CommunityTab::Messages && ov.forum2.messenger_available()).then_some(*id)
        });
        let Some(messages_window) = messages_window else {
            return;
        };

        // Debounced recipient-search (owner ruling 2026-09-09, community-
        // modal-and-forum-ladder arc, piece 2 item b) — mirrors the web
        // shell's own `drive()` (`cloud_session.rs`): every keystroke into
        // the "start a conversation" field (re)arms `lookup_due_at` via
        // `DmState::arm_lookup_debounce` (`text_action_dm_peer`); once the
        // timer elapses this fires `GET /api/dm/lookup?q=`, routed to
        // `messages_window` — the only window this pass ever touches (see
        // that binding's own doc).
        let lookup_due = self
            .windows
            .get(&messages_window)
            .map(|pw| pw.chart.panel_app.community_state.dm.lookup_due())
            .unwrap_or(false);
        if lookup_due {
            if let Some(pw) = self.windows.get_mut(&messages_window) {
                let q = {
                    let coord = pw.chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::DM_PEER))
                        .trim()
                        .to_string()
                };
                let dm = &mut pw.chart.panel_app.community_state.dm;
                dm.lookup_due_at = None;
                if q.is_empty() {
                    dm.lookup_query = None;
                    dm.lookup_results = Vec::new();
                } else {
                    dm.last_error = None;
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::Lookup { q: q.clone() },
                    ));
                    self.dm_pending = Some((messages_window, DmChain::Lookup { q }));
                }
            }
            return;
        }

        // Passive: the open conversation's messages, on open or
        // due-refresh — silent (no view flip), same tolerance v1's own
        // thread refresh gives an in-progress draft. Parity with the web
        // shell's own poll (`cloud_session.rs`'s `drive()`); this native
        // shell had no cadence here at all before this pass — a
        // conversation left open never refreshed until it was closed and
        // reopened. `after = highest id held` once anything has landed
        // (`DmState::refresh_page`) — owner report F4 fix, 2026-09-23:
        // this used to always send `after: None`, which pinned any
        // conversation past one page to its oldest page forever.
        if let Some(pw) = self.windows.get(&messages_window) {
            let dm = &pw.chart.panel_app.community_state.dm;
            if let DmView::Conversation { id, .. } = &dm.view {
                let id = *id;
                let due = dm.messages_fetched_at.is_none() || dm.messages_due_for_refresh();
                if due {
                    let page = dm.refresh_page().unwrap_or(DmMessagePage::Newest);
                    if let Some(pw) = self.windows.get_mut(&messages_window) {
                        pw.chart.panel_app.community_state.dm.messages_refreshing = true;
                    }
                    let (after, before) = match page {
                        DmMessagePage::Newest => (None, None),
                        DmMessagePage::After(a) => (Some(a), None),
                        DmMessagePage::Before(b) => (None, Some(b)),
                    };
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                        mlc_updater::DmRequest::ListMessages { id, after, before, limit: DM_MESSAGE_PAGE_LIMIT },
                    ));
                    self.dm_pending = Some((messages_window, DmChain::LoadingMessages { page }));
                    return;
                }
            }
        }

        // Backfill: one peer-key lookup at a time for a LIST-sourced
        // conversation the `Open` flow never touched. Routed to
        // `messages_window` — the only window this pass ever touches (see
        // that binding's own doc).
        while let Some((conversation_id, label)) = self.dm_backfill_queue.pop_front() {
            if self.dm_peer_keys.contains_key(&conversation_id) {
                continue;
            }
            let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Dm(
                mlc_updater::DmRequest::GetIdentity { label },
            ));
            self.dm_pending = Some((messages_window, DmChain::BackfillPeerKey { conversation_id }));
            break;
        }
    }

    fn build_profile_for_save(&self, window_states: Vec<mlc_core::WindowState>) -> mlc_core::UserProfile {
        let preferred_key = self.last_focused
            .filter(|id| self.windows.contains_key(id))
            .or_else(|| self.windows.keys().next().copied());

        let mut profile = self.profile.clone();
        profile.ui.windows = window_states;

        // Use the shared core as the canonical source for connector_enabled
        // (Phase C); AppState for theme + device identity (the three §2.3
        // survivor fields).
        profile.domain.connector_enabled = self.core.user().connector_enabled().clone();
        // Favorite drawing tools (favorite-primitives arc) live on the shared
        // core profile (mutated in-place by `ToggleFavoritePrimitive`) —
        // carry them like `connector_enabled` above so a save never clobbers
        // the live vec with a stale `self.profile` copy.
        profile.ui.favorite_primitives = self.core.user().profile().ui.favorite_primitives.clone();
        // Favorite indicators (indicator-search-favorites arc) — same carry
        // as `favorite_primitives` above (mutated in-place on the shared core
        // profile by `ToggleIndicatorStar`).
        profile.ui.favorite_indicators = self.core.user().profile().ui.favorite_indicators.clone();
        // The welcome wizard's own "seen" flag — the fourth field to need this
        // carry, and the one that proves the shape is wrong rather than the
        // list incomplete.
        //
        // `mark_welcome_wizard_seen` sets it on the core copy. Every save
        // starts from `self.profile`, which does not have it, so finishing the
        // wizard wrote `false` back over the `true` it had just set — the
        // wizard's completion triggers a save on itself. That is why the owner
        // saw the theme picker on every launch of a profile with 55 presets and
        // a linked account, and why marking it seen on the two bypass paths
        // (57b0be78) could not have helped: the loud path was being undone too.
        //
        // The real defect is above this line: two live copies of one profile,
        // each authoritative for different fields, reconciled by a list a human
        // has to remember to extend. Inverting the base does not fix it —
        // language, display name, avatar, active preset and open tabs are
        // written on `self.profile` and would become the next victims. One
        // owner per field is the fix, and it is a refactor, not a carry.
        profile.ui.welcome_wizard_seen = self.core.user().profile().ui.welcome_wizard_seen;
        let core_ui = self.core.user().profile().ui.clone();
        profile.ui.persist_toolbar_last_tools = core_ui.persist_toolbar_last_tools;
        profile.ui.persist_toolbar_toggles = core_ui.persist_toolbar_toggles;
        if !core_ui.persist_toolbar_last_tools {
            profile.ui.toolbar_last_tools = core_ui.toolbar_last_tools;
        }
        if !core_ui.persist_toolbar_toggles {
            profile.ui.drawings_locked = core_ui.drawings_locked;
            profile.ui.drawings_visible = core_ui.drawings_visible;
        }
        profile.ui.active_theme = self.app_state.theme_preset.clone();
        profile.ui.device_name = self.app_state.device_name.clone();
        profile.domain.app_version = self.app_state.app_version.clone();
        // recalc_mode is now persisted to DeviceSettings, not UserProfile.

        if let Some(key) = preferred_key {
            if let Some(pw) = self.windows.get(&key) {
                profile.ui.active_ui_style = pw.chart.panel_app.theme_manager.current().style.as_name().to_string();
                profile.ui.sidebar_visible = pw.chart.sidebar_state.is_right_open();
                profile.ui.sidebar_panel = mlc_app::ChartApp::panel_to_str(pw.chart.sidebar_state.right_panel);
                profile.ui.sidebar_width = Some(pw.chart.sidebar_state.right_sidebar_width);
                let inline = &pw.chart.panel_app.toolbar_state.floating_inline_bar;
                profile.ui.inline_bar_x = Some(inline.x);
                profile.ui.inline_bar_y = Some(inline.y);
                let dock_str = match inline.dock_edge {
                    mlc_core::InlineDockEdge::Bottom => "Bottom",
                    mlc_core::InlineDockEdge::Top => "Top",
                    mlc_core::InlineDockEdge::Free => "Free",
                };
                profile.ui.inline_bar_dock = Some(dock_str.to_string());
                // Favorites quick-pick bar position + visibility (favorite-
                // primitives arc) — same preferred-window carry as the
                // inline bar above.
                let fav = &pw.chart.panel_app.toolbar_state.favorites_bar;
                profile.ui.favorites_bar_x = Some(fav.x);
                profile.ui.favorites_bar_y = Some(fav.y);
                let fav_dock_str = match fav.dock_edge {
                    mlc_core::InlineDockEdge::Bottom => "Bottom",
                    mlc_core::InlineDockEdge::Top => "Top",
                    mlc_core::InlineDockEdge::Free => "Free",
                };
                profile.ui.favorites_bar_dock = Some(fav_dock_str.to_string());
                profile.ui.favorites_bar_visible = pw.chart.panel_app.toolbar_state.favorites_bar_visible;
                // Screenshot destination (screenshot-destination arc) —
                // live toolbar-state value, same preferred-window carry as
                // the favorites bar above.
                profile.ui.screenshot_destination = pw.chart.panel_app.toolbar_state.screenshot_destination;
                // Community modal per-tab size (community-modal chrome arc)
                // — same preferred-window carry as the favorites bar above.
                #[cfg(feature = "cloud")]
                {
                    profile.ui.community_forum_size = pw.chart.panel_app.community_state.forum_size;
                    profile.ui.community_messages_size = pw.chart.panel_app.community_state.messages_size;
                }
                // System event log (system-event-bus arc, 2026-09-05) —
                // same preferred-window carry as the screenshot destination
                // above: `chart.system_events()` is the live value, mutated
                // in place by `push_system_event`/
                // `mark_all_system_events_read`, not by this snapshot.
                profile.ui.system_events = pw.chart.system_events().clone();
                if profile.ui.persist_toolbar_last_tools {
                    profile.ui.toolbar_last_tools = pw.chart.panel_app.toolbar_state.quick_select_memory
                        .iter()
                        .map(|(k, (t, _))| (k.clone(), t.clone()))
                        .collect();
                }
                if profile.ui.persist_toolbar_toggles {
                    profile.ui.drawings_locked = pw.chart.panel_app.toolbar_state.drawings_locked;
                    profile.ui.drawings_visible = pw.chart.panel_app.toolbar_state.drawings_visible;
                }
            }
        }

        profile
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

        // 1b. Drain preset actions queued by autosave_snapshot directly into
        // the shared core (Phase B — no more AppState mirror to stage them in).
        for pw in self.windows.values_mut() {
            for action in pw.chart.preset_actions.drain(..) {
                match action {
                    mlc_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.core.presets_mut().insert(id, preset);
                    }
                    mlc_app::PresetAction::Delete { id } => {
                        self.core.presets_mut().remove(&id);
                    }
                    mlc_app::PresetAction::Rename { id, new_name } => {
                        if let Some(p) = self.core.presets_mut().get_mut(&id) {
                            p.name = new_name;
                        }
                    }
                }
            }
        }

        // 2. Sync OS position/size into ChartApp fields.
        self.sync_window_geometry_from_os();

        // 3. Collect window states, excluding deleted ones.
        let window_states: Vec<mlc_core::WindowState> = self.windows.values()
            .filter(|pw| !exclude_window_ids.contains(&pw.window_id))
            .map(|pw| pw.chart.build_window_state())
            .collect();

        for ws in &window_states {
            debug!("[save_all] window_id={} pos=({:?},{:?}) size=({:?},{:?}) tabs={} active={}",
                ws.window_id, ws.x, ws.y, ws.width, ws.height,
                ws.open_tabs.len(), ws.active_preset_id);
        }

        // 4. Build profile from in-memory profile base, updating window list and
        //    sidebar/toolbar state from the preferred window (last focused, or first
        //    available as fallback) for deterministic output instead of HashMap order.
        let mut profile = self.build_profile_for_save(window_states);

        // save_all additionally persists server settings and notification
        // settings — fields the lighter-weight dirty-drain paths
        // (profile-dirty / geometry-only in about_to_wait) do not touch, so
        // they stay out of `build_profile_for_save` itself. Scale mode is
        // never persisted (viewport SWC arc, 2026-07-03) — every cold
        // preset load forces Auto by design.
        profile.domain.server_enabled = self.core.user().server_enabled();
        profile.domain.server_port = self.core.user().server_port();

        // Persist notification settings from the first window's alert settings state.
        // Secrets only follow when the vault was unlocked this session — a
        // locked window's `notification_secrets` is an empty snapshot, and
        // writing it unconditionally would wipe `profile.domain
        // .notification_secrets` in RAM for the rest of the session (moot for
        // the on-disk `vault.enc`, since `save_profile` never touches it
        // without a key — but the in-memory copy matters if the vault gets
        // unlocked later without a full profile reload).
        if let Some(pw) = self.windows.values().next() {
            let s = &pw.chart.panel_app.alert_settings_state;
            profile.domain.notification_settings = s.notification_settings.clone();
            if s.secrets_available {
                profile.domain.notification_secrets = s.notification_secrets.clone();
            }
        }

        // 5. Save profile.json once.
        let vault_key = self.core.user().vault_key();
        let vault_key = vault_key.as_ref();
        if let Err(e) = mlc_core::save_profile(&profile, vault_key) {
            error!("[App] Failed to save profile: {}", e);
        } else {
            // Keep in-memory profile up to date.
            self.profile = profile;
        }

        // 6. Save watchlists.json from the shared core (always plaintext — no vault key).
        {
            let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
            // Bound first so the `watchlists()` read guard drops before the body
            // runs (clippy::significant_drop_in_scrutinee).
            let watchlists_snapshot = self.core.watchlists().snapshot();
            if let Err(e) = mlc_core::save_json(&watchlists_path, &watchlists_snapshot, None) {
                error!("[App] Failed to save watchlists: {}", e);
            }
        }

        // 7. Save settings snapshots from the shared core (always plaintext — no vault key).
        {
            let path = mlc_core::active_profile_data_dir().join("settings_snapshots.json");
            // Bound first so the `user()` read guard drops before the save call
            // runs (clippy::significant_drop_in_scrutinee).
            let save_result = mlc_core::save_json(&path, self.core.user().snapshots(), None);
            if let Err(e) = save_result {
                error!("[App] Failed to save settings snapshots: {}", e);
            }
        }

        // 8. Save templates from the shared core (single canonical source of truth).
        // Bound first so the `templates()` guard drops before the body runs
        // (clippy::significant_drop_in_scrutinee).
        let templates_save_result = self.core.templates().save_to_default_dir(vault_key);
        if let Err(e) = templates_save_result {
            error!("[App] Failed to save templates: {:?}", e);
        }

        // 9. Save all presets from the shared core (single canonical source of truth),
        // except read-only shared-viewer tabs (native-share-viewer arc,
        // 2026-09-06). This is the sync hand-off: `mlc-updater::cloud_sync`
        // never sees `ChartApp` — it just reads every `*.json` under the
        // on-disk `presets/` directory (`collect_local_items`) — so writing
        // a shared preset here at all is what would hand it to the next
        // sync cycle as an ordinary preset, the server's own frozen copy
        // pushed straight back at it. `is_readonly_tab` is per-preset-id on
        // `ChartApp` (see that method's own doc), so this checks every open
        // window rather than reading a shell-global flag.
        for preset in self.core.presets().values() {
            if self.windows.values().any(|pw| pw.chart.is_readonly_tab(&preset.id)) {
                continue;
            }
            if let Err(e) = mlc_core::preset::storage::save_preset(preset) {
                error!("[App] failed to save preset {}: {}", preset.id, e);
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
                    if self.core.user().is_connector_enabled(eid_str) {
                        self.core.bootstrap_request_symbols(exchange_id);
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

        // No skeleton branch here any more. A first run reaches this with a
        // profile already on disk (see `main()`) and no saved windows, so it
        // falls into the same "create a default window" path every other
        // profile-with-no-saved-geometry takes — and gets a live chart.
        if self.saved_windows.is_empty() {
            info!("[App] No saved windows — creating default");
            self.create_window(event_loop, None, None);
        } else {
            let windows_to_restore = self.saved_windows.clone();
            for ws in &windows_to_restore {
                debug!("[App] Restoring window: id={} pos=({:?},{:?}) size=({:?},{:?}) tabs={} active={}",
                    ws.window_id, ws.x, ws.y, ws.width, ws.height,
                    ws.open_tabs.len(), ws.active_preset_id);
                self.create_window(event_loop, Some(ws), None);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // ── Native share deep link — apply once the first window exists ────
        // `main()` parses argv into `pending_deep_link_share` before any
        // window is created (`ChartApp::request_open_share` needs a live
        // `ChartApp` to queue onto). One-shot via `.take()`; the per-window
        // drain further down (next to `drain_pending_share_live`) picks up
        // the resulting `pending_open_share` and sends the actual command.
        if self.pending_deep_link_share.is_some() && !self.windows.is_empty() {
            if let Some(raw) = self.pending_deep_link_share.take() {
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.request_open_share(&raw);
                }
            }
        }

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
                    debug!(
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

        // ── Drain watchlist actions from all windows → shared core ──────────
        // Windows queue WatchlistAction instead of mutating directly.
        // `watchlist_manager` (symbol data) is domain — goes through the
        // shared `core.watchlists_mut()` (Phase A/B, one instance for the
        // whole process). `watchlist_ui` (which tab is active, column
        // widths, sort snapshot) is per-window PRESENTATION — stays on
        // `pw.chart.watchlist_ui`, never synced across windows (verdict:
        // shared-backend-core-2026-07-03.md Phase-B note — the agent-API
        // snapshot builder already treats it as per-window).
        for pw in self.windows.values_mut() {
            let actions: Vec<mlc_app::WatchlistAction> = pw.chart.watchlist_actions.drain(..).collect();
            for action in actions {
                let active_list_id = pw.chart.watchlist_ui.active_list_id;
                match action {
                    mlc_app::WatchlistAction::Toggle { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange.clone(), symbol.clone(), account_type.parse().unwrap_or_default());
                        let now_in = self.core.watchlists_mut().toggle_symbol(active_list_id, &address);
                        info!("[App] watchlist toggle: {}:{}:{} -> in_watchlist={}", symbol, exchange, account_type, now_in);
                        if now_in {
                            if let Some(eid) = mlc_app::ExchangeId::from_str(&exchange) {
                                let enabled = self.core.user().is_connector_enabled(eid.as_str());
                                if enabled {
                                    let at = mlc_app::account_type_from_label(&account_type);
                                    self.core.ensure_connector(eid);
                                    self.bridge.subscribe_mini_ticker(eid, &symbol, at);
                                }
                            }
                            pw.chart.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                        } else {
                            if let Some(eid) = mlc_app::ExchangeId::from_str(&exchange) {
                                let at = mlc_app::account_type_from_label(&account_type);
                                self.core.unsubscribe_mini_ticker(eid, &symbol, at);
                            }
                            if let Some(ref mut snap) = pw.chart.watchlist_ui.list_mut(active_list_id).order_snapshot {
                                snap.retain(|s| s.symbol != symbol);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::Remove { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        self.core.watchlists_mut().remove_symbol(active_list_id, &address);
                    }
                    mlc_app::WatchlistAction::ClearOrderSnapshot => {
                        pw.chart.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    }
                    mlc_app::WatchlistAction::Reorder { from_idx, to_idx } => {
                        self.core.watchlists_mut().reorder_symbol(active_list_id, from_idx, to_idx);
                    }
                    mlc_app::WatchlistAction::CreateList { name } => {
                        let new_id = self.core.watchlists_mut().create_list(name);
                        pw.chart.watchlist_ui.active_list_id = new_id;
                    }
                    mlc_app::WatchlistAction::RenameList { id, new_name } => {
                        if let Some(list) = self.core.watchlists_mut().find_mut(id) {
                            list.name = new_name;
                        }
                    }
                    mlc_app::WatchlistAction::DeleteList { id } => {
                        if self.core.watchlists_mut().delete_list(id) {
                            pw.chart.watchlist_ui.remove_list(id);
                            if pw.chart.watchlist_ui.active_list_id == id {
                                pw.chart.watchlist_ui.active_list_id = self.core.watchlists().lists().first().map(|l| l.id).unwrap_or(0);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::SetActiveList { id } => {
                        pw.chart.watchlist_ui.active_list_id = id;
                    }
                    mlc_app::WatchlistAction::SetColorFlag { symbol, exchange, account_type, color } => {
                        if let Some(list) = self.core.watchlists_mut().find_mut(active_list_id) {
                            let color_str = color.as_deref().unwrap_or("");
                            let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                            list.set_color_flag(&address, color_str);
                        }
                    }
                    mlc_app::WatchlistAction::MoveToGroup { symbol, exchange, account_type, group_name } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        if let Some(list) = self.core.watchlists_mut().find_mut(active_list_id) {
                            if let Some(group) = list.groups.iter().find(|g| g.name == group_name) {
                                let group_id = group.id;
                                list.move_to_group(&address, group_id);
                            }
                        }
                    }
                    mlc_app::WatchlistAction::RemoveFromGroup { symbol, exchange, account_type } => {
                        let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                        if let Some(list) = self.core.watchlists_mut().find_mut(active_list_id) {
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
                        pw.chart.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = Some(offsets);
                    }
                    mlc_app::WatchlistAction::ResetSeparatorOffsets => {
                        pw.chart.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = None;
                    }
                    mlc_app::WatchlistAction::SetSeparatorOffset { index, value } => {
                        let list_ui = pw.chart.watchlist_ui.list_mut(active_list_id);
                        // Count visible columns to get correct separator count.
                        let n_cols = {
                            let c = &list_ui.column_config;
                            let mut n: usize = 1; // Symbol always on
                            if c.show_exchange     { n += 1; }
                            if c.show_account_type { n += 1; }
                            if c.show_last_price   { n += 1; }
                            if c.show_change_pct   { n += 1; }
                            if c.show_spark        { n += 1; }
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
                        let list_ui = pw.chart.watchlist_ui.list_mut(active_list_id);
                        match column.as_str() {
                            "exchange"     => list_ui.column_config.show_exchange     = !list_ui.column_config.show_exchange,
                            "last_price"   => list_ui.column_config.show_last_price   = !list_ui.column_config.show_last_price,
                            "change_pct"   => list_ui.column_config.show_change_pct   = !list_ui.column_config.show_change_pct,
                            "change_abs"   => list_ui.column_config.show_change_abs   = !list_ui.column_config.show_change_abs,
                            "spark"        => list_ui.column_config.show_spark        = !list_ui.column_config.show_spark,
                            "ticker"       => list_ui.column_config.show_ticker_section = !list_ui.column_config.show_ticker_section,
                            "counters"     => list_ui.column_config.show_market_counters = !list_ui.column_config.show_market_counters,
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
                        pw.chart.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    }
                    mlc_app::WatchlistAction::ResetSort => {
                        pw.chart.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    }
                }
            }
        }

        // ── Drain connector actions from all windows → shared core ───────────
        // Domain value (which exchanges are enabled) lives on core.user() now
        // (Phase C) — the per-window sidebar_state.connector_enabled
        // presentation copy is refreshed unconditionally every frame
        // (see about_to_wait), no dirty flag needed.
        for pw in self.windows.values_mut() {
            for action in pw.chart.connector_actions.drain(..) {
                match action {
                    mlc_app::ConnectorAction::ToggleEnabled { exchange_id } => {
                        let mut user = self.core.user_mut();
                        let enabled = !user.is_connector_enabled(&exchange_id);
                        user.set_connector_enabled_one(exchange_id.clone(), enabled);
                        info!("[App] connector toggle: {} -> enabled={}", exchange_id, enabled);
                    }
                }
            }
        }

        // ── Drain preset actions from all windows → shared core ─────────
        for pw in self.windows.values_mut() {
            for action in pw.chart.preset_actions.drain(..) {
                match action {
                    mlc_app::PresetAction::Upsert(preset) => {
                        let id = preset.id.clone();
                        self.core.presets_mut().insert(id.clone(), preset);
                        self.preset_dirty_ids.insert(id);
                    }
                    mlc_app::PresetAction::Delete { id } => {
                        self.core.presets_mut().remove(&id);
                        if let Err(e) = mlc_core::preset::storage::delete_preset(&id) {
                            warn!("[App] failed to delete preset file {}: {}", id, e);
                        }
                        self.preset_dirty_ids.remove(&id);
                    }
                    mlc_app::PresetAction::Rename { id, new_name } => {
                        if let Some(preset) = self.core.presets_mut().get_mut(&id) {
                            preset.name = new_name;
                            self.preset_dirty_ids.insert(id);
                        }
                    }
                }
            }
        }

        // ── Drain snapshot actions from all windows → shared core ───────
        for (_wid, pw) in self.windows.iter_mut() {
            for action in pw.chart.snapshot_actions.drain(..) {
                let mut snapshots = self.core.user().snapshots().clone();
                match action {
                    mlc_app::SnapshotAction::ChartSettings(data) => {
                        snapshots.chart_settings = Some(data);
                    }
                    mlc_app::SnapshotAction::PrimitiveSettings { type_id, data } => {
                        snapshots.primitive_settings.insert(type_id, data);
                    }
                    mlc_app::SnapshotAction::IndicatorSettings { type_id, data } => {
                        snapshots.indicator_settings.insert(type_id, data);
                    }
                    mlc_app::SnapshotAction::CompareSettings(data) => {
                        snapshots.compare_settings = Some(data);
                    }
                    mlc_app::SnapshotAction::DrawingStyle { type_id, data } => {
                        snapshots.last_used_drawing_styles.insert(type_id, data);
                    }
                }
                self.core.user_mut().set_snapshots(snapshots);
                self.snapshots_disk_dirty = true;
            }
        }

        // ── Drain template actions from all windows → shared core ───────
        // The apply lives on ChartApp; the shell's only share of it is the
        // disk flag.
        for (_wid, pw) in self.windows.iter_mut() {
            if pw.chart.drain_template_actions() {
                self.templates_disk_dirty = true;
            }
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
                        info!("[App] FPS limit → {}", v);
                    }
                    mlc_app::PerfAction::SetMsaa(v) => {
                        self.msaa_samples = v;
                        let mut ds = mlc_core::user_profile::DeviceSettings::load();
                        ds.msaa_samples = v;
                        ds.save();
                        info!("[App] MSAA → {}", v);
                    }
                    mlc_app::PerfAction::SetRecalcMode(ref mode) => {
                        let recalc_mode = match mode.as_str() {
                            "PerTick" => mlc_app::RecalcMode::PerTick,
                            "PerBar"  => mlc_app::RecalcMode::PerBar,
                            _         => mlc_app::RecalcMode::PerFrame,
                        };
                        self.core.user_mut().set_recalc_mode(recalc_mode);
                        let mut ds = mlc_core::user_profile::DeviceSettings::load();
                        ds.recalc_mode = mode.clone();
                        ds.save();
                        info!("[App] RecalcMode → {:?}", recalc_mode);
                    }
                    mlc_app::PerfAction::TogglePerfLog => {
                        self.perf_log_enabled = !self.perf_log_enabled;
                        info!("[App] Perf logging → {}", self.perf_log_enabled);
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
                        info!("[App] Backend → {:?}", self.render_backend);
                        // Persist to device_settings.json
                        {
                            let mut ds = mlc_core::user_profile::DeviceSettings::load();
                            ds.render_backend = Some(match new_backend {
                                RenderBackend::VelloGpu => mlc_core::user_profile::device_settings::RenderBackend::VelloGpu,
                                RenderBackend::InstancedWgpu => mlc_core::user_profile::device_settings::RenderBackend::InstancedWgpu,
                                RenderBackend::VelloCpu => mlc_core::user_profile::device_settings::RenderBackend::VelloCpu,
                                RenderBackend::VelloHybrid => mlc_core::user_profile::device_settings::RenderBackend::VelloHybrid,
                                RenderBackend::TinySkia => mlc_core::user_profile::device_settings::RenderBackend::TinySkia,
                                RenderBackend::Canvas2d => unreachable!("Canvas2d is wasm-only"),
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
        if !self.preset_dirty_ids.is_empty() {
            let ids: Vec<String> = self.preset_dirty_ids.drain().collect();
            for id in ids {
                if let Some(preset) = self.core.presets().get(&id) {
                    if let Err(e) = mlc_core::preset::storage::save_preset(preset) {
                        error!("[App] failed to save preset {}: {}", id, e);
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
                self.sync_window_geometry_from_os();

                // Collect window states from ALL windows.
                let window_states: Vec<mlc_core::WindowState> = self.windows.values()
                    .map(|pw| pw.chart.build_window_state())
                    .collect();

                // Build profile from the preferred (last-focused) window's UI state.
                let profile = self.build_profile_for_save(window_states);

                let vault_key = self.core.user().vault_key();
                if let Err(e) = mlc_core::save_profile(&profile, vault_key.as_ref()) {
                    error!("[App] Failed to save profile: {}", e);
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
                self.sync_window_geometry_from_os();

                let window_states: Vec<mlc_core::WindowState> = self.windows.values()
                    .map(|pw| pw.chart.build_window_state())
                    .collect();

                let profile = self.build_profile_for_save(window_states);

                let vault_key = self.core.user().vault_key();
                if let Err(e) = mlc_core::save_profile(&profile, vault_key.as_ref()) {
                    error!("[App] Failed to save profile (geometry): {}", e);
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
                // Bound first so the `watchlists()` read guard drops before the body
                // runs (clippy::significant_drop_in_scrutinee).
                let watchlists_snapshot = self.core.watchlists().snapshot();
                if let Err(e) = mlc_core::save_json(&watchlists_path, &watchlists_snapshot, None) {
                    error!("[App] Failed to save watchlists: {}", e);
                }
                // Clear dirty flags.
                for pw in self.windows.values_mut() {
                    pw.chart.watchlists_dirty = false;
                }
            }

            // ── Alerts sync-dirty drain ────────────────────────────────
            // `persist_alerts()` (mlc-app) already wrote alerts.json
            // synchronously — this only tells the updater to run a sync
            // cycle now rather than waiting for the next unrelated trigger
            // (ForceSync / sign-in / SetSyncEnabled). See
            // `ChartApp::alerts_sync_dirty`'s own doc.
            let any_alerts_sync_dirty = self.windows.values().any(|pw| pw.chart.alerts_sync_dirty);
            if any_alerts_sync_dirty {
                let _ = self.updater_handle.cmd_tx.send(
                    mlc_updater::UpdaterCommand::SyncPushChanged(vec!["alerts".to_string()]),
                );
                for pw in self.windows.values_mut() {
                    pw.chart.alerts_sync_dirty = false;
                }
            }

            // ── Flush dirty settings snapshots to disk ───────────────────
            // Same durability class as preset/watchlist dirty-drain above —
            // closes the gap where settings_snapshots.json only ever wrote
            // on exit/close/switch/wizard via save_all, so a crash between
            // an edit (e.g. changing a primitive's default style) and the
            // next full save silently lost it. Always plaintext, no vault
            // key needed, cheap enough to write every dirty frame.
            if self.snapshots_disk_dirty {
                let path = mlc_core::active_profile_data_dir().join("settings_snapshots.json");
                // Bound first so the `user()` read guard drops before the save call
                // runs (clippy::significant_drop_in_scrutinee).
                let save_result = mlc_core::save_json(&path, self.core.user().snapshots(), None);
                if let Err(e) = save_result {
                    error!("[App] Failed to save settings snapshots: {}", e);
                } else {
                    self.snapshots_disk_dirty = false;
                }
            }

            // ── Flush dirty templates to disk (debounced) ────────────────
            // Templates are heavier than snapshots (vault-key encryption +
            // multiple files under `templates/`), so unlike snapshots this
            // drain debounces: it only writes once at least
            // `TEMPLATES_WRITE_DEBOUNCE_MS` has elapsed since the previous
            // write, so a burst of edits in the same second doesn't
            // re-encrypt the whole set on every frame. The flag stays set
            // (deferred, not dropped) until the debounce window passes.
            if self.templates_disk_dirty {
                let now = web_time::Instant::now();
                let debounce_elapsed = self.templates_last_write_at
                    .map(|t| now.duration_since(t).as_millis() >= TEMPLATES_WRITE_DEBOUNCE_MS)
                    .unwrap_or(true);
                if debounce_elapsed {
                    let vault_key = self.core.user().vault_key();
                    // Bound first so the `templates()` guard drops before the body
                    // runs (clippy::significant_drop_in_scrutinee).
                    let templates_save_result = self.core.templates().save_to_default_dir(vault_key.as_ref());
                    if let Err(e) = templates_save_result {
                        error!("[App] Failed to save templates: {:?}", e);
                    } else {
                        self.templates_disk_dirty = false;
                        self.templates_last_write_at = Some(now);
                    }
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
                            self.core.presets_mut().insert(id.clone(), preset);
                            self.preset_dirty_ids.insert(id);
                            second_pass_dirty = true;
                        }
                        mlc_app::PresetAction::Delete { id } => {
                            self.core.presets_mut().remove(&id);
                            if let Err(e) = mlc_core::preset::storage::delete_preset(&id) {
                                warn!("[App] failed to delete preset file {}: {}", id, e);
                            }
                            self.preset_dirty_ids.remove(&id);
                        }
                        mlc_app::PresetAction::Rename { id, new_name } => {
                            if let Some(preset) = self.core.presets_mut().get_mut(&id) {
                                preset.name = new_name;
                                self.preset_dirty_ids.insert(id);
                                second_pass_dirty = true;
                            }
                        }
                    }
                }
            }
            if second_pass_dirty && !self.preset_dirty_ids.is_empty() {
                let ids: Vec<String> = self.preset_dirty_ids.drain().collect();
                for id in ids {
                    if let Some(preset) = self.core.presets().get(&id) {
                        if let Err(e) = mlc_core::preset::storage::save_preset(preset) {
                            error!("[App] failed to save preset {}: {}", id, e);
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
        // Restart into the update that is already installed on disk. The only
        // path in this application that replaces the running process, and it
        // exists only because someone pressed a control that said "Restart" —
        // the updater installs quietly and then waits (see `ChromeUpdate`).
        if self.windows.values().any(|pw| pw.restart_requested) {
            for pw in self.windows.values_mut() {
                pw.restart_requested = false;
            }
            info!("[App] restart requested — saving, then handing over to the new binary");
            self.save_all(&[]);
            match mlc_updater::replace::spawn_and_exit(
                self.windows
                    .values()
                    .next()
                    .map(|pw| pw.chart.core.user().server_port()),
            ) {
                Ok(()) => return,
                Err(e) => error!("[App] restart failed, staying on the running build: {e}"),
            }
        }

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
                    if let Some(pw) = self.windows.remove(&wid) {
                        // Release this window's AppSession + any presets it
                        // still had claimed (§4.1/§5.3.5) — tied to the same
                        // lifecycle event so a closed window never leaves a
                        // stale claim behind (risk 5 in the plan: "do not
                        // invent a second cleanup mechanism").
                        self.core.deregister_session(&pw.app_id);
                        self.core.release_all_presets_for_app(&pw.app_id);
                    }
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

        // ── Drain recalc_mode changes → shared core ──────────────────────
        // If any window changed the recalc mode from the User Settings modal,
        // write it once to core.user() (Phase C) — every window's next read
        // sees it immediately, no per-window propagate loop needed.
        {
            let new_mode: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.recalc_mode_changed.take());
            if let Some(ref mode_str) = new_mode {
                let recalc_mode = match mode_str.as_str() {
                    "PerTick" => mlc_app::RecalcMode::PerTick,
                    "PerBar"  => mlc_app::RecalcMode::PerBar,
                    _         => mlc_app::RecalcMode::PerFrame,
                };
                self.core.user_mut().set_recalc_mode(recalc_mode);
                self.core.indicators_mut().set_recalc_mode(recalc_mode);
                // Persist to DeviceSettings so the choice survives restarts.
                let mut ds = mlc_core::user_profile::DeviceSettings::load();
                ds.recalc_mode = mode_str.clone();
                ds.save();
                info!("[App] recalc_mode changed to: {}", mode_str);
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
                self.profile.ui.language = lang_code.clone();
                self.profile_manager.profile.ui.language = lang_code.clone();
                info!("[App] language changed to: {}", lang_code);
            }
        }

        // ── Drain server_enabled changes → shared core ────────────────────
        {
            let server_change: Option<bool> = self.windows.values_mut()
                .find_map(|pw| pw.chart.server_enabled_changed.take());
            if let Some(enabled) = server_change {
                self.core.user_mut().set_server_enabled(enabled);
                // Sync to all windows
                let status = if enabled { "running" } else { "stopped" };
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.user_settings_state.server_enabled = enabled;
                    pw.chart.panel_app.user_settings_state.server_status = status.to_string();
                }
                info!("[App] server_enabled changed to: {}", enabled);
                // Actual listener start/stop happens later this same frame,
                // off `account_signed_in && server_enabled()`
                // (agent-api-auth-and-secret-store plan, piece 4) — see the
                // `want`/`listener_handle` edge below, which also corrects
                // `server_status` to the real bound/not-bound fact.
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
            if let Some((label, _tier)) = create_req {
                if let Some(ref agent_state) = self.agent_state {
                    use mlc_backend_shell_http::auth::{AgentKeyRole, Tier};
                    use mlc_backend_shell_http::state::AgentCommand;
                    // No tier/role picker is wired into the UI yet (piece 4
                    // owns `render_server_tab`'s key-manager rebuild) —
                    // every route is gated at `Tier::SignedIn` today (plan
                    // §2), and a key minted with no role picker defaults to
                    // the more capable `Control` rather than silently
                    // handing out a read-only key nobody asked for.
                    agent_state.push_command(AgentCommand::CreateKey {
                        label,
                        tier: Tier::SignedIn,
                        role: AgentKeyRole::Control,
                    });
                }
            }
        }

        // ── Drain key delete requests from UI ────────────────────────────
        {
            let delete_req: Option<String> = self.windows.values_mut()
                .find_map(|pw| pw.chart.key_delete_request.take());
            if let Some(fingerprint_hex) = delete_req {
                if let Some(ref agent_state) = self.agent_state {
                    use mlc_backend_shell_http::state::AgentCommand;
                    // `key_delete_request` still carries the pre-auth
                    // "label to delete" shape — no key-manager UI stages it
                    // yet (see the field's own doc in `mlc-app`). Piece 4's
                    // list-backed UI rebuild is what starts staging a real
                    // fingerprint here.
                    agent_state.push_command(AgentCommand::DeleteKey { fingerprint_hex });
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
                        warn!("[App] clipboard copy failed: {}", e);
                    }
                } else {
                    warn!("[App] clipboard unavailable");
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
                    use std::os::windows::process::CommandExt;
                    // CREATE_NO_WINDOW — this shell runs windowless in
                    // release; without the flag `cmd /c start` would flash
                    // its own console box.
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    if let Err(e) = std::process::Command::new("cmd")
                        .args(["/c", "start", "", url])
                        .creation_flags(CREATE_NO_WINDOW)
                        .spawn()
                    {
                        warn!("[App] failed to open URL {}: {}", url, e);
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
                        warn!("[App] failed to open URL {}: {}", url, e);
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
                        warn!("[App] failed to open URL {}: {}", url, e);
                    }
                }
            }
        }

        // ── Drain a picked OAuth provider ────────────────────────────────
        // The modal records WHICH provider the user chose and stops there —
        // what to do with it is the shell's, and the two shells differ. Here
        // it runs in-process: `start_oauth_flow` opens the browser, holds a
        // localhost callback listener, exchanges the code and stores the
        // token, then `auth_rx` broadcasts `LoggedIn` and the UI follows on
        // its own. No device code, no polling.
        #[cfg(feature = "cloud")]
        {
            let picked: Option<String> = self
                .windows
                .values_mut()
                .find_map(|pw| pw.chart.panel_app.cloud_login_state.oauth_login_requested.take());
            if let Some(provider) = picked {
                info!("[Updater] sign-in: starting loopback OAuth with '{provider}'");
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::StartOAuth(provider));
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.cloud_login_state.is_open = false;
                    pw.chart_dirty = true;
                    pw.overlay_dirty = true;
                    pw.window.request_redraw();
                }
            }
        }

        // ── Drain the account overlay's Sign out ─────────────────────
        // The overlay sets a one-shot and stops there, like every other action
        // it owns. Nothing on this shell read it, so the button was inert and
        // an account could not be left at all — which mattered most for the
        // account nobody could use, a token the server had stopped accepting
        // (owner, 2026-09-04).
        #[cfg(feature = "cloud")]
        {
            let asked = self
                .windows
                .values_mut()
                .any(|pw| std::mem::take(&mut pw.chart.panel_app.cloud_login_state.sign_out_requested));
            if asked {
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::Logout);
                use mlc_app_layout::ui::modal_settings::{CloudLoginPhase, RedeemStatus};
                use mlc_core::state::toolbar_state::ScreenshotDestination;
                for pw in self.windows.values_mut() {
                    let st = &mut pw.chart.panel_app.cloud_login_state;
                    st.phase = CloudLoginPhase::ChooseProvider;
                    st.is_paid = false;
                    st.paid_until = None;
                    // `redeem_status`/`redeem_requested` live on
                    // `UserSettingsState` now (Account tab, not the login
                    // overlay) — the identity that submitted a code just
                    // left, so its outcome is not this session's news either.
                    pw.chart.panel_app.user_settings_state.redeem_requested = false;
                    pw.chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Idle;
                    // Same-frame gating (owner ruling 2026-09-06): a
                    // ShareLink pick made while signed in must not survive
                    // sign-out and keep firing uploads afterward.
                    if pw.chart.panel_app.toolbar_state.screenshot_destination == ScreenshotDestination::ShareLink {
                        pw.chart.panel_app.toolbar_state.screenshot_destination = ScreenshotDestination::Clipboard;
                    }
                    pw.chart_dirty = true;
                    pw.overlay_dirty = true;
                    pw.window.request_redraw();
                }
                // Routed through the system-event bus (system-event-bus arc,
                // 2026-09-05) — `toast=true` makes `ChartApp::
                // refresh_active_toasts` (driven every frame from
                // `render_modal_layer`) pick it up and show it the same
                // frame; no shell-owned toast Vec involved.
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.push_system_event(
                        mlc_core::SystemEventKind::Account,
                        "Signed out",
                        "You have been logged out.",
                        true,
                    );
                }
                info!("[App] sign out requested from the account overlay");
            }
        }

        // ── Drain a submitted gift/trial code (Settings → Account, NOT the
        // login overlay) ──────────────────────────────────────────────────
        // Owner ruling 2026-09-23: moved off `CloudLoginState` onto
        // `UserSettingsState`, same "compiles in every build" reasoning the
        // account-nickname block below already documents for its own field
        // — deliberately NOT `#[cfg(feature = "cloud")]` and NOT gated on
        // `cloud_login_state.is_open` (the Account tab is a different modal
        // entirely). The tab records that a code was submitted and stops
        // there; the text itself lives in the shared text-field store, and
        // the round trip belongs to the updater, which already holds the
        // token and the client.
        {
            let submitted = self
                .windows
                .values_mut()
                .any(|pw| std::mem::take(&mut pw.chart.panel_app.user_settings_state.redeem_requested));
            if submitted {
                let code = self
                    .windows
                    .values()
                    .find_map(|pw| {
                        let coord = pw.chart.input_coordinator.borrow();
                        let text = coord
                            .text_fields()
                            .text(&uzor::WidgetId::from(mlc_app::text_input::REDEEM_CODE))
                            .trim()
                            .to_string();
                        (!text.is_empty()).then_some(text)
                    })
                    .unwrap_or_default();
                if code.is_empty() {
                    use mlc_app_layout::ui::modal_settings::RedeemStatus;
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.user_settings_state.redeem_status =
                            RedeemStatus::Failed { message: "Enter a code first".to_string() };
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    }
                } else {
                    let _ = self
                        .updater_handle
                        .cmd_tx
                        .send(mlc_updater::UpdaterCommand::RedeemCode(code));
                }
            }
        }

        // ── Drain a submitted account-nickname edit ──────────────────────
        // Unlike the two blocks above, NOT `#[cfg(feature = "cloud")]` —
        // this row lives in the User Settings modal
        // (`UserSettingsState::nick_edit_mode`), which compiles in every
        // build, not the login overlay. Local validation already ran in
        // `ChartApp` before `nick_save_requested` was armed
        // (`account_nick_confirm`/`text_action_account_nick`), so an empty
        // submission here would mean the field was blanked after that —
        // treated the same way the redeem block treats an empty code.
        {
            let submitted = self
                .windows
                .values_mut()
                .any(|pw| std::mem::take(&mut pw.chart.panel_app.user_settings_state.nick_save_requested));
            if submitted {
                let nick = self
                    .windows
                    .values()
                    .find_map(|pw| {
                        let coord = pw.chart.input_coordinator.borrow();
                        let text = coord
                            .text_fields()
                            .text(&uzor::WidgetId::from(mlc_app::text_input::ACCOUNT_NICK))
                            .trim()
                            .to_string();
                        (!text.is_empty()).then_some(text)
                    })
                    .unwrap_or_default();
                if nick.is_empty() {
                    use mlc_app_layout::ui::modal_settings::NickStatus;
                    for pw in self.windows.values_mut() {
                        pw.chart.panel_app.user_settings_state.nick_status =
                            NickStatus::Failed { message: "Enter a nickname first".to_string() };
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    }
                    // Same status is applied to every window above (shared
                    // account-level fact) — the event itself is logged once,
                    // not toasted (the settings row already shows it inline).
                    if let Some(pw) = self.system_event_window_mut() {
                        pw.chart.push_system_event(
                            mlc_core::SystemEventKind::Nick,
                            "Nickname not saved",
                            "Enter a nickname first.",
                            false,
                        );
                    }
                } else {
                    let _ = self
                        .updater_handle
                        .cmd_tx
                        .send(mlc_updater::UpdaterCommand::SetNick(nick));
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

                // Flipped `true` by every recognized arm below (including the
                // two auth arms just added). A command nobody recognizes
                // falls through to the final `else` and gets logged instead
                // of vanishing silently, which is what happened to `logout`/
                // `start_device_auth` before this arc.
                let mut cmd_handled = false;

                // ── Auth commands ──────────────────────────────────────────
                // Emitted by `mlc-app/src/input/panel_click.rs` (profile_mgr
                // sign_in / logout buttons). Both routes through the OTA/auth
                // background task started in `App::new`.
                if cmd_str == "logout" {
                    cmd_handled = true;
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Logout);
                    // `updater_handle.auth_rx` flips to `NotLoggedIn` once the
                    // background task processes the command. There is no
                    // ChartApp/BackendCore auth-status field to write here
                    // (shell contract forbids writing those directly anyway)
                    // — routed through the system-event bus (system-event-
                    // bus arc, 2026-09-05); `toast=true` below is what
                    // makes it show (see the sibling call site's own doc).
                    if let Some(pw) = self.system_event_window_mut() {
                        pw.chart.push_system_event(
                            mlc_core::SystemEventKind::Account,
                            "Signed out",
                            "You have been logged out.",
                            true,
                        );
                    }
                    info!("[App] logout requested — forwarded to updater");
                } else if cmd_str == "start_device_auth" {
                    cmd_handled = true;
                    // Native does not use the device-code flow at all. It has
                    // a loopback OAuth flow already — `mlc_updater`'s
                    // `oauth::start_oauth_flow` opens the browser, listens on
                    // a localhost callback port and exchanges the code for a
                    // token — which is the other standard answer for a
                    // desktop client and the one that fits an app that CAN
                    // receive a redirect. Device codes exist for clients that
                    // cannot; showing one here would be ceremony.
                    //
                    // This used to refuse outright, on the grounds that the
                    // server enforced build attestation and this shell's
                    // build.rs signs none. Measured against production
                    // 2026-09-02: `POST /api/auth/device-code` with no
                    // attestation header answers 200. `BUILD_ATTEST_ENFORCE`
                    // defaults to false and is not set on the box, so an
                    // unsigned client is allowed through the grace path. The
                    // refusal outlived its reason.
                    #[cfg(feature = "cloud")]
                    {
                        use mlc_app_layout::ui::modal_settings::CloudLoginPhase;
                        for pw in self.windows.values_mut() {
                            let st = &mut pw.chart.panel_app.cloud_login_state;
                            st.is_open = true;
                            st.phase = CloudLoginPhase::ChooseProvider;
                            pw.chart_dirty = true;
                            pw.overlay_dirty = true;
                            pw.window.request_redraw();
                        }
                    }
                    info!("[Updater] sign-in: offering OAuth providers (loopback flow)");
                }

                // ── Profile commands: shared dispatcher (mlc-app) ────────────
                // `ChartApp::dispatch_profile_command` owns the five commands'
                // `ProfileManager` mutation only — every side effect below
                // (AppState/window fanout, toast, pending_new_profile_id,
                // deferred window rebuild) is unchanged from before this arc
                // and stays here because it's shell-specific (multi-window on
                // native; nothing this crate can express identically on web).
                use mlc_app::user_manager::{ProfileCommandOutcome, ProfileEffect};
                match mlc_app::ChartApp::dispatch_profile_command(cmd_str, &mut self.profile_manager) {
                    ProfileCommandOutcome::Handled(ProfileEffect::Renamed { id, new_name, was_active }) => {
                        cmd_handled = true;
                        if was_active {
                            self.profile.ui.display_name = new_name.clone();
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.profile_display_name =
                                    new_name.clone();
                            }
                        }
                        self.sync_profiles_to_windows();
                        info!("[App] profile renamed: id={} name={}", id, new_name);
                    }
                    ProfileCommandOutcome::Handled(ProfileEffect::AvatarSet { avatar, .. }) => {
                        cmd_handled = true;
                        self.profile.ui.avatar = avatar.clone();
                        self.sync_profiles_to_windows();
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.profile_avatar = avatar.clone();
                        }
                        info!("[App] profile avatar set to: {}", avatar);
                    }
                    ProfileCommandOutcome::Handled(ProfileEffect::Created { id, display_name }) => {
                        cmd_handled = true;
                        info!("[App] profile created: {} ({})", display_name, id);
                        self.sync_profiles_to_windows();
                        // Show passphrase setup in the current window — no profile switch yet.
                        // The switch happens only after the user successfully creates the vault.
                        use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                        self.pending_new_profile_id = Some(id.clone());
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.show_profile_manager = true;
                            pw.chart.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                            pw.chart.panel_app.user_settings_state.profile_manager_target_id = id.clone();
                            pw.chart.panel_app.user_settings_state.profile_manager_target_name = display_name.clone();
                            {
                                let np_wid = uzor::WidgetId::from(mlc_app::text_input::NEW_PASSPHRASE);
                                pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                            }
                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                        }
                    }
                    ProfileCommandOutcome::Handled(ProfileEffect::SwitchAlreadyActive) => {
                        cmd_handled = true;
                        warn!("[App] profile_switch: already on this profile, ignoring");
                    }
                    ProfileCommandOutcome::Handled(ProfileEffect::SwitchRequested { id }) => {
                        cmd_handled = true;
                        info!("[App] profile_switch: scheduling hot-reload for profile {}", id);
                        self.pending_profile_switch = Some(id);
                    }
                    ProfileCommandOutcome::Handled(ProfileEffect::Deleted { id }) => {
                        cmd_handled = true;
                        self.sync_profiles_to_windows();
                        info!("[App] profile deleted: {}", id);
                    }
                    ProfileCommandOutcome::Failed(e) => {
                        cmd_handled = true;
                        warn!("[App] profile command failed: {} ({})", e, cmd_str);
                    }
                    ProfileCommandOutcome::NotForMe => {
                        // Not a profile-family command — fall through to the
                        // remaining arms below (wizard/e2e/vault/auth).
                    }
                }

                // ── Wizard complete ──
                // Format: bare "wizard_complete" — no name attached. Owner's
                // identity ruling 2026-09-02: a profile's identity is its
                // UUID; an anonymous profile carries no user-facing name at
                // all, so the wizard's Profile Name page (and this command's
                // former "wizard_complete:{name}" payload / rename step) is
                // gone. The wizard ends the moment the language page's
                // forward button is pressed — no passphrase is collected
                // here either. The vault opens later, at the point of use,
                // via the profile manager's own (unrelated) passphrase/
                // recovery-key flow.
                if cmd_str == "wizard_complete" {
                    cmd_handled = true;
                    if self.is_first_run {
                        // A first run has ALREADY had its profile created, by
                        // the bootstrap in `main()`. Creating another one here
                        // would hand the user two profiles for one wizard, and
                        // there is no skeleton left to promote — the chart has
                        // been live behind this modal from the first frame.
                        // Finishing the wizard is now just: keep the theme and
                        // language it collected, and stop being a first run.
                        self.save_all(&[]);
                        self.is_first_run = false;
                        self.needs_vault_unlock = false;
                        info!("[App] wizard_complete — first run finished over a live chart");
                    } else if !mlc_core::active_profile_data_dir().join("profile.json").exists() {
                        // Reached only when there is genuinely no profile on
                        // disk. Creating one over an existing profile is
                        // forbidden outright — see the refusal below for what
                        // it cost.
                        //
                        // `Some("")`, not `None` — an explicitly nameless
                        // profile. `None` would fall through to
                        // `auto_generate_name()` and silently hand it a "New
                        // Profile" label nobody asked for; that label belongs
                        // to the paid tier's explicit "add another profile".
                        match self.profile_manager.create_profile(Some(""), "chart") {
                            Ok(meta) => {
                                info!(
                                    "[App] wizard_complete: created profile directory ({})",
                                    meta.id
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
                                // Clear stale presets/templates on the shared core
                                // (Phase A/B: one instance for the whole process,
                                // not a per-window AppState mirror).
                                self.core.presets_mut().replace_all(std::collections::HashMap::new());
                                self.core.templates_mut().replace_all(self.profile_manager.template_manager.clone());
                            }
                            Err(e) => {
                                error!("[App] wizard_complete: failed to create profile: {}", e);
                            }
                        }

                        // No vault key to derive — the profile stays plaintext
                        // until the user opens the vault themselves later.
                        // Rebuild the windows onto the new profile.
                        self.save_all(&[]);
                        self.needs_vault_unlock = false;
                        info!("[App] wizard_complete — rebuilding windows for the added profile");
                        self.pending_skeleton_promote = true;
                    } else {
                        // A profile already exists on disk. The wizard may not
                        // create another one over it — ever, however it got
                        // raised.
                        //
                        // Without this guard the sequence on 2026-09-03 was:
                        // a legacy profile has `welcome_wizard_seen: false`
                        // because the field was added after it was written, so
                        // the wizard raises over a live session; finishing it
                        // is not `is_first_run`, so it fell into the branch
                        // above, made a fresh empty profile, rewrote
                        // `active_profile_id` to it and switched. The owner's
                        // profile — 55 presets, twelve open tabs — stopped
                        // being the active one, the chart came up "Untitled"
                        // with nothing in it, and the window promote that
                        // followed deadlocked with all 45 threads waiting.
                        //
                        // The gate on the raise (see `show_wizard`) already
                        // stops that chain at the start. This is the second
                        // line: a bug in the wizard must not be able to cost
                        // someone their profile. Adding a profile deliberately
                        // is a different action and belongs to its own command,
                        // not to "the wizard finished".
                        self.save_all(&[]);
                        self.needs_vault_unlock = false;
                        warn!(
                            "[App] wizard_complete: REFUSED to create a profile — one already exists at {}. Keeping the active profile and the theme/language the wizard collected.",
                            mlc_core::active_profile_data_dir().display()
                        );
                    }
                }

                // ── Local vault key derivation (e2e_setup) ──
                if let Some(passphrase) = cmd_str.strip_prefix("e2e_setup:") {
                    cmd_handled = true;
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
                                        info!("[App] new profile vault created, switching to {}", new_profile_id);

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
                                                    let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY_DISPLAY);
                                                    pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, key_str);
                                                    if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                                        // Cursor at the START, not the end: the reveal box
                                                        // is now a real (scrollable) input, and a cursor
                                                        // seeded at the far end would scroll the box to
                                                        // show the TAIL of the key by default — exactly the
                                                        // "first three groups off-screen" defect the
                                                        // two-line paint this replaced was built to avoid.
                                                        fs.cursor = 0;
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
                                                // `close_profile_manager` (not a bare flag write): the
                                                // `clear()` two lines up empties the passphrase TEXT
                                                // but does not blur it — the field stayed genuinely
                                                // focused in TextFieldStore, silently killing every
                                                // hotkey afterward (owner-review 2026-09-23 audit).
                                                pw.chart.close_profile_manager();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("[App] new profile: failed to create vault: {}", e);
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
                    let current_id = self.profile_manager.profile.domain.profile_id.clone();
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
                                            info!("[App] pre-switch: passphrase validated for profile {}", target_id);
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
                                                // `close_profile_manager` blurs the (already
                                                // text-cleared) passphrase field — see the sibling
                                                // branch below for why a bare flag write here used
                                                // to leave it focused.
                                                pw.chart.close_profile_manager();
                                                // `.close()` (not a raw `is_open = false`) so the
                                                // one-time key reveal box is dropped along with
                                                // every other close of this modal.
                                                pw.chart.panel_app.user_settings_state.close();
                                            }
                                        }
                                        Err(e) => {
                                            warn!("[App] pre-switch: wrong passphrase for profile {} ({})", target_id, e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some("Wrong passphrase — please try again".to_string());
                                                pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                            }
                                        }
                                    }
                                } else {
                                    // Target profile has no vault — create vault with this passphrase
                                    info!("[App] pre-switch: creating vault for unencrypted profile {}", target_id);
                                    if let Ok(salt) = mlc_vault::vault::load_or_create_salt(&salt_path) {
                                        let key = mlc_vault::vault::derive_key(passphrase, &salt);
                                        let empty_secrets = mlc_core::user_profile::VaultSecrets::default();
                                        if let Ok(()) = mlc_vault::vault::save_encrypted(&key, &vault_path, &empty_secrets) {
                                            info!("[App] pre-switch: vault created, switching to profile {}", target_id);
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
                                                // `close_profile_manager` blurs the (already
                                                // text-cleared) passphrase field — see the sibling
                                                // branch above for why a bare flag write here used
                                                // to leave it focused, and for why `.close()`.
                                                pw.chart.close_profile_manager();
                                                pw.chart.panel_app.user_settings_state.close();
                                            }
                                        } else {
                                            error!("[App] pre-switch: failed to create vault.enc");
                                        }
                                    } else {
                                        error!("[App] pre-switch: failed to create salt");
                                    }
                                }
                            }
                        }
                    } else {
                        // Validate an existing vault (locked at boot) or derive
                        // a fresh one (first-time setup) for the ACTIVE
                        // profile — shared with the web shell
                        // (`ChartApp::apply_vault_setup`,
                        // `mlc-app/src/input/credentials.rs`), which is also
                        // what the browser's own "Set Up Encryption" defect
                        // this method was written to fix now calls. Which of
                        // the two branches runs is decided INSIDE that method
                        // by the window's own `needs_vault_unlock` mirror,
                        // which this shell already keeps in lockstep with
                        // `self.needs_vault_unlock` at every mutation site.
                        let outcome = self.windows.values_mut().next().map(|pw| {
                            pw.chart.apply_vault_setup(passphrase, &mut self.profile_manager)
                        });
                        match outcome {
                            Some(mlc_app::VaultSetupOutcome::Ready { recovery_key: Some(rk) }) => {
                                // First-time vault creation — show the recovery key before promoting.
                                info!("[App] vault key derived and set — showing recovery key");
                                self.needs_vault_unlock = false;
                                #[cfg(feature = "cloud")]
                                self.refresh_dm_identity();
                                for pw in self.windows.values_mut() {
                                    use mlc_app_layout::ui::modal_settings::{
                                        CredentialsView, ProfileManagerPage, UserSettingsTab,
                                    };
                                    pw.chart.panel_app.user_settings_state.needs_vault_unlock = false;
                                    pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                    pw.chart.panel_app.user_settings_state.recovery_key_display =
                                        Some(rk.clone());
                                    // Sync TextFieldStore recovery key display so it can be selected/copied.
                                    {
                                        let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY_DISPLAY);
                                        pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, &rk);
                                        if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                            // Cursor at the START — see the sibling seed site above
                                            // for why (real scrollable input now, not a hand-painted
                                            // two-line box).
                                            fs.cursor = 0;
                                            fs.selection_start = None;
                                        }
                                    }
                                    // Which page-selector enum advances is
                                    // decided by which host is already up —
                                    // the standalone modal (native boot) or
                                    // the Vault tab (mid-session "Set up").
                                    // Forcing the modal open unconditionally
                                    // here, as before, is exactly the
                                    // modal-on-modal bug this arc closes.
                                    if pw.chart.panel_app.user_settings_state.show_profile_manager {
                                        pw.chart.panel_app.user_settings_state.profile_manager_page =
                                            ProfileManagerPage::ShowRecoveryKey;
                                    } else {
                                        pw.chart.panel_app.credentials_state.view =
                                            CredentialsView::ShowRecoveryKey;
                                        pw.chart.panel_app.user_settings_state.active_tab =
                                            UserSettingsTab::Vault;
                                        pw.chart.panel_app.user_settings_state.is_open = true;
                                    }
                                }
                            }
                            Some(mlc_app::VaultSetupOutcome::Ready { recovery_key: None }) => {
                                // Existing vault, correctly validated (no new
                                // recovery key to show) — promote immediately.
                                info!("[App] vault passphrase validated OK — promoting skeleton");
                                self.profile = self.profile_manager.profile.clone();
                                self.needs_vault_unlock = false;
                                #[cfg(feature = "cloud")]
                                self.refresh_dm_identity();
                                // Drop skeleton windows and recreate with live data.
                                self.pending_skeleton_promote = true;
                            }
                            Some(mlc_app::VaultSetupOutcome::WrongPassphrase(e)) => {
                                warn!("[App] vault unlock REJECTED: wrong passphrase ({})", e);
                                for pw in self.windows.values_mut() {
                                    pw.chart.panel_app.user_settings_state.needs_vault_unlock = true;
                                    pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                        Some("Wrong passphrase — please try again".to_string());
                                    pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                }
                            }
                            Some(mlc_app::VaultSetupOutcome::Error(e)) => {
                                warn!("[App] vault salt error: {}", e);
                            }
                            None => {
                                warn!("[App] e2e_setup: no window to apply vault setup to");
                            }
                        }
                    }
                    } // end else (not a new-profile vault creation)
                }

                // ── Vault tab mid-session re-unlock ───────────────────────────────────────────
                // Emitted by `"profile_mgr:unlock"` instead of `e2e_setup:` when the passphrase
                // page was reached via the Vault tab's "Unlock" button (see that arm's comment
                // for why `e2e_setup:`'s own branches are all wrong for this case). Deliberately
                // does NOT touch `pending_skeleton_promote` / rebuild any window — this profile
                // is already fully live, only its secret state was sealed.
                if let Some(passphrase) = cmd_str.strip_prefix("cred_unlock:") {
                    cmd_handled = true;
                    let mut last_err: Option<String> = None;
                    for pw in self.windows.values_mut() {
                        if let Err(e) = pw.chart.try_unlock_credentials(passphrase) {
                            last_err = Some(e);
                        }
                    }
                    if let Some(e) = last_err {
                        for pw in self.windows.values_mut() {
                            pw.chart.panel_app.user_settings_state.vault_unlock_error = Some(e.clone());
                            pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                        }
                        warn!("[App] credentials: unlock failed: {}", e);
                    } else {
                        info!("[App] credentials: vault unlocked mid-session");
                        #[cfg(feature = "cloud")]
                        self.refresh_dm_identity();
                    }
                }

                // ── Recovery key unlock ──────────────────────────────────────────────────────
                // Emitted when the user enters a recovery key to restore vault access.
                if let Some(recovery_key_text) = cmd_str.strip_prefix("recovery_unlock:") {
                    cmd_handled = true;
                    let target_id = self.windows.values().next()
                        .map(|pw| pw.chart.panel_app.user_settings_state.profile_manager_target_id.clone())
                        .unwrap_or_default();
                    let current_id = self.profile_manager.profile.domain.profile_id.clone();

                    // Which profile's vault this recovery key is being spent
                    // on. Computed ONCE and reused by the navigation decision
                    // below, because the two must not be able to disagree:
                    // whichever directory we decrypt is the profile whose
                    // follow-up page the user has to land on.
                    //
                    // The navigation used to test `self.needs_vault_unlock`
                    // instead — a boot-time flag. Sealing the vault mid-session
                    // with the Lock button does not set it, so a recovery
                    // unlock from the Vault tab took the "different profile"
                    // branch with `target_id == current_id`, scheduled a window
                    // rebuild onto the profile already loaded, and never showed
                    // the mandatory new-passphrase page. The vault was open and
                    // the passphrase that could no longer be remembered was
                    // still the only one that worked.
                    let unlocking_other_profile =
                        !target_id.is_empty() && target_id != current_id && !self.needs_vault_unlock;

                    // Determine which profile dir to use
                    let profile_dir = if unlocking_other_profile {
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
                                    warn!("[App] recovery_unlock: no recovery_key.enc found in {:?}", profile_dir);
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
                                                    info!("[App] recovery_unlock: master key recovered — vault key derived");

                                                    if !unlocking_other_profile {
                                                        // Active profile unlock via recovery — vault is
                                                        // now readable but the user must set a new
                                                        // passphrase before we promote the skeleton.
                                                        self.profile_manager.set_vault_key(vault_key);
                                                        if let Err(e) = self.profile_manager.load_vault_secrets() {
                                                            warn!("[App] failed to load vault secrets: {}", e);
                                                        }
                                                        self.core.user_mut().set_vault_key(Some(vault_key));
                                                        self.core.templates_mut().set_vault_key(Some(vault_key));
                                                        if let Some(ref agent_state) = self.agent_state {
                                                            if let Err(e) = agent_state.agent_key_store().unlock_with_key(&vault_key) {
                                                                warn!("[App] agent key store: failed to unlock after recovery-key unlock: {}", e);
                                                            }
                                                        }
                                                        self.profile_manager.vault_key = Some(vault_key);
                                                        self.profile = self.profile_manager.profile.clone();
                                                        self.needs_vault_unlock = false;
                                                        #[cfg(feature = "cloud")]
                                                        self.refresh_dm_identity();

                                                        // Store master_key so set_new_passphrase: can re-key the vault.
                                                        self.pending_recovery_master_key = Some(master_key);

                                                        // Navigate to the mandatory SetNewPassphrase page —
                                                        // in whichever host is already up (see the
                                                        // `e2e_setup:` handler's own comment on this same
                                                        // `show_profile_manager` test above).
                                                        use mlc_app_layout::ui::modal_settings::{
                                                            CredentialsView, ProfileManagerPage, UserSettingsTab,
                                                        };
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
                                                            if pw.chart.panel_app.user_settings_state.show_profile_manager {
                                                                pw.chart.panel_app.user_settings_state.profile_manager_page =
                                                                    ProfileManagerPage::SetNewPassphrase;
                                                            } else {
                                                                pw.chart.panel_app.credentials_state.view =
                                                                    CredentialsView::SetNewPassphrase;
                                                                pw.chart.panel_app.user_settings_state.active_tab =
                                                                    UserSettingsTab::Vault;
                                                                pw.chart.panel_app.user_settings_state.is_open = true;
                                                            }
                                                        }
                                                        info!("[App] recovery_unlock: navigating to SetNewPassphrase");
                                                    } else {
                                                        // Pre-switch recovery for a different profile
                                                        self.pending_switch_vault_key = Some(vault_key);
                                                        self.pending_profile_switch = Some(target_id.clone());
                                                        for pw in self.windows.values_mut() {
                                                            pw.chart.panel_app.user_settings_state.vault_unlock_error = None;
                                                            {
                                                                let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY);
                                                                pw.chart.input_coordinator.borrow_mut().text_fields_mut().clear(&rk_wid);
                                                            }
                                                            pw.chart.panel_app.user_settings_state.recovery_key_display_focused = false;
                                                            // `close_profile_manager` blurs RECOVERY_KEY
                                                            // (already text-cleared above) — the explicit
                                                            // `.blur()` this used to do by hand is now the
                                                            // shared helper every close path routes through.
                                                            pw.chart.close_profile_manager();
                                                            // `.close()` — see the pre-switch passphrase
                                                            // branches above for why.
                                                            pw.chart.panel_app.user_settings_state.close();
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("[App] recovery_unlock: decryption failed: {}", e);
                                                    for pw in self.windows.values_mut() {
                                                        pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                            Some("Wrong recovery key — decryption failed".to_string());
                                                        pw.chart.panel_app.user_settings_state.vault_unlock_attempts += 1;
                                                    }
                                                }
                                            }
                                        }
                                        (Err(e), _) => {
                                            warn!("[App] recovery_unlock: failed to read salt: {}", e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some(format!("Failed to read vault data: {}", e));
                                            }
                                        }
                                        (_, Err(e)) => {
                                            warn!("[App] recovery_unlock: failed to read recovery_key.enc: {}", e);
                                            for pw in self.windows.values_mut() {
                                                pw.chart.panel_app.user_settings_state.vault_unlock_error =
                                                    Some(format!("Failed to read recovery data: {}", e));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("[App] recovery_unlock: invalid recovery key format: {}", e);
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
                    cmd_handled = true;
                    self.profile_manager.clear_pending_recovery_key();
                    if let Some((profile_id, vault_key)) = self.pending_switch_after_recovery.take() {
                        // Recovery key was shown for a NEW profile — complete the switch immediately.
                        // Sync level is no longer chosen in a wizard step; new profiles default to
                        // device-level OTA control.
                        info!("[App] recovery key confirmed — completing switch for new profile {}", profile_id);
                        self.pending_switch_vault_key = Some(vault_key);
                        self.pending_profile_switch = Some(profile_id);
                    } else if self.pending_promote_after_recovery_key {
                        // Recovery key was shown after a vault re-key (post-recovery passphrase reset).
                        info!("[App] recovery key confirmed (post re-key) — promoting skeleton to live");
                        self.pending_promote_after_recovery_key = false;
                        self.pending_skeleton_promote = true;
                    } else {
                        // Recovery key was shown during wizard / first-run — promote skeleton.
                        info!("[App] recovery key confirmed — promoting skeleton to live");
                        // Don't patch windows — skeleton promote will recreate them.
                        self.pending_skeleton_promote = true;
                    }
                }

                // ── Set new passphrase after recovery key unlock ─────────────────────────────
                // Emitted by the SetNewPassphrase page after the user enters matching passphrases.
                // Re-keys the vault: new passphrase + same salt → new master_key + vault_key,
                // re-encrypts vault.enc, generates a fresh recovery key, then shows ShowRecoveryKey.
                if let Some(new_passphrase) = cmd_str.strip_prefix("set_new_passphrase:") {
                    cmd_handled = true;
                    use mlc_app_layout::ui::modal_settings::{
                        CredentialsView, ProfileManagerPage, UserSettingsTab,
                    };

                    match self.pending_recovery_master_key.take() {
                        None => {
                            // This should never happen — guard state is invalid.
                            warn!("[App] set_new_passphrase: no pending_recovery_master_key — ignoring");
                            for pw in self.windows.values_mut() {
                                pw.chart.panel_app.user_settings_state.set_passphrase_error =
                                    "Internal error: no recovery context. Please restart.".to_string();
                            }
                        }
                        Some(_old_master_key) => {
                            // Re-key the vault + sync the new key into `core` —
                            // shared with the web shell
                            // (`ChartApp::apply_new_passphrase`,
                            // `mlc-app/src/input/credentials.rs`) so the
                            // "forgot `core.templates_mut()`" bug class this
                            // path already had to learn about once cannot
                            // silently reopen in only one shell.
                            let outcome = self.windows.values_mut().next().map(|pw| {
                                pw.chart.apply_new_passphrase(new_passphrase, &mut self.profile_manager)
                            });
                            match outcome {
                                Some(Ok(new_recovery_key_fmt)) => {
                                    info!("[App] set_new_passphrase: vault re-keyed successfully");

                                    // A re-key derives a brand-new vault key from the new
                                    // passphrase (`ProfileManager::rekey_vault`'s own step
                                    // 3 — same salt, new passphrase, new master key), and
                                    // the DM identity is a deterministic function of that
                                    // key: re-deriving here keeps `self.dm_identity` (and
                                    // the auto-publish guard) in step with what every
                                    // future unlock under the NEW passphrase will derive,
                                    // rather than a stale in-RAM copy of the old identity.
                                    #[cfg(feature = "cloud")]
                                    self.refresh_dm_identity();

                                    // Templates on disk are still encrypted under the OLD
                                    // key until rewritten — force an immediate (debounce
                                    // bypassed below via disk_dirty + last_write_at reset)
                                    // re-encrypt so `templates/*` never sits stale relative
                                    // to `vault.enc`, which `apply_new_passphrase` above
                                    // already re-encrypted synchronously.
                                    self.templates_disk_dirty = true;
                                    self.templates_last_write_at = None;

                                    // Mark that skeleton promote should happen after recovery key is confirmed.
                                    self.pending_promote_after_recovery_key = true;

                                    // Navigate to ShowRecoveryKey so user can record the new key.
                                    for pw in self.windows.values_mut() {
                                        pw.chart.panel_app.user_settings_state.recovery_key_display =
                                            Some(new_recovery_key_fmt.clone());
                                        // Sync TextFieldStore recovery key display so it can be selected/copied.
                                        {
                                            let rk_wid = uzor::WidgetId::from(mlc_app::text_input::RECOVERY_KEY_DISPLAY);
                                            pw.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, &new_recovery_key_fmt);
                                            if let Some(fs) = pw.chart.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                                                // Cursor at the START — see the sibling seed sites
                                                // above for why (real scrollable input now, not a
                                                // hand-painted two-line box).
                                                fs.cursor = 0;
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
                                        // Same host test as `e2e_setup:`'s own ShowRecoveryKey
                                        // navigation above.
                                        if pw.chart.panel_app.user_settings_state.show_profile_manager {
                                            pw.chart.panel_app.user_settings_state.profile_manager_page =
                                                ProfileManagerPage::ShowRecoveryKey;
                                        } else {
                                            pw.chart.panel_app.credentials_state.view =
                                                CredentialsView::ShowRecoveryKey;
                                            pw.chart.panel_app.user_settings_state.active_tab =
                                                UserSettingsTab::Vault;
                                            pw.chart.panel_app.user_settings_state.is_open = true;
                                        }
                                    }

                                }
                                Some(Err(e)) => {
                                    error!("[App] set_new_passphrase: rekey_vault failed: {}", e);
                                    for pw in self.windows.values_mut() {
                                        pw.chart.panel_app.user_settings_state.set_passphrase_error =
                                            format!("Failed to re-key vault: {}", e);
                                    }
                                }
                                None => {
                                    warn!("[App] set_new_passphrase: no window to re-key against");
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
                    cmd_handled = true;
                    // Create a brand-new profile so the wizard operates on fresh data,
                    // not the old encrypted profile the user can no longer unlock.
                    // `Some("")`, not `None` — this profile is headed into the
                    // wizard, same anonymous-first-run category as
                    // `wizard_complete`'s own creation; `None` would fall
                    // through to `auto_generate_name()` and hand it a "New
                    // Profile" label, which matters if wizard completion's
                    // own create fails and this profile is what persists.
                    match self.profile_manager.create_profile(Some(""), "chart") {
                        Ok(meta) => {
                            info!(
                                "[App] vault_skip_to_wizard: created new profile ({})",
                                meta.id
                            );
                            // Switch the active profile in the index so that
                            // active_profile_data_dir() resolves to the new empty directory.
                            if let Some(mut index) = mlc_core::load_profile_index() {
                                index.active_profile_id = meta.id.clone();
                                if let Err(e) = mlc_core::save_profile_index(&index) {
                                    warn!(
                                        "[App] vault_skip_to_wizard: failed to update index: {}",
                                        e
                                    );
                                }
                            }
                            // Reload ProfileManager from the new empty profile directory.
                            // No key — no encrypted data yet.
                            self.profile_manager = mlc_app::user_manager::ProfileManager::load(None);
                            self.profile = self.profile_manager.profile.clone();
                            // Reset shared app state + shared core from the old profile.
                            self.core.user_mut().set_vault_key(None);
                            // The fresh profile has no vault yet — seal every
                            // secret-store sidecar rather than leave the OLD
                            // profile's RAM copy reachable under a new
                            // identity (every `set_vault_key(..)` site keeps
                            // them in sync; `None` means seal).
                            self.seal_secret_sidecars();
                            self.core.presets_mut().replace_all(std::collections::HashMap::new());
                            self.preset_dirty_ids.clear();
                        }
                        Err(e) => {
                            error!(
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
                        // `close_profile_manager` (not a bare flag write) blurs
                        // NEW_PASSPHRASE — the `clear()` below only wipes its
                        // text, not its focus.
                        pw.chart.close_profile_manager();
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
                    info!("[App] vault_skip_to_wizard: dismissed vault lock, showing wizard on fresh profile");
                }

                // A command nobody above recognized used to be taken from
                // `pending_updater_cmd` and dropped with no trace at all —
                // that silence is exactly what let `logout`/`start_device_auth`
                // go unnoticed. Never let that happen again.
                if !cmd_handled {
                    warn!("[App] unhandled updater command: {:?}", cmd_str);
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
                    let active_ids = s.notification_secrets.telegram.active_chat_ids()
                        .into_iter()
                        .map(|id| id.to_owned())
                        .collect();
                    test_req = Some((
                        s.notification_secrets.telegram.bot_token.clone(),
                        active_ids,
                    ));
                    break;
                }
                if s.tg_detect_pending {
                    s.tg_detect_pending = false;
                    detect_req = Some(s.notification_secrets.telegram.bot_token.clone());
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
                            warn!("[Telegram] Test skipped: no active subscribers");
                            return;
                        }
                        for chat_id in &chat_ids {
                            match mlc_alert_delivery::telegram::send_test(&client, &token, chat_id).await {
                                Ok(()) => info!("[Telegram] Test message sent to {}", chat_id),
                                Err(e) => warn!("[Telegram] Test failed for {}: {}", chat_id, e),
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
                    let s = &pw.chart.panel_app.alert_settings_state;
                    let prefs = s.notification_settings.clone();
                    let secrets_available = s.secrets_available;
                    let secrets = s.notification_secrets.clone();
                    // Push to live delivery engine — both halves always;
                    // `secrets` is an empty snapshot while locked, which
                    // matches AlertDelivery's own no-op-on-empty contract.
                    if let Some(ref delivery) = self.mlc_alert_delivery {
                        delivery.update_settings(prefs.clone(), secrets.clone());
                    }
                    // Preferences are plaintext profile content — always
                    // persist, vault key or not.
                    self.profile.domain.notification_settings = prefs;
                    let vault_key = self.core.user().vault_key();
                    if let Err(e) = mlc_core::save_profile(&self.profile, vault_key.as_ref()) {
                        warn!("[App] Failed to persist notification settings: {}", e);
                    }
                    // Secrets now live in the `alert_secrets.enc` sidecar
                    // (alerts-account-wide-and-telegram-delivery plan §3),
                    // not `VaultSecrets` — write there directly, only while
                    // unlocked this session (an empty snapshot while locked
                    // must never overwrite what a prior unlock already put
                    // on disk). Re-unlocking the sidecar off the same key
                    // right after the write refreshes its RAM cache, so a
                    // later `get()` this same session sees the new value
                    // instead of the one cached at the last full unlock.
                    if secrets_available {
                        if let Some(ref key) = vault_key {
                            let alert_secrets_path = mlc_core::active_profile_data_dir().join("alert_secrets.enc");
                            match mlc_vault::vault::save_encrypted(key, &alert_secrets_path, &secrets) {
                                Ok(()) => {
                                    if let Err(e) = self.alert_secrets_store.unlock_with_key(key) {
                                        warn!("[App] alert secrets store: failed to refresh after save: {}", e);
                                    }
                                }
                                Err(e) => warn!("[App] Failed to persist alert secrets: {}", e),
                            }
                        }
                    }
                }
                // Clear dirty flag on all windows
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.alert_settings_state.notification_settings_dirty = false;
                }
            }
        }

        // ── Drain cloud Download overlay: fresh check on open ────────────
        // The overlay must not show whatever the updater last happened to
        // think — force one check right as it opens (false→true edge only;
        // never per-frame, that would hammer the manifest endpoint at
        // render-loop rate).
        #[cfg(feature = "cloud")]
        {
            use mlc_app_layout::ui::modal_settings::CloudOverlayKind;
            let any_open = self.windows.values().any(|pw| {
                let ov = &pw.chart.panel_app.cloud_overlay_state;
                ov.is_open && ov.kind == CloudOverlayKind::Download
            });
            if any_open && !self.download_overlay_was_open {
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::ForceCheck);
                self.download_check_inflight = true;
            }
            self.download_overlay_was_open = any_open;
        }

        // ── Drain cloud Download overlay: map updater status → view ──────
        // Runs every frame a window's Download overlay is open — the
        // updater's own watch channel is the source of truth for install
        // progress, so this just mirrors it into the overlay's data. The
        // mapped phase is compared against what is already there before
        // writing: this block runs unconditionally each pass, so an
        // unconditional `request_redraw` here would pin the window at full
        // framerate for as long as the overlay stayed open.
        #[cfg(feature = "cloud")]
        {
            use mlc_app_layout::ui::modal_settings::{
                CloudOverlayData, CloudOverlayKind, UpdateOverlayPhase,
            };
            let phase = match &*self.updater_handle.status_rx.borrow() {
                // Idle is the updater's state both before a check and after
                // one that found nothing, so it alone cannot distinguish
                // "up to date" from "not asked yet" — the flag armed on the
                // open edge does. Without it the overlay claimed the latest
                // release for the frames between opening and the forced
                // check being picked up, which is a claim about the server
                // made before the server was asked.
                mlc_updater::UpdateStatus::Idle if self.download_check_inflight => {
                    UpdateOverlayPhase::Checking
                }
                mlc_updater::UpdateStatus::Idle => UpdateOverlayPhase::UpToDate {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                mlc_updater::UpdateStatus::Checking => UpdateOverlayPhase::Checking,
                mlc_updater::UpdateStatus::UpdateAvailable(info) => UpdateOverlayPhase::Available {
                    version: info.version.clone(),
                    notes: info.release_notes.clone(),
                },
                mlc_updater::UpdateStatus::Downloading { percent } => {
                    UpdateOverlayPhase::Downloading { percent: *percent }
                }
                mlc_updater::UpdateStatus::Verifying => UpdateOverlayPhase::Verifying,
                mlc_updater::UpdateStatus::Installing => UpdateOverlayPhase::Installing,
                mlc_updater::UpdateStatus::RestartPending => UpdateOverlayPhase::RestartPending,
                mlc_updater::UpdateStatus::Error(message) => {
                    UpdateOverlayPhase::Failed { message: message.clone() }
                }
            };
            // Any status other than Idle is an answer: the check landed.
            if !matches!(phase, UpdateOverlayPhase::Checking) {
                self.download_check_inflight = false;
            }
            for pw in self.windows.values_mut() {
                let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                if ov.is_open && ov.kind == CloudOverlayKind::Download {
                    let next = CloudOverlayData::Update { phase: phase.clone() };
                    if ov.data != next {
                        ov.data = next;
                        pw.chart_dirty = true;
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    }
                }
            }
        }

        // ── Drain cloud Download overlay: install / retry requests ───────
        // One-shot mailbox set by the overlay's Install (and Retry, same
        // flag) button. The overlay itself does not change state here — the
        // status-mapping block above reflects the updater's own
        // Downloading/Verifying/Installing progress back once it starts
        // moving.
        #[cfg(feature = "cloud")]
        {
            let install_requested = self
                .windows
                .values()
                .any(|pw| pw.chart.panel_app.cloud_overlay_state.update_install_requested);
            if install_requested {
                for pw in self.windows.values_mut() {
                    pw.chart.panel_app.cloud_overlay_state.update_install_requested = false;
                }
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::InstallNow);
            }
        }

        // ── Drain cloud overlay: outbound links + News feed fetch ────────
        {
            // Links: the overlay hands back either an absolute URL or a
            // site-relative path; join the latter onto the production
            // domain, then hand off to the existing open-URL drain above
            // (`pending_open_url`) instead of a second `cmd /c start` copy.
            #[cfg(feature = "cloud")]
            for pw in self.windows.values_mut() {
                if let Some(raw_url) = pw.chart.panel_app.cloud_overlay_state.open_url_requested.take() {
                    let url = if raw_url.starts_with("http") {
                        raw_url
                    } else {
                        let path = raw_url.strip_prefix('/').unwrap_or(&raw_url);
                        format!("https://chart.example/{path}")
                    };
                    pw.chart.pending_open_url = Some(url);
                }
            }

            // News: one fetch per Empty→open transition, PLUS a quiet
            // background re-fetch on the same `FORUM_REFRESH_MS` cadence the
            // Forum tab's own `forum_list_due_for_refresh()` uses while the
            // panel stays open (recon item F1 — nothing used to reset `data`
            // back to `Empty` once it became `News{..}`, so a post published
            // after this run's first fetch was invisible until a full
            // restart). Fire-and-forget on the live-data runtime (never
            // `block_on` here — this runs on the render thread every frame).
            // The result comes back through `news_feed_rx`, polled below
            // with a non-blocking `try_recv`.
            #[cfg(feature = "cloud")]
            {
                use mlc_app_layout::ui::modal_settings::{CloudOverlayData, CloudOverlayKind};
                let needs_initial_fetch = self.windows.values().any(|pw| {
                    let ov = &pw.chart.panel_app.cloud_overlay_state;
                    ov.is_open
                        && ov.kind == CloudOverlayKind::News
                        && matches!(ov.data, CloudOverlayData::Empty)
                });
                let needs_refresh = self.windows.values().any(|pw| {
                    let ov = &pw.chart.panel_app.cloud_overlay_state;
                    ov.is_open
                        && ov.kind == CloudOverlayKind::News
                        && matches!(ov.data, CloudOverlayData::News { .. })
                        && ov.news_due_for_refresh()
                });
                if needs_initial_fetch || needs_refresh {
                    for pw in self.windows.values_mut() {
                        let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                        if !(ov.is_open && ov.kind == CloudOverlayKind::News) {
                            continue;
                        }
                        if matches!(ov.data, CloudOverlayData::Empty) {
                            ov.data = CloudOverlayData::Loading;
                            pw.chart_dirty = true;
                            pw.overlay_dirty = true;
                            pw.window.request_redraw();
                        } else if matches!(ov.data, CloudOverlayData::News { .. }) && ov.news_due_for_refresh() {
                            // Quiet: `data` stays on screen as-is, only the
                            // in-flight flag flips — no redraw needed for
                            // this branch, nothing visible changed yet.
                            ov.news_refreshing = true;
                        }
                    }
                    let tx = self.news_feed_tx.clone();
                    self.live_data_handle.spawn(async move {
                        let result = fetch_news_feed().await;
                        let _ = tx.send(result);
                    });
                }

                if let Ok(result) = self.news_feed_rx.try_recv() {
                    for pw in self.windows.values_mut() {
                        let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                        let is_initial = matches!(ov.data, CloudOverlayData::Loading);
                        let is_refresh = ov.kind == CloudOverlayKind::News && ov.news_refreshing;
                        if !is_initial && !is_refresh {
                            continue;
                        }
                        match &result {
                            Ok(posts) => {
                                ov.data = CloudOverlayData::News { posts: posts.clone() };
                                ov.mark_news_fetched();
                            }
                            Err(e) => {
                                // A due-refresh failure must not blank an
                                // already-shown page — only the initial
                                // (still-`Loading`) fetch's error replaces
                                // `data`; a refresh just clears its own
                                // in-flight flag and quietly retries next
                                // cadence, same tolerance the Forum tab's own
                                // background refresh gives its failures.
                                if is_initial {
                                    ov.data = CloudOverlayData::Error { message: e.clone() };
                                }
                                ov.news_refreshing = false;
                            }
                        }
                        pw.chart_dirty = true;
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    }
                }
            }
        }

        // ── Drain Forum tab (paid-only): explicit actions + passive fetch ──
        // Goes through `mlc_updater` (auth + paid gate, same as
        // redeem/set_nick), NOT the direct-fetch path News uses above — the
        // forum endpoints require a bearer token the anonymous blog fetch
        // never carries. `forum_rx`'s answer is applied further down,
        // alongside `redeem_status`/`nick_status`.
        #[cfg(feature = "cloud")]
        {
            use mlc_app_layout::ui::modal_settings::{
                CommunityTab, ForumListState, ForumRequest, ForumSubView,
            };

            let mut to_send: Vec<mlc_updater::ForumCommand> = Vec::new();
            for pw in self.windows.values_mut() {
                if let Some(req) = pw.chart.panel_app.cloud_overlay_state.forum_request.take() {
                    to_send.push(match req {
                        ForumRequest::LoadMoreThreads { before } => {
                            mlc_updater::ForumCommand::ListThreads { before: Some(before) }
                        }
                        ForumRequest::LoadThread { id } => mlc_updater::ForumCommand::LoadThread { id },
                        ForumRequest::CreateThread { title, body } => {
                            mlc_updater::ForumCommand::CreateThread { title, body }
                        }
                        ForumRequest::Reply { thread_id, body } => {
                            mlc_updater::ForumCommand::Reply { thread_id, body }
                        }
                        ForumRequest::DeleteThread { id } => mlc_updater::ForumCommand::DeleteThread { id },
                        ForumRequest::DeleteReply { id, thread_id } => {
                            mlc_updater::ForumCommand::DeleteReply { id, thread_id }
                        }
                    });
                }
            }
            let explicit_action_this_tick = !to_send.is_empty();
            for cmd in to_send {
                let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Forum(cmd));
            }

            // Passive: fetch page one the moment the tab is looked at with
            // nothing loaded yet — same "Empty -> fire" idiom the News fetch
            // above uses.
            let needs_fetch = self.windows.values().any(|pw| {
                let community_forum_open = pw.chart.panel_app.community_state.is_open
                    && pw.chart.panel_app.community_state.tab == CommunityTab::Forum;
                let ov = &pw.chart.panel_app.cloud_overlay_state;
                community_forum_open && matches!(ov.forum_list, ForumListState::Empty)
            });
            if needs_fetch {
                for pw in self.windows.values_mut() {
                    let community_forum_open = pw.chart.panel_app.community_state.is_open
                        && pw.chart.panel_app.community_state.tab == CommunityTab::Forum;
                    let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                    if community_forum_open && matches!(ov.forum_list, ForumListState::Empty) {
                        ov.forum_list = ForumListState::Loading;
                    }
                }
                let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Forum(
                    mlc_updater::ForumCommand::ListThreads { before: None },
                ));
            }

            // Passive, same idiom: once `forum_list_due_for_refresh()`/
            // `forum_thread_due_for_refresh()` says `FORUM_REFRESH_MS` has
            // elapsed on an already-loaded page or the open thread, reissue
            // the fetch quietly (no `Loading`/placeholder flip, so the
            // visible page and its scroll stay put) so another
            // participant's reply or new thread shows up without the
            // caller leaving and reopening the overlay. One shared updater
            // channel across every window (unlike the web shell's own
            // per-driver `forum_in_flight`), so this gates on the settled
            // `ForumState::busy` peeked here, and skips a tick that already
            // sent an explicit action — avoids doubling a request the
            // caller's own click just fired. List refresh takes priority
            // over thread refresh when both are due the same tick; the
            // other one waits for the next due tick.
            let forum_busy = self.updater_handle.forum_rx.borrow().busy;
            if !explicit_action_this_tick && !forum_busy && !needs_fetch {
                let list_due = self.windows.values().any(|pw| {
                    let community_forum_open = pw.chart.panel_app.community_state.is_open
                        && pw.chart.panel_app.community_state.tab == CommunityTab::Forum;
                    let ov = &pw.chart.panel_app.cloud_overlay_state;
                    community_forum_open
                        && matches!(ov.forum_list, ForumListState::Loaded { .. })
                        && ov.forum_list_due_for_refresh()
                });
                if list_due {
                    for pw in self.windows.values_mut() {
                        let community_forum_open = pw.chart.panel_app.community_state.is_open
                            && pw.chart.panel_app.community_state.tab == CommunityTab::Forum;
                        let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                        if community_forum_open
                            && matches!(ov.forum_list, ForumListState::Loaded { .. })
                            && ov.forum_list_due_for_refresh()
                        {
                            ov.forum_list_refreshing = true;
                        }
                    }
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Forum(
                        mlc_updater::ForumCommand::ListThreads { before: None },
                    ));
                } else if let Some(id) = self.windows.values().find_map(|pw| {
                    let community_forum_open = pw.chart.panel_app.community_state.is_open
                        && pw.chart.panel_app.community_state.tab == CommunityTab::Forum;
                    if !community_forum_open {
                        return None;
                    }
                    let ov = &pw.chart.panel_app.cloud_overlay_state;
                    if let ForumSubView::Thread { thread: Some(t), loading: false, posting_reply: false, .. } =
                        &ov.forum_view
                    {
                        if ov.forum_thread_due_for_refresh() {
                            return Some(t.id);
                        }
                    }
                    None
                }) {
                    for pw in self.windows.values_mut() {
                        let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                        let matches_due = matches!(
                            &ov.forum_view,
                            ForumSubView::Thread { thread: Some(t), loading: false, posting_reply: false, .. }
                                if t.id == id
                        ) && ov.forum_thread_due_for_refresh();
                        if matches_due {
                            ov.forum_thread_refreshing = true;
                        }
                    }
                    let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::Forum(
                        mlc_updater::ForumCommand::LoadThread { id },
                    ));
                }
            }
        }

        // ── Drain Forum v2 tab (public boards) + Messages tab (DM) ─────────
        #[cfg(feature = "cloud")]
        {
            self.drive_forum2();
            self.drive_dm();
        }

        // ── Drain agent commands every frame ─────────────────────────────
        self.drain_agent_commands();

        // Per-window: tick ALL windows (not just focused) so live data,
        // watchlist updates, and state changes are processed even for
        // background windows that Windows won't deliver WM_PAINT to.
        let _t4 = web_time::Instant::now();
        {
            // watchlists/presets/snapshots/templates/recalc_mode/
            // server_enabled/server_port dirty-flag sync blocks are GONE
            // (shared-BackendCore arc Phase B+C) — every window's
            // `chart.core` is the SAME `Arc<BackendCore>` now, so a write
            // through `core.X_mut()`/`core.user_mut()` anywhere is
            // immediately visible to every window's next read.
            //
            // `sidebar_state.connector_enabled` is a HashMap<String,bool>
            // clone into `mlc-app`'s own read path (~10 call sites there
            // check it directly, not through `core`) — cheap enough
            // (small map) to refresh unconditionally every frame like
            // `recalc_mode` below, no dirty flag needed.
            let connector_enabled = self.core.user().connector_enabled().clone();
            for pw in self.windows.values_mut() {
                pw.chart.sidebar_state.connector_enabled = connector_enabled.clone();
            }

            // `UserSettingsState::account_signed_in` — this shell's token
            // store is `mlc_updater::token_store`, filled by the loopback
            // OAuth flow (see the `start_device_auth` arm above; native does
            // not use device codes), never the entitlements answer. A `watch::Receiver::borrow()` peek is a channel read,
            // not network IO, so it costs nothing to keep exact every frame
            // rather than only at boot and at the login/logout instant.
            let account_signed_in = matches!(
                *self.updater_handle.auth_rx.borrow(),
                mlc_updater::AuthStatus::LoggedIn { .. }
            );
            // Sign-in completed → system event (owner ask 2026-09-05, scope
            // §8): edge off the same `auth_rx` channel `account_signed_in`/
            // `account_label` read via plain `.borrow()` above/below —
            // `has_changed()`/`borrow_and_update()` is the idiom
            // `redeem_rx`/`nick_rx` already use below, so a transition fires
            // exactly once. Sign-out is logged at its own two
            // `UpdaterCommand::Logout` call sites instead (this only ever
            // matches `LoggedIn` on its edge). Resolved to an owned
            // `Option<String>` here (mirrors `redeem_status`/`nick_status`
            // below) so the `watch::Ref` guard is gone before
            // `system_event_window_mut()` needs `&mut self` again.
            // `just_signed_out` (agent-api-auth-and-secret-store plan, piece
            // 4): the sign-out edge off this SAME channel/`has_changed()`
            // read — a second, separate `has_changed()` call below would
            // find the "changed" bit already consumed by the
            // `borrow_and_update()` this match already does.
            let (newly_signed_in_as, just_signed_out): (Option<String>, bool) =
                if self.updater_handle.auth_rx.has_changed().unwrap_or(false) {
                    match &*self.updater_handle.auth_rx.borrow_and_update() {
                        mlc_updater::AuthStatus::LoggedIn { display_name, .. } => (Some(display_name.clone()), false),
                        mlc_updater::AuthStatus::NotLoggedIn => (None, true),
                    }
                } else {
                    (None, false)
                };
            // Forum-v2 `caller_roles`/`caller_classes` belong to a specific
            // identity (or the signed-out default) — either edge off the
            // SAME `has_changed()` read above invalidates them, so the
            // passive fetch in `drive_forum2` re-asks under whichever
            // identity answers next. Computed before `newly_signed_in_as`
            // is consumed just below.
            #[cfg(feature = "cloud")]
            let auth_identity_changed = newly_signed_in_as.is_some() || just_signed_out;
            if let Some(display_name) = newly_signed_in_as {
                let body = format!("Signed in as {display_name}.");
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.push_system_event(mlc_core::SystemEventKind::Account, "Signed in", body, true);
                }
                // Sign-in edge: this session gets its own DM auto-publish
                // attempt in `drive_dm`, even if an earlier attempt this
                // same unlock already failed while signed out (owner ruling
                // 2026-09-09: "again after a sign-in edge").
                #[cfg(feature = "cloud")]
                {
                    self.dm_identity_publish_attempted_for = None;
                }
            }
            // Seal every secret-store sidecar on sign-out — a signed-out
            // session must not keep an unlocked RAM copy of either, even
            // though the listener start/stop edge just below already stops
            // the Agent API server on this same transition (`seal()` is
            // idempotent, so a stray fire on the very first frame — before
            // this receiver has ever seen a real change — costs nothing).
            // Native Telegram delivery is signed-in-only (alerts-account-
            // wide-and-telegram-delivery plan §3), so `alert_secrets_store`
            // belongs in this same edge, not just the agent-key store.
            if just_signed_out {
                self.seal_secret_sidecars();
            }
            #[cfg(feature = "cloud")]
            if auth_identity_changed {
                for pw in self.windows.values_mut() {
                    let comm = &mut pw.chart.panel_app.community_state;
                    comm.forum2.invalidate_me();
                    // The board LIST and DM conversation list are per-caller
                    // too (owner defect 2026-09-09: an admin signing in saw
                    // zero boards until a hard reload) — `invalidate_me()`
                    // alone only re-asks classes/roles, never the boards a
                    // DIFFERENT account can read, nor whether this account's
                    // own conversations differ from whoever was signed in
                    // before. Also drop any recipient-search dropdown state
                    // so a stale answer from a previous account can never
                    // show under the new one.
                    comm.forum2.invalidate_boards();
                    comm.dm.invalidate_conversations();
                    comm.dm.close_lookup_dropdown();
                }
            }
            // Agent API listener start/stop: the port only ever binds for a
            // signed-in session with the Server-tab toggle on — never at
            // boot, regardless of the toggle (agent-api-auth-and-secret-
            // store plan, piece 4). `bind_with_reuse`'s own retry loop
            // tolerates the brief window where a just-stopped listener's
            // socket has not yet been released by the OS.
            let want_agent_listener = account_signed_in && self.core.user().server_enabled();
            if want_agent_listener && self.listener_handle.is_none() {
                // Cloned out of `self.agent_state` (rather than matched by
                // `ref`) so the borrow ends here — `system_event_window_mut`
                // below needs `&mut self` and cannot run while any part of
                // `self` is still immutably borrowed.
                if let Some(agent_state) = self.agent_state.clone() {
                    let port = self.core.user().server_port();
                    let handle = mlc_backend_shell_http::start_server(
                        agent_state, &self.live_data_handle, port,
                    );
                    self.listener_handle = Some(handle);
                    if let Some(pw) = self.system_event_window_mut() {
                        pw.chart.push_system_event(
                            mlc_core::SystemEventKind::Agent,
                            format!("Agent API listening on 127.0.0.1:{port}"),
                            String::new(),
                            false,
                        );
                    }
                    info!("[App] Agent API listener started on 127.0.0.1:{}", port);
                }
            } else if !want_agent_listener && self.listener_handle.is_some() {
                if let Some(handle) = self.listener_handle.take() {
                    handle.abort();
                }
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.push_system_event(
                        mlc_core::SystemEventKind::Agent,
                        "Agent API stopped",
                        String::new(),
                        false,
                    );
                }
                info!("[App] Agent API listener stopped");
            }
            let agent_server_status = if self.listener_handle.is_some() { "running" } else { "stopped" };

            // Drain the most recently minted raw agent key into the Server
            // tab's one-time reveal box, and mirror the current key list +
            // the store's sealed state every frame — this crate cannot see
            // `AgentState` (dependency direction), so `UserSettingsState`
            // carries plain mirrors instead (see that struct's own doc).
            let freshly_minted_key = self.agent_state.as_ref().and_then(|s| s.take_last_minted_key());
            let (agent_keys, agent_keys_sealed): (Vec<(String, String, String, u64)>, bool) =
                match self.agent_state.as_ref().map(|s| s.list_keys()) {
                    Some(Ok(records)) => (
                        records.iter().map(|r| (
                            r.fingerprint_hex.clone(),
                            r.label.clone(),
                            format!("{:?}", r.role),
                            r.created_at_ms,
                        )).collect(),
                        false,
                    ),
                    _ => (Vec::new(), true),
                };
            // The chrome's sign-in button reads the same channel, so the
            // strip cannot disagree with the settings modal about who is
            // signed in — one source, two readers.
            let (account_label, account_provider): (Option<String>, Option<String>) =
                match &*self.updater_handle.auth_rx.borrow() {
                    mlc_updater::AuthStatus::LoggedIn {
                        display_name,
                        provider,
                        ..
                    } => (Some(display_name.clone()), Some(provider.clone())),
                    _ => (None, None),
                };
            // An installed update waiting for a restart: the chrome's download
            // button becomes a restart glyph in the accent, and it stays that
            // way until the app is restarted. Read from the updater's own
            // state, so it cannot drift from what actually happened.
            // What the chrome's update control should be saying. One
            // button with four readings; the toast that used to announce a
            // pending restart is gone, because a toast fades and the pending
            // restart does not (owner, 2026-09-04).
            let update_state = {
                use mlc_app_input_shell::chrome::ChromeUpdate;
                match &*self.updater_handle.status_rx.borrow() {
                    mlc_updater::UpdateStatus::Downloading { percent } => {
                        ChromeUpdate::Working { percent: Some(*percent) }
                    }
                    mlc_updater::UpdateStatus::Verifying
                    | mlc_updater::UpdateStatus::Installing => {
                        ChromeUpdate::Working { percent: None }
                    }
                    mlc_updater::UpdateStatus::RestartPending => ChromeUpdate::Restart,
                    mlc_updater::UpdateStatus::Error(_) => ChromeUpdate::Failed,
                    // Idle, Checking and a freshly-found update have nothing
                    // to ask of anyone: the install runs on its own.
                    _ => ChromeUpdate::Idle,
                }
            };
            // Update available/downloaded → system event (owner ask
            // 2026-09-05, scope §8): edge off the SAME `status_rx` channel
            // `update_state` just read via `.borrow()` above —
            // `has_changed()`/`borrow_and_update()` only fires once per
            // transition, matching `redeem_rx`/`nick_rx`'s idiom below.
            // Resolved to an owned tuple (mirrors `newly_signed_in_as`
            // above) so the `watch::Ref` guard is gone before
            // `system_event_window_mut()` needs `&mut self` again.
            let new_update_event: Option<(&'static str, String, bool)> =
                if self.updater_handle.status_rx.has_changed().unwrap_or(false) {
                    match &*self.updater_handle.status_rx.borrow_and_update() {
                        mlc_updater::UpdateStatus::UpdateAvailable(info) => Some((
                            "Update available",
                            format!("Version {} is available.", info.version),
                            true,
                        )),
                        mlc_updater::UpdateStatus::RestartPending => Some((
                            "Update downloaded",
                            "Restart to finish installing the update.".to_string(),
                            true,
                        )),
                        _ => None,
                    }
                } else {
                    None
                };
            if let Some((title, body, toast)) = new_update_event {
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.push_system_event(mlc_core::SystemEventKind::Update, title, body, toast);
                }
            }
            for pw in self.windows.values_mut() {
                pw.chart.panel_app.user_settings_state.account_signed_in = account_signed_in;
                // Same mirror for the alert-settings modal's Telegram gate
                // (alerts-account-wide-and-telegram-delivery plan §3) —
                // native delivery is "own bot, signed-in, vault-backed".
                pw.chart.panel_app.alert_settings_state.account_signed_in = account_signed_in;
                // `Server` tab is not drawn while signed out
                // (`UserSettingsTab::visible`) — force off it here too, so a
                // sign-out that happens while the tab is open does not leave
                // `active_tab` pointing at a tab the rail no longer shows
                // (agent-api-auth-and-secret-store plan, piece 4).
                if !account_signed_in
                    && pw.chart.panel_app.user_settings_state.active_tab
                        == mlc_app_layout::ui::modal_settings::UserSettingsTab::Server
                {
                    pw.chart.panel_app.user_settings_state.active_tab =
                        mlc_app_layout::ui::modal_settings::UserSettingsTab::General;
                }
                // `Account` is likewise never in the rail while signed out
                // (`UserSettingsTab::visible`) — same defensive snap, mirror
                // image of `Server`'s own gate above.
                if !account_signed_in
                    && pw.chart.panel_app.user_settings_state.active_tab
                        == mlc_app_layout::ui::modal_settings::UserSettingsTab::Account
                {
                    pw.chart.panel_app.user_settings_state.active_tab =
                        mlc_app_layout::ui::modal_settings::UserSettingsTab::General;
                }
                // `ForumV2State::caller_signed_in` mirrors the same source
                // as `account_signed_in` above — see that field's own doc
                // for why it is not derived from `caller_classes` (which
                // stays empty until the `Me` fetch answers).
                #[cfg(feature = "cloud")]
                {
                    pw.chart.panel_app.community_state.forum2.caller_signed_in = account_signed_in;
                }
                pw.chart.panel_app.user_settings_state.server_status = agent_server_status.to_string();
                pw.chart.panel_app.user_settings_state.agent_keys = agent_keys.clone();
                pw.chart.panel_app.user_settings_state.agent_keys_sealed = agent_keys_sealed;
                if let Some(ref raw_key) = freshly_minted_key {
                    pw.chart.panel_app.user_settings_state.last_created_key = Some(raw_key.clone());
                }
                // Mirrors the chrome's own `account_label` below — the nick
                // edit box prefills from this, not the unrelated local
                // profile name.
                pw.chart.panel_app.user_settings_state.account_label =
                    account_label.clone().unwrap_or_default();
                pw.chart.panel_app.user_settings_state.account_provider =
                    account_provider.clone().unwrap_or_default();
                if pw.chrome_state.account_label != account_label {
                    pw.chrome_state.account_label = account_label.clone();
                    pw.toolbar_dirty = true;
                    pw.window.request_redraw();
                }
                pw.chrome_state.account_provider = account_provider.clone();
                // Header account-button display mode (owner ruling
                // 2026-09-10) — same per-frame mirror as `account_label`
                // above, from the SAME `user_settings_state` this loop just
                // wrote it into.
                let header_account_display =
                    pw.chart.panel_app.user_settings_state.header_account_display;
                if pw.chrome_state.header_account_display != header_account_display {
                    pw.chrome_state.header_account_display = header_account_display;
                    pw.toolbar_dirty = true;
                    pw.window.request_redraw();
                }
                if pw.chrome_state.update != update_state {
                    pw.chrome_state.update = update_state;
                    pw.toolbar_dirty = true;
                    pw.window.request_redraw();
                }
            }
            // `UserSettingsState::multi_profile` — this shell has no
            // `mlc-cloud` transport (native side does not exist, see that
            // crate's own module doc), but `mlc_updater` now carries its
            // own `GET /api/entitlements` caller, since it already owns
            // the reqwest client, the token store, and this watch-channel
            // publishing pattern. Same peek-cost reasoning as
            // `account_signed_in` above: a `borrow()` is a channel read,
            // not network IO. `Entitlements::default()` (no fetch yet, or
            // last fetch failed) reads as `false` here, matching the
            // "degrade to free, never to paid" rule enforced on the
            // publish side in `mlc_updater::do_entitlements_check`.
            let multi_profile = self
                .updater_handle
                .entitlements_rx
                .borrow()
                .has("multi_profile");
            // The Forum tab's sole visibility switch — same peek-cost
            // reasoning as `multi_profile` above, read from the same
            // channel so the two can never disagree about which
            // entitlements answer they reflect.
            #[cfg(feature = "cloud")]
            let forum_entitled = self.updater_handle.entitlements_rx.borrow().has("forum");
            // The account overlay states the tier outright rather than
            // implying it from which surfaces are unlocked. Same channel, and
            // the server's own `paid` field — never a guess from the feature
            // list, which is what paid currently buys and will move.
            // Tier transition → system event (owner ask 2026-09-05, scope
            // §8): edge-triggered off `entitlements_rx` via
            // `has_changed()`/`borrow_and_update()` — the sibling pattern
            // `redeem_rx`/`nick_rx` already use below, which this channel
            // previously did not (native's own gap, per recon). `is_paid`/
            // `paid_until` still resolve every frame (a window opened
            // between two publishes still needs the current value applied
            // below), but the transition compare only runs — and
            // `self.prev_entitlements` only advances — on the edge, so a
            // window that just isn't looking this frame can never cause a
            // missed or duplicate event. `None` in `prev_entitlements` means
            // no answer has landed yet THIS session (boot, or a restart) —
            // falls back to the last answer this device persisted via
            // `mlc_updater::tier_store` for the SAME identity, so a tier
            // change applied while the app was closed (a gift code redeemed
            // via the API, a subscription that expired, then a restart)
            // still surfaces once instead of silently becoming the new
            // baseline. No stored record, or one for a different user, keeps
            // the old behaviour: no event.
            #[cfg(feature = "cloud")]
            let (is_paid, paid_until, tier_event): (bool, Option<String>, Option<(&'static str, String)>) =
                if self.updater_handle.entitlements_rx.has_changed().unwrap_or(false) {
                    let (paid, until) = {
                        let ent = self.updater_handle.entitlements_rx.borrow_and_update();
                        (ent.paid, ent.paid_until.clone())
                    };
                    let current_user_id = match &*self.updater_handle.auth_rx.borrow() {
                        mlc_updater::AuthStatus::LoggedIn { user_id, .. } => Some(*user_id),
                        mlc_updater::AuthStatus::NotLoggedIn => None,
                    };
                    let prev = self.prev_entitlements.replace((paid, until.clone()));
                    let baseline = prev.or_else(|| {
                        let uid = current_user_id?;
                        let stored = mlc_updater::tier_store::load_tier()?;
                        (stored.user_id == uid).then_some((stored.paid, stored.paid_until))
                    });
                    let event = match baseline {
                        None => None,
                        Some((prev_paid, prev_until)) if prev_paid == paid && prev_until == until => None,
                        Some(_) if paid => Some((
                            "Paid tier active",
                            match &until {
                                Some(u) => format!("Active until {u}."),
                                None => "Your paid tier is active.".to_string(),
                            },
                        )),
                        Some(_) => Some(("Paid tier expired", "Your paid tier has expired.".to_string())),
                    };
                    // Persist this APPLIED answer as the new baseline — only
                    // when signed in; an anonymous entitlements answer has
                    // no identity to key the record on.
                    if let Some(uid) = current_user_id {
                        mlc_updater::tier_store::save_tier(&mlc_updater::tier_store::StoredTier {
                            user_id: uid,
                            paid,
                            paid_until: until.clone(),
                        });
                    }
                    (paid, until, event)
                } else {
                    let ent = self.updater_handle.entitlements_rx.borrow();
                    (ent.paid, ent.paid_until.clone(), None)
                };
            #[cfg(feature = "cloud")]
            if let Some((title, body)) = tier_event {
                if let Some(pw) = self.system_event_window_mut() {
                    pw.chart.push_system_event(mlc_core::SystemEventKind::Tier, title, body, true);
                }
            }
            // Outcome of the last submitted gift code. `RedeemState` is the
            // updater's word for it; `RedeemStatus` is the Account tab's.
            // They are two crates that do not know each other, so the map is
            // here. NOT `cfg(feature = "cloud")`-gated (owner ruling
            // 2026-09-23: the row it drives moved off the login overlay onto
            // `UserSettingsState`, same reasoning `nick_status` below
            // already gives for its own field).
            let redeem_status = if !self.updater_handle.redeem_rx.has_changed().unwrap_or(false) {
                // Only on the edge. The channel keeps its last outcome for the
                // life of the process, so copying it every frame would put
                // last hour's "already used" back on a freshly opened tab
                // the instant `UserSettingsState::close` had cleared it.
                None
            } else {
                use mlc_app_layout::ui::modal_settings::RedeemStatus;
                Some(match &*self.updater_handle.redeem_rx.borrow_and_update() {
                    mlc_updater::RedeemState::Idle => RedeemStatus::Idle,
                    mlc_updater::RedeemState::Working => RedeemStatus::Working,
                    mlc_updater::RedeemState::Redeemed { paid_until } => {
                        RedeemStatus::Redeemed { paid_until: paid_until.clone() }
                    }
                    mlc_updater::RedeemState::Failed { message } => {
                        RedeemStatus::Failed { message: message.clone() }
                    }
                })
            };
            // Outcome of the last submitted account nickname. `NickState` is
            // the updater's word for it; `NickStatus` is the settings row's.
            // Same "compiles in every build" reasoning as `redeem_status`
            // above.
            let nick_status = if !self.updater_handle.nick_rx.has_changed().unwrap_or(false) {
                None
            } else {
                use mlc_app_layout::ui::modal_settings::NickStatus;
                Some(match &*self.updater_handle.nick_rx.borrow_and_update() {
                    mlc_updater::NickState::Idle => NickStatus::Idle,
                    mlc_updater::NickState::Working => NickStatus::Working,
                    mlc_updater::NickState::Saved { effective_label } => {
                        NickStatus::Saved { effective_label: effective_label.clone() }
                    }
                    mlc_updater::NickState::Failed { message } => {
                        NickStatus::Failed { message: message.clone() }
                    }
                })
            };
            // Nick saved/failed → system event (owner ask 2026-09-05, scope
            // §8): `nick_status` above is already edge-triggered (`None`
            // unless `nick_rx` changed this frame) — no extra bookkeeping
            // needed. Not toasted: the settings row already shows the
            // outcome inline the same frame.
            if let Some(status) = nick_status.as_ref() {
                use mlc_app_layout::ui::modal_settings::NickStatus;
                match status {
                    NickStatus::Saved { effective_label } => {
                        let body = format!("Now shown as {effective_label}.");
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Nick, "Nickname saved", body, false);
                        }
                    }
                    NickStatus::Failed { message } => {
                        let body = message.clone();
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Nick, "Nickname not saved", body, false);
                        }
                    }
                    _ => {}
                }
            }
            // Chart-share result → clipboard + system event. Edge-triggered
            // off `share_rx` same as `redeem_rx`/`nick_rx` above. Mirrors
            // `mlc-app-shell-web::cloud_session`'s own "── Chart-share
            // result → system event" block: on success the URL goes to the
            // clipboard (there: `crate::clipboard_write_text`; here:
            // `arboard`, the same crate the clipboard-destination
            // screenshot path already uses) and a toast names the link; on
            // failure only the toast fires. No dedicated UI slot exists for
            // this on either shell (there is no `ShareStatus` type) — the
            // system-event log is the whole feedback surface.
            if self.updater_handle.share_rx.has_changed().unwrap_or(false) {
                let outcome = self.updater_handle.share_rx.borrow_and_update().clone();
                match outcome {
                    mlc_updater::ShareState::Created { url } => {
                        match arboard::Clipboard::new() {
                            Ok(mut cb) => {
                                if let Err(e) = cb.set_text(&url) {
                                    warn!("[share] clipboard copy failed: {}", e);
                                }
                            }
                            Err(e) => warn!("[share] clipboard unavailable: {}", e),
                        }
                        let body = format!("Link copied to clipboard: {url}");
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Share, "Chart shared", body, true);
                        }
                    }
                    mlc_updater::ShareState::Failed { message } => {
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Share, "Share failed", message, true);
                        }
                    }
                    mlc_updater::ShareState::Idle | mlc_updater::ShareState::Working => {}
                }
            }
            // Outcome of the most recent open/clone-share request —
            // edge-triggered off `share_open_rx` same as `share_rx` above,
            // but applied to a tab (`ChartApp::open_shared_preset`/
            // `apply_cloned_preset`) on the active window
            // (`system_event_window_mut`, same "one window speaks for the
            // profile" choice that helper's own doc explains) rather than
            // just a clipboard write + a generic toast.
            if self.updater_handle.share_open_rx.has_changed().unwrap_or(false) {
                let outcome = self.updater_handle.share_open_rx.borrow_and_update().clone();
                match outcome {
                    mlc_updater::ShareOpenState::Opened { meta, preset_json } => {
                        // `Opened` carries no token of its own (one-shot
                        // mailbox) — read back what the drain above sent.
                        let token = self.pending_open_share_token.take().unwrap_or_default();
                        let outcome: Result<(), String> = if meta.kind != "live" {
                            Err("this share has no chart to open".to_string())
                        } else {
                            match preset_json {
                                None => Err("server sent no preset for a live share".to_string()),
                                Some(value) => match serde_json::from_value::<mlc_core::preset::preset::ChartPreset>(value) {
                                    Err(e) => Err(format!("could not read shared preset: {}", e)),
                                    Ok(preset) => {
                                        let shared_meta = mlc_app::SharedChartMeta {
                                            token,
                                            title: meta.title,
                                            owner_label: meta.owner_label,
                                            kind: meta.kind,
                                            rights: meta.rights,
                                            expires_at: meta.expires_at,
                                        };
                                        match self.system_event_window_mut() {
                                            None => Err("no window to open the shared chart in".to_string()),
                                            Some(pw) => pw.chart.open_shared_preset(shared_meta, preset).map(|_| ()),
                                        }
                                    }
                                },
                            }
                        };
                        let (title, body) = match outcome {
                            Ok(()) => (
                                "Shared chart opened",
                                "Read-only — use Clone to make it yours.".to_string(),
                            ),
                            Err(message) => ("Could not open shared chart", message),
                        };
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Share, title, body, true);
                        }
                    }
                    mlc_updater::ShareOpenState::Cloned { preset_json, stripped } => {
                        match serde_json::from_value::<mlc_core::preset::preset::ChartPreset>(preset_json) {
                            Ok(preset) => {
                                let stripped: Vec<mlc_app::StrippedIndicator> = stripped
                                    .into_iter()
                                    .map(|s| mlc_app::StrippedIndicator { type_id: s.type_id, name: s.name })
                                    .collect();
                                // `apply_cloned_preset` pushes its own
                                // "Chart cloned" system event (naming any
                                // stripped indicator) — nothing more to do
                                // on success.
                                if let Some(pw) = self.system_event_window_mut() {
                                    pw.chart.apply_cloned_preset(preset, stripped);
                                }
                            }
                            Err(e) => {
                                if let Some(pw) = self.system_event_window_mut() {
                                    pw.chart.push_system_event(
                                        mlc_core::SystemEventKind::Share,
                                        "Clone failed",
                                        format!("could not read cloned preset: {}", e),
                                        true,
                                    );
                                }
                            }
                        }
                    }
                    mlc_updater::ShareOpenState::Failed { message } => {
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(mlc_core::SystemEventKind::Share, "Share failed", message, true);
                        }
                    }
                    mlc_updater::ShareOpenState::Idle | mlc_updater::ShareOpenState::Working => {}
                }
            }
            // Boot-time reopen outcome (shared-tab-reload-anchor arc,
            // 2026-09-23) — edge-triggered same as `share_open_rx` above,
            // but routed by `placeholder_id` to WHICHEVER window actually
            // holds that tab (`ChartApp::has_open_tab`) rather than
            // `system_event_window_mut()`'s "one window speaks for the
            // profile" pick: presets live in the shared `BackendCore`, but
            // `open_tabs`/`readonly_preset_ids` are per-window `ChartApp`
            // fields, so the outcome must land on the SAME window that
            // installed the placeholder.
            if self.updater_handle.share_reopen_rx.has_changed().unwrap_or(false) {
                let outcome = self.updater_handle.share_reopen_rx.borrow_and_update().clone();
                match outcome {
                    mlc_updater::ShareReopenState::Opened { placeholder_id, meta, preset_json } => {
                        let outcome: Result<(), String> = if meta.kind != "live" {
                            Err("a screenshot share has no chart to re-anchor".to_string())
                        } else {
                            match preset_json {
                                None => Err("server sent no preset for a live share".to_string()),
                                Some(value) => match serde_json::from_value::<mlc_core::preset::preset::ChartPreset>(value) {
                                    Err(e) => Err(format!("could not read shared preset: {}", e)),
                                    Ok(preset) => {
                                        let shared_meta = mlc_app::SharedChartMeta {
                                            // `fill_shared_placeholder` recovers the real
                                            // token from its own `pending_share_tokens`
                                            // (`ShareReopenState::Opened` doesn't carry
                                            // one — see that variant's doc).
                                            token: String::new(),
                                            title: meta.title,
                                            owner_label: meta.owner_label,
                                            kind: meta.kind,
                                            rights: meta.rights,
                                            expires_at: meta.expires_at,
                                        };
                                        match self.windows.values_mut().find(|pw| pw.chart.has_open_tab(&placeholder_id)) {
                                            None => Ok(()), // tab already closed this session — nothing to fill
                                            Some(pw) => pw.chart.fill_shared_placeholder(&placeholder_id, shared_meta, preset).map(|_| ()),
                                        }
                                    }
                                },
                            }
                        };
                        if let Err(message) = outcome {
                            warn!("[ChartApp] reopen_share '{}' failed to apply: {}", placeholder_id, message);
                            if let Some(pw) = self.windows.values_mut().find(|pw| pw.chart.has_open_tab(&placeholder_id)) {
                                pw.chart.share_reopen_gone(&placeholder_id);
                            }
                        }
                    }
                    mlc_updater::ShareReopenState::Gone { placeholder_id } => {
                        if let Some(pw) = self.windows.values_mut().find(|pw| pw.chart.has_open_tab(&placeholder_id)) {
                            pw.chart.share_reopen_gone(&placeholder_id);
                        }
                    }
                    mlc_updater::ShareReopenState::Transient { placeholder_id, token, message } => {
                        warn!("[ChartApp] reopen_share '{}' transient failure: {}", placeholder_id, message);
                        if let Some(pw) = self.windows.values_mut().find(|pw| pw.chart.has_open_tab(&placeholder_id)) {
                            pw.chart.share_reopen_transient(&placeholder_id, &token);
                        }
                    }
                    mlc_updater::ShareReopenState::Idle => {}
                }
            }
            // Forum board state — edge-triggered same as `redeem_rx`/
            // `nick_rx` above, held as the raw `mlc_updater::ForumState`
            // (not mapped into a UI shape here) because interpreting it
            // needs each window's OWN `forum_view` for context — see the
            // per-window apply block below for why. `busy` answers are
            // skipped: this shell does not model an intermediate
            // "Working" view for the Forum tab (the optimistic `loading`/
            // `posting`/`posting_reply` flags the click handlers already
            // set serve that purpose), so only a SETTLED answer is worth
            // applying.
            #[cfg(feature = "cloud")]
            let forum_state: Option<mlc_updater::ForumState> =
                if self.updater_handle.forum_rx.has_changed().unwrap_or(false) {
                    let fs = self.updater_handle.forum_rx.borrow_and_update().clone();
                    if fs.busy { None } else { Some(fs) }
                } else {
                    None
                };
            // Cloud sync status → system event (owner ask 2026-09-05, scope
            // §8; recon: "sync_status_rx ... no shell reads it" — finally
            // drained). Edge-triggered same as `redeem_rx`/`nick_rx` above.
            // Only facts a person needs to react to surface here —
            // `Idle`/`Syncing`/`Completed` are routine chatter, not events.
            if self.updater_handle.sync_status_rx.has_changed().unwrap_or(false) {
                let event: Option<(&'static str, String, bool)> =
                    match &*self.updater_handle.sync_status_rx.borrow_and_update() {
                        mlc_updater::SyncStatus::Error(message) => {
                            Some(("Cloud sync error", message.clone(), true))
                        }
                        mlc_updater::SyncStatus::ConflictsDetected(conflicts) => Some((
                            "Cloud sync conflicts",
                            format!("{} item(s) changed on both this device and the cloud.", conflicts.len()),
                            true,
                        )),
                        mlc_updater::SyncStatus::NeedsSetup => Some((
                            "Cloud data found",
                            "This account has cloud data from another device — open sync settings to decide.".to_string(),
                            true,
                        )),
                        mlc_updater::SyncStatus::ProfileRestored { profile_id } => Some((
                            "Profile restored",
                            format!("Cloud profile {profile_id} was downloaded to this device."),
                            true,
                        )),
                        mlc_updater::SyncStatus::ProfileRestoreError(message) => {
                            Some(("Profile restore failed", message.clone(), true))
                        }
                        _ => None,
                    };
                if let Some((title, body, toast)) = event {
                    if let Some(pw) = self.system_event_window_mut() {
                        pw.chart.push_system_event(mlc_core::SystemEventKind::Sync, title, body, toast);
                    }
                }
            }
            // `sync_checksums_rx` carries `last_synced_checksums` — internal
            // diff bookkeeping consumed by the NEXT sync cycle, not a fact a
            // person recognizes (no meaningful title/body) — deliberately
            // left undrained here. Wiring it into
            // `profile_manager.profile.sync_state` persistence (per its own
            // doc comment) is a separate, non-event concern outside this
            // arc's scope.
            for pw in self.windows.values_mut() {
                pw.chart.panel_app.user_settings_state.multi_profile = multi_profile;
                #[cfg(feature = "cloud")]
                {
                    let st = &mut pw.chart.panel_app.cloud_login_state;
                    st.is_paid = is_paid;
                    st.paid_until = paid_until.clone();
                    pw.chart.panel_app.cloud_overlay_state.forum_entitled = forum_entitled;
                }
                // `redeem_status` lives on `UserSettingsState` now (Account
                // tab, not the login overlay) — NOT `cfg(feature =
                // "cloud")`-gated, same as `nick_status` below.
                if let Some(status) = redeem_status.as_ref() {
                    use mlc_app_layout::ui::modal_settings::RedeemStatus;
                    let redeemed = matches!(status, RedeemStatus::Redeemed { .. });
                    pw.chart.panel_app.user_settings_state.redeem_status = status.clone();
                    // Redeeming changes the ladder's `paid` class too (owner
                    // ask 2026-09-23: "refresh the tier/role line ...
                    // immediately") — same `invalidate_me` edge a saved nick
                    // already triggers below, so `account_classes`/
                    // `account_roles` re-fetch instead of waiting out the
                    // passive cadence. Entitlements (`is_paid`/`paid_until`
                    // above) refresh on their own: `mlc_updater`'s own
                    // `RedeemCode` handler re-polls them immediately on
                    // success.
                    #[cfg(feature = "cloud")]
                    if redeemed {
                        pw.chart.panel_app.community_state.forum2.invalidate_me();
                    }
                    pw.overlay_dirty = true;
                    pw.window.request_redraw();
                }
                if let Some(status) = nick_status.as_ref() {
                    use mlc_app_layout::ui::modal_settings::NickStatus;
                    let saved = matches!(status, NickStatus::Saved { .. });
                    let uss = &mut pw.chart.panel_app.user_settings_state;
                    uss.nick_status = status.clone();
                    if saved {
                        uss.nick_edit_mode = false;
                        uss.nick_edit_focused = false;
                    }
                    // A saved nick invalidates the `Me` cache the forum/DM
                    // "choose a nick" gate reads (owner mandate 2026-09-09)
                    // — without this, `caller_nick` would keep answering
                    // "none" until the next passive refresh cadence.
                    #[cfg(feature = "cloud")]
                    if saved {
                        pw.chart.panel_app.community_state.forum2.invalidate_me();
                    }
                    pw.overlay_dirty = true;
                    pw.window.request_redraw();
                }
                // Forum board — apply the settled `forum_state` answer onto
                // whichever sub-view this window's overlay is CURRENTLY
                // showing. The updater's `ForumState` does not tag which
                // request it answers (unlike the web shell's per-action
                // `FlowResult` variants), so the target is inferred from the
                // optimistic flag each click handler already set before
                // firing the request (`loading`/`posting`/`posting_reply`)
                // — the same trick `panel_click.rs` uses to show "Working"
                // immediately. One imprecision this inference cannot avoid:
                // a failed `DeleteThread` looks identical to a failed
                // `Reply`/`DeleteReply` (thread stays loaded, an error
                // appears), so it also lands in `reply_error` rather than a
                // dedicated slot — the message text itself still names what
                // went wrong, only the highlighted row is generic.
                #[cfg(feature = "cloud")]
                if let Some(fs) = forum_state.as_ref() {
                    use mlc_app_layout::ui::modal_settings::{ForumListState, ForumSubView};
                    let mapped_thread = fs.thread.as_ref().map(map_forum_thread_native);
                    // Silent background thread refresh (30s cadence, fired
                    // below alongside the list's own) — routed BEFORE the
                    // optimistic-flag dispatch further down since the view
                    // was never touched when this request went out (unlike
                    // every other Forum action, which sets
                    // `loading`/`posting`/`posting_reply` before firing), so
                    // the answer landing must not touch the view beyond the
                    // thread payload itself.
                    let ov = &mut pw.chart.panel_app.cloud_overlay_state;
                    if ov.is_open && ov.forum_thread_refreshing {
                        ov.forum_thread_refreshing = false;
                        if let (None, Some(t)) = (&fs.last_error, &mapped_thread) {
                            let is_same_thread = matches!(
                                &ov.forum_view,
                                ForumSubView::Thread { thread: Some(existing), .. } if existing.id == t.id
                            );
                            if is_same_thread {
                                if let ForumSubView::Thread { thread, .. } = &mut ov.forum_view {
                                    *thread = Some(t.clone());
                                }
                                ov.mark_forum_thread_fetched();
                            }
                        }
                        // Failure, or the caller switched threads/left the
                        // view while this was in flight: leave the view
                        // exactly as it is — same tolerance the list's own
                        // background refresh gets below.
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    } else if ov.is_open {
                        match &mut ov.forum_view {
                            ForumSubView::Compose { posting: true, .. } => {
                                match (&fs.last_error, &mapped_thread) {
                                    (None, Some(t)) => {
                                        ov.forum_view = ForumSubView::Thread {
                                            thread: Some(t.clone()),
                                            loading: false,
                                            error: None,
                                            posting_reply: false,
                                            reply_error: None,
                                        };
                                        ov.mark_forum_thread_fetched();
                                        // A new thread now exists — invalidate
                                        // the cached list page so Back
                                        // re-fetches rather than showing a
                                        // stale page missing it.
                                        ov.invalidate_forum_list();
                                        // Compose form is gone — clear its
                                        // fields so a later "New thread" does
                                        // not resurrect this post's text.
                                        let mut coord = pw.chart.input_coordinator.borrow_mut();
                                        let tf = coord.text_fields_mut();
                                        let title_wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_TITLE);
                                        let body_wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_BODY);
                                        if !tf.text(&title_wid).is_empty() {
                                            tf.clear(&title_wid);
                                        }
                                        if !tf.text(&body_wid).is_empty() {
                                            tf.clear(&body_wid);
                                        }
                                    }
                                    (Some(e), _) => {
                                        if let ForumSubView::Compose { posting, error } = &mut ov.forum_view {
                                            *posting = false;
                                            *error = Some(e.clone());
                                        }
                                    }
                                    (None, None) => {}
                                }
                            }
                            ForumSubView::Thread { loading: true, thread: None, .. } => {
                                match (&fs.last_error, &mapped_thread) {
                                    (None, Some(t)) => {
                                        ov.forum_view = ForumSubView::Thread {
                                            thread: Some(t.clone()),
                                            loading: false,
                                            error: None,
                                            posting_reply: false,
                                            reply_error: None,
                                        };
                                        ov.mark_forum_thread_fetched();
                                    }
                                    (err, _) => {
                                        ov.forum_view = ForumSubView::Thread {
                                            thread: None,
                                            loading: false,
                                            error: Some(err.clone().unwrap_or_else(|| "Thread not found".to_string())),
                                            posting_reply: false,
                                            reply_error: None,
                                        };
                                    }
                                }
                            }
                            ForumSubView::Thread { posting_reply: true, .. } => {
                                match (&fs.last_error, &mapped_thread) {
                                    (None, Some(t)) => {
                                        let clear_reply = ov.forum_reply_pending;
                                        ov.forum_reply_pending = false;
                                        ov.forum_view = ForumSubView::Thread {
                                            thread: Some(t.clone()),
                                            loading: false,
                                            error: None,
                                            posting_reply: false,
                                            reply_error: None,
                                        };
                                        ov.mark_forum_thread_fetched();
                                        // The reply (or delete) landed — the
                                        // list's `reply_count`/`last_reply_at`
                                        // for this thread are now stale.
                                        ov.invalidate_forum_list();
                                        if clear_reply {
                                            let mut coord = pw.chart.input_coordinator.borrow_mut();
                                            let tf = coord.text_fields_mut();
                                            let wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_REPLY);
                                            if !tf.text(&wid).is_empty() {
                                                tf.clear(&wid);
                                            }
                                        }
                                    }
                                    // Thread cleared with no error — a
                                    // `DeleteThread` succeeded. Back to the
                                    // list, refreshed from the same answer.
                                    (None, None) => {
                                        ov.forum_reply_pending = false;
                                        ov.forum_thread_fetched_at = None;
                                        ov.forum_view = ForumSubView::List;
                                        ov.forum_list = ForumListState::Loaded {
                                            threads: fs.list.iter().map(map_forum_summary_native).collect(),
                                            next_before: fs.next_before.clone(),
                                            loading_more: false,
                                        };
                                        ov.mark_forum_list_fetched();
                                    }
                                    (Some(e), _) => {
                                        ov.forum_reply_pending = false;
                                        if let ForumSubView::Thread { posting_reply, reply_error, .. } = &mut ov.forum_view {
                                            *posting_reply = false;
                                            *reply_error = Some(e.clone());
                                        }
                                    }
                                }
                            }
                            ForumSubView::List => {
                                if matches!(ov.forum_list, ForumListState::Loading) {
                                    ov.forum_list = match &fs.last_error {
                                        None => ForumListState::Loaded {
                                            threads: fs.list.iter().map(map_forum_summary_native).collect(),
                                            next_before: fs.next_before.clone(),
                                            loading_more: false,
                                        },
                                        Some(e) => ForumListState::Error { message: e.clone() },
                                    };
                                    if fs.last_error.is_none() {
                                        ov.mark_forum_list_fetched();
                                    }
                                } else if ov.forum_list_refreshing {
                                    ov.forum_list_refreshing = false;
                                    if fs.last_error.is_none() {
                                        ov.forum_list = ForumListState::Loaded {
                                            threads: fs.list.iter().map(map_forum_summary_native).collect(),
                                            next_before: fs.next_before.clone(),
                                            loading_more: false,
                                        };
                                        ov.mark_forum_list_fetched();
                                    }
                                    // Failure: the existing page stays up and
                                    // usable — same tolerance load-more
                                    // failures get below.
                                } else if let ForumListState::Loaded { threads, next_before, loading_more } =
                                    &mut ov.forum_list
                                {
                                    if *loading_more {
                                        match &fs.last_error {
                                            None => {
                                                threads.extend(fs.list.iter().map(map_forum_summary_native));
                                                *next_before = fs.next_before.clone();
                                            }
                                            // Load-more failure: the existing
                                            // page stays up and usable — no
                                            // dedicated UI slot for this.
                                            Some(_) => {}
                                        }
                                        *loading_more = false;
                                    }
                                }
                            }
                            _ => {}
                        }
                        pw.overlay_dirty = true;
                        pw.window.request_redraw();
                    }
                }
            }

            // Global scale_mode persistence channel removed entirely
            // (viewport SWC arc, 2026-07-03) — `core.user()` no longer
            // carries a scale_mode at all; every window seeds Auto on
            // fresh load and cold-load preset restore forces Auto by
            // design (see hydration_impl.rs). recalc_mode still needs a
            // one-way push into IndicatorManager's own operational copy —
            // that field drives the
            // manager's internal dirty/recalc scheduling and is a legitimate
            // second copy (ONE core.user() value kept in sync with the ONE
            // IndicatorManager, not an N-window duplication).
            let recalc_mode = self.core.user().recalc_mode();
            self.core.indicators_mut().set_recalc_mode(recalc_mode);

            // Drain the shared broadcast channel exactly ONCE per frame —
            // every window's `core` is the same `Arc<BackendCore>` (Phase A),
            // so draining per-window here would each only see a fraction of
            // the events. The resulting slice is handed to every open
            // window's `react_to_events`, which runs everything `tick` used
            // to run internally (viewport/indicator/panel reactions, alert
            // checking, scroll-fetch, etc) minus the drain itself.
            let frame_time = screenshot::now_ms();
            let mut events = self.core.tick_writers(frame_time);
            // Budget-sliced catalogue ingestion (owner ruling 2026-09-05,
            // wasm-lag-measurements doc): `tick_writers`' `SymbolsLoaded` arm
            // only enqueues; this walks the pending queue a few symbols at a
            // time so a large catalogue (Deribit, 11k+ symbols) never lands
            // inside one frame. Native gets a larger budget than wasm — see
            // `CATALOGUE_INGEST_BUDGET_NATIVE`'s doc comment.
            events.extend(self.core.ingest_catalogues(mlc_backend::CATALOGUE_INGEST_BUDGET_NATIVE));
            for pw in self.windows.values_mut() {
                // Skip tick for minimized windows — no point updating viewport
                // or processing trades for an invisible surface.  On restore,
                // snap_to_end repositions the viewport to the latest bars.
                if pw.was_minimized {
                    continue;
                }
                pw.chart.react_to_events(&events, frame_time);
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
                        .map(|at| at.to_string()).unwrap_or_else(|| "S".to_string());
                    let tf = active_cid_rc
                        .and_then(|cid| pw.chart.timeframe_for_chart(cid))
                        .unwrap_or_else(mlc_core::state::Timeframe::h1);
                    if sym.is_empty() && active_cid_rc.is_none() {
                        continue;
                    }
                    (sym, exch, at, tf, pw.chart.core.user().profile().domain.bar_count as usize)
                };

                // disk persistence removed — delete_storage flag is a no-op now
                let _ = delete_storage;

                // Clear bars on the window and mark pending load.
                if let Some(window) = pw.chart.panel_app.panel_grid.active_container_mut() {
                    window.begin_fresh_load();
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
                        let account_type_typed: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
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
                                    && pw.chart.account_type_for_chart(pid).map_or(false, |a| a == account_type_typed)
                                })
                                .copied()
                                .collect()
                        };
                        for peer_id in peers_matching {
                            if let Some(peer_window) = pw.chart.panel_app.panel_grid.containers_mut().get_mut(&peer_id) {
                                // Phase X0 / C5c: storage on bubble; window cleared via viewport reset only.
                                peer_window.begin_fresh_load();
                                info!(
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
                    self.core.request_kline(eid, &symbol, &timeframe, at, None, Some(bar_count), true);
                }

                info!(
                    "[App] Reset {} for {}:{} {} ({})",
                    if delete_storage { "storage+cache" } else { "cache" },
                    symbol, exchange, timeframe.name, account_type
                );
            }
        }

        let _t5 = web_time::Instant::now();

        // ── Populate performance panel data ─────────────────────────────────
        // Only run when the merged Monitoring panel (which now hosts the
        // Performance section, merged 2026-07-18) is actually visible —
        // avoids per_core_cpu.clone() + gpu_name.clone() + ~25 field writes
        // every frame. CRITICAL: this must track `RightSidebarPanel::Monitoring`
        // — if it silently reverts to matching the old `Performance` variant
        // (which no longer exists), this gate never fires again and the
        // native Performance-data writer stops populating fields.
        let perf_visible = self.windows.values().any(|pw| {
            pw.chart.sidebar_state.is_right_open()
                && pw.chart.sidebar_state.right_panel
                    == mlc_app_sidebar_content::state::RightSidebarPanel::Monitoring
        });
        if perf_visible {
            let ws_connections = self.bridge.ws_task_count_total();
            let window_count = self.windows.len();
            // Use the cached connector count — refreshed once per second below.
            let active_connectors = self.cached_connector_count;
            let fps_limit = self.fps_limit;
            let fps = self.fps_ema;
            let frame_time_ms = self.last_frame_time_ms;
            let recalc_label = format!("{:?}", self.core.user().recalc_mode());
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
                    .map(|w| pw.chart.core.bubbles().bars(w.current_tag_id).len())
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

        // ── Drain alert-delivery toast notifications into the system-event
        // bus ───────────────────────────────────────────────────────────────
        // (toast restyle-and-anchor follow-up to §8, owner ask 2026-09-09):
        // `toast=true` is what used to be a second, ad-hoc `active_toasts`
        // push — `ChartApp::refresh_active_toasts` (driven every frame from
        // `render_modal_layer`) is now the ONE toast queue, so an alert
        // delivery is just another bus entry that happens to carry `toast=
        // true`, same as e.g. the "Signed out" account event above.
        let mut drained_alert_toasts: Vec<mlc_alert_delivery::ToastNotification> = Vec::new();
        if let Some(ref mut rx) = self.toast_rx {
            while let Ok(toast) = rx.try_recv() {
                drained_alert_toasts.push(toast);
            }
        }
        for toast in drained_alert_toasts {
            if let Some(pw) = self.system_event_window_mut() {
                pw.chart.push_system_event(
                    mlc_core::SystemEventKind::Alert,
                    toast.title,
                    toast.message,
                    true,
                );
            }
        }
        // ── Drain memory-watchdog warnings into the system-event bus ───────────
        // (owner ask 2026-09-05, scope §8; recon: "memory warnings are
        // tracing-log-only, never surfaced to the user" — `mem_warnings_rx`
        // was bound as `_mem_warnings` and never drained before this arc.)
        // `toast=false`: a growing-RAM warning is exactly the kind of fact
        // that belongs in history without demanding attention mid-session.
        while let Ok(warning) = self.mem_warnings_rx.try_recv() {
            let (title, available_mb, total_mb) = match warning {
                mlc_shell_diagnostics::watchdog::MemoryWarning::Low { available_mb, total_mb } => {
                    ("Memory low", available_mb, total_mb)
                }
                mlc_shell_diagnostics::watchdog::MemoryWarning::Critical { available_mb, total_mb } => {
                    ("Memory critical", available_mb, total_mb)
                }
                mlc_shell_diagnostics::watchdog::MemoryWarning::Emergency { available_mb, total_mb } => {
                    ("Memory emergency", available_mb, total_mb)
                }
            };
            let body = format!("{available_mb} MB available of {total_mb} MB total.");
            if let Some(pw) = self.system_event_window_mut() {
                pw.chart.push_system_event(mlc_core::SystemEventKind::Memory, title, body, false);
            }
        }

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
                // Arm instead of setting `screenshot_pending` directly: the
                // GPU thread is one tick behind scene build (see
                // `screenshot_armed`'s doc), so the scene it is about to
                // capture right now was built BEFORE this click landed.
                // Force a full rebuild on the very next `build_window_scene`
                // pass too — the toolbar/chart/overlay sub-scenes are
                // cached, and a stale one built before the request could
                // still carry an already-painted tooltip/dropdown/preview.
                pw.screenshot_armed = true;
                pw.chart_dirty = true;
                pw.overlay_dirty = true;
                pw.toolbar_dirty = true;
            }

            // Camera button active-phase expiry (owner ruling 2026-09-05):
            // the toolbar strip is cached behind `toolbar_dirty`, and
            // nothing else invalidates it purely on the passage of time —
            // compare against the last value seen so the strip repaints
            // exactly once when `screenshot_active()` flips false, instead
            // of visibly staying "active" past its ~1s window.
            {
                let screenshot_active_now = pw.chart.panel_app.toolbar_state.screenshot_active();
                if screenshot_active_now != pw.screenshot_active_prev {
                    pw.screenshot_active_prev = screenshot_active_now;
                    pw.toolbar_dirty = true;
                    pw.window.request_redraw();
                }
            }
            // Unread system-events badge edge check (owner scope §8) — same
            // "cached strip, no click to invalidate it" gap as the block above.
            {
                let unread_now = pw.chart.unread_count() > 0;
                if unread_now != pw.unread_events_prev {
                    pw.unread_events_prev = unread_now;
                    pw.toolbar_dirty = true;
                    pw.window.request_redraw();
                }
            }

            // Chart-sharing arc: the toolbar "share_chart" button sets this
            // via `ChartOutEvent::ShareLiveChart`. Mirrors `mlc-app-shell-
            // web`'s own drain (`crates/mlc-app-shell-web/src/lib.rs:2090`)
            // — the registry preset is the restorable skeleton already
            // (`bars` is `skip_serializing`), and `strip_preset_for_sync`
            // runs over the serialized JSON anyway as the same belt-and-
            // braces the sync path applies to on-disk presets that still
            // carry `bars` from before that field existed. The upload
            // itself, and the clipboard write on success, happen in the
            // `share_rx` apply block below (a request here is fire-and-
            // forget from this call site's point of view).
            if pw.chart.drain_pending_share_live() {
                let id = pw.chart.panel_app.active_preset_id.clone();
                match self.core.presets().get(&id) {
                    Some(preset) => {
                        let title = if preset.name.is_empty() { "chart".to_string() } else { preset.name.clone() };
                        match serde_json::to_string(preset) {
                            Ok(raw) => {
                                let stripped = mlc_updater::cloud_sync::strip_preset_for_sync(&raw);
                                match serde_json::from_str::<serde_json::Value>(&stripped) {
                                    Ok(preset_json) => {
                                        let _ = self.updater_handle.cmd_tx.send(
                                            mlc_updater::UpdaterCommand::ShareLive { title, preset_json },
                                        );
                                    }
                                    Err(e) => warn!("[share] stripped preset failed to re-parse: {}", e),
                                }
                            }
                            Err(e) => warn!("[share] preset serialize failed: {}", e),
                        }
                    }
                    None => warn!("[share] live share requested but no active preset"),
                }
            }

            // Chart-sharing arc, screenshot leg: `gpu_submit`'s
            // `ScreenshotDestination::ShareLink` capture branch has no
            // `UpdaterHandle` (it runs off a bare `&mut PerWindowState`),
            // so it stages the PNG bytes here instead of sending the
            // command itself.
            if let Some((title, png)) = pw.pending_share_screenshot.take() {
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::ShareScreenshot { title, png });
            }

            // Native share-viewer arc: open/clone requests queued by
            // `ChartApp::request_open_share`/`request_clone_share` (the
            // deep-link drain above, or a future toolbar/Agent-API caller)
            // — same "ChartApp does no network IO itself" hand-off the
            // `drain_pending_share_live` block above already makes.
            // `ShareOpenState::Opened` carries no token of its own (one-shot
            // mailbox, mirroring `ShareState`), so the bare token is
            // remembered here for the apply block below to read back.
            if let Some(raw) = pw.chart.take_pending_open_share() {
                let token = extract_share_token(&raw);
                self.pending_open_share_token = Some(token.clone());
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::OpenShare { token });
            }
            if let Some(raw) = pw.chart.take_pending_clone_share() {
                let token = extract_share_token(&raw);
                let _ = self
                    .updater_handle
                    .cmd_tx
                    .send(mlc_updater::UpdaterCommand::CloneShare { token });
            }

            // Boot-time reopen queue (shared-tab-reload-anchor arc,
            // 2026-09-23): `ChartApp::new_window`'s restore path seeded a
            // placeholder + queue entry per shared tab that survived a
            // reload at its saved slot — fire one `ReopenShare` per entry,
            // same "ChartApp does no network IO itself" hand-off as the two
            // blocks above. The reply lands on `share_reopen_rx` below and
            // is routed back by `placeholder_id`, not by window, so it does
            // not matter that this loop iterates every window every frame.
            for pending in pw.chart.take_pending_reopen_shares() {
                let _ = self.updater_handle.cmd_tx.send(mlc_updater::UpdaterCommand::ReopenShare {
                    token: pending.token,
                    placeholder_id: pending.placeholder_id,
                });
            }

            // Tooltip tick — fires when cursor is stationary (no CursorMoved events)
            {
                let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                let chrome_was = pw.chrome_state.tooltip.is_visible();
                chrome::update_tooltip(&mut pw.chrome_state, pw.last_mouse_pos.0, pw.last_mouse_pos.1, time_ms);
                let toolbar_was = pw.toolbar_tooltip.is_visible();
                let dropdown_open = pw.chart.panel_app.toolbar_state.has_open_dropdown();
                pw.toolbar_tooltip.update(pw.toolbar_tooltip.hovered_widget().cloned(), time_ms, dropdown_open);
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
                let metrics = self.core.agent_metrics_snapshot();
                self.cached_connector_count = metrics.len();
            }

            // Refresh the telemetry snapshot from App's own single-threaded
            // state — unconditional (unlike `PerformanceData` below, which
            // only updates while the monitoring panel is open and would
            // otherwise send telemetry full of zeros). `total_bars` sums the
            // same per-window bubble count the perf panel computes, just
            // aggregated across every open window instead of one.
            {
                let total_bars: u64 = self
                    .windows
                    .values()
                    .map(|pw| {
                        pw.chart
                            .panel_app
                            .panel_grid
                            .containers()
                            .values()
                            .map(|w| pw.chart.core.bubbles().bars(w.current_tag_id).len() as u64)
                            .sum::<u64>()
                    })
                    .sum();
                self.telemetry.update_periodic(
                    self.fps_ema,
                    self.cached_connector_count,
                    self.windows.len(),
                    total_bars,
                );
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
                    match self.agent_snapshot_phase % 5 {
                        0 => self.update_indicator_snapshot(&agent_state),
                        1 => self.update_terminal_snapshot(&agent_state),
                        2 => self.update_watchlist_snapshot(&agent_state),
                        3 => self.update_connector_snapshot(&agent_state),
                        _ => self.update_ui_state_snapshot(&agent_state),
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
                        error!("[App] GPU render thread channel closed unexpectedly");
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
                    //
                    // The bars test was `.map(|_w| false)` from 7fc33a90
                    // (2026-06-19) until 2026-09-02 — the Phase X0/C5c
                    // refactor moved per-Kind series storage out of
                    // `ChartContainer` into `DataBubble`, `has_data()` went
                    // with it, and the call site was stubbed to a constant
                    // rather than rewritten. A constant `false` cannot ever
                    // satisfy the middle arm, so for two and a half months
                    // EVERY non-skeleton window was gated on nothing but the
                    // 4s timeout: a returning user's main window sat
                    // invisible for four seconds with its bars already in
                    // hand. The promote path never showed the symptom
                    // because it sets `visible_set` itself.
                    //
                    // `bubbles().bars(current_tag_id)` is how the rest of
                    // this file has asked the same question since that
                    // refactor (see the perf panel's `total_bars` sum).
                    let ready = pw.skeleton
                        || pw.chart
                            .panel_app
                            .panel_grid
                            .active_container()
                            .map(|w| !pw.chart.core.bubbles().bars(w.current_tag_id).is_empty())
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
        //
        // `self.mlc_alert_delivery.is_some()` (a bool check, no live borrow)
        // gates the whole block instead of `if let Some(ref delivery) = ...`
        // — the sealed-notice branch below needs `self.system_event_window_
        // mut()` (`&mut self`), which cannot run while `delivery` still
        // holds an immutable borrow of `self.mlc_alert_delivery`. The actual
        // `&self.mlc_alert_delivery` borrow for `.deliver()` is taken fresh,
        // after that branch has already returned it.
        if self.mlc_alert_delivery.is_some() {
            let mut events: Vec<mlc_alert_delivery::DeliveryEvent> = Vec::new();
            for pw in self.windows.values_mut() {
                events.extend(pw.chart.pending_delivery_events.drain(..));
            }
            if !events.is_empty() {
                // `alert_secrets_store.get()` is the literal "delivery reads
                // the token from the sidecar" check (alerts-account-wide-
                // and-telegram-delivery plan §3) — `delivery.deliver` below
                // already no-ops on an empty bot token regardless, so this
                // gates only the user-facing notice, once per seal rather
                // than once per tick: `alert_delivery_seal_notice_shown`
                // resets the moment the store is next observed unlocked.
                if self.alert_secrets_store.get().is_err() {
                    if !self.alert_delivery_seal_notice_shown {
                        self.alert_delivery_seal_notice_shown = true;
                        if let Some(pw) = self.system_event_window_mut() {
                            pw.chart.push_system_event(
                                mlc_core::SystemEventKind::Alert,
                                "Unlock the vault to deliver alerts",
                                "An alert fired while the vault was locked — Telegram delivery is skipped until you unlock it.",
                                true,
                            );
                        }
                    }
                } else {
                    self.alert_delivery_seal_notice_shown = false;
                }
                if let Some(ref delivery) = self.mlc_alert_delivery {
                    for event in events {
                        delivery.deliver(event);
                    }
                }
            }

            // Drain per-attempt Telegram delivery outcomes — independent of
            // whether THIS tick fired any new events: a previous tick's
            // `deliver()` call resolves asynchronously, often several
            // frames later (`alert_outcome_rx`'s own doc).
            if let Some(ref mut rx) = self.alert_outcome_rx {
                let mut outcomes: Vec<mlc_alert_delivery::DeliveryOutcome> = Vec::new();
                while let Ok(outcome) = rx.try_recv() {
                    outcomes.push(outcome);
                }
                for outcome in outcomes {
                    let (title, body, toast) = match outcome {
                        mlc_alert_delivery::DeliveryOutcome::TelegramDelivered { alert_name } => {
                            ("Delivered to Telegram".to_string(), alert_name, false)
                        }
                        mlc_alert_delivery::DeliveryOutcome::TelegramFailed { alert_name, reason } => {
                            (format!("Delivery failed: {}", reason), alert_name, true)
                        }
                    };
                    if let Some(pw) = self.system_event_window_mut() {
                        pw.chart.push_system_event(mlc_core::SystemEventKind::Alert, title, body, toast);
                    }
                }
            }
        }

        // Collect mutable references to non-minimized windows.
        let mut window_refs: Vec<&mut PerWindowState> = self
            .windows
            .values_mut()
            .filter(|pw| !pw.window.is_minimized().unwrap_or(false))
            .collect();

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
                let _ = build_window_scene(pw, frame_time);
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
                        build_window_scene(pw_ref, frame_time)
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
                debug!(
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
            debug!(
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
                debug!(
                    "[App] deferred background-tab warm: {} target(s), one per frame",
                    self.pending_warm_targets.len(),
                );
            }
        } else if let Some(target) = self.pending_warm_targets.pop() {
            let t_warm = web_time::Instant::now();
            if let Some(pw) = self.windows.values_mut().find(|pw| !pw.skeleton) {
                pw.chart.warm_one_background_tab(&target);
            }
            debug!(
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
                debug!(
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
        for pw in self.windows.values_mut() {
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
            // The clock's hover animation is a real animation and needs the
            // same treatment as the spinners above. Native only repaints on
            // events, so with the pointer resting on the clock and not
            // moving there is no next frame at all — measured on the built
            // binary, three consecutive captures came back byte-identical
            // while the web stand (which repaints continuously) animated
            // fine. Without this the effect only advances while the mouse is
            // in motion, which reads as broken rather than as still.
            let clock_hovered = pw
                .chart
                .panel_app
                .toolbar_state
                .hovered_bottom_toolbar_id
                .as_deref()
                == Some("clock")
                // With the effect off there is nothing to animate — the
                // toolbar draws the plain `fill_text` path either way, so
                // holding the window awake here would just burn frames for
                // no visible change (clock popup's third checkbox, owner ask
                // 2026-09-02).
                && pw.chart.core.user().profile().ui.clock_glitch_enabled;
            if clock_hovered {
                // The glitch is painted by the toolbar pass, so the chart
                // alone being clean is not enough to skip it.
                pw.toolbar_dirty = true;
            }
            // One-shot magic-hat animation on the right toolbar's "agents"
            // button (owner ruling 2026-09-10: tab open/close, nothing
            // loops, nothing redraws while idle). Same time-boxed pulse
            // shape as `clock_hovered` above, but gated on wall-clock
            // elapsed time instead of pointer position — keep redrawing
            // only while the current trigger is still inside its own
            // duration, then let it lapse on its own (`MascotAnim::
            // is_live`), never an unconditional per-frame repaint.
            let mascot_animating = pw
                .chart
                .panel_app
                .toolbar_state
                .mascot_anim
                .map(|anim| anim.is_live())
                .unwrap_or(false);
            if mascot_animating {
                // The hat is painted by the toolbar pass too.
                pw.toolbar_dirty = true;
            }
            // Modal wheel-scroll ease (owner ask 2026-09-23: "сделать скролл
            // колёсиком менее чувствительным") — same time-boxed-pulse shape
            // as the two flags above: keep redrawing only while some modal's
            // `ScrollState` is still catching up to its wheel target, then
            // let it settle on its own (`ScrollState::is_animating`). Modals
            // are painted by the chart-content pass (`chart_dirty`), not the
            // toolbar or overlay passes — see scene_builder.rs's "Chart
            // content (chart panels, modals)" comment.
            let modal_scroll_animating = mlc_app::scroll_dispatch::any_modal_scroll_animating(&pw.chart);
            if modal_scroll_animating {
                pw.chart_dirty = true;
            }
            if spinner_up || per_chart_loading || cold_apply_stashed || clock_hovered || mascot_animating
                || modal_scroll_animating
            {
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

/// Parse a native share deep link off argv — `mylittlechart://share/<token>`
/// or a bare `https://chart.example/?share=<token>` passed as a whole
/// argument (native-share-viewer arc, `docs/mlc/plans/native-share-viewer-
/// and-chart-rights-2026-09-06.md` §1). Checked next to `--wait-pid` because
/// both are OS-launch-time inputs read before any window exists — unlike
/// `--wait-pid` this one is a single self-contained argument, not a
/// `--flag value` pair, so a plain `find` over `std::env::args()` is enough.
///
/// v1 gap, not solved here (per the plan): the single-instance mutex below
/// exits this whole process before this function's result is ever applied
/// when another instance is already running, so a link clicked while the
/// app is already open is silently dropped — there is no IPC to a running
/// instance yet (a pipe relay is a follow-up).
fn parse_deep_link_share_argv() -> Option<String> {
    std::env::args().find(|a| a.starts_with("mylittlechart://share/") || a.contains("?share="))
}

/// Normalize a raw share reference down to the bare token the server's
/// `/api/share/{token}` routes take — a bare token, a full
/// `https://chart.example/?share=<token>` URL, and a
/// `mylittlechart://share/<token>` deep link all resolve here.
/// `ChartApp::request_open_share`'s own doc names all three as accepted
/// input; this is the one place the stripping happens, so the argv parse
/// above and the future "Open shared chart…" overlay's submit button never
/// duplicate the rule.
fn extract_share_token(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("mylittlechart://share/") {
        return rest.split(['?', '#']).next().unwrap_or(rest).trim_end_matches('/').to_string();
    }
    if let Some(idx) = raw.find("?share=") {
        let after = &raw[idx + "?share=".len()..];
        return after.split('&').next().unwrap_or(after).to_string();
    }
    raw.to_string()
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
            warn!("[App] another instance is already running — exiting.");
            std::process::exit(0);
        }
        if handle == 0 {
            warn!("[App] WARNING: CreateMutexW failed (error {}), continuing without single-instance guard", unsafe { GetLastError() });
        }
        handle // keep alive for process lifetime
    };

    info!("chart-app-vello v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "Controls: Left drag=Pan chart, Scroll=Zoom, Escape=Cancel/close modal, \
         Ctrl+S=Screenshot (copied to clipboard + saved to Downloads)"
    );

    // Initialize diagnostics (logging, crash reporting, memory watchdog).
    // The guard must live until the end of main() to keep the log file writer alive.
    let log_dir = mlc_shell_diagnostics::default_log_dir();
    let _diagnostics = mlc_shell_diagnostics::init(&log_dir, env!("CARGO_PKG_VERSION"));

    // Start memory watchdog — logs warnings via tracing regardless of whether
    // the receiver is consumed. Drained every frame into `Memory` system
    // events (system-event-bus arc, 2026-09-05) — see `App::mem_warnings_rx`.
    let mem_warnings_rx = mlc_shell_diagnostics::watchdog::start();

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
    // 2. vault.enc exists: profile.json readable, chart opens straight away
    // 3. No vault.enc, no salt: plaintext-only install or migration needed
    let is_first_run = !has_profile;

    // Whether the active profile's vault is currently locked (has a key
    // container but no key derived yet this session). The app is fully
    // functional without it — presets, tabs, watchlists are plaintext
    // (`SyncTier::CloudSync`); only exchange credentials and bot tokens live
    // behind it. Owner's ruling 2026-08-31 (docs/mlc/audits/2026-08-30-entry-
    // lock-vault-and-user-persistence.md §10): this no longer gates entry —
    // it is truth carried into `App` so the profile manager can route to the
    // UnlockPassphrase page when the user opens it themselves.
    let needs_vault_unlock = has_vault && has_salt;
    // There is deliberately no `needs_migration` here any more (owner's
    // ruling 2026-09-02). It used to be `has_profile && !has_vault`, and it
    // both forced skeleton mode and opened a NON-DISMISSIBLE CreatePassphrase
    // page in front of the chart.
    //
    // Two things were wrong with it. `vault.enc` is the only place a
    // credential has ever lived — `profile.json` is written plaintext and
    // carries none by construction — so a profile without a vault holds no
    // secrets and has nothing whatsoever to migrate; the screen demanded a
    // passphrase to encrypt an empty set. And since a vault is now created
    // only when the user asks for one, `!has_vault` is the NORMAL state, so
    // the gate fired for ordinary users on every single launch. That is the
    // fossil of the era when the vault was mandatory at entry, and the
    // reason every profile on this machine has a `vault.enc`: the gate made
    // them.

    if is_first_run {
        info!("[App] first-run detected — welcome wizard will be shown");
    }

    // First-run bootstrap, matching the web shell's `bootstrap_first_run`
    // (`mlc-app-shell-web/src/persistence.rs`): create the profile NOW rather
    // than waiting for the welcome wizard to finish.
    //
    // Until 2026-09-02 native did the opposite — it booted a SKELETON window
    // (a spinner, no chart) and only created the profile when the wizard was
    // completed. Two things came out of that. The app did not start until the
    // user finished a first-run tour, which is the gate the owner ruled out.
    // And dismissing the wizard — its × button, the dimmer, Esc, all of which
    // it advertises as "always allowed, the wizard is a first-run courtesy,
    // not a gate" — hid the modal without ever completing it, so the skeleton
    // was never promoted and the spinner stayed up until the process was
    // killed. Promotion has exactly three triggers (wizard completion, vault
    // passphrase validation, recovery-key confirmation) and dismissal is none
    // of them.
    //
    // With the profile on disk before the first window is built, the wizard
    // becomes what its own module doc already claims it is on both shells: an
    // ordinary dismissible modal floating over a live chart. The profile is
    // nameless — see the identity ruling in `create_profile`'s callers.
    if is_first_run {
        match mlc_core::create_profile("", "chart") {
            Ok(meta) => {
                if let Some(mut index) = mlc_core::load_profile_index() {
                    index.active_profile_id = meta.id.clone();
                    let _ = mlc_core::save_profile_index(&index);
                }
                info!("[App] first-run bootstrap: created profile {}", meta.id);
            }
            Err(e) => {
                // Not fatal: the wizard's completion path still creates one.
                warn!("[App] first-run bootstrap failed: {} — wizard will create the profile", e);
            }
        }
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
            error!("[App] failed to save profile at startup: {}", e);
        }
    }

    let profile = profile_manager.profile.clone();
    let saved_windows = profile.ui.windows.clone();

    let mut app = App::new(bridge, live_data_rt, live_data_handle, saved_windows, profile, profile_manager, connector_ready_rx, is_first_run, needs_vault_unlock, mem_warnings_rx);
    // Store the event-loop proxy so the promote worker can send PromoteReady
    // back to the main thread without polling.
    app.promote_proxy = Some(event_loop.create_proxy());
    // Native share deep link (see `parse_deep_link_share_argv`'s own doc) —
    // applied once the first window exists, in `about_to_wait`.
    app.pending_deep_link_share = parse_deep_link_share_argv();
    event_loop.run_app(&mut app).expect("Event loop error");
}
