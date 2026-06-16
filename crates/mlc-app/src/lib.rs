//! chart-app — standalone chart application crate
//!
//! A minimal, self-contained chart application that owns one `ChartPanelApp`
//! (which itself holds one `ChartContainer` via `ChartPanelGrid`) plus an
//! `InputCoordinator` for widget routing and a `DefaultChartInputHandler`
//! for chart canvas input.
//!
//! # Usage
//!
//! ```ignore
//! let mut app = ChartApp::new("BTCUSDT");
//! app.resize(1280, 800);
//! app.render(&mut ctx, current_time_ms);
//! app.on_click(x, y);
//! ```

pub mod agent;
pub mod chart_panel_bridge;
pub mod chart_panel_state;
pub mod chart_state;
pub mod frame_lifecycle;
pub mod input;
pub mod panels_render;
pub mod panels_store;
pub mod preset_cache;
pub mod scroll_dispatch;
pub mod text_input;
use uzor::input::TextFieldConfig;
use uzor::WidgetId;
pub mod persistence;
pub mod workspace;

pub use panels_store::TradingPanelsStore;

pub use input::KeyPress;
pub use digdigdig3::ExchangeId;
pub use mlc_sidebar_content::watchlist::{WatchlistManager, WatchlistSymbol};
pub use mlc_indicators::RecalcMode;
pub use mlc_core::{Language, set_language};

// ── Debug log gate (MLC_PERF_LOG) ────────────────────────────────────────────
// Checked once at first use via OnceLock; hot path reads a single bool.
static DEBUG_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn debug_log_enabled() -> bool {
    *DEBUG_LOG.get_or_init(|| std::env::var("MLC_PERF_LOG").is_ok())
}

/// Diagnostic log — gated by `MLC_PERF_LOG` env var.
/// Set `MLC_PERF_LOG=1` (any non-empty value) to enable.
macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug_log_enabled() {
            eprintln!($($arg)*);
        }
    };
}
pub(crate) use dlog;

use std::cell::RefCell;

use mlc_core::{
    ToolbarConfig,
    LayoutRect,
    ScaleCornerHitZones,
    ToolbarState,
    render::RenderContext,
    DefaultChartInputHandler, ChartOutputAction, UndoAction,
    Command, ViewportState,
    data_provider::SharedDataProvider,
    ScaleMode,
    ChartId,
};
use mlc_layout::layout::{
    ChartPanelRenderData, render_full_chart_panel,
    ChartModalLayout, ChartModalRenderResult,
    ChartRenderConfig,
    ContextMenuResult,
};
use mlc_layout::panel_app::ChartPanelLayout;
use mlc_layout::layout::modals::context_menu::render_context_menu;
use mlc_layout::layout::modals::indicator_overlay::render_indicator_overlay;
use mlc_layout::layout::modals::search_overlay::render_search_overlay;
use mlc_layout::layout::ModalSearchResult;
use mlc_layout::layout::render_ui::IndicatorOverlayInfo;
use mlc_core::indicator_source::{IndicatorSource, AlertRenderData, AlertRenderStatus};
use mlc_core::state::modal_state::{ModalState, IndicatorCatalogItem};
use mlc_layout::ui::modal_settings::{ChartScreenArea, WatchlistModalState, WatchlistGroupNameInputState};
use mlc_layout::layout::modals::watchlist_modal::{render_watchlist_modal, WatchlistEntry, WatchlistGroupInfo, render_wl_group_name_input, WlGroupNameInputResult};
use mlc_layout::layout::modals::clock_popup::render_clock_popup;
use mlc_indicators::IndicatorManager;
use mlc_live_data::{DataBridge, LiveUpdate, LiveDataProvider};

use mlc_layout::panel_app::ChartPanelApp;
use uzor::input::{InputCoordinator, InputState};

/// Every exchange whose connector is warmed up at startup (in small batches from
/// tick(), spread across the skeleton/loading screen so dials don't burst).
const ALL_EXCHANGE_IDS: &[digdigdig3::ExchangeId] = &[
    digdigdig3::ExchangeId::Binance, digdigdig3::ExchangeId::Bybit,
    digdigdig3::ExchangeId::OKX, digdigdig3::ExchangeId::KuCoin,
    digdigdig3::ExchangeId::GateIO, digdigdig3::ExchangeId::Bitget,
    digdigdig3::ExchangeId::MEXC, digdigdig3::ExchangeId::HTX,
    digdigdig3::ExchangeId::Kraken, digdigdig3::ExchangeId::Coinbase,
    digdigdig3::ExchangeId::BingX, digdigdig3::ExchangeId::Bitfinex,
    digdigdig3::ExchangeId::Bitstamp, digdigdig3::ExchangeId::Gemini,
    digdigdig3::ExchangeId::CryptoCom, digdigdig3::ExchangeId::Lighter,
    digdigdig3::ExchangeId::Upbit, digdigdig3::ExchangeId::Deribit,
    digdigdig3::ExchangeId::HyperLiquid, digdigdig3::ExchangeId::Dydx,
    digdigdig3::ExchangeId::Moex,
];


// =============================================================================
// Account type helpers
// =============================================================================

/// Convert a short account-type label (e.g. `"S"`, `"FC"`) into the
/// `digdigdig3::AccountType` enum.
///
/// Falls back to `AccountType::Spot` for unknown or empty labels so that
/// existing serialised data without an `account_type` field continues to
/// work correctly.
pub fn account_type_from_label(label: &str) -> digdigdig3::AccountType {
    use digdigdig3::AccountType;
    match label {
        "M"        => AccountType::Margin,
        "F" | "FC" => AccountType::FuturesCross,
        "FI"       => AccountType::FuturesIsolated,
        "E"        => AccountType::Earn,
        "L"        => AccountType::Lending,
        "O"        => AccountType::Options,
        "C" | "CV" => AccountType::Convert,
        _          => AccountType::Spot, // "S" and unknown → Spot (backward compat)
    }
}

// =============================================================================
// Timeframe parsing
// =============================================================================

/// Parse a timeframe name string (e.g. "1m", "5m", "1H", "4h", "1D") into a
/// [`Timeframe`].  Case-insensitive for the suffix letter.
fn parse_timeframe_name(name: &str) -> Option<mlc_core::state::Timeframe> {
    use mlc_core::state::Timeframe;
    // Match exact Timeframe.name values (case-sensitive: "1m" = minute, "1M" = month).
    match name {
        "1m"  => Some(Timeframe::m1()),
        "3m"  => Some(Timeframe::new("3m", 3)),
        "5m"  => Some(Timeframe::m5()),
        "15m" => Some(Timeframe::m15()),
        "30m" => Some(Timeframe::m30()),
        "1H" | "1h"  => Some(Timeframe::h1()),
        "2H" | "2h"  => Some(Timeframe::new("2H", 120)),
        "4H" | "4h"  => Some(Timeframe::h4()),
        "6H" | "6h"  => Some(Timeframe::new("6H", 360)),
        "8H" | "8h"  => Some(Timeframe::new("8H", 480)),
        "12H" | "12h" => Some(Timeframe::new("12H", 720)),
        "1D" | "1d"  => Some(Timeframe::d1()),
        "3D" | "3d"  => Some(Timeframe::new("3D", 4320)),
        "1W" | "1w"  => Some(Timeframe::w1()),
        "1M"  => Some(Timeframe::mn1()),
        _ => None,
    }
}

// =============================================================================
// Cross-drag overlay rendering
// =============================================================================

/// Draw the ghost rect + compass drop-zone indicator for an active cross-drag.
///
// =============================================================================
// MiniTickerData
// =============================================================================

/// Cached 24-hour ticker statistics from the WebSocket mini-ticker stream.
///
/// One entry per symbol, updated every second from `LiveUpdate::MiniTickerUpdate`.
/// Used as a fallback price source in the watchlist sidebar for symbols that
/// do not have a chart window open.
struct MiniTickerData {
    last_price: f64,
    price_change_percent: f64,
    high_price: f64,
    low_price: f64,
    volume: f64,
}

// =============================================================================
// ChartApp
// =============================================================================

/// Standalone chart application.
///
/// Owns a single `ChartPanelApp` (which holds exactly one `ChartContainer` and
/// its own modal state structs), an `InputCoordinator` for widget routing,
/// and a `DefaultChartInputHandler` for chart canvas input.
///
/// There is no sync bridge — with only one window the state flows directly.
pub struct ChartApp {
    /// The chart panel — owns ChartContainer, toolbar state, split grid, and modal state.
    pub panel_app: ChartPanelApp,

    /// Central widget router — registers hit-zones each frame, processes clicks.
    ///
    /// Uses `RefCell` so that `render_to_scene(&self)` can register widgets via
    /// interior mutability without requiring `&mut self`.
    pub input_coordinator: RefCell<InputCoordinator>,

    /// Chart input handler — processes drag/scroll/click actions.
    pub input_handler: DefaultChartInputHandler,

    /// Screen dimensions (pixels)
    pub width: u32,
    pub height: u32,

    /// Hit-zone results from last render (used for click dispatch)
    pub(crate) frame_result: Option<ChartModalRenderResult>,
    /// Per-leaf scale-corner hit zones, populated each frame.
    /// Single-window mode stores one entry for the active leaf;
    /// split mode stores one entry per visible leaf.
    pub(crate) scale_corner_zones_by_leaf: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones>,

    /// Search overlay render result from last frame (used for rect-based hit testing).
    pub(crate) search_modal_result: Option<ModalSearchResult>,

    /// Context menu render result from last frame (used for rect-based hit testing).
    pub(crate) context_menu_result: Option<ContextMenuResult>,

    /// Currently hovered context menu item id (for hover highlight rendering).
    pub(crate) hovered_context_menu_item_id: Option<String>,

    /// Mouse position from the last `on_mouse_move` call
    pub(crate) last_mouse_pos: (f64, f64),

    /// Chart content rect from last render — toolbar-offset area used for
    /// coordinate transforms (bar index, price) in drawing tool hit testing.
    pub(crate) content_rect: LayoutRect,

    /// Inline floating bar rect from last render — used for drag hit-testing.
    pub(crate) last_inline_bar_rect: Option<LayoutRect>,

    /// Indicator manager — owns all indicator instances and definitions.
    pub indicator_manager: IndicatorManager,

    /// Modal state for search overlays (indicator search, symbol search).
    modal_state: ModalState,

    /// Pending screenshot request — set by context menu, drained by the renderer.
    pending_screenshot: bool,

    /// When set, the frame loop should clear in-memory bars and re-fetch.
    pub pending_reset_cache: bool,
    /// When set, the frame loop should delete the .bin file, clear in-memory bars, and re-fetch.
    pub pending_reset_storage: bool,

    /// Points of the primitive captured at drag start, keyed by primitive index.
    ///
    /// Set in `StartPrimitiveDrag` so that `EndPrimitiveDrag` can compare old
    /// vs new points and push a `MovePrimitive` command to history.
    drag_start_points: Option<(usize, Vec<(i64, f64)>)>,

    /// Viewport state captured at drag start.
    ///
    /// Set in `on_drag_start` so that `on_drag_end` can compare before/after
    /// and push a `ViewportChange` command to history when panning or zooming.
    viewport_before_drag: Option<ViewportState>,

    /// Active color picker L2 drag state.
    ///
    /// Set in `on_drag_start` when the user begins dragging in the SV square or
    /// hue bar of an L2 color picker.  Cleared in `on_drag_end`.
    color_picker_drag: Option<ColorPickerDragState>,

    /// True while a drag is active on a UI element (modal, slider, scrollbar, color picker).
    /// Set in `on_drag_start` when `is_over_ui()` is true; cleared in `on_drag_end`.
    /// Used to suppress crosshair during UI drags without enumerating every drag type.
    pub(crate) ui_drag_active: bool,

    /// True when `on_drag_start` dismissed a color-picker popup.
    /// While set, `on_drag_move` and `on_drag_end` are swallowed so the chart
    /// doesn't receive a spurious pan/draw drag.  Cleared in `on_drag_end`.
    pub(crate) drag_dismissed_popup: bool,

    /// In-progress separator drag state when the user is dragging a split-panel divider.
    /// Set in `on_drag_start`, updated in `on_drag_move`, cleared in `on_drag_end`.
    pub(crate) split_separator_drag: Option<SplitSeparatorDragState>,

    /// Stored overlay-tab hit zones per leaf (from last render frame).
    /// In single mode, only one entry with the active leaf id.
    pub(crate) leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_layout::layout::panel_overlay::LeafTabHitZones>,

    /// Currently hovered overlay-tab zone (shared across all tabs — only one can be hovered at a time).
    pub(crate) leaf_tab_hover: mlc_layout::layout::panel_overlay::LeafTabHoverZone,

    /// LeafId of the tab currently being hovered (if any).
    pub(crate) leaf_tab_hovered_leaf: Option<mlc_core::LeafId>,

    /// Toolbar render result from the last frame — used to read chevron rects and
    /// max_scroll values when dispatching chevron clicks.
    pub(crate) last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult>,

    /// User's preferred default scale mode, synced from profile settings.
    /// Applied to windows on initial bar load.
    pub default_scale_mode: ScaleMode,

    /// Top-level manager container (K2.7 — alert moved here from ChartApp direct field).
    pub managers: mlc_core::managers::Managers,

    /// Triggered alert events waiting for delivery (Telegram, toast, etc.)
    pub pending_delivery_events: Vec<mlc_alert_delivery::DeliveryEvent>,

    /// Whether an alert-triggered screenshot capture is needed.
    /// Set by tick() when an alert fires. Cleared by the renderer after capture.
    pub pending_alert_screenshot: bool,

    /// Sidebar panel state — tracks which right-side panel is open and its content.
    pub sidebar_state: mlc_sidebar_content::state::SidebarState,

    /// Right sidebar render result from the last frame (hit zones for click dispatch).
    pub last_sidebar_result: Option<mlc_sidebar_content::render::RightSidebarResult>,

    /// True while the user is dragging the right-sidebar separator to resize it.
    ///
    /// Set in `on_drag_start` when the drag begins on `"right_sidebar_separator"`,
    /// cleared in `on_drag_end`.
    pub(crate) sidebar_separator_drag_active: bool,

    /// When true, split operations create the new window as an independent
    /// auto-created group instead of inheriting the source window's tag.
    pub split_without_group: bool,

    /// Left edge X of the right toolbar from the last render.
    ///
    /// Used during sidebar separator drag to compute the new sidebar width as
    /// `right_toolbar_left_x - mouse_x`.
    pub(crate) right_toolbar_left_x: f64,

    /// Currently selected indicator id (for selection marker rendering).
    ///
    /// Set when the user left-clicks on an indicator line; cleared when the user
    /// clicks on a primitive or on empty chart background.
    pub selected_indicator_id: Option<u64>,

    /// Expanded watchlist overlay modal state.
    ///
    /// Open/closed by the watchlist sidebar header button or a keyboard shortcut.
    /// Displays all watchlist symbols in a searchable, scrollable list with
    /// price data and delete buttons.
    pub watchlist_modal: WatchlistModalState,

    /// Render result from the last watchlist modal frame (hit zones for click/scroll dispatch).
    pub(crate) last_watchlist_modal_result: Option<mlc_layout::layout::modals::watchlist_modal::WatchlistModalResult>,

    /// State for the watchlist group name input modal (create new / rename).
    pub wl_group_name_input: WatchlistGroupNameInputState,

    /// Render result from the last watchlist group name input modal frame (hit zones for drag dispatch).
    pub(crate) last_wl_group_name_result: Option<WlGroupNameInputResult>,

    /// Set `true` when a preset was restored at startup before the first
    /// `resize()`.  On the first resize (when `chart_width` becomes known)
    /// the viewport is repositioned so the last bar sits with 5 empty bars
    /// of right margin.
    needs_initial_viewport_fit: bool,

    /// Live data bridge — owns the tokio runtime and connector pool.
    bridge: std::sync::Arc<DataBridge>,

    /// Receiver for async live data updates (bars loaded, errors, etc.).
    live_update_rx: tokio::sync::broadcast::Receiver<LiveUpdate>,

    /// Cached mini ticker prices from WebSocket (symbol → 24h stats).
    ///
    /// Updated every second from `LiveUpdate::MiniTickerUpdate`. Used as a
    /// fallback price source for watchlist items whose symbol is not open in
    /// any chart window.
    mini_ticker_cache: std::collections::HashMap<String, MiniTickerData>,

    /// Currently active exchange.
    pub active_exchange: digdigdig3::ExchangeId,

    /// Real exchange symbols loaded from the exchange info endpoint.
    ///
    /// Populated asynchronously via `LiveUpdate::SymbolsLoaded`. Keyed by
    /// `ExchangeId` so that symbols from multiple exchanges are stored
    /// simultaneously. Used by the symbol search overlay.
    exchange_symbols: std::collections::HashMap<digdigdig3::ExchangeId, Vec<mlc_live_data::SymbolInfo>>,

    /// Unique identifier for this OS window (e.g. "win_1728503941").
    ///
    /// Used when performing a coordinated multi-window save so that each
    /// window's tab/preset state is stored under its own slot in
    /// [`UserProfile::windows`].
    pub window_id: String,

    /// Last known OS window position/size — updated by the windowing layer
    /// (main.rs) so that `build_window_state()` can include them without
    /// requiring access to the winit `Window` handle.
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,

    /// Set to true when this window's tab/preset state changed and needs saving.
    /// Checked and cleared by App each frame.
    pub profile_dirty: bool,
    /// Live preset state for open tabs (except the active one).
    /// Key = preset id. On tab switch: park active → cache, unpark target → active.
    live_preset_cache: std::collections::HashMap<String, preset_cache::LivePresetState>,
    /// Set to true when window geometry (position/size) changed.
    /// Triggers local save but NOT cloud sync.
    pub profile_geometry_dirty: bool,
    /// Set to true when watchlist data changed and needs saving.
    /// Checked and cleared by App each frame.
    pub watchlists_dirty: bool,

    /// Queued watchlist mutations — drained by App each frame and applied to AppState.
    /// Windows never mutate watchlist directly; they push actions here.
    pub watchlist_actions: Vec<WatchlistAction>,

    /// Queued connector mutations — drained by App each frame and applied to AppState.
    /// Windows never mutate connector state directly; they push actions here.
    pub connector_actions: Vec<ConnectorAction>,

    /// Queued preset mutations — drained by App each frame and applied to AppState.
    pub preset_actions: Vec<PresetAction>,

    /// Set by any callsite that calls `autosave_snapshot()` during a frame.
    /// Drained by `flush_pending_autosave` once per frame from the app tick,
    /// so a preset switch (which triggers 5-7 autosave callsites in one
    /// frame) results in a single deep-copy + Upsert instead of 5-7.
    pub pending_autosave: bool,

    /// Queued settings snapshot mutations — drained by App each frame and applied to AppState.
    pub snapshot_actions: Vec<SnapshotAction>,

    /// Queued template mutations — drained by App each frame and applied to AppState.
    pub template_actions: Vec<TemplateAction>,

    /// Performance control actions queued by sidebar clicks.
    pub perf_actions: Vec<PerfAction>,

    /// Set when the user switches theme preset in this window.
    /// Drained by App in about_to_wait(); App then updates AppState.theme_preset
    /// and syncs the new preset to all windows.
    pub theme_changed: Option<String>,

    /// Set when the user selects a new RecalcMode in the User Settings modal.
    /// Drained by App in about_to_wait(); App then updates AppState.recalc_mode
    /// and syncs the new mode to all windows.
    pub recalc_mode_changed: Option<String>,

    /// Set when the user selects a new language in the User Settings General tab.
    /// Drained by App in about_to_wait(); App then applies set_language(), propagates
    /// to all windows, and saves to the active profile.
    pub language_changed: Option<String>,

    /// Signal: server enabled/disabled changed. Consumed by main.rs.
    pub server_enabled_changed: Option<bool>,

    /// Signal: API key was changed/generated. Consumed by main.rs.
    pub local_agent_key_changed: Option<String>,

    /// Signal: text to copy to clipboard. Consumed by main.rs.
    pub clipboard_text: Option<String>,

    /// Signal: user clicked "Create" in the key manager. Consumed by main.rs
    /// which generates the key, calls `agent_state.add_key()`, and writes the
    /// raw key back into `user_settings_state.last_created_key`.
    pub key_create_request: Option<(String, String)>, // (label, tier)

    /// Signal: user clicked delete [X] for a managed key. Consumed by main.rs
    /// which calls `agent_state.remove_key()` and refreshes `managed_keys`.
    pub key_delete_request: Option<String>, // label to delete

    /// Signal: request to refresh `managed_keys` from AgentState. Set after
    /// create/delete so main.rs re-reads the list on the next about_to_wait().
    pub key_list_refresh: bool,

    /// Signal: open a URL in the system browser. Set by click handlers in input.rs,
    /// drained by main.rs which calls `open::that()`.
    pub pending_open_url: Option<String>,

    /// Signal: send a command to the OTA updater. Set by click handlers in input.rs,
    /// drained by main.rs which forwards it to `updater_handle.cmd_tx`.
    pub pending_updater_cmd: Option<String>, // "logout" | "start_oauth:{provider}"

    /// Timestamp of the last backfill triggered after a broadcast lag event.
    ///
    /// Used to debounce rapid successive `Lagged` errors — backfill is only
    /// triggered when at least 1 second has elapsed since the last one.
    last_backfill_time: std::time::Instant,

    /// Count of broadcast lag events since startup (for performance monitoring).
    pub lag_event_count: u64,

    /// Number of indicator recalculations performed since the last log flush.
    recalc_count: u32,

    /// Timer controlling the periodic RecalcMode diagnostic log (every 5 s).
    recalc_log_timer: std::time::Instant,

    /// Number of TradeUpdate events received since the last log flush.
    trade_count: u32,

    /// Whether the periodic RecalcMode diagnostic log is enabled.
    /// Controlled via the Performance tab toggle in User Settings.
    pub diagnostics_enabled: bool,

    /// Sidebar data needs rebuild — set by mutations, cleared after sidebar populate.
    ///
    /// When `false` the sidebar population block is skipped entirely, saving the
    /// per-frame cost of iterating primitives, indicators, watchlist entries, and
    /// the connector registry. Set to `true` by `tick()` whenever a `LiveUpdate`
    /// message is received, and by sidebar panel toggle handlers in `input.rs`.
    ///
    /// Also read by `chart-app-vello` to propagate the dirty state to the
    /// per-window `sidebar_dirty_scene` flag for scene caching.
    pub sidebar_data_dirty: bool,

    /// Tracks the last active leaf so we can detect leaf switches and
    /// mark sidebar_data_dirty automatically.
    last_active_leaf: Option<mlc_core::LeafId>,

    /// Rate-limit gate for the per-live-event sidebar dirty mark (Watchlist /
    /// Connectors panels). Live data (ticks, connector-ready) arrives in bursts
    /// — at startup all ~21 connectors flood it in one frame, forcing a full
    /// sidebar rebuild (~30ms) per frame. We collapse that: a live-data event
    /// only re-marks the sidebar dirty at most once per this interval. The
    /// per-second prepare-timer guarantees the panel still refreshes even if no
    /// event slips through. Input-driven marks (clicks, settings) are NOT gated
    /// — they set sidebar_data_dirty directly for instant feedback.
    last_sidebar_data_mark: Option<std::time::Instant>,

    /// Last render timing breakdown in microseconds (for PERF diagnostics).
    /// Tuple: (chart_us, toolbar_us, sidebar_us, setup_us)
    /// - chart_us:   time spent in the chart render block (split or single)
    /// - toolbar_us: time spent rendering toolbars + registering toolbar hit zones
    /// - sidebar_us: time spent rendering sidebar, watchlist modal, and other modals
    /// - setup_us:   time spent in layout setup before chart render starts
    pub render_timing_us: (u64, u64, u64, u64),

    // ── Sub-pane height restore ───────────────────────────────────────────────
    /// Per-window sub-pane height ratios pending application on the next
    /// `sync_sub_panes_from_manager` call.  Populated during `LoadPreset` from
    /// the saved `ChartWindowSnapshot::sub_pane_height_ratios`.
    ///
    /// Format: `window_id → (instance_id → height_ratio)`.  Cleared after the
    /// ratios are applied.
    pub(crate) pending_sub_pane_ratios: std::collections::HashMap<u64, std::collections::HashMap<u64, f32>>,
    /// Instance ids already given a one-shot heal recalc (tick.rs indicator
    /// heal). Prevents per-tick recalc storms for indicators that
    /// legitimately produce no values.
    pub(crate) indicator_heal_attempted: std::collections::HashSet<u64>,
    /// Per-window set of sub-pane instance_ids that should have `above_main = true`
    /// after the next `sync_sub_panes_from_manager` call.  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_above_main`.
    ///
    /// Format: `window_id → HashSet<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_above_main: std::collections::HashMap<u64, std::collections::HashSet<u64>>,
    /// Per-window ordered list of sub-pane instance_ids to restore the saved
    /// Vec order (above-main first, then below-main).  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_order`.
    ///
    /// Format: `window_id → Vec<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_order: std::collections::HashMap<u64, Vec<u64>>,
    /// Per-window set of sub-pane instance_ids that should be hidden after
    /// the next `sync_sub_panes_from_manager` call.  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_hidden`.
    ///
    /// Format: `window_id → HashSet<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_hidden: std::collections::HashMap<u64, std::collections::HashSet<u64>>,

    /// Per-window maximized sub-pane state.
    ///
    /// When a sub-pane is expanded (maximized), this map stores:
    /// - key: `window.id.0` (ChartId)
    /// - value: `(maximized_instance_id, pre_maximize_proportions)`
    ///
    /// The pre-maximize proportions are the `inner_docking` root proportions
    /// before the maximize, used to restore the original layout on Restore.
    /// Runtime-only — not persisted to disk.
    pub(crate) maximized_sub_panes: std::collections::HashMap<u64, (u64, Vec<f64>)>,

    /// Agent session manager — owns PTY and pipe sessions.
    ///
    /// Call `drain_events` each frame (in `tick`) to process incoming terminal
    /// output. Call `snapshot` to read rendering state without OS handles.
    pub agent: agent::AgentSessionManager,

    /// True when the PTY panel received focus via hover rather than a click.
    ///
    /// Hover-focus is weaker than click-focus: it is cleared when the cursor
    /// leaves the PTY area, whereas click-focus persists until an explicit
    /// blur.
    pub agent_pty_hover_focused: bool,

    /// True while a host-side PTY text-selection drag is in progress.
    pub agent_pty_drag_active: bool,

    /// True while a host-side chat text-selection drag is in progress.
    pub agent_chat_drag_active: bool,

    /// Set to `true` after `autostart_all` has been called once.
    ///
    /// The autostart fires on the first frame of the event loop so that a
    /// Tokio runtime is guaranteed to be available via `bridge.runtime()`.
    pub agent_autostarted: bool,

    // ── Leak / backpressure diagnostics (snapshotted each tick) ──────────────
    /// Pending events in the broadcast queue after this tick's drain (backpressure).
    pub diag_queue_len: usize,
    /// Live `mini_ticker_cache` map size (should be bounded by watchlist size).
    pub diag_mini_ticker: usize,

    // ── Internal CPU profiling timers (updated each tick, read by sidebar) ────
    /// Total time spent in the last tick() call, in microseconds.
    pub last_tick_us: u64,
    /// Time spent in indicator recalculation (calculate_for_window) in last tick.
    pub last_indicator_recalc_us: u64,
    /// Time spent processing LiveUpdate events in the last tick.
    pub last_event_process_us: u64,
    /// Accumulated time in calc_auto_scale() calls during the last tick.
    pub last_auto_scale_us: u64,

    /// Scene composite time (scene.reset + scene.append for chart/overlay/sidebar/toolbar)
    /// measured in the last build_window_scene call, in microseconds.
    /// Written by scene_builder.rs, read by the PERF populate block in main.rs.
    pub last_composite_us: u64,

    /// Accumulated time in orderbook panel .tick() calls (DOM + L2Tape + LiquidityHeatmap)
    /// for OrderbookSnapshot and OrderbookDelta events during the last tick.
    pub last_orderbook_panel_us: u64,
    /// Per-panel breakdown: DOM .tick() time (μs) in last tick.
    pub last_dom_panel_us: u64,
    /// Per-panel breakdown: L2Tape .tick() time (μs) in last tick.
    pub last_l2_panel_us: u64,
    /// Per-panel breakdown: LiquidityHeatmap .tick() time (μs) in last tick.
    pub last_heatmap_panel_us: u64,
    /// Accumulated time in trade panel .tick() calls (BigTrades + VolumeProfile + Footprint + TradeTape)
    /// for TradeUpdate events during the last tick.
    pub last_trade_panel_us: u64,
    /// Accumulated time in bar apply operations (BarsLoaded + BackfillComplete + ScrollBarsLoaded + TradeUpdate bar update)
    /// during the last tick.
    pub last_bar_apply_us: u64,
    /// Number of OrderbookSnapshot + OrderbookDelta events drained during the last tick.
    pub last_ob_event_count: u32,
    /// Number of TradeUpdate + BarUpdate events drained during the last tick.
    pub last_trade_event_count: u32,

    /// Heavy state for all trading panels docked into the free-slot sidebars.
    ///
    /// Keyed by `PanelId`. The matching `Panel` leaves in each slot's
    /// `DockingManager<Panel>` carry only the `PanelId`, keeping
    /// `sidebar-content` free of `zengeld-panels`.
    pub panels_store: panels_store::TradingPanelsStore,

    /// Active agent-panel separator drag.
    ///
    /// `(separator_index, start_mouse_pos, total_available_size)` — the total
    /// available size (width for vertical separators, height for horizontal) is
    /// captured at drag-start so that `drag_separator` can convert pixel deltas
    /// to proportions correctly even as the window resizes.
    pub(crate) agent_sep_drag: Option<(usize, f64, f32)>,

    /// Active free-slot separator drag.
    ///
    /// `(slot_index, separator_index, start_mouse_pos, total_available_size)`.
    pub(crate) slot_sep_drag: Option<(usize, usize, f64, f32)>,

    /// Active sub-pane separator drag (between main chart and sub-pane, or
    /// between two sub-panes).  Mirrors slot_sep_drag pattern.
    ///
    /// `(instance_id, start_mouse_y, available_height)`.
    /// Active sub-pane separator drag:
    /// `(instance_id, start_cursor_y, content_w, content_h, _, _)`.
    /// All math goes through `ChartContainer::drag_sub_pane_separator_via_uzor`
    /// which delegates to uzor's `inner_docking.drag_separator` -- the same
    /// API the sidebar/agent/outer-grid separators already use. Last two
    /// tuple slots are reserved for forward-compat and unused today.
    pub(crate) sub_pane_sep_drag: Option<(u64, f64, f64, f64, f64, Option<u64>)>,

    /// Active coordinator-routed panel drag (DOM, L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile).
    ///
    /// `(panel, local_id, last_x, last_y)` — replaces per-panel fields.
    pub(crate) active_drag_panel: Option<(mlc_core::state::Panel, String, f64, f64)>,

    /// Active TradeTape drag-to-scroll.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_tradetape_drag: Option<(mlc_core::state::PanelId, f64, f64)>,

    /// Trading manager — order routing, position tracking, paper engine.
    pub mlc_trading_manager: Option<mlc_trading_manager::TradingManager>,

    /// Active-leaf (or single-window) chart geometry for the current frame.
    ///
    /// PRODUCED once per frame by the layout phase (`sync_sub_pane_geometry`),
    /// CONSUMED read-only by input handlers and `process_output_actions`.
    /// Plain mouse movement never changes geometry, so it is built in the layout
    /// phase rather than rebuilt per mouse event (was a 1500+ calls/sec hotspot).
    pub(crate) active_frame_layout: Option<mlc_core::ExtendedFrameLayout>,
    /// Per-leaf chart geometry for split mode, keyed by LeafId. Same lifecycle.
    pub(crate) frame_layouts: std::collections::HashMap<mlc_core::LeafId, mlc_core::ExtendedFrameLayout>,

}

/// An action that mutates the app-level watchlist.
/// Queued by windows, drained and applied by App.
#[derive(Debug, Clone)]
pub enum WatchlistAction {
    /// Toggle symbol in/out of watchlist (star button in search overlay).
    Toggle { symbol: String, exchange: String, account_type: String },
    /// Remove a specific symbol from watchlist.
    Remove { symbol: String, exchange: String, account_type: String },
    /// Reorder a symbol within the active list (drag & drop).
    Reorder { from_idx: usize, to_idx: usize },
    /// Create a new watchlist with given name.
    CreateList { name: String },
    /// Rename an existing watchlist.
    RenameList { id: u64, new_name: String },
    /// Delete a watchlist by id.
    DeleteList { id: u64 },
    /// Switch to a different active watchlist.
    SetActiveList { id: u64 },
    /// Clear the order snapshot on the active list.
    ClearOrderSnapshot,
    /// Set a color flag on a symbol.
    SetColorFlag { symbol: String, exchange: String, account_type: String, color: Option<String> },
    /// Move symbol into a group.
    MoveToGroup { symbol: String, exchange: String, account_type: String, group_name: String },
    /// Remove symbol from its group (back to ungrouped).
    RemoveFromGroup { symbol: String, exchange: String, account_type: String },
    /// Set column separator offsets.
    SetSeparatorOffsets { offsets: Vec<f64> },
    /// Reset column separator offsets to equal widths.
    ResetSeparatorOffsets,
    /// Set a single column separator offset by index.
    SetSeparatorOffset { index: usize, value: f64 },
    /// Toggle visibility of a watchlist column.
    ToggleColumnVisibility { column: String },
    /// Cycle sort mode: 0 → 1 → 2 → 0.
    SortCycle,
    /// Reset sort mode to 0 (unsorted).
    ResetSort,
}

/// An action that mutates app-level connector state.
/// Queued by windows, drained and applied by App.
#[derive(Debug, Clone)]
pub enum ConnectorAction {
    /// Toggle a connector's enabled/disabled state.
    ToggleEnabled { exchange_id: String },
}

/// An action that mutates app-level preset state.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum PresetAction {
    /// Upsert a preset — stores a fully-collected ChartPreset into AppState.
    Upsert(mlc_core::preset::preset::ChartPreset),
    /// Delete a preset by id (removes from memory and disk).
    Delete { id: String },
    /// Rename a preset in-place.
    Rename { id: String, new_name: String },
}

/// An action that mutates the app-level settings snapshots.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum SnapshotAction {
    /// Update the chart settings snapshot.
    ChartSettings(serde_json::Value),
    /// Update the primitive settings snapshot for a given type_id.
    PrimitiveSettings { type_id: String, data: serde_json::Value },
    /// Update the indicator settings snapshot for a given type_id.
    IndicatorSettings { type_id: String, data: serde_json::Value },
    /// Update the compare overlay settings snapshot.
    CompareSettings(serde_json::Value),
    /// Update the last-used drawing style for a given primitive `type_id`.
    ///
    /// Persisted globally so the next primitive of the same type is
    /// pre-populated with the user's last-used color, width, line style, and
    /// extended style properties.
    DrawingStyle { type_id: String, data: serde_json::Value },
}

/// An action that mutates app-level template state.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum TemplateAction {
    /// Add a primitive style template.
    AddPrimitive(mlc_core::templates::primitive_template::PrimitiveTemplate),
    /// Remove a primitive template by id.
    RemovePrimitive { id: String },
    /// Add an indicator template.
    AddIndicator(mlc_core::templates::indicator_template::IndicatorTemplate),
    /// Remove an indicator template by id.
    RemoveIndicator { id: String },
    /// Add a compare overlay template.
    AddCompare(mlc_core::templates::compare_template::CompareTemplate),
    /// Remove a compare template by id.
    RemoveCompare { id: String },
    /// Add a chart settings template.
    AddChart(mlc_core::templates::chart_template::ChartTemplate),
    /// Remove a chart template by id.
    RemoveChart { id: String },
    /// Add an indicator set.
    AddIndicatorSet(mlc_core::templates::indicator_set::IndicatorSet),
    /// Remove an indicator set by id.
    RemoveIndicatorSet { id: String },
}

/// An action that changes a performance control setting.
/// Queued by the Performance panel, drained and applied by App.
#[derive(Debug, Clone)]
pub enum PerfAction {
    SetFpsLimit(u32),
    SetMsaa(u8),
    SetRecalcMode(String),
    TogglePerfLog,
    SetBackend(String),
    ToggleVsync,
}

/// Identifies which region of a color picker is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorPickerDragArea {
    /// Saturation/Value square (x → saturation, y → value)
    SVSquare,
    /// Vertical hue bar (y → hue)
    HueBar,
    /// Horizontal opacity slider (x → opacity 0..1)
    OpacitySlider,
}

/// State for an in-progress color picker drag.
#[derive(Debug, Clone)]
pub(crate) struct ColorPickerDragState {
    /// Which part of the picker is being dragged
    pub area: ColorPickerDragArea,
    /// Which settings modal owns the picker ("primitive", "indicator", "chart")
    pub source: String,
    /// SV square: (x, y, width, height)
    pub sv_rect: (f64, f64, f64, f64),
    /// Hue bar: (x, y, width, height)
    pub hue_rect: (f64, f64, f64, f64),
    /// Opacity slider track: (x, y, width, height)
    pub opacity_rect: (f64, f64, f64, f64),
}

/// State for an in-progress split-panel separator drag.
#[derive(Debug, Clone)]
pub(crate) struct SplitSeparatorDragState {
    /// Index into `docking.separators()`
    pub separator_idx: usize,
    /// Orientation of the separator being dragged
    pub orientation: mlc_core::SeparatorOrientation,
    /// Screen X at drag start
    pub start_x: f64,
    /// Screen Y at drag start
    pub start_y: f64,
}

// =============================================================================
// RenderOutput
// =============================================================================

/// Cached results produced by one call to [`ChartApp::render_to_scene`].
///
/// `render_to_scene` is `&self` — it stores computed layout and result data
/// here instead of mutating `ChartApp` fields directly.  The caller passes
/// this struct to [`ChartApp::apply_render_output`] to persist the values.
#[derive(Default)]
pub struct RenderOutput {
    /// Per-leaf scale-corner hit zones computed during chart content rendering.
    pub scale_corner_zones_by_leaf: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones>,
    /// Toolbar render result (hit zones, dropdown state) for this frame.
    pub last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult>,
    /// Modal render result (settings panels, color pickers).
    pub frame_result: Option<ChartModalRenderResult>,
    /// Search-overlay render result (indicator / symbol search).
    pub search_modal_result: Option<mlc_layout::layout::ModalSearchResult>,
    /// Context-menu render result.
    pub context_menu_result: Option<ContextMenuResult>,
    /// Right-sidebar render result.
    pub last_sidebar_result: Option<mlc_sidebar_content::render::RightSidebarResult>,
    /// Watchlist-modal render result.
    pub last_watchlist_modal_result: Option<mlc_layout::layout::modals::watchlist_modal::WatchlistModalResult>,
    /// Watchlist group-name input result.
    pub last_wl_group_name_result: Option<WlGroupNameInputResult>,
    /// Per-leaf tab hit zones, rebuilt every frame.
    pub leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_layout::layout::panel_overlay::LeafTabHitZones>,
    /// Frame timing breakdown (chart_us, toolbar_us, sidebar_us, setup_us).
    pub render_timing_us: (u64, u64, u64, u64),
    /// Content rect (chart area minus sidebar) computed this frame.
    pub content_rect: LayoutRect,
    /// Left edge of the right toolbar (for sidebar separator drag math).
    pub right_toolbar_left_x: f64,
    /// Bounding rect of the floating inline config bar, if visible.
    pub last_inline_bar_rect: Option<LayoutRect>,
    /// If `Some(v)`, sets `toolbar_state.open_submenu_id = v` after render.
    pub open_submenu_update: Option<Option<String>>,
    /// Sub-pane range writebacks: `(leaf_id, pane_index, min, max)`.
    ///
    /// Populated by the render path after `render_sub_pane` returns the
    /// final (symmetrized + padded) range.  Applied in `apply_render_output`
    /// so that stored values always match what was displayed.
    pub sub_pane_range_writebacks: Vec<(mlc_core::LeafId, usize, f64, f64)>,
    /// Sub-pane overlay result writebacks: `(leaf_id, overlay_results)`.
    ///
    /// Produced each frame by `render_full_chart_panel`.  Applied in
    /// `apply_render_output` so the hit tester can read button rects next frame.
    pub sub_pane_overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)>,
}

impl ChartApp {
    /// Create a new chart application for the given symbol.
    ///
    /// Connects to Binance as the default exchange. Bars are requested
    /// asynchronously and fed into the chart window once they arrive.
    pub fn new(symbol: &str) -> Self {
        // Create DataBridge eagerly — always live, no demo fallback.
        // The third value is a ConnectorReady mpsc receiver used by the
        // chart-app-vello App struct; here it is dropped since ChartApp::tick()
        // receives ConnectorReady via the broadcast channel directly.
        let (bridge, live_update_rx, _connector_ready_rx) = DataBridge::new();
        let bridge = std::sync::Arc::new(bridge);
        // live_update_rx is a broadcast::Receiver — see DataBridge::add_listener()
        // for spawning additional windows that share this bridge.

        let mut app = Self {
            panel_app: ChartPanelApp::new(symbol),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            indicator_manager: IndicatorManager::new(),
            modal_state: ModalState::new(),
            pending_screenshot: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            viewport_before_drag: None,
            color_picker_drag: None,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            managers: mlc_core::managers::Managers::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: mlc_sidebar_content::state::SidebarState::new(),
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            watchlist_modal: WatchlistModalState::new(),
            last_watchlist_modal_result: None,
            wl_group_name_input: WatchlistGroupNameInputState::new(),
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            bridge: bridge.clone(),
            live_update_rx,
            mini_ticker_cache: std::collections::HashMap::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            exchange_symbols: std::collections::HashMap::new(),
            window_id: format!("win_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: false,
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            last_backfill_time: std::time::Instant::now(),
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: std::time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            last_sidebar_data_mark: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            mlc_trading_manager: mlc_trading_manager::TradingManager::new(
                bridge.clone(),
                std::path::PathBuf::from("."),
            ).ok(),
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
        };

        if let Some(tm) = &app.mlc_trading_manager {
            app.panels_store.set_trading_snapshot(tm.snapshot());
        }

        // Load persisted user state (templates, presets, profile) from disk.
        // Must happen before toolbar_config override so that the active_preset_id
        // from the profile is respected during the first render.
        app.panel_app.load_user_state();

        // Restore user profile (sidebar state) and watchlist from disk.
        app.load_user_profile();

        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        // Check if a saved preset exists for the active_preset_id.
        let has_saved_preset = app.panel_app.presets.contains_key(&app.panel_app.active_preset_id);

        if has_saved_preset {
            // ── Restore from saved preset ───────────────────────────────
            // Attach data_provider to the initial window so LoadPreset can clone it.
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();

            // Trigger the full preset restore pipeline (layout, windows, indicators, etc.).
            let preset_id = app.panel_app.active_preset_id.clone();
            app.process_chart_out_event(
                mlc_core::events::ChartOutEvent::LoadPreset { id: preset_id },
            );

            // Ensure all windows have a data_provider and toolbar config.
            // Phase 4 B: read exchange/account_type from branch_meta (no longer on struct).
            let window_data: Vec<(String, String, mlc_core::state::Timeframe, String)> = {
                let containers = app.panel_app.panel_grid.containers();
                containers.keys()
                    .filter_map(|cid| {
                        let sym = app.managers.bubble.symbol_for_chart(*cid).map(String::from).unwrap_or_default();
                        let exch = app.managers.bubble.exchange_for_chart(*cid).map(String::from).unwrap_or_default();
                        let tf = app.managers.bubble.timeframe_for_chart(*cid).cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let at = app.managers.bubble.account_type_for_chart(*cid).map(String::from).unwrap_or_else(|| "S".to_string());
                        Some((sym, exch, tf, at))
                    })
                    .collect()
            };

            // Pre-collect (chart_id → exchange, account_type) for data_provider seeding.
            let provider_seeds: Vec<(mlc_core::state::ChartId, String, String)> = {
                app.panel_app.panel_grid.containers().keys()
                    .map(|cid| {
                        let exch = app.managers.bubble.exchange_for_chart(*cid).map(String::from).unwrap_or_default();
                        let at = app.managers.bubble.account_type_for_chart(*cid).map(String::from).unwrap_or_else(|| "S".to_string());
                        (*cid, exch, at)
                    })
                    .collect()
            };
            for (cid, exch, at_label) in provider_seeds {
                if let Some(window) = app.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                    let win_exchange_id = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    let win_at = account_type_from_label(&at_label);
                    let win_provider: SharedDataProvider = std::sync::Arc::new(
                        LiveDataProvider::new(win_exchange_id, &exch, win_at, bridge.clone()),
                    );
                    window.data_provider = win_provider;
                    window.toolbar_config = ToolbarConfig::standalone();
                }
            }

            // Set active_exchange from the active window's exchange.
            // S2.7.1: read exchange via branch_meta instead of struct field.
            if let Some(cid) = app.panel_app.panel_grid.active_container_id() {
                let exch = app.managers.bubble.exchange_for_chart(cid)
                    .unwrap_or("");
                app.active_exchange = digdigdig3::ExchangeId::from_str(exch)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
            }

            // Ensure connectors are ready and request bars for each window's own exchange.
            for (sym, exch, tf, at_label) in &window_data {
                let eid = digdigdig3::ExchangeId::from_str(exch)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                if !app.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                    continue;
                }
                let at = account_type_from_label(at_label);
                bridge.ensure_connector(eid);
                bridge.request_bars(eid, sym, tf, at, None, Some(app.panel_app.user_manager.profile.bar_count as usize), false);
            }

            // Defer viewport positioning until the first resize() when chart_width is known.
            app.needs_initial_viewport_fit = true;
            // Preset restored successfully.
        } else {
            // ── Fresh state — Binance default ──────────────────────────────
            // Phase 4 B: capture chart_id BEFORE the mutating block.
            let active_chart_id = app.panel_app.panel_grid.active_container_id();
            let active_pid_init = app.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            if let Some(pid) = active_pid_init {
                crate::chart_panel_bridge::drawing_manager_for_panel_mut(&mut app.panels_store, pid)
                    .set_current_symbol_key("BTCUSDT", &exchange_name, "S");
            }
            if let Some(chart_id) = active_chart_id {
                // Create auto-bubble for the initial chart and assign instrument.
                let auto_bid = app.managers.bubble.create_group_auto(
                    [0.0, 0.0, 0.0, 0.0],
                    "BTCUSDT".to_string(),
                    mlc_core::state::Timeframe::new("1H", 60),
                );
                let _ = app.managers.bubble.attach_chart_to_bubble(chart_id, auto_bid);
                if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                    window.auto_bubble_id = auto_bid;
                    window.current_bubble_id = auto_bid;
                }
                if let Some(bubble) = app.managers.bubble.bubble_mut(auto_bid) {
                    bubble.symbol = "BTCUSDT".to_string();
                    bubble.timeframe = mlc_core::state::Timeframe::new("1H", 60);
                    bubble.exchange = exchange_name.clone();
                    bubble.account_type = "S".to_string();
                }
                // Bind drawing manager style store to auto-bubble's shared Arc.
                if let Some(pid) = active_pid_init {
                    if let Some(style_arc) = app.managers.bubble
                        .bubble(auto_bid)
                        .map(|g| g.last_used_style.clone())
                    {
                        if let Some(panel) = app.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.bind_style_store(style_arc);
                        }
                    }
                }
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();

            // Ensure Binance connector and request paginated bars.
            bridge.ensure_connector(exchange_id);
            bridge.request_bars(exchange_id, "BTCUSDT", &mlc_core::state::Timeframe::new("1H", 60), digdigdig3::AccountType::Spot, None, Some(app.panel_app.user_manager.profile.bar_count as usize), false);
        }

        // The full set of enabled connectors is NOT connected here (that fired 21
        // simultaneous dials in the constructor). They warm up once from tick()
        // via warmup_connectors() → bridge.warmup_all_connectors (bridge-guarded,
        // off the UI thread) during the skeleton/loading screen. The bridge is
        // shared and outlives the skeleton→live promote, so by the time the chart
        // appears the connectors are up and the live window does no connector work.
        // The exchanges actually NEEDED right away (active window + watchlist) are
        // connected eagerly above/below this point.

        // Request the full symbol list from Binance (already started above).
        // Moex symbols will be requested via ConnectorReady once it initialises.
        bridge.request_symbols(exchange_id);

        // Subscribe to mini ticker updates for each symbol in the active watchlist,
        // using each symbol's own exchange.
        if let Some(wl) = app.sidebar_state.watchlist_manager.active_list() {
            for ws in wl.all_symbols() {
                let ws_exchange = digdigdig3::ExchangeId::from_str(&ws.exchange)
                    .unwrap_or(exchange_id);
                if !app.sidebar_state.connector_enabled.get(ws_exchange.as_str()).copied().unwrap_or(true) {
                    continue;
                }
                let ws_at = account_type_from_label(&ws.account_type);
                bridge.ensure_connector(ws_exchange);
                bridge.subscribe_mini_ticker(ws_exchange, &ws.symbol, ws_at);
            }
        }

        // Register text fields on the InputCoordinator's TextFieldStore.
        {
            let mut coord = app.input_coordinator.borrow_mut();
            let tf = coord.text_fields_mut();
            tf.register(text_input::HEX_COLOR, TextFieldConfig::text()
                .with_filter(|c| c == '#' || c.is_ascii_hexdigit())
                .with_max_len(9));
            tf.register(text_input::AGENT_PTY, TextFieldConfig::raw());
            tf.register(text_input::AGENT_CHAT, TextFieldConfig::text());
            // Bar-mode parameter: digits and decimal point only, max 12 chars.
            tf.register(text_input::BAR_MODE_PARAM, TextFieldConfig::text()
                .with_filter(|c| c.is_ascii_digit() || c == '.')
                .with_max_len(12));
        }

        // Populate sub_panes from the real indicator_manager.
        app.sync_sub_panes_from_manager();

        app
    }

    /// Create a blank chart window without loading the user profile from disk.
    ///
    /// Use this when spawning additional windows in a multi-window setup.
    /// The window starts with Binance, "BTCUSDT", "1H" and default indicators.
    /// Create a new chart application that shares an existing [`DataBridge`].
    ///
    /// Unlike [`new`], this does **not** spin up a second tokio runtime or
    /// connector pool.  The provided `bridge` is shared with the caller (and
    /// any other windows created from the same bridge). A new independent
    /// broadcast receiver is created via [`DataBridge::add_listener`] so that
    /// this window gets its own queue of live updates.
    pub fn new_empty(bridge: std::sync::Arc<DataBridge>) -> Self {
        let live_update_rx = bridge.add_listener();

        let mut app = Self {
            panel_app: ChartPanelApp::new(""),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            indicator_manager: IndicatorManager::new(),
            modal_state: ModalState::new(),
            pending_screenshot: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            viewport_before_drag: None,
            color_picker_drag: None,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            managers: mlc_core::managers::Managers::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: mlc_sidebar_content::state::SidebarState::new(),
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            watchlist_modal: WatchlistModalState::new(),
            last_watchlist_modal_result: None,
            wl_group_name_input: WatchlistGroupNameInputState::new(),
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            bridge: bridge.clone(),
            live_update_rx,
            mini_ticker_cache: std::collections::HashMap::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            // Seed from the bridge's session-long symbol cache so that windows
            // created after the connect wave (notably the live window after
            // skeleton→live promote) start with every venue already loaded
            // instead of waiting for re-broadcast that never comes.
            exchange_symbols: bridge.symbols_snapshot(),
            window_id: format!(
                "win_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: false,
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            last_backfill_time: std::time::Instant::now(),
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: std::time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            last_sidebar_data_mark: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            mlc_trading_manager: None,
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
        };

        app.sidebar_state.watchlist_manager = mlc_sidebar_content::watchlist::WatchlistManager::new(
            vec![
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
            ],
        );

        // Fresh state — Binance default
        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        // Phase 4 B: capture chart_id BEFORE the mutating block.
        let active_chart_id = app.panel_app.panel_grid.active_container_id();
        let active_pid_init2 = app.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
        if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
            window.data_provider = data_provider.clone();
            window.toolbar_config = ToolbarConfig::standalone();
        }
        if let Some(pid) = active_pid_init2 {
            crate::chart_panel_bridge::drawing_manager_for_panel_mut(&mut app.panels_store, pid)
                .set_current_symbol_key("BTCUSDT", &exchange_name, "S");
        }
        // Create auto-bubble for the initial chart and assign instrument.
        if let Some(chart_id) = active_chart_id {
            let auto_bid = app.managers.bubble.create_group_auto(
                [0.0, 0.0, 0.0, 0.0],
                "BTCUSDT".to_string(),
                mlc_core::state::Timeframe::new("1H", 60),
            );
            let _ = app.managers.bubble.attach_chart_to_bubble(chart_id, auto_bid);
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.auto_bubble_id = auto_bid;
                window.current_bubble_id = auto_bid;
            }
            if let Some(bubble) = app.managers.bubble.bubble_mut(auto_bid) {
                bubble.symbol = "BTCUSDT".to_string();
                bubble.timeframe = mlc_core::state::Timeframe::new("1H", 60);
                bubble.exchange = exchange_name.clone();
                bubble.account_type = "S".to_string();
            }
            if let Some(pid) = active_pid_init2 {
                if let Some(style_arc) = app.managers.bubble
                    .bubble(auto_bid)
                    .map(|g| g.last_used_style.clone())
                {
                    if let Some(panel) = app.panels_store.chart_panel.get_mut(&pid) {
                        panel.drawing_manager.bind_style_store(style_arc);
                    }
                }
            }
        }
        app.panel_app.toolbar_config = ToolbarConfig::standalone();

        // Create a single "Untitled" preset tab.
        let untitled_preset = mlc_core::preset::preset::ChartPreset::new("Untitled".to_string());
        let untitled_id = untitled_preset.id.clone();
        app.panel_app.presets.insert(untitled_id.clone(), untitled_preset);
        app.panel_app.open_tabs = vec![untitled_id.clone()];
        app.panel_app.active_preset_id = untitled_id;

        // Ensure Binance connector and request bars.
        bridge.ensure_connector(exchange_id);
        bridge.request_bars(
            exchange_id,
            "BTCUSDT",
            &mlc_core::state::Timeframe::new("1H", 60),
            digdigdig3::AccountType::Spot,
            None,
            Some(app.panel_app.user_manager.profile.bar_count as usize),
            false,
        );

        // Register text fields on the InputCoordinator's TextFieldStore.
        {
            let mut coord = app.input_coordinator.borrow_mut();
            let tf = coord.text_fields_mut();
            tf.register(text_input::HEX_COLOR, TextFieldConfig::text()
                .with_filter(|c| c == '#' || c.is_ascii_hexdigit())
                .with_max_len(9));
            tf.register(text_input::AGENT_PTY, TextFieldConfig::raw());
            tf.register(text_input::AGENT_CHAT, TextFieldConfig::text());
            // Bar-mode parameter: digits and decimal point only, max 12 chars.
            tf.register(text_input::BAR_MODE_PARAM, TextFieldConfig::text()
                .with_filter(|c| c.is_ascii_digit() || c == '.')
                .with_max_len(12));
        }

        app.needs_initial_viewport_fit = true;
        app.sync_sub_panes_from_manager();

        app
    }

    // -------------------------------------------------------------------------
    // new_window — unified per-window constructor (no primary/secondary distinction)
    // -------------------------------------------------------------------------

    /// Create a per-window chart instance.
    ///
    /// All windows are equal — no primary/secondary distinction.
    /// The bridge, profile, and user state are owned by the App (main.rs).
    ///
    /// `user_manager` is loaded once at application startup and passed in here
    /// so that presets, templates, and profile data are not re-read from disk
    /// for each window.
    pub fn new_window(
        bridge: std::sync::Arc<DataBridge>,
        live_update_rx: tokio::sync::broadcast::Receiver<LiveUpdate>,
        window_id: String,
        restore: Option<&mlc_core::WindowState>,
        profile: &mlc_core::UserProfile,
        user_manager: &mlc_core::ProfileManager,
        skeleton: bool,
    ) -> Self {
        let mut app = Self {
            panel_app: ChartPanelApp::new(""),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            indicator_manager: IndicatorManager::new(),
            modal_state: ModalState::new(),
            pending_screenshot: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            viewport_before_drag: None,
            color_picker_drag: None,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            managers: mlc_core::managers::Managers::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: mlc_sidebar_content::state::SidebarState::new(),
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            watchlist_modal: WatchlistModalState::new(),
            last_watchlist_modal_result: None,
            wl_group_name_input: WatchlistGroupNameInputState::new(),
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            bridge: bridge.clone(),
            live_update_rx,
            mini_ticker_cache: std::collections::HashMap::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            // Seed from the bridge's session-long symbol cache (see new_empty).
            exchange_symbols: bridge.symbols_snapshot(),
            window_id,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: false,
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            last_backfill_time: std::time::Instant::now(),
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: std::time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            last_sidebar_data_mark: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            mlc_trading_manager: None,
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
        };

        // Initialize watchlist with a minimal default — overwritten by load_user_state below.
        app.sidebar_state.watchlist_manager = mlc_sidebar_content::watchlist::WatchlistManager::new(
            vec![
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                mlc_sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
            ],
        );

        // Phase 4: use app-level UserManager (loaded once in main.rs) instead of
        // calling load_user_state() per-window.  Eliminates redundant disk reads
        // when multiple windows are opened.
        app.panel_app.template_manager = user_manager.template_manager.clone();
        app.panel_app.presets = user_manager.presets.clone();

        // Always restore active_preset_id from profile (not just on restore).
        // This prevents skeleton launches from creating spurious new Untitled presets.
        if !user_manager.profile.active_preset_id.is_empty()
            && app.panel_app.presets.contains_key(&user_manager.profile.active_preset_id)
        {
            app.panel_app.active_preset_id = user_manager.profile.active_preset_id.clone();
        }

        // Restore open_tabs from profile — both for saved windows and promote_skeleton().
        // Without this, promote_skeleton (restore=None) would leave open_tabs empty,
        // causing the phantom "__default__" Chart tab to appear.
        {
            app.panel_app.open_tabs = user_manager.profile.open_tabs.clone();
            app.panel_app.open_tabs.retain(|id| app.panel_app.presets.contains_key(id));

            // Migration: old data has no open_tabs — open only the active preset.
            if app.panel_app.open_tabs.is_empty() && !app.panel_app.presets.is_empty() {
                if !app.panel_app.active_preset_id.is_empty()
                    && app.panel_app.presets.contains_key(&app.panel_app.active_preset_id)
                {
                    app.panel_app.open_tabs = vec![app.panel_app.active_preset_id.clone()];
                } else {
                    // No active preset known — open the newest one.
                    let mut all: Vec<_> = app.panel_app.presets.values().collect();
                    all.sort_by_key(|p| std::cmp::Reverse(p.created_at));
                    if let Some(newest) = all.first() {
                        app.panel_app.open_tabs = vec![newest.id.clone()];
                    }
                }
            }
        }

        // Keep profile + snapshots in user_manager.
        app.panel_app.user_manager.profile = user_manager.profile.clone();
        app.panel_app.user_manager.snapshots = user_manager.snapshots.clone();

        dlog!(
            "[ChartApp] new_window: using {} presets, {} prim-templates, {} ind-templates",
            app.panel_app.presets.len(),
            app.panel_app.template_manager.primitive_templates.len(),
            app.panel_app.template_manager.indicator_templates.len(),
        );

        // Load watchlists from disk (profile.json is NOT re-read here — it was already
        // loaded once at startup and is passed in via `profile`).
        app.load_watchlists();

        // Apply per-window profile state (sidebar width, inline bar, connector prefs).
        // Pass the saved WindowState so per-window sidebar fields take precedence
        // over the legacy flat profile fields (backwards-compatible fallback).
        app.apply_profile_state(profile, restore);

        // If restoring a saved window, apply its tab/preset state.
        if let Some(ws) = restore {
            let valid_tabs: Vec<String> = ws.open_tabs.iter()
                .filter(|id| app.panel_app.presets.contains_key(*id))
                .cloned()
                .collect();
            if !valid_tabs.is_empty() {
                app.panel_app.open_tabs = valid_tabs;
            }
            if !ws.active_preset_id.is_empty()
                && app.panel_app.presets.contains_key(&ws.active_preset_id)
            {
                app.panel_app.active_preset_id = ws.active_preset_id.clone();
            }
        }

        // Setup data provider and load preset or fresh state.
        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        if skeleton {
            // Skeleton is a pure loading screen — no presets, no connectors,
            // no data providers.  Everything happens after promote_skeleton().
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();
        } else {
            let has_saved_preset = app.panel_app.presets.contains_key(&app.panel_app.active_preset_id);

            if has_saved_preset {
                // Restore from saved preset.
                if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                    window.data_provider = data_provider.clone();
                    window.toolbar_config = ToolbarConfig::standalone();
                }
                app.panel_app.toolbar_config = ToolbarConfig::standalone();

                let preset_id = app.panel_app.active_preset_id.clone();
                // Clear active_preset_id so LoadPreset doesn't skip with
                // "already active" — the constructor set it above, but the
                // preset windows haven't been built yet.
                app.panel_app.active_preset_id = String::new();
                app.process_chart_out_event(
                    mlc_core::events::ChartOutEvent::LoadPreset { id: preset_id },
                );

                // Ensure all windows have data_provider and toolbar config.
                // Phase 4 B: read exchange/account_type from branch_meta.
                let window_data: Vec<(String, String, mlc_core::state::Timeframe, String)> = {
                    let containers = app.panel_app.panel_grid.containers();
                    containers.keys()
                        .filter_map(|cid| {
                            let sym = app.managers.bubble.symbol_for_chart(*cid).map(String::from).unwrap_or_default();
                            let exch = app.managers.bubble.exchange_for_chart(*cid).map(String::from).unwrap_or_default();
                            let tf = app.managers.bubble.timeframe_for_chart(*cid).cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = app.managers.bubble.account_type_for_chart(*cid).map(String::from).unwrap_or_else(|| "S".to_string());
                            Some((sym, exch, tf, at))
                        })
                        .collect()
                };

                let provider_seeds_win: Vec<(mlc_core::state::ChartId, String, String)> = {
                    app.panel_app.panel_grid.containers().keys()
                        .map(|cid| {
                            let exch = app.managers.bubble.exchange_for_chart(*cid).map(String::from).unwrap_or_default();
                            let at = app.managers.bubble.account_type_for_chart(*cid).map(String::from).unwrap_or_else(|| "S".to_string());
                            (*cid, exch, at)
                        })
                        .collect()
                };
                for (cid, exch, at_label) in provider_seeds_win {
                    if let Some(window) = app.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                        let win_exchange_id = digdigdig3::ExchangeId::from_str(&exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        let win_at = account_type_from_label(&at_label);
                        let win_provider: SharedDataProvider = std::sync::Arc::new(
                            LiveDataProvider::new(win_exchange_id, &exch, win_at, bridge.clone()),
                        );
                        window.data_provider = win_provider;
                        window.toolbar_config = ToolbarConfig::standalone();
                    }
                }

                // S2.7.1: read exchange via branch_meta instead of struct field.
                if let Some(cid) = app.panel_app.panel_grid.active_container_id() {
                    let exch = app.managers.bubble.exchange_for_chart(cid)
                        .unwrap_or("");
                    app.active_exchange = digdigdig3::ExchangeId::from_str(exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                }

                for (sym, exch, tf, at_label) in &window_data {
                    let eid = digdigdig3::ExchangeId::from_str(exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    if !app.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                        continue;
                    }
                    let at = account_type_from_label(at_label);
                    bridge.ensure_connector(eid);
                    bridge.request_bars(eid, sym, tf, at, None, Some(app.panel_app.user_manager.profile.bar_count as usize), false);
                }

                app.needs_initial_viewport_fit = true;
                // NOTE: background-tab warm-up is intentionally NOT done here.
                // When this runs inside the off-thread promote worker, firing
                // ~11 series requests floods the broadcast channel with
                // BarsLoaded before any live window exists to receive them;
                // the first live tick then drains that backlog through the
                // full (no-match) matching path — the bulk of the ~228ms
                // first-frame freeze. The warm-up is driven AFTER the first
                // live frame instead (app-vello: pending_warm_targets, one
                // series per frame), so the cache still heats without blocking
                // the promote-to-chart transition.
            } else {
                // Fresh state — Binance default.
                // Phase 4 B: capture chart_id BEFORE the mutating block.
                let active_chart_id = app.panel_app.panel_grid.active_container_id();
                let active_pid_init3 = app.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                    window.data_provider = data_provider.clone();
                    window.toolbar_config = ToolbarConfig::standalone();
                }
                if let Some(pid) = active_pid_init3 {
                    crate::chart_panel_bridge::drawing_manager_for_panel_mut(&mut app.panels_store, pid)
                        .set_current_symbol_key("BTCUSDT", &exchange_name, "S");
                }
                // Create auto-bubble for the initial chart and assign instrument.
                if let Some(chart_id) = active_chart_id {
                    let auto_bid = app.managers.bubble.create_group_auto(
                        [0.0, 0.0, 0.0, 0.0],
                        "BTCUSDT".to_string(),
                        mlc_core::state::Timeframe::new("1H", 60),
                    );
                    let _ = app.managers.bubble.attach_chart_to_bubble(chart_id, auto_bid);
                    if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                        window.auto_bubble_id = auto_bid;
                        window.current_bubble_id = auto_bid;
                    }
                    if let Some(bubble) = app.managers.bubble.bubble_mut(auto_bid) {
                        bubble.symbol = "BTCUSDT".to_string();
                        bubble.timeframe = mlc_core::state::Timeframe::new("1H", 60);
                        bubble.exchange = exchange_name.clone();
                        bubble.account_type = "S".to_string();
                    }
                    if let Some(pid) = active_pid_init3 {
                        if let Some(style_arc) = app.managers.bubble
                            .bubble(auto_bid)
                            .map(|g| g.last_used_style.clone())
                        {
                            if let Some(panel) = app.panels_store.chart_panel.get_mut(&pid) {
                                panel.drawing_manager.bind_style_store(style_arc);
                            }
                        }
                    }
                }
                app.panel_app.toolbar_config = ToolbarConfig::standalone();

                // Create "Untitled 1" preset for genuinely fresh windows
                // (no presets at all — brand new profile).  Uses same numbering
                // logic as OpenPresetNewChart (input.rs) for consistency.
                if app.panel_app.presets.is_empty() {
                    let max_n = app.panel_app.presets.values()
                        .filter_map(|p| p.name.strip_prefix("Untitled "))
                        .filter_map(|s| s.parse::<u32>().ok())
                        .max()
                        .unwrap_or(0);
                    let untitled_preset = mlc_core::preset::preset::ChartPreset::new(format!("Untitled {}", max_n + 1));
                    let untitled_id = untitled_preset.id.clone();
                    app.panel_app.presets.insert(untitled_id.clone(), untitled_preset);
                    app.panel_app.open_tabs = vec![untitled_id.clone()];
                    app.panel_app.active_preset_id = untitled_id;
                }

                bridge.ensure_connector(exchange_id);
                bridge.request_bars(
                    exchange_id,
                    "BTCUSDT",
                    &mlc_core::state::Timeframe::new("1H", 60),
                    digdigdig3::AccountType::Spot,
                    None,
                    Some(app.panel_app.user_manager.profile.bar_count as usize),
                    false,
                );
            }
        }

        // Connectors warm up once from tick() via warmup_connectors() (bridge-
        // guarded, off the UI thread, during the skeleton screen), not here.
        // Exchanges needed immediately (window + watchlist) connect eagerly above.

        if !skeleton {
            bridge.request_symbols(exchange_id);
        }

        // Subscribe mini tickers for active watchlist.
        if !skeleton {
            if let Some(wl) = app.sidebar_state.watchlist_manager.active_list() {
                for ws in wl.all_symbols() {
                    let ws_exchange = digdigdig3::ExchangeId::from_str(&ws.exchange)
                        .unwrap_or(exchange_id);
                    if !app.sidebar_state.connector_enabled.get(ws_exchange.as_str()).copied().unwrap_or(true) {
                        continue;
                    }
                    let ws_at = account_type_from_label(&ws.account_type);
                    bridge.ensure_connector(ws_exchange);
                    bridge.subscribe_mini_ticker(ws_exchange, &ws.symbol, ws_at);
                }
            }
        }

        // Register text fields on the InputCoordinator's TextFieldStore.
        {
            let mut coord = app.input_coordinator.borrow_mut();
            let tf = coord.text_fields_mut();
            tf.register(text_input::HEX_COLOR, TextFieldConfig::text()
                .with_filter(|c| c == '#' || c.is_ascii_hexdigit())
                .with_max_len(9));
            tf.register(text_input::AGENT_PTY, TextFieldConfig::raw());
            tf.register(text_input::AGENT_CHAT, TextFieldConfig::text());
            // Bar-mode parameter: digits and decimal point only, max 12 chars.
            tf.register(text_input::BAR_MODE_PARAM, TextFieldConfig::text()
                .with_filter(|c| c.is_ascii_digit() || c == '.')
                .with_max_len(12));
        }

        app.sync_sub_panes_from_manager();
        app
    }

    /// Apply profile-level state to this window instance.
    ///
    /// Sets sidebar width/panel, inline bar position/dock, and connector enabled state.
    /// When `window_state` is provided its per-window sidebar fields take precedence over
    /// the flat fields on `profile` (backwards-compatible: old profiles without per-window
    /// sidebar data fall back to the profile-level flat fields automatically).
    pub fn apply_profile_state(
        &mut self,
        profile: &mlc_core::UserProfile,
        window_state: Option<&mlc_core::WindowState>,
    ) {
        // Sidebar width — prefer per-window, fall back to profile flat field.
        let sidebar_width = window_state
            .and_then(|ws| ws.sidebar_width)
            .or(profile.sidebar_width);
        if let Some(width) = sidebar_width {
            self.sidebar_state.set_right_width(width);
        }

        // Sidebar panel/visibility — prefer per-window, fall back to profile flat fields.
        let sidebar_visible = window_state.map(|ws| ws.sidebar_visible).unwrap_or(profile.sidebar_visible);
        let sidebar_panel = window_state
            .and_then(|ws| ws.sidebar_panel.as_ref())
            .or(profile.sidebar_panel.as_ref());
        if sidebar_visible {
            if let Some(panel_name) = sidebar_panel {
                let panel = Self::str_to_panel(panel_name);
                self.sidebar_state.set_right_panel(panel);
            }
        }

        // Connector enabled/disabled state — always from profile (app-level, not per-window).
        if !profile.connector_enabled.is_empty() {
            self.sidebar_state.connector_enabled = profile.connector_enabled.clone();
        }

        // Inline bar position/dock — prefer per-window, fall back to profile flat fields.
        let inline_bar_x = window_state
            .and_then(|ws| ws.inline_bar_x)
            .or(profile.inline_bar_x);
        let inline_bar_y = window_state
            .and_then(|ws| ws.inline_bar_y)
            .or(profile.inline_bar_y);
        let inline_bar_dock = window_state
            .and_then(|ws| ws.inline_bar_dock.as_ref())
            .or(profile.inline_bar_dock.as_ref());
        if let Some(x) = inline_bar_x {
            self.panel_app.toolbar_state.floating_inline_bar.x = x;
        }
        if let Some(y) = inline_bar_y {
            self.panel_app.toolbar_state.floating_inline_bar.y = y;
        }
        if let Some(dock) = inline_bar_dock {
            self.panel_app.toolbar_state.floating_inline_bar.dock_edge = match dock.as_str() {
                "Top" => mlc_core::InlineDockEdge::Top,
                "Free" => mlc_core::InlineDockEdge::Free,
                _ => mlc_core::InlineDockEdge::Bottom,
            };
        }

        // Notification settings — load from profile into alert settings state.
        dlog!(
            "[ChartApp] apply_profile_state: tg_enabled={} token_len={} subscribers={}",
            profile.notification_settings.telegram.enabled,
            profile.notification_settings.telegram.bot_token.len(),
            profile.notification_settings.telegram.subscribers.len(),
        );
        self.panel_app.alert_settings_state.notification_settings = profile.notification_settings.clone();
        self.panel_app.alert_settings_state.tg_bot_token_input = profile.notification_settings.telegram.bot_token.clone();

        // Agents docking container restore — per-window profile state.
        // Layout topology (tree shape + splits) is rebuilt from LayoutSnapshot.
        // Per-leaf descriptors are inserted into `agent_leaves`; no sessions
        // are spawned — PTY leaves show "Click Start" skeleton, Chat leaves
        // lazily resume via `--resume <chat_session_id>` on first interaction.
        if let Some(ws) = window_state {
            log::info!(
                "[agents-diag] apply_profile_state: layout_json={} persisted_leaves={}",
                if ws.agents_tab_layout.is_some() { "Some" } else { "None" },
                ws.agents_tab_leaves.len(),
            );
            if let Some(layout_json) = &ws.agents_tab_layout {
                match uzor::panels::serialize::LayoutSnapshot::from_json(layout_json) {
                    Ok(snap) => {
                        // Build a lookup of persisted leaves by their numeric id
                        // so restore_tree can reconstruct the right AgentPaneLeaf
                        // payload for each leaf in the snapshot.
                        let by_id: std::collections::HashMap<u64, &mlc_core::PersistedAgentLeaf> =
                            ws.agents_tab_leaves.iter().map(|p| (p.leaf_id, p)).collect();

                        // Dummy descriptor used if the snapshot references a leaf
                        // id that has no matching PersistedAgentLeaf (defensive
                        // fallback — should never happen for well-formed profiles).
                        let fallback_cli = gate4agent::AgentCli::Claude;
                        let fallback_mode = gate4agent::InstanceMode::Pty;

                        // Rebuild the docking tree. `restore_tree` invokes the
                        // closure once per leaf — we return a zero-instance
                        // `AgentPaneLeaf` keyed by cli/mode so rendering works;
                        // the real InstanceId is stored in `agent_leaves`.
                        let restore_result = snap.restore_tree_with_id(|leaf_id, _type_id| {
                            let (cli, mode) = if let Some(persisted) = by_id.get(&leaf_id) {
                                let cli = match persisted.cli {
                                    mlc_core::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                    mlc_core::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                    mlc_core::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                    mlc_core::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                };
                                let mode = match persisted.mode {
                                    mlc_core::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                    mlc_core::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                };
                                (cli, mode)
                            } else {
                                (fallback_cli, fallback_mode)
                            };
                            Some(mlc_sidebar_content::AgentPaneLeaf {
                                instance_id: gate4agent::InstanceId::new(),
                                cli,
                                mode,
                            })
                        });

                        match restore_result {
                            Ok(tree) => {
                                // Replace the docking manager with the restored tree.
                                *self.sidebar_state.agent_docking.inner_mut() =
                                    uzor::layout::DockState::from_tree(tree);

                                // Rebuild the agent_leaves map from persisted descriptors.
                                // Each leaf gets a registered instance via create_instance so
                                // the Start button can find the ID in MultiCliManager's map.
                                self.sidebar_state.agent_leaves.clear();
                                for persisted in &ws.agents_tab_leaves {
                                    let cli = match persisted.cli {
                                        mlc_core::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                        mlc_core::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                        mlc_core::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                        mlc_core::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                    };
                                    let mode = match persisted.mode {
                                        mlc_core::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                        mlc_core::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                    };
                                    let leaf_id = uzor::panels::LeafId(persisted.leaf_id);
                                    let workdir = persisted.workdir.clone();
                                    let _ = std::fs::create_dir_all(&workdir);
                                    match self.agent.create_instance(cli, mode, workdir.clone()) {
                                        Ok(instance_id) => {
                                            let desc = mlc_sidebar_content::agents_dock::AgentLeafDescriptor {
                                                instance_id,
                                                cli,
                                                mode,
                                                workdir,
                                                chat_session_id: persisted.chat_session_id.clone(),
                                            };
                                            self.sidebar_state.agent_leaves.insert(leaf_id, desc);
                                        }
                                        Err(e) => {
                                            eprintln!("[ChartApp] agents restore create_instance failed: {}", e);
                                        }
                                    }
                                }
                                dlog!(
                                    "[ChartApp] agents docking restored: {} leaves",
                                    self.sidebar_state.agent_leaves.len()
                                );
                                log::info!(
                                    "[agents-diag] restore OK: {} leaves in agent_leaves",
                                    self.sidebar_state.agent_leaves.len()
                                );
                            }
                            Err(e) => {
                                eprintln!("[ChartApp] agents restore_tree failed: {}", e);
                                log::warn!("[agents-diag] restore_tree FAILED: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[ChartApp] agents layout deserialize failed: {}", e);
                        log::warn!("[agents-diag] layout deserialize FAILED: {}", e);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Bridge access
    // -------------------------------------------------------------------------

    /// Return a clone of the `Arc<DataBridge>` so it can be shared with other
    /// windows created via [`new_empty`].
    pub fn bridge(&self) -> std::sync::Arc<DataBridge> {
        self.bridge.clone()
    }

    // -------------------------------------------------------------------------
    // Screenshot
    // -------------------------------------------------------------------------

    /// Request a screenshot to be taken on the next rendered frame.
    ///
    /// The vello renderer drains this flag via `drain_pending_screenshot()` and
    /// performs GPU readback to capture the frame as a PNG file.
    pub fn request_screenshot(&mut self) {
        self.pending_screenshot = true;
        dlog!("[Screenshot] Capture requested");
    }

    // -------------------------------------------------------------------------
    // Live exchange support
    // -------------------------------------------------------------------------

    /// Switch the active chart window to use live data from a real exchange.
    ///
    /// Starts the connector asynchronously and immediately requests bars for
    /// the current symbol / timeframe. The `tick()` method polls the bridge
    /// channel each frame and feeds bars into the window once they arrive.
    pub fn switch_to_exchange(&mut self, exchange_id: digdigdig3::ExchangeId) {
        if !self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
            dlog!("[ChartApp] Exchange {} is disabled, skipping", exchange_id.as_str());
            return;
        }
        // Capture old trade-stream identity BEFORE updating active_exchange or window state.
        let old_trade_exchange = self.active_exchange;
        // S2.7.1: read symbol/account_type via branch_meta instead of struct field.
        let old_chart_id = self.panel_app.panel_grid.active_container_id();
        let old_trade_symbol = old_chart_id
            .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let old_trade_at = old_chart_id
            .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
            .map(|at_str| account_type_from_label(at_str))
            .unwrap_or(digdigdig3::AccountType::Spot);

        let bridge = self.bridge.clone();
        self.active_exchange = exchange_id;

        // Start the connector (no-op if already running).
        bridge.ensure_connector(exchange_id);

        // Human-readable exchange name used by the window header.
        let exchange_name = exchange_id.as_str().to_string();

        // Use the active window's account_type for the new provider.
        // S2.7.1: read account_type via branch_meta.
        let switch_at = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
            .map(account_type_from_label)
            .unwrap_or(digdigdig3::AccountType::Spot);

        // Build a LiveDataProvider backed by this bridge.
        let provider: SharedDataProvider =
            std::sync::Arc::new(LiveDataProvider::new(
                exchange_id,
                exchange_name.clone(),
                switch_at,
                bridge.clone(),
            ));

        // Unsubscribe only the old symbol's trade stream on the old exchange,
        // leaving other windows' streams intact.
        if !old_trade_symbol.is_empty() {
            bridge.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
        }

        // Phase 4 S2.2: capture chart_id BEFORE the mutating block.
        let active_chart_id = self.panel_app.panel_grid.active_container_id();
        // Attach the provider to the active window and request paginated bars.
        // Phase 4 B: read symbol/timeframe/account_type from branch_meta.
        let active_cid_exc = self.panel_app.panel_grid.active_container_id();
        let cur_sym_exc = active_cid_exc
            .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let cur_tf_exc = active_cid_exc
            .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
            .cloned()
            .unwrap_or_else(mlc_core::state::Timeframe::h1);
        let cur_at_label_exc = active_cid_exc
            .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
            .map(String::from)
            .unwrap_or_else(|| "S".to_string());
        let mut mirror_args: Option<(String, mlc_core::state::Timeframe, String, String)> = None;
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.data_provider = provider;
            let at = account_type_from_label(&cur_at_label_exc);
            mirror_args = Some((cur_sym_exc.clone(), cur_tf_exc.clone(), exchange_name.clone(), cur_at_label_exc.clone()));
            bridge.request_bars(exchange_id, &cur_sym_exc, &cur_tf_exc, at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
        }
        if let (Some(chart_id), Some((_, _, exch, _))) = (active_chart_id, mirror_args) {
            if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                    bubble.exchange = exch;
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Preset / tab management — public wrappers for the chrome tab system
    // -------------------------------------------------------------------------

    /// Load a preset by its id.
    ///
    /// Fires the `LoadPreset` chart-out event, which restores the full layout,
    /// windows, indicators, and settings saved under that preset id.
    pub fn load_preset(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::LoadPreset {
            id: id.to_string(),
        });
    }

    /// Close a tab without deleting the preset from disk.
    ///
    /// Fires `CloseTab` which removes the ID from `open_tabs` and switches
    /// to an adjacent tab if the closed tab was active.
    pub fn close_tab(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::CloseTab {
            id: id.to_string(),
        });
    }

    /// Open the "new chart" flow (shows the preset-name input modal).
    pub fn new_chart(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::NewChart);
    }

    /// Open an existing (closed) preset as a new tab.
    pub fn open_tab(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenTab {
            id: id.to_string(),
        });
    }

    /// Open the chart browser modal (list of all saved presets).
    pub fn open_chart_browser(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenChartBrowser);
    }

    /// Open the chart settings modal.
    pub fn open_settings(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenChartSettings);
    }

    /// Open (or toggle) the user settings modal.
    ///
    /// Called when the chrome gear button is clicked.
    pub fn open_user_settings(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenUserSettings);
    }

    /// Drain the pending screenshot request.
    ///
    /// Returns `true` if a screenshot was requested since the last call.
    /// Called by the vello renderer each frame in `about_to_wait`.
    pub fn drain_pending_screenshot(&mut self) -> bool {
        let pending = self.pending_screenshot;
        self.pending_screenshot = false;
        pending
    }

    /// Return the chart content rect (toolbar-excluded area) from the last render.
    pub fn content_rect(&self) -> &LayoutRect {
        &self.content_rect
    }

    /// Return pixel crop coordinates `(x, y, width, height)` for screenshot
    /// cropping.  Derived from the content_rect set during the last render().
    pub fn screenshot_rect(&self) -> (u32, u32, u32, u32) {
        let r = &self.content_rect;
        (r.x as u32, r.y as u32, r.width as u32, r.height as u32)
    }

    // -------------------------------------------------------------------------
    // Resize
    // -------------------------------------------------------------------------

    /// Update screen dimensions and immediately sync viewport to the new
    /// candle area so that `tick()` (called before `render()`) uses correct
    /// dimensions for Focus-mode follow calculations.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        // Refresh the cached layout so sync_viewport_from_layout reads fresh
        // geometry — resize runs outside prepare_frame where the layout phase
        // has not yet run, so active_frame_layout could be stale or None.
        self.active_frame_layout = Some(self.build_extended_layout());
        self.sync_viewport_from_layout();

        // On the very first resize after a preset restore, chart_width is now
        // known. Reposition every window so the last bar has 5 empty bars of
        // right margin (set_bars() couldn't do this because chart_width was 0).
        if self.needs_initial_viewport_fit {
            self.needs_initial_viewport_fit = false;
            for window in self.panel_app.panel_grid.containers_mut().values_mut() {
                // Only snap if chart dimensions are valid and bars are loaded.
                if window.viewport.chart_width > 0.0 && window.viewport.bar_spacing > 0.0 && !window.bars.is_empty() {
                    window.snap_to_end(mlc_core::DEFAULT_SNAP_MARGIN);
                }
                window.calc_auto_scale();
            }
        }
    }

    /// Recompute viewport chart_width/chart_height from the current window
    /// size, toolbar config, and scale settings.  Called from `resize()` and
    /// at the start of `render()`.
    ///
    /// Uses `build_extended_layout()` (which accounts for sub-panes) so that
    /// `viewport.chart_height` matches `main_chart.chart.height` used by the
    /// renderer.  Previously this called `FrameLayout::from_chart_panel_with_config`
    /// which does NOT subtract sub-pane heights, causing coordinate mismatches
    /// when RSI/MACD sub-panes are visible.
    fn sync_viewport_from_layout(&mut self) {
        // In split mode, per-leaf dimensions are handled by the split-pane layout
        // block in prepare_frame() which calls build_extended_layout_for_leaf().
        // For non-split mode we compute one layout and apply it to ALL windows so
        // that every window (not just the active one) gets a valid chart_width.
        // Without this, non-active windows keep chart_width = 0.0 which blocks the
        // needs_auto_scale_after_bars deferred snap from ever firing.
        if self.panel_app.panel_grid.is_split() {
            return;
        }
        // Prefer the frame-cached layout; fall back to building on demand so
        // that resize() (called outside prepare_frame, where active_frame_layout
        // may be stale/None) always uses fresh geometry.  Clone to end the
        // immutable borrow before the mutable containers_mut() call below.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
        let new_width = extended.main_chart.chart.width;
        let new_height = extended.main_chart.chart.height;
        for window in self.panel_app.panel_grid.containers_mut().values_mut() {
            let old_width = window.viewport.chart_width;
            // Pin right edge: shift view_start so the last visible bars stay
            // anchored when the window is resized, matching terminal behavior.
            // Skip bar_shift for windows still waiting for their initial snap
            // (needs_auto_scale_after_bars = true and old_width = 0) — the
            // deferred snap in prepare_frame() will compute view_start correctly
            // once chart_width is set here.
            if !window.needs_auto_scale_after_bars
                && (old_width - new_width).abs() > 0.5
                && window.viewport.bar_spacing > 0.0
                && old_width > 0.0
                && new_width > 0.0
            {
                let bar_shift = (old_width - new_width) / window.viewport.bar_spacing;
                window.viewport.view_start += bar_shift;
            }
            // Don't store negative/zero widths — they'd poison the next bar_shift calc.
            if new_width > 0.0 {
                window.viewport.chart_width = new_width;
            }
            if new_height > 0.0 {
                window.viewport.chart_height = new_height;
            }
        }
    }

    // -------------------------------------------------------------------------
    // Backfill after broadcast lag
    // -------------------------------------------------------------------------

    /// Re-fetches only the missing bars for all active chart windows after a
    /// broadcast `Lagged` event.
    ///
    /// Because `DataBridge::request_bars` uses incremental mode when a bar
    /// cache already exists for the symbol (fetching only bars newer than
    /// `last_cached_ts`), this call is cheap — it downloads the gap, not the
    /// full history.
    ///
    /// Call sites must check the debounce guard (`last_backfill_time`) before
    /// invoking this method.
    fn trigger_backfill_after_lag(&self) {
        // S2.7.1: iterate .iter() to get chart_id, then read via branch_meta.
        for (chart_id, _window) in self.panel_app.panel_grid.containers().iter() {
            let sym = match self.managers.bubble.symbol_for_chart(*chart_id) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let exch = self.managers.bubble.exchange_for_chart(*chart_id)
                .unwrap_or("");
            let eid = digdigdig3::ExchangeId::from_str(exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            let at_str = self.managers.bubble.account_type_for_chart(*chart_id)
                .unwrap_or("");
            let at = account_type_from_label(at_str);
            let tf = match self.managers.bubble.timeframe_for_chart(*chart_id) {
                Some(t) => t.clone(),
                None => continue,
            };
            // Passing `None` for both limit and total_bars lets the bridge
            // pick up incremental mode from its bar cache.
            self.bridge.request_bars(eid, sym, &tf, at, None, None, false);
        }
    }

    // -------------------------------------------------------------------------
    // Indicator recalculation helpers
    // -------------------------------------------------------------------------

    /// Recalculate indicators for every window that displays `symbol`.
    ///
    /// Iterates ALL windows with a matching symbol and calls
    /// `calculate_for_window()` for each, so that indicator instances scoped to
    /// a window are calculated against that window's own bars.  This is
    /// correct when multiple windows show the same symbol on different
    /// timeframes (each window has its own bar series keyed by its timeframe).
    fn recalc_indicators_for_symbol(&mut self, symbol: &str) {
        // Collect only the ChartId values (cheap u64 copies) for every window
        // showing this symbol, avoiding a full bars clone at this stage.
        // S2.7.1: filter by branch_meta symbol instead of w.symbol.
        let matching_ids: Vec<u64> = self
            .panel_app
            .panel_grid
            .containers()
            .iter()
            .filter(|(id, _w)| {
                self.managers.bubble.symbol_for_chart(**id)
                    .map_or(false, |s| s == symbol)
            })
            .map(|(id, _w)| id.0)
            .collect();

        // Split-borrow: `panel_app.panel_grid` and `indicator_manager` are
        // distinct fields, so Rust allows simultaneous borrows of both.
        for window_id in matching_ids {
            let chart_id = ChartId(window_id);
            if let Some(w) = self.panel_app.panel_grid.containers().get(&chart_id) {
                self.indicator_manager.calculate_for_window(window_id, &w.bars);
            }
        }
        let _ = symbol; // legacy — recalc filters by window_id, not symbol
        self.recalc_count += 1;
        self.sync_sub_panes_from_manager();
    }


    // -------------------------------------------------------------------------
    // Render
    // -------------------------------------------------------------------------

    // prepare_frame lives in frame_lifecycle/prepare.rs
    /// Convenience wrapper: calls `prepare_frame`, `render_to_scene`, then
    /// `apply_render_output` in sequence.
    ///
    /// Existing call sites can continue using this method unchanged.
    pub fn render(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64, skip_toolbar_draw: bool) {
        self.prepare_frame(self.width as f64, self.height as f64);
        let output = self.render_to_scene(ctx, current_time_ms, skip_toolbar_draw);
        self.apply_render_output(output);
    }

    /// Render only the cursor-dependent overlay for every visible chart leaf.
    ///
    /// This emits crosshair lines, scale cursor labels, drawing previews,
    /// tooltips, and OHLC legends on top of the already-rendered static chart.
    ///
    /// Call AFTER [`render`] (or [`render_to_scene`] + [`apply_render_output`])
    /// so that overlay elements appear on top of all chart content.
    ///
    /// The cached [`ExtendedFrameLayout`] built during [`prepare_frame`] is used
    /// directly — no geometry is recomputed here.
    pub fn render_overlay(&self, ctx: &mut dyn RenderContext) {
        use mlc_layout::layout::render_chart_overlay;
        use mlc_core::chart::ChartRect;

        let chart_theme = self.panel_app.chart_theme_for_render();
        let is_split = self.panel_app.panel_grid.is_split();

        if is_split {
            // Iterate all split leaves using the cached per-leaf layouts.
            let leaf_ids: Vec<_> = self.frame_layouts.keys().copied().collect();
            for leaf_id in leaf_ids {
                let layout = match self.frame_layouts.get(&leaf_id) {
                    Some(l) => l,
                    None => continue,
                };
                let window = match self.panel_app.panel_grid.container_for_leaf(leaf_id) {
                    Some(w) => w,
                    None => continue,
                };
                // S2.7.1: read instrument fields via branch_meta.
                let chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                let overlay_tf = chart_id
                    .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                    .cloned();
                let overlay_tf_name: String = overlay_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
                let overlay_tf_interval_ms: Option<i64> = overlay_tf.map(|tf| tf.interval_ms);
                let overlay_symbol: String = chart_id
                    .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();

                // Build a chart rect from the main chart area in the layout.
                let chart_rect = ChartRect::new(
                    layout.main_chart.chart.x,
                    layout.main_chart.chart.y,
                    layout.main_chart.chart.width,
                    layout.main_chart.chart.height,
                );
                // Phase 4 C: get panel state (grid/legend/chart_type/tooltip from panels_store).
                let overlay_panel_id = window.chart_panel_id;
                let overlay_default_panel = mlc_core::ChartPanelState::new(overlay_panel_id);
                let overlay_panel = self.panels_store.chart_panel
                    .get(&overlay_panel_id)
                    .unwrap_or(&overlay_default_panel);
                let render_state = window.to_render_state(
                    chart_rect,
                    &chart_theme,
                    Some(overlay_tf_name.as_str()),
                    Some(&window.scale_settings.time_format),
                    overlay_tf_interval_ms,
                    &overlay_panel.grid_options,
                    &overlay_panel.legend,
                );
                let crosshair_config = mlc_core::chart::CrosshairConfig::default();
                let render_config = ChartRenderConfig {
                    scale_theme: self.panel_app.scale_theme_for_render(),
                    chart_type: overlay_panel.chart_type,
                    crosshair_config,
                    ..ChartRenderConfig::default()
                };

                render_chart_overlay(
                    ctx,
                    &render_state,
                    layout,
                    &render_config,
                    None, // sub_pane_ranges not cached here; callers needing accurate labels cache from RenderOutput
                    Some(&overlay_panel.drawing_manager),
                    Some(&overlay_panel.tooltip),
                    Some(overlay_symbol.as_str()),
                    &overlay_tf_name,
                );
            }
        } else {
            // Single-window path: use active_frame_layout.
            let layout = match &self.active_frame_layout {
                Some(l) => l,
                None => return,
            };
            // Get the active leaf window.
            let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
            let window = active_leaf
                .and_then(|lid| self.panel_app.panel_grid.container_for_leaf(lid))
                .or_else(|| {
                    // Fallback: first window in the grid
                    self.panel_app.panel_grid.containers().values().next()
                });
            let window = match window {
                Some(w) => w,
                None => return,
            };
            // S2.7.1: read instrument fields via branch_meta for overlay.
            let single_overlay_cid = self.panel_app.panel_grid.active_container_id();
            let single_overlay_tf = single_overlay_cid
                .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                .cloned();
            let single_overlay_tf_name: String = single_overlay_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
            let single_overlay_tf_interval_ms: Option<i64> = single_overlay_tf.map(|tf| tf.interval_ms);
            let single_overlay_symbol: String = single_overlay_cid
                .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                .map(String::from)
                .unwrap_or_default();

            let chart_rect = ChartRect::new(
                layout.main_chart.chart.x,
                layout.main_chart.chart.y,
                layout.main_chart.chart.width,
                layout.main_chart.chart.height,
            );
            // Phase 4 C: panel state for single-window overlay.
            let single_pid = window.chart_panel_id;
            let single_default_panel = mlc_core::ChartPanelState::new(single_pid);
            let single_panel = self.panels_store.chart_panel
                .get(&single_pid)
                .unwrap_or(&single_default_panel);
            let render_state = window.to_render_state(
                chart_rect,
                &chart_theme,
                Some(single_overlay_tf_name.as_str()),
                Some(&window.scale_settings.time_format),
                single_overlay_tf_interval_ms,
                &single_panel.grid_options,
                &single_panel.legend,
            );
            let crosshair_config = mlc_core::chart::CrosshairConfig::default();
            let render_config = ChartRenderConfig {
                scale_theme: self.panel_app.scale_theme_for_render(),
                chart_type: single_panel.chart_type,
                crosshair_config,
                ..ChartRenderConfig::default()
            };

            render_chart_overlay(
                ctx,
                &render_state,
                layout,
                &render_config,
                None,
                Some(&single_panel.drawing_manager),
                Some(&single_panel.tooltip),
                Some(single_overlay_symbol.as_str()),
                &single_overlay_tf_name,
            );
        }
    }

    /// Pure-rendering pass.
    ///
    /// Emits all vector graphics into `ctx`, registers widget hit-zones with the
    /// `input_coordinator` (via interior mutability), and returns a [`RenderOutput`]
    /// containing the cached results of this frame.
    ///
    /// Call [`prepare_frame`] first to sync mutable state (viewport, sidebar data,
    /// etc.), then call [`apply_render_output`] afterward to persist the results.
    ///
    /// When `skip_toolbar_draw` is `true`, the toolbar vector graphics are NOT
    /// re-emitted into `ctx`.  Instead, hit zones are re-registered from the
    /// cached `self.last_toolbar_result` so input routing remains correct.
    pub fn render_to_scene(&mut self, ctx: &mut dyn RenderContext, _current_time_ms: u64, skip_toolbar_draw: bool) -> RenderOutput {
        let _rt0 = std::time::Instant::now();
        let w = self.width as f64;
        let h = self.height as f64;

        // Sidebar width — when open, the sidebar appears between the chart
        // content (price scale) and the right toolbar.  The right toolbar stays
        // fixed at the window edge.  Only the chart content area shrinks.
        //
        // In skeleton mode (profile manager / welcome wizard covering full screen),
        // hide the sidebar completely so the login area fills the entire content area.
        let sidebar_w = if self.panel_app.user_settings_state.show_profile_manager
            || self.panel_app.user_settings_state.show_welcome_wizard
        {
            0.0
        } else {
            self.sidebar_state.right_width()
        };

        // 1. Begin frame — propagate current pointer position
        let input_state = InputState::default()
            .with_pointer_pos(self.last_mouse_pos.0, self.last_mouse_pos.1);
        self.input_coordinator.borrow_mut().begin_frame(input_state);

        // 2. Full window rect for toolbar layout (right toolbar at window edge).
        let window_rect = LayoutRect::new(0.0, 0.0, w, h);

        // 3. Compute toolbar layout from full window (right toolbar at edge).
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Content rect for chart rendering — shrink by sidebar_w.
        // Sidebar appears between chart's right edge and right toolbar.
        // Right toolbar stays at window edge, unaffected.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        // content_rect and right_toolbar_left_x are returned in RenderOutput and
        // applied by apply_render_output().
        let out_right_toolbar_left_x = panel_layout.right_toolbar_rect.x;

        // 4. Render chart content.
        //    When split, iterate over panel_grid leaves and call
        //    render_full_chart_panel() for each with ToolbarConfig::minimal()
        //    (0 toolbars — full chart with scales, crosshair, sub-panes, etc.).
        //    When not split, use the original single-window path.

        // Local leaf tab hit zones for this frame — returned via RenderOutput.
        let mut out_leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_layout::layout::panel_overlay::LeafTabHitZones> = std::collections::HashMap::new();
        // Submenu state update from toolbar dropdown rendering.
        let mut out_open_submenu_update: Option<Option<String>> = None;
        // Sub-pane range writebacks: render computes symmetrized+padded ranges,
        // applied by apply_render_output() via container_for_leaf_mut.
        let mut out_sub_pane_range_writebacks: Vec<(mlc_core::LeafId, usize, f64, f64)> = Vec::new();
        // Sub-pane overlay result writebacks: cached per-window for hit testing.
        let mut out_sub_pane_overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)> = Vec::new();

        let _rt1 = std::time::Instant::now(); // checkpoint: before chart render
        let frame_theme = self.panel_app.frame_theme_for_render();
        let leaf_tab_toolbar_theme = self.panel_app.toolbar_theme_for_render();
        // main_chart_y is set in the single-window branch below; used by the
        // indicator overlay chevron rendered after the chart block.
        let mut main_chart_y_single = content_rect.y;
        // False while a maximized sub-pane covers the single-mode container —
        // chart-anchored overlays (leaf tab, chevron) are skipped then.
        let mut single_leaf_tab_visible = true;
        let corner_zones = if self.panel_app.panel_grid.is_split() {
            // Compute sub-chart rectangles.
            // Use origin (0, 0) so that panel_rects() returns rects relative
            // to the content area.  We add content_rect origin later when
            // converting to absolute screen coords.
            // Snapshot leaf rects (layout was already computed in prepare_frame).
            let leaf_rects: Vec<_> = self.panel_app.panel_grid.panel_rects()
                .iter()
                .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
                .collect();

            // Register chart panes on InputCoordinator as BlackboxPanel widgets.
            // BlackboxPanel makes is_over_ui() return false while the cursor is over
            // the chart canvas, so ui_drag_active no longer blocks scrollbar drags.
            // Chart-internal input (scales, sub-pane separators, drawing tools) is
            // dispatched by the chart's own hit-tester via panel_grid.resolve_input.
            {
                use uzor::input::{Sense, WidgetKind};
                use mlc_layout::ui::z_order::ZLayer;
                let chart_layer = ZLayer::ChartCanvas.push(&mut self.input_coordinator.borrow_mut());
                for &(leaf_id, sub_rect) in &leaf_rects {
                    if sub_rect.width > 0.0 && sub_rect.height > 0.0 {
                        self.input_coordinator.borrow_mut().register_composite(
                            format!("chart:pane:{}", leaf_id.0),
                            WidgetKind::BlackboxPanel,
                            uzor::Rect::new(
                                content_rect.x + sub_rect.x as f64,
                                content_rect.y + sub_rect.y as f64,
                                sub_rect.width as f64,
                                sub_rect.height as f64,
                            ),
                            Sense::CLICK
                                | Sense::DRAG
                                | Sense::SCROLL
                                | Sense::RIGHT_CLICK
                                | Sense::DOUBLE_CLICK
                                | Sense::HOVER,
                            &chart_layer,
                        );
                    }
                }
            }

            let chart_theme = self.panel_app.chart_theme_for_render();
            let no_toolbar = mlc_core::ToolbarConfig::minimal();

            // Compute alert indicator values ONCE per render — they are
            // window-independent (only depend on alert_manager + indicator_manager)
            // and can be reused across all split-pane leaves.
            let alert_indicator_values_cache = Self::build_indicator_values_for_alerts(
                &self.managers.alert,
                &self.indicator_manager,
            );

            // Pre-build drawing points per chart window. In split-pane mode the
            // same window may appear as multiple leaves; computing points once
            // and reusing avoids redundant Vec allocations per leaf.
            let drawing_points_cache: std::collections::HashMap<
                mlc_core::ChartId,
                Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)>,
            > = self.panel_app.panel_grid.containers()
                .iter()
                .map(|(&chart_id, w)| {
                    let bars = &w.bars;
                    let empty_prims: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> = Vec::new();
                    let prims_iter = self.panels_store.chart_panel.get(&w.chart_panel_id)
                        .map(|panel| panel.drawing_manager.primitives())
                        .unwrap_or(&empty_prims);
                    let pts: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> = prims_iter
                        .iter()
                        .map(|p| {
                            let pts_bar: Vec<(f64, f64)> = p.points()
                                .into_iter()
                                .map(|(ts_ms, price)| (mlc_core::timestamp_ms_to_bar_f64(bars, ts_ms), price))
                                .collect();
                            (p.data().id, pts_bar, mlc_alerts::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                        })
                        .collect();
                    (chart_id, pts)
                })
                .collect();

            let mut sub_pane_writebacks: Vec<(mlc_core::LeafId, Vec<(usize, f64, f64)>)> = Vec::new();
            let mut overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)> = Vec::new();
            // Collect rendered scale-corner zones per leaf so handle_canvas_click
            // can hit-test them in split mode (avoids the empty-default bug).
            let mut split_corner_zones: Vec<(mlc_core::LeafId, ScaleCornerHitZones)> = Vec::new();

            for (leaf_id, sub_rect) in leaf_rects {
                let window = match self.panel_app.panel_grid.container_for_leaf(leaf_id) {
                    Some(w) => w,
                    None => continue,
                };
                // S2.7.1: read instrument fields via branch_meta.
                // branch_meta and panel_app.panel_grid are disjoint fields — split borrow OK.
                let leaf_chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                let split_symbol: String = leaf_chart_id
                    .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let split_exchange: String = leaf_chart_id
                    .and_then(|cid| self.managers.bubble.exchange_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let split_account_type: String = leaf_chart_id
                    .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let split_tf = leaf_chart_id
                    .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                    .cloned();
                let split_tf_name: String = split_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
                let split_tf_interval_ms: Option<i64> = split_tf.map(|tf| tf.interval_ms);

                // Convert sub_rect to absolute LayoutRect.
                let leaf_rect = LayoutRect {
                    x: content_rect.x + sub_rect.x as f64,
                    y: content_rect.y + sub_rect.y as f64,
                    width: sub_rect.width as f64,
                    height: sub_rect.height as f64,
                };

                use mlc_core::chart::ChartRect;
                let chart_rect = ChartRect::new(
                    leaf_rect.x, leaf_rect.y,
                    leaf_rect.width, leaf_rect.height,
                );
                // Phase 4 C: get panel state from panels_store.
                let split_pid = window.chart_panel_id;
                let split_default_panel = mlc_core::ChartPanelState::new(split_pid);
                let split_panel = self.panels_store.chart_panel
                    .get(&split_pid)
                    .unwrap_or(&split_default_panel);
                let scale_corner_state = window.to_corner_state();
                let render_state = window.to_render_state(
                    chart_rect,
                    &chart_theme,
                    Some(split_tf_name.as_str()),
                    Some(&window.scale_settings.time_format),
                    split_tf_interval_ms,
                    &split_panel.grid_options,
                    &split_panel.legend,
                );
                let crosshair_config = if self.pending_alert_screenshot {
                    mlc_core::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
                } else {
                    mlc_core::chart::CrosshairConfig::default()
                };
                let render_config = ChartRenderConfig {
                    scale_theme: self.panel_app.scale_theme_for_render(),
                    chart_type: split_panel.chart_type,
                    crosshair_config,
                    ..ChartRenderConfig::default()
                };

                // Scope indicator queries to this window so each split pane only
                // renders the indicators that belong to it.
                if let Some(chart_id) = leaf_chart_id {
                    self.indicator_manager.current_render_window_id.set(Some(chart_id.0));
                }

                let alert_current_bar = window.bars.len().saturating_sub(1) as f64;
                // Look up pre-built drawing points from cache (computed once per
                // chart window before this loop to avoid per-leaf reallocation).
                let empty_pts: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> = Vec::new();
                let chart_id_for_cache = leaf_chart_id;
                let alert_drawing_points: &Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> =
                    chart_id_for_cache
                        .and_then(|cid| drawing_points_cache.get(&cid))
                        .unwrap_or(&empty_pts);
                // Reuse the indicator values computed once before this loop.
                let alert_indicator_values = &alert_indicator_values_cache;
                let alert_render_data: Vec<AlertRenderData> = self.managers.alert.items()
                    .iter()
                    .filter(|a| a.status == mlc_alerts::AlertStatus::Active)
                    .filter(|a| matches!(a.source, mlc_alerts::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(&split_symbol, &split_exchange, &split_account_type))
                    .filter_map(|alert| {
                        let price = mlc_alerts::AlertManager::resolve_price_static(
                            alert,
                            alert_current_bar,
                            alert_drawing_points,
                            alert_indicator_values,
                        )?;
                        Some(AlertRenderData {
                            price,
                            status: AlertRenderStatus::Active,
                        })
                    })
                    .collect();

                let inner_proportions_vec = mlc_core::state::chart_container::collect_inner_proportions(&window.inner_docking);
                let inner_sub_ids_vec = mlc_core::state::chart_container::collect_inner_instance_ids(&window.inner_docking);
                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&split_panel.drawing_manager),
                    indicator_source: Some(&self.indicator_manager),
                    symbol: Some(split_symbol.as_str()),
                    sub_pane_y_state: Some(&window.sub_pane_y_state),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: split_panel.watermark.as_ref(),
                    tooltip: Some(&split_panel.tooltip),
                    alert_render_data: &alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &no_toolbar,
                    is_split: true,
                    inner_proportions: &inner_proportions_vec,
                    inner_sub_instance_ids: &inner_sub_ids_vec,
                    data_error: window.data_error.as_deref(),
                    maximized_sub_pane_id: self.maximized_sub_panes
                        .get(&window.id.0)
                        .map(|(iid, _)| *iid),
                    above_main_ids: Some(&window.sub_pane_above_main),
                };

                let render_result = render_full_chart_panel(ctx, &leaf_rect, &panel_data);
                // Capture exact rendered button rects for this leaf's scale corner.
                split_corner_zones.push((leaf_id, render_result.corner_zones.clone()));
                if !render_result.sub_pane_ranges.is_empty() {
                    sub_pane_writebacks.push((leaf_id, render_result.sub_pane_ranges));
                }
                // Store overlay results for next frame's hit testing.
                overlay_writebacks.push((leaf_id, render_result.sub_pane_overlays));

                // Post-render: draw bell icons for alerts bound to drawing
                // primitives and overlay indicators in this leaf.
                let main_chart_y;
                let leaf_tab_visible;
                {
                    // Sub-pane order + proportions both come from the
                    // container's own InnerLayout -- single source of truth.
                    // indicator_manager visibility is a filter applied on top.
                    // S2.7.1: use split_symbol (from branch_meta) for indicator lookup.
                    let visible_set: std::collections::HashSet<u64> = if let Some(chart_id) = leaf_chart_id {
                        self.indicator_manager
                            .get_instances_for_symbol_in_window(&split_symbol, chart_id.0)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    } else {
                        self.indicator_manager
                            .get_instances_for_symbol(&split_symbol)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    };
                    let all_ids = mlc_core::state::chart_container::collect_inner_instance_ids(&window.inner_docking);
                    let all_props = mlc_core::state::chart_container::collect_inner_proportions(&window.inner_docking);
                    let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = all_ids
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| visible_set.contains(id))
                        .map(|(i, &id)| (id, all_props.get(i + 1).copied().unwrap_or(0.0)))
                        .unzip();
                    // Mirror render_full_chart_panel's maximize override: when a
                    // sub-pane is maximized it covers the whole container, the
                    // main chart is zero-height, and chart-anchored overlays
                    // (leaf tab, alert bells) must not poke through.
                    let maximized_iid = leaf_chart_id
                        .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
                        .map(|(iid, _)| *iid)
                        .filter(|iid| sub_pane_ids.contains(iid));
                    let extended = if let Some(miid) = maximized_iid {
                        mlc_core::ExtendedFrameLayout::maximized(&leaf_rect, miid, &window.scale_settings)
                    } else {
                        mlc_core::ExtendedFrameLayout::from_proportions(
                            &leaf_rect,
                            &sub_pane_ids,
                            &sub_proportions,
                            all_props.first().copied().filter(|_| all_props.len() > 1),
                            &window.scale_settings,
                            1.0,
                            &window.sub_pane_above_main,
                        )
                    };
                    let main = &extended.main_chart;
                    let chart_area = LayoutRect {
                        x: main.chart.x,
                        y: main.chart.y,
                        width: main.chart.width,
                        height: main.chart.height,
                    };
                    let mut corrected_vp = window.viewport.clone();
                    corrected_vp.chart_width = main.chart.width;
                    corrected_vp.chart_height = main.chart.height;

                    let window_id = leaf_chart_id.map(|cid| cid.0);

                    // S2.7.1: pass split_symbol/exchange/account_type from branch_meta.
                    // Skip chart-anchored bells when the main chart is collapsed
                    // (maximized sub-pane covers the container).
                    if chart_area.height >= 1.0 {
                        let bells = Self::draw_alert_bell_icons(
                            ctx,
                            chart_area,
                            &corrected_vp,
                            window.price_scale.price_min,
                            window.price_scale.price_max,
                            &window.bars,
                            &split_panel.drawing_manager,
                            &self.indicator_manager,
                            &self.managers.alert,
                            window_id,
                            &split_symbol,
                            &split_exchange,
                            &split_account_type,
                        );
                        for (widget_id, bx, by, bsize) in bells {
                            use uzor::input::Sense;
                            let hw = bsize / 2.0 + 2.0;
                            self.input_coordinator.borrow_mut().register(
                                widget_id,
                                uzor::Rect::new(bx - hw, by - hw, hw * 2.0, hw * 2.0),
                                Sense::CLICK,
                            );
                        }
                    }
                    main_chart_y = extended.main_chart.chart.y;
                    leaf_tab_visible = extended.main_chart.chart.height >= 1.0;

                    // chart:price_scale, chart:time_scale, chart:sub_sep, chart:sub_price_scale
                    // registrations removed — chart is now a BlackboxPanel and its
                    // internal hit-tester (ExtendedLayoutHitTester / panel_grid.resolve_input)
                    // handles these interactions without coordinator involvement.
                }

                // Reset render scope after this leaf is done.
                self.indicator_manager.current_render_window_id.set(None);

                // Render overlay tab header at the top-left of this leaf.
                let is_active_leaf = self.panel_app.panel_grid.docking()
                    .active_leaf() == Some(leaf_id);
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(leaf_id) {
                    self.leaf_tab_hover
                } else {
                    mlc_layout::layout::panel_overlay::LeafTabHoverZone::None
                };
                let color_tag = self.panel_app.leaf_color_tags.get(&leaf_id).copied();
                // S2.7.1: use split_symbol/tf_name/exchange/account_type from branch_meta.
                // Skipped entirely while a maximized sub-pane covers the container
                // (leaf_tab_visible == false): the tab belongs to the chart layer,
                // which sits BELOW the maximized pane in z-order.
                if leaf_tab_visible {
                let hit_zones = mlc_layout::layout::panel_overlay::render_leaf_tab(
                    ctx,
                    leaf_rect.x + 2.0,
                    main_chart_y + 2.0,
                    leaf_rect.width - 4.0,
                    &split_symbol,
                    &split_tf_name,
                    &split_exchange,
                    &split_account_type,
                    is_active_leaf,
                    hover_zone,
                    color_tag,
                    &leaf_tab_toolbar_theme,
                );
                let tab_rect = hit_zones.tab_rect;
                let dots_rect = hit_zones.dots_rect;
                let color_tag_rect = hit_zones.color_tag_rect;
                out_leaf_tab_hit_zones.insert(leaf_id, hit_zones);

                // Register overlay tab sub-zones so crosshair hides, cursor is default,
                // and hover state can be derived from InputCoordinator hovered_widget().
                {
                    use uzor::input::Sense;
                    let mut coord = self.input_coordinator.borrow_mut();
                    // Body registered first — sub-zones registered after win overlaps.
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:body", leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                    let [dx, dy, dw, dh] = dots_rect;
                    if dw > 0.0 && dh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:gear", leaf_id.0),
                            uzor::Rect::new(dx, dy, dw, dh),
                            Sense::CLICK,
                        );
                    }
                    let [cx, cy, cw, ch] = color_tag_rect;
                    if cw > 0.0 && ch > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:color_tag", leaf_id.0),
                            uzor::Rect::new(cx, cy, cw, ch),
                            Sense::CLICK,
                        );
                    }
                }
                }

                // Render per-leaf indicator overlay (chevron) for this leaf.
                {
                    // S2.7.1: use split_symbol from branch_meta (already captured above).
                    let symbol = split_symbol.clone();
                    // Collect compare series info before any mutable borrow of panel_app.
                    let compare_series_info: Vec<(String, bool, String)> = window
                        .compare_overlay
                        .series
                        .iter()
                        .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                        .collect();
                    let instances = if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
                        self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0)
                    } else {
                        self.indicator_manager.get_instances_for_symbol(&symbol)
                    };
                    let _has_indicators = !instances.is_empty();
                    let _has_compare = !compare_series_info.is_empty();

                    // overlay visibility was set in prepare_frame; read the state here.
                    let overlay_state = self.panel_app
                        .indicator_overlay_states
                        .get(&leaf_id)
                        .cloned()
                        .unwrap_or_default();

                    // leaf_tab_visible: hidden while a maximized sub-pane covers
                    // the container (chevron belongs to the chart layer).
                    if leaf_tab_visible && (overlay_state.visible || overlay_state.is_open) {
                        let mut indicators: Vec<IndicatorOverlayInfo> = instances
                            .iter()
                            .map(|inst| {
                                let display_name = Self::format_indicator_display_name(
                                    &self.indicator_manager, inst,
                                );
                                IndicatorOverlayInfo {
                                    id: inst.id,
                                    display_name,
                                    visible: inst.visible,
                                    is_compare: false,
                                    symbol: None,
                                    color: None,
                                }
                            })
                            .collect();

                        for (sym, vis, col) in &compare_series_info {
                            indicators.push(IndicatorOverlayInfo {
                                id: 0,
                                display_name: sym.clone(),
                                visible: *vis,
                                is_compare: true,
                                symbol: Some(sym.clone()),
                                color: Some(col.clone()),
                            });
                        }

                        if !indicators.is_empty() {
                            let toolbar_theme_for_overlay = self.panel_app.toolbar_theme_for_render();
                            let overlay_rect = uzor::types::Rect::new(
                                leaf_rect.x,
                                main_chart_y,
                                leaf_rect.width,
                                leaf_rect.height,
                            );
                            let overlay_result = render_indicator_overlay(
                                ctx,
                                &overlay_rect,
                                &indicators,
                                &overlay_state,
                                &frame_theme,
                                &toolbar_theme_for_overlay,
                            );

                            // Register indicator overlay hit zones for this leaf.
                            {
                                use uzor::input::Sense;
                                use mlc_layout::ui::z_order::ZLayer;

                                let prefix = format!("ind_overlay:leaf{}:", leaf_id.0);
                                let cmp_prefix = format!("cmp_overlay:leaf{}:", leaf_id.0);
                                let ov_layer = ZLayer::Toolbar.push_named(
                                    &mut self.input_coordinator.borrow_mut(),
                                    &format!("ind_overlay_leaf{}", leaf_id.0),
                                );

                                let br = &overlay_result.button_rect;
                                if br.width > 0.0 {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}toggle", prefix),
                                        uzor::Rect::new(br.x, br.y, br.width, br.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                if let Some(ref close_rect) = overlay_result.close_button_rect {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}close", prefix),
                                        uzor::Rect::new(close_rect.x, close_rect.y, close_rect.width, close_rect.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                for row in &overlay_result.indicator_rows {
                                    let id = row.instance_id;
                                    // Compare entries use cmp_overlay:leaf{N}: prefix;
                                    // regular indicators use ind_overlay:leaf{N}: prefix.
                                    let row_prefix: &str = if row.is_compare { &cmp_prefix } else { &prefix };
                                    let rr = &row.row_rect;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}row:{}", row_prefix, id),
                                        uzor::Rect::new(rr.x, rr.y, rr.width, rr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let ar = &row.alert_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}alert:{}", row_prefix, id),
                                        uzor::Rect::new(ar.x, ar.y, ar.width, ar.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let vr = &row.visibility_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}vis:{}", row_prefix, id),
                                        uzor::Rect::new(vr.x, vr.y, vr.width, vr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let sr = &row.settings_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}settings:{}", row_prefix, id),
                                        uzor::Rect::new(sr.x, sr.y, sr.width, sr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let dr = &row.delete_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}delete:{}", row_prefix, id),
                                        uzor::Rect::new(dr.x, dr.y, dr.width, dr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                self.input_coordinator.borrow_mut().pop_layer(&ov_layer);
                            }
                        }
                    }
                }
            }

            // Flatten multi-panel sub-pane writebacks into the output list.
            for (leaf_id, ranges) in sub_pane_writebacks {
                for (pane_idx, min, max) in ranges {
                    out_sub_pane_range_writebacks.push((leaf_id, pane_idx, min, max));
                }
            }

            // Collect multi-panel overlay writebacks.
            out_sub_pane_overlay_writebacks.extend(overlay_writebacks);

            // Render separators between split leaves.
            // thickness_for_state() returns 2.0 for idle, 4.0 for hover/dragging.
            let separators: Vec<_> = self.panel_app.panel_grid.docking()
                .separators()
                .iter()
                .map(|sep| {
                    let thickness = sep.thickness_for_state();
                    (sep.orientation, sep.position, sep.start, sep.length, thickness)
                })
                .collect();

            use mlc_core::SeparatorOrientation;
            for (_idx, (orientation, position, start, length, thickness)) in separators.iter().copied().enumerate() {
                let color: &str = &frame_theme.toolbar_border;
                // Convert content-relative separator coords to absolute screen coords.
                let (rect_x, rect_y, rect_w, rect_h) = match orientation {
                    SeparatorOrientation::Vertical => {
                        // position = x center, start = y begin, length = y extent
                        let abs_x = content_rect.x + (position - thickness / 2.0) as f64;
                        let abs_y = content_rect.y + start as f64;
                        (abs_x, abs_y, thickness as f64, length as f64)
                    }
                    SeparatorOrientation::Horizontal => {
                        // position = y center, start = x begin, length = x extent
                        let abs_x = content_rect.x + start as f64;
                        let abs_y = content_rect.y + (position - thickness / 2.0) as f64;
                        (abs_x, abs_y, length as f64, thickness as f64)
                    }
                };
                ctx.set_fill_color(color);
                ctx.fill_rect(rect_x, rect_y, rect_w, rect_h);
            }

            split_corner_zones.into_iter().collect::<std::collections::HashMap<_, _>>()
        } else {
            let scale_corner_state = self.panel_app.panel_grid.active_container()
                .map(|w| w.to_corner_state())
                .unwrap_or_default();
            let active_pid_for_ct = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            let active_chart_type = active_pid_for_ct
                .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .map(|p| p.chart_type)
                .unwrap_or("candles");
            let crosshair_config = if self.pending_alert_screenshot {
                mlc_core::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
            } else {
                mlc_core::chart::CrosshairConfig::default()
            };
            let render_config = ChartRenderConfig {
                scale_theme: self.panel_app.scale_theme_for_render(),
                chart_type: active_chart_type,
                crosshair_config,
                ..ChartRenderConfig::default()
            };

            let chart_theme = self.panel_app.chart_theme_for_render();

            // S2.7.1: snapshot symbol/timeframe/exchange/account_type from branch_meta.
            let single_window_info: Option<(String, String, String, String)> = {
                let cid = self.panel_app.panel_grid.active_container_id();
                cid.map(|chart_id| {
                    let sym = self.managers.bubble.symbol_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let tf_name = self.managers.bubble.timeframe_for_chart(chart_id)
                        .map(|tf| tf.name.clone())
                        .unwrap_or_default();
                    let exch = self.managers.bubble.exchange_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let at = self.managers.bubble.account_type_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    (sym, tf_name, exch, at)
                })
            };
            let single_tf_interval_ms: Option<i64> = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                .map(|tf| tf.interval_ms);

            // Scope indicator queries to the active window for single-window mode.
            if let Some(chart_id) = self.panel_app.panel_grid.active_container_id() {
                self.indicator_manager.current_render_window_id.set(Some(chart_id.0));
            }

            // Group primitive sync was done in prepare_frame; use the already-synced drawing_manager here.

            let single_active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            let (single_alert_current_bar, single_alert_drawing_points) = if let Some(window) = self.panel_app.panel_grid.active_container() {
                let cb = window.bars.len().saturating_sub(1) as f64;
                let bars = &window.bars;
                let empty_prims2: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> = Vec::new();
                let prims2_iter = single_active_pid
                    .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                    .map(|panel| panel.drawing_manager.primitives())
                    .unwrap_or(&empty_prims2);
                let pts: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> = prims2_iter
                    .iter()
                    .map(|p| {
                        let pts_bar: Vec<(f64, f64)> = p.points()
                            .into_iter()
                            .map(|(ts_ms, price)| (mlc_core::timestamp_ms_to_bar_f64(bars, ts_ms), price))
                            .collect();
                        (p.data().id, pts_bar, mlc_alerts::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                    })
                    .collect();
                (cb, pts)
            } else {
                (0.0, Vec::new())
            };
            let single_alert_indicator_values = Self::build_indicator_values_for_alerts(
                &self.managers.alert,
                &self.indicator_manager,
            );
            let single_alert_render_data: Vec<AlertRenderData> = {
                // single_window_info = (symbol, timeframe, exchange, account_type) captured above.
                let (single_sym, _, single_exch, single_at) = single_window_info
                    .as_ref()
                    .map(|(s, tf, e, at)| (s.as_str(), tf.as_str(), e.as_str(), at.as_str()))
                    .unwrap_or(("", "", "", ""));
                self.managers.alert.items()
                    .iter()
                    .filter(|a| a.status == mlc_alerts::AlertStatus::Active)
                    .filter(|a| matches!(a.source, mlc_alerts::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(single_sym, single_exch, single_at))
                    .filter_map(|alert| {
                        let price = mlc_alerts::AlertManager::resolve_price_static(
                            alert,
                            single_alert_current_bar,
                            &single_alert_drawing_points,
                            &single_alert_indicator_values,
                        )?;
                        Some(AlertRenderData {
                            price,
                            status: AlertRenderStatus::Active,
                        })
                    })
                    .collect()
            };

            let mut single_sub_pane_ranges: Vec<(usize, f64, f64)> = Vec::new();
            let mut single_sub_pane_overlays: Vec<mlc_core::SubPaneOverlayResult> = Vec::new();

            // S2.7.1: extract single-window instrument values from branch_meta tuple.
            let (single_sym, single_tf_name, single_exch, single_at) = single_window_info
                .as_ref()
                .map(|(s, tf, e, at)| (s.as_str(), tf.as_str(), e.as_str(), at.as_str()))
                .unwrap_or(("", "", "", ""));

            // Phase 4 C: get single-window panel state before borrowing window.
            let single_window_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            let single_fallback_panel = mlc_core::ChartPanelState::new(
                single_window_pid.unwrap_or(mlc_core::state::ChartPanelId(0))
            );
            let single_window_panel: &mlc_core::ChartPanelState = single_window_pid
                .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .unwrap_or(&single_fallback_panel);
            let window_opt = self.panel_app.panel_grid.active_container();
            let corner_zones_single = if let Some(window) = window_opt {
                use mlc_core::chart::ChartRect;
                let chart_rect = ChartRect::new(
                    content_rect.x,
                    content_rect.y,
                    content_rect.width,
                    content_rect.height,
                );
                let render_state = window.to_render_state(
                    chart_rect,
                    &chart_theme,
                    Some(single_tf_name),
                    Some(&window.scale_settings.time_format),
                    single_tf_interval_ms,
                    &single_window_panel.grid_options,
                    &single_window_panel.legend,
                );

                let inner_proportions_vec = mlc_core::state::chart_container::collect_inner_proportions(&window.inner_docking);
                let inner_sub_ids_vec = mlc_core::state::chart_container::collect_inner_instance_ids(&window.inner_docking);
                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&single_window_panel.drawing_manager),
                    indicator_source: Some(&self.indicator_manager),
                    symbol: Some(single_sym),
                    sub_pane_y_state: Some(&window.sub_pane_y_state),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: single_window_panel.watermark.as_ref(),
                    tooltip: Some(&single_window_panel.tooltip),
                    alert_render_data: &single_alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &self.panel_app.toolbar_config,
                    is_split: false,
                    inner_proportions: &inner_proportions_vec,
                    inner_sub_instance_ids: &inner_sub_ids_vec,
                    data_error: window.data_error.as_deref(),
                    maximized_sub_pane_id: self.maximized_sub_panes
                        .get(&window.id.0)
                        .map(|(iid, _)| *iid),
                    above_main_ids: Some(&window.sub_pane_above_main),
                };

                // Render chart into content_rect (shrunk by sidebar).
                // Use content_rect as the full rect with minimal toolbar config
                // so render_full_chart_panel treats the entire rect as chart area
                // (toolbars are rendered separately via render_toolbars_with_theme).
                let chart_render_rect = content_rect;
                let mut chart_panel_data = panel_data;
                let no_toolbar = mlc_core::ToolbarConfig::minimal();
                chart_panel_data.toolbar_config = &no_toolbar;
                let render_result = render_full_chart_panel(ctx, &chart_render_rect, &chart_panel_data);
                // Stash ranges and overlay results for writeback after the immutable borrow on `window` ends.
                single_sub_pane_ranges = render_result.sub_pane_ranges;
                single_sub_pane_overlays = render_result.sub_pane_overlays;
                let corner_zones_ret = render_result.corner_zones;

                // Post-render: draw bell icons for alerts bound to this window's
                // drawing primitives and overlay indicators.
                // main_chart_y is updated once ExtendedFrameLayout is computed.
                {
                    // Compute the corrected chart area rect (same logic as
                    // render_full_chart_panel's internal ExtendedFrameLayout).
                    // S2.7.1: use single_sym from branch_meta for indicator lookup.
                    let visible_set: std::collections::HashSet<u64> = if let Some(chart_id) = self.panel_app.panel_grid.active_container_id() {
                        self.indicator_manager
                            .get_instances_for_symbol_in_window(single_sym, chart_id.0)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    } else {
                        self.indicator_manager
                            .get_instances_for_symbol(single_sym)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    };
                    let all_ids2 = mlc_core::state::chart_container::collect_inner_instance_ids(&window.inner_docking);
                    let all_props2 = mlc_core::state::chart_container::collect_inner_proportions(&window.inner_docking);
                    let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = all_ids2
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| visible_set.contains(id))
                        .map(|(i, &id)| (id, all_props2.get(i + 1).copied().unwrap_or(0.0)))
                        .unzip();
                    // Mirror render_full_chart_panel's maximize override (see the
                    // split-mode twin above): chart-anchored overlays must not
                    // poke through a maximized sub-pane.
                    let maximized_iid = self.panel_app.panel_grid.active_container_id()
                        .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
                        .map(|(iid, _)| *iid)
                        .filter(|iid| sub_pane_ids.contains(iid));
                    let extended = if let Some(miid) = maximized_iid {
                        mlc_core::ExtendedFrameLayout::maximized(&chart_render_rect, miid, &window.scale_settings)
                    } else {
                        mlc_core::ExtendedFrameLayout::from_proportions(
                            &chart_render_rect,
                            &sub_pane_ids,
                            &sub_proportions,
                            all_props2.first().copied().filter(|_| all_props2.len() > 1),
                            &window.scale_settings,
                            1.0,
                            &window.sub_pane_above_main,
                        )
                    };
                    let main = &extended.main_chart;
                    let chart_area = LayoutRect {
                        x: main.chart.x,
                        y: main.chart.y,
                        width: main.chart.width,
                        height: main.chart.height,
                    };
                    // Corrected viewport matching main chart area dimensions.
                    let mut corrected_vp = window.viewport.clone();
                    corrected_vp.chart_width = main.chart.width;
                    corrected_vp.chart_height = main.chart.height;

                    let window_id = self.panel_app.panel_grid
                        .active_container_id()
                        .map(|cid| cid.0);

                    // S2.7.1: use single_sym/exch/at from branch_meta.
                    // Skip chart-anchored bells when the main chart is collapsed
                    // (maximized sub-pane covers the container).
                    if chart_area.height >= 1.0 {
                        let bells = Self::draw_alert_bell_icons(
                            ctx,
                            chart_area,
                            &corrected_vp,
                            window.price_scale.price_min,
                            window.price_scale.price_max,
                            &window.bars,
                            &single_window_panel.drawing_manager,
                            &self.indicator_manager,
                            &self.managers.alert,
                            window_id,
                            single_sym,
                            single_exch,
                            single_at,
                        );
                        // Register bell click zones.
                        for (widget_id, bx, by, bsize) in bells {
                            use uzor::input::Sense;
                            let hw = bsize / 2.0 + 2.0; // a little larger than the icon
                            self.input_coordinator.borrow_mut().register(
                                widget_id,
                                uzor::Rect::new(bx - hw, by - hw, hw * 2.0, hw * 2.0),
                                Sense::CLICK,
                            );
                        }
                    }
                    main_chart_y_single = extended.main_chart.chart.y;
                    single_leaf_tab_visible = extended.main_chart.chart.height >= 1.0;
                }

                corner_zones_ret
            } else {
                main_chart_y_single = content_rect.y;
                ScaleCornerHitZones::default()
            };

            // Register single chart pane as BlackboxPanel — see split-mode comment above.
            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                use uzor::input::{Sense, WidgetKind};
                use mlc_layout::ui::z_order::ZLayer;
                let chart_layer = ZLayer::ChartCanvas.push(&mut self.input_coordinator.borrow_mut());
                self.input_coordinator.borrow_mut().register_composite(
                    format!("chart:pane:{}", active_leaf.0),
                    WidgetKind::BlackboxPanel,
                    uzor::Rect::new(
                        content_rect.x,
                        content_rect.y,
                        content_rect.width,
                        content_rect.height,
                    ),
                    Sense::CLICK
                        | Sense::DRAG
                        | Sense::SCROLL
                        | Sense::RIGHT_CLICK
                        | Sense::DOUBLE_CLICK
                        | Sense::HOVER,
                    &chart_layer,
                );
            }

            // Collect single-window sub-pane ranges for writeback via RenderOutput.
            if !single_sub_pane_ranges.is_empty() {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                for (pane_idx, min, max) in single_sub_pane_ranges {
                    out_sub_pane_range_writebacks.push((single_leaf_id, pane_idx, min, max));
                }
            }

            // Collect single-window overlay results for writeback via RenderOutput.
            {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                out_sub_pane_overlay_writebacks.push((single_leaf_id, single_sub_pane_overlays));
            }

            // Reset render scope after single-window render is complete.
            self.indicator_manager.current_render_window_id.set(None);

            // Render overlay tab header (single mode) — skipped while a
            // maximized sub-pane covers the container.
            if single_leaf_tab_visible {
            if let Some((symbol, timeframe, exchange, account_type_label)) = single_window_info {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(single_leaf_id) {
                    self.leaf_tab_hover
                } else {
                    mlc_layout::layout::panel_overlay::LeafTabHoverZone::None
                };
                let single_color_tag = self.panel_app.leaf_color_tags.get(&single_leaf_id).copied();
                let hit_zones = mlc_layout::layout::panel_overlay::render_leaf_tab(
                    ctx,
                    content_rect.x + 2.0,
                    main_chart_y_single + 2.0,
                    content_rect.width - 4.0,
                    &symbol,
                    &timeframe,
                    &exchange,
                    &account_type_label,
                    true, // always active in single mode
                    hover_zone,
                    single_color_tag,
                    &leaf_tab_toolbar_theme,
                );
                let tab_rect = hit_zones.tab_rect;
                let dots_rect = hit_zones.dots_rect;
                let color_tag_rect = hit_zones.color_tag_rect;
                out_leaf_tab_hit_zones.insert(single_leaf_id, hit_zones);

                // Register overlay tab sub-zones so crosshair hides, cursor is default,
                // and hover state can be derived from InputCoordinator hovered_widget().
                {
                    use uzor::input::Sense;
                    let mut coord = self.input_coordinator.borrow_mut();
                    // Body registered first — sub-zones registered after win overlaps.
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:body", single_leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                    let [dx, dy, dw, dh] = dots_rect;
                    if dw > 0.0 && dh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:gear", single_leaf_id.0),
                            uzor::Rect::new(dx, dy, dw, dh),
                            Sense::CLICK,
                        );
                    }
                    let [cx, cy, cw, ch] = color_tag_rect;
                    if cw > 0.0 && ch > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:color_tag", single_leaf_id.0),
                            uzor::Rect::new(cx, cy, cw, ch),
                            Sense::CLICK,
                        );
                    }
                }
            }
            }

            // Wrap single-window corner zones in a per-leaf map so the
            // type matches the split path and handle_canvas_click can use
            // a unified walk regardless of split/single mode.
            let single_leaf_id = self.panel_app.panel_grid.docking()
                .active_leaf()
                .unwrap_or(mlc_core::LeafId(0));
            let mut single_map: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones> =
                std::collections::HashMap::new();
            single_map.insert(single_leaf_id, corner_zones_single);
            single_map
        };
        let out_scale_corner_zones = corner_zones;
        let _rt2 = std::time::Instant::now(); // checkpoint: after chart render
        // Build selected primitive config for the inline config toolbar.
        //
        // DrawingManager::get_selected_config() reads the primitive registry and
        // the selected primitive's data in one call, returning None when nothing
        // is selected.  The result is a cheap owned value so no borrow escapes.
        let selected_config: Option<mlc_core::state::selected_config::SelectedPrimitiveConfig> = {
            let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            pid.and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .and_then(|panel| panel.drawing_manager.get_selected_config())
        };

        // Generate clock time from active window's timezone settings
        let clock_time = {
            let utc_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                if !time_fmt.show_utc_prefix {
                    time
                } else if time_fmt.timezone_offset_hours >= 0 {
                    format!("[UTC+{}] {}", time_fmt.timezone_offset_hours, time)
                } else {
                    format!("[UTC{}] {}", time_fmt.timezone_offset_hours, time)
                }
            } else {
                let hours = (utc_secs / 3600) % 24;
                let minutes = (utc_secs / 60) % 60;
                let seconds = utc_secs % 60;
                format!("[UTC+0] {:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        };

        // 4b. Render indicator overlay (top-left of chart content area).
        //     Drawn BEFORE toolbars so that toolbar dropdowns render on top.
        //     In split mode each leaf renders its own chevron inside the split loop above.
        if !self.panel_app.panel_grid.is_split() && single_leaf_tab_visible {
            // overlay visibility was set in prepare_frame; read the state here.

            let toolbar_theme_for_overlay = self.panel_app.toolbar_theme_for_render();
            let overlay_state = &self.panel_app.indicator_overlay_state;
            if overlay_state.visible || overlay_state.is_open {
                // S2.7.1: read symbol from branch_meta; compare_series still needs window borrow.
                let ind_overlay_chart_id = self.panel_app.panel_grid.active_container_id();
                let symbol: String = ind_overlay_chart_id
                    .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let compare_series: Vec<(String, bool, String)> =
                    self.panel_app.panel_grid.active_container()
                        .map(|w| w.compare_overlay.series.iter()
                            .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                            .collect())
                        .unwrap_or_default();

                let instances = if let Some(chart_id) = ind_overlay_chart_id {
                    self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0)
                } else {
                    self.indicator_manager.get_instances_for_symbol(&symbol)
                };
                let mut indicators: Vec<IndicatorOverlayInfo> = instances
                    .iter()
                    .map(|inst| {
                        let display_name = Self::format_indicator_display_name(
                            &self.indicator_manager, inst,
                        );
                        IndicatorOverlayInfo {
                            id: inst.id,
                            display_name,
                            visible: inst.visible,
                            is_compare: false,
                            symbol: None,
                            color: None,
                        }
                    })
                    .collect();

                for (sym, vis, col) in compare_series {
                    indicators.push(IndicatorOverlayInfo {
                        id: 0,
                        display_name: sym.clone(),
                        visible: vis,
                        is_compare: true,
                        symbol: Some(sym),
                        color: Some(col),
                    });
                }

                if !indicators.is_empty() {
                    let chart_rect = uzor::types::Rect::new(
                        content_rect.x,
                        main_chart_y_single,
                        content_rect.width,
                        content_rect.height,
                    );
                    let overlay_result = render_indicator_overlay(
                        ctx,
                        &chart_rect,
                        &indicators,
                        overlay_state,
                        &frame_theme,
                        &toolbar_theme_for_overlay,
                    );

                    // Register indicator overlay hit zones with InputCoordinator.
                    {
                        use uzor::input::Sense;
                        use mlc_layout::ui::z_order::ZLayer;

                        let ov_layer = ZLayer::Toolbar.push_named(&mut self.input_coordinator.borrow_mut(), "ind_overlay");

                        // Main toggle button (when dropdown is closed)
                        let br = &overlay_result.button_rect;
                        if br.width > 0.0 {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "ind_overlay:toggle",
                                uzor::Rect::new(br.x, br.y, br.width, br.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        // Close button (when dropdown is open)
                        if let Some(ref close_rect) = overlay_result.close_button_rect {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "ind_overlay:close",
                                uzor::Rect::new(close_rect.x, close_rect.y, close_rect.width, close_rect.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        // Per-indicator / per-compare-series row action buttons.
                        // Compare entries use "cmp_overlay:" prefix; indicators use "ind_overlay:".
                        for row in &overlay_result.indicator_rows {
                            let id = row.instance_id;
                            let row_prefix = if row.is_compare { "cmp_overlay" } else { "ind_overlay" };
                            // Full row rect — registered first (lower z-priority) so individual
                            // icon widgets take precedence, but the text/gap area still triggers
                            // default cursor via its prefix in input.rs.
                            let rr = &row.row_rect;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:row:{}", row_prefix, id),
                                uzor::Rect::new(rr.x, rr.y, rr.width, rr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Alert button (only meaningful for indicators; compare entries show it too
                            // for layout consistency but the handler is a no-op for compare)
                            let ar = &row.alert_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:alert:{}", row_prefix, id),
                                uzor::Rect::new(ar.x, ar.y, ar.width, ar.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Visibility toggle
                            let vr = &row.visibility_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:vis:{}", row_prefix, id),
                                uzor::Rect::new(vr.x, vr.y, vr.width, vr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Settings button
                            let sr = &row.settings_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:settings:{}", row_prefix, id),
                                uzor::Rect::new(sr.x, sr.y, sr.width, sr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Delete button
                            let dr = &row.delete_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:delete:{}", row_prefix, id),
                                uzor::Rect::new(dr.x, dr.y, dr.width, dr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        self.input_coordinator.borrow_mut().pop_layer(&ov_layer);
                    }
                }
            }
        }

        // 4c. Render toolbars and their dropdowns on top of the indicator overlay.
        //
        // When `skip_toolbar_draw` is true, skip the expensive vector draw and
        // re-register hit zones from the cached `last_toolbar_result` instead.
        // The caller composites the previously-built toolbar scene on top.
        let out_last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult> = if !skip_toolbar_draw {
            // S2.7.1: read active symbol/timeframe from branch_meta.
            let (active_sym_str, active_tf_str, active_gid) = {
                let cid = self.panel_app.panel_grid.active_container_id();
                let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                    .map(String::from).unwrap_or_default();
                let tf = cid.and_then(|c| self.managers.bubble.timeframe_for_chart(c))
                    .map(|tf| tf.name.clone())
                    .unwrap_or_default();
                let gid = cid.and_then(|c| self.managers.bubble.bubble_for_chart(c));
                (sym, tf, gid)
            };
            let toolbar_result = self.panel_app.render_toolbars_with_theme(
                ctx,
                &panel_layout,
                selected_config.as_ref(),
                Some(clock_time.as_str()),
                self.split_without_group,
                Some(active_sym_str.as_str()),
                Some(active_tf_str.as_str()),
                sidebar_w,
                active_gid,
                &self.managers.bubble,
            );
            Some(toolbar_result)
        } else {
            // Preserve the cached toolbar result from the previous frame.
            self.last_toolbar_result.clone()
        };

        // Register toolbar hit zones — always done every frame (cheap coordinate
        // registration), whether the toolbar was redrawn or cached.
        let mut out_last_inline_bar_rect: Option<LayoutRect> = None;
        if let Some(ref toolbar_result) = out_last_toolbar_result {
            use uzor::input::Sense;
            use mlc_layout::ui::z_order::ZLayer;

            let tb_layer = ZLayer::Toolbar.push(&mut self.input_coordinator.borrow_mut());

            // Drawing toolbar (left side)
            for (id, rect) in &toolbar_result.left_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("dtb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Control strip (top)
            for (id, rect) in &toolbar_result.top_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("csb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Right toolbar
            for (id, rect) in &toolbar_result.right_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("rtb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Bottom toolbar
            for (id, rect) in &toolbar_result.bottom_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("btb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Floating inline config toolbar items
            if let Some(ref inline_cfg) = toolbar_result.inline_config {
                // Register bar background first so gap clicks between buttons are absorbed
                // and don't fall through to handle_canvas_click() which would deselect the primitive.
                self.input_coordinator.borrow_mut().register_on_layer(
                    "ilb:__bg__",
                    uzor::Rect::new(inline_cfg.bar_rect.x, inline_cfg.bar_rect.y, inline_cfg.bar_rect.width, inline_cfg.bar_rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
                // Register individual button hit zones
                for (id, rect) in &inline_cfg.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("ilb:{}", id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
                out_last_inline_bar_rect = Some(inline_cfg.bar_rect);
            } else {
                out_last_inline_bar_rect = None;
            }

            // Register chevron hit zones for each toolbar that is overflowing.
            // IDs use the same prefix as the toolbar's items so dispatch_panel_click
            // can route them.  The suffix "__chevron_left" / "__chevron_right"
            // identifies the direction.
            let chevron_pairs: &[(&str, Option<uzor::types::Rect>, Option<uzor::types::Rect>)] = &[
                ("csb",
                 toolbar_result.top_left_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.top_right_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("btb",
                 toolbar_result.bottom_left_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.bottom_right_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("dtb",
                 toolbar_result.left_up_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.left_down_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("rtb",
                 toolbar_result.right_up_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.right_down_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
            ];
            for (prefix, left_rect, right_rect) in chevron_pairs {
                if let Some(rect) = left_rect {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("{}:__chevron_left", prefix),
                        *rect,
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
                if let Some(rect) = right_rect {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("{}:__chevron_right", prefix),
                        *rect,
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
            }

            // Inline dropdown hit zones (style/width popup)
            if let Some(ref idd) = toolbar_result.inline_dropdown_result {
                // Background absorber
                self.input_coordinator.borrow_mut().register_on_layer(
                    "ilb:inline_dropdown:__bg__",
                    uzor::Rect::new(idd.menu_rect.x, idd.menu_rect.y, idd.menu_rect.width, idd.menu_rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
                // Individual items — format: "ilb:{item_id}"
                for (item_id, rect) in &idd.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("ilb:{}", item_id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
            }

            self.input_coordinator.borrow_mut().pop_layer(&tb_layer);

            // Register dropdown hit zones (higher z-order)
            if let Some(ref dd) = toolbar_result.dropdown_result {
                use uzor::input::Sense;
                use mlc_layout::ui::z_order::ZLayer;

                let dd_layer = ZLayer::Dropdown.push_named(&mut self.input_coordinator.borrow_mut(), "chart_dropdown");

                // Menu background
                self.input_coordinator.borrow_mut().register_on_layer(
                    "dropdown:__bg__",
                    uzor::Rect::new(dd.menu_rect.x, dd.menu_rect.y, dd.menu_rect.width, dd.menu_rect.height),
                    Sense::CLICK,
                    &dd_layer,
                );

                // Dropdown items — format: "dropdown:{dropdown_id}:{item_id}"
                for (item_id, rect) in &dd.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("dropdown:{}:{}", dd.dropdown_id, item_id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &dd_layer,
                    );
                }

                self.input_coordinator.borrow_mut().pop_layer(&dd_layer);

                // Hover-based submenu: if draw_dropdown detected the pointer is over a
                // submenu-trigger item, open that submenu immediately (no click required).
                // If the pointer is over a non-submenu item, clear the open submenu.
                // Only update submenu state when we actually redrew the toolbar; on
                // cached frames the submenu state from the previous draw is preserved.
                if !skip_toolbar_draw {
                    if let Some(ref submenu_id) = dd.open_submenu {
                        out_open_submenu_update = Some(Some(submenu_id.clone()));
                    } else if dd.hovered.is_some() {
                        // Hovering a regular (non-submenu) item — close any open submenu.
                        out_open_submenu_update = Some(None);
                    }
                }
            }
        }

        let _rt3 = std::time::Instant::now(); // checkpoint: after toolbars

        // 5. Modals are NO LONGER drawn here.
        // They are drawn in render_modal_layer(), called by the VelloGpu compositor
        // AFTER the crosshair overlay is composited, so modals appear on top of the
        // crosshair rather than below it.
        // render_modal_layer() stores results directly into self.frame_result,
        // self.search_modal_result, self.context_menu_result, etc.
        // For non-VelloGpu backends, render_modal_layer() is called on the same ctx
        // immediately after render() returns (see scene_builder.rs).
        let out_frame_result: Option<ChartModalRenderResult> = None;
        let out_search_modal_result: Option<mlc_layout::layout::ModalSearchResult> = None;
        let out_context_menu_result: Option<ContextMenuResult> = None;

        // 8. Render right sidebar if a panel is open.
        //    Sidebar sits between chart content (price scale) and right toolbar.
        //    Right toolbar stays at window edge.  content_rect was already shrunk.
        //      x = content_rect.x + content_rect.width (right edge of shrunk chart)
        //      y = top_toolbar_h
        //      h = window_h - top_toolbar_h - bottom_toolbar_h
        //
        // The sidebar is always rendered on every frame so that widget
        // hit-zone registration happens inside the open begin_frame/end_frame
        // window.  The caller (chart-app-vello) composites the cached
        // sidebar_scene on top via Scene::append, visually covering these
        // pixels when the scene is unchanged.

        let skeleton_active = self.panel_app.user_settings_state.show_profile_manager
            || self.panel_app.user_settings_state.show_welcome_wizard;
        let out_last_sidebar_result: Option<mlc_sidebar_content::render::RightSidebarResult> = if self.sidebar_state.is_right_open() && !skeleton_active {
            let top_h = panel_layout.top_toolbar_rect.height;
            let bottom_h = panel_layout.bottom_toolbar_rect.height;
            let sidebar_x = content_rect.x + content_rect.width;
            let sidebar_y = top_h;
            let sidebar_h = h - top_h - bottom_h;

            let sidebar_rect = LayoutRect::new(sidebar_x, sidebar_y, sidebar_w, sidebar_h);
            let sidebar_toolbar_theme = self.panel_app.toolbar_theme_for_render();
            let panel_theme = panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());

            // Draw sidebar and register hit zones every frame.
            // When the cached sidebar_scene is composited on top these pixels
            // are overwritten, but the widget registrations survive until end_frame().
            let panels_store = &self.panels_store;
            let sidebar_result = mlc_sidebar_content::render::render_right_sidebar(
                ctx,
                &sidebar_rect,
                &mut self.sidebar_state,
                &sidebar_toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                &mut |item, rect, ctx, coord, slot_prefix| {
                    panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme, coord, slot_prefix);
                },
                &|item: &mlc_core::state::Panel| -> Option<String> {
                    use mlc_core::state::Panel;
                    use mlc_panels::trading::SymbolSource;
                    // For account-bound panels that still carry SymbolSource.
                    fn label_from_source(source: &SymbolSource, symbol: &str, exchange: &str, account_type: &str) -> Option<String> {
                        match source {
                            SymbolSource::Fixed { symbol, exchange, account_type } => {
                                Some(format!("{}:{}:{}", exchange, symbol, account_type))
                            }
                            SymbolSource::BoundToChart { leaf_id } => Some(format!("L#{}", leaf_id)),
                            SymbolSource::HyperFocus => Some(format!("{}:{}:{}", exchange, symbol, account_type)),
                        }
                    }
                    // Market-data panels: always format directly from stored fields (no SymbolSource).
                    fn label_from_fields(symbol: &str, exchange: &str, account_type: &str) -> Option<String> {
                        if symbol.is_empty() { return None; }
                        Some(format!("{}:{}:{}", exchange, symbol, account_type))
                    }
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::Footprint(id) => panels_store.footprint.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::BigTrades(id) => panels_store.big_trades.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                            .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                        Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                            .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                        Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        Panel::PositionManager(_)
                        | Panel::TradeLog(_)
                        | Panel::RiskCalculator(_)
                        // Chart doesn't expose symbol-source / overlay info to
                        // the sidebar header — it renders via dedicated path.
                        | Panel::Chart(_) => None,
                    }
                },
                &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64)> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter)),
                        _ => None,
                    }
                },
                &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> {
                    use mlc_core::tag_manager::SyncMemberId;
                    let panel_id = item.panel_id().0;
                    let member = SyncMemberId::Panel(panel_id);
                    let gid = self.managers.bubble.group_for_member(member)?;
                    let group = self.managers.bubble.bubble(gid)?;
                    if group.auto_created { None } else { Some(group.color) }
                },
                &|item: &mlc_core::state::Panel| -> Option<mlc_sidebar_content::render::PanelSyncFlagsSnapshot> {
                    use mlc_core::tag_manager::SyncMemberId;
                    let panel_id = item.panel_id().0;
                    let member = SyncMemberId::Panel(panel_id);
                    let gid = self.managers.bubble.group_for_member(member)?;
                    let group = self.managers.bubble.bubble(gid)?;
                    Some(mlc_sidebar_content::render::PanelSyncFlagsSnapshot {
                        sync_symbol: group.effective_sync_symbol(member),
                        sync_crosshair: group.effective_sync_crosshair(member),
                    })
                },
                &|item: &mlc_core::state::Panel| -> Option<(String, String, String)> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::Footprint(id) => panels_store.footprint.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::BigTrades(id) => panels_store.big_trades.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        Panel::PositionManager(_)
                        | Panel::TradeLog(_)
                        | Panel::RiskCalculator(_)
                        // Chart doesn't expose symbol-source / overlay info
                        // to the sidebar header — dedicated render path.
                        | Panel::Chart(_) => None,
                    }
                },
                &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                            s.column_config.show_bid_orders,
                            s.column_config.show_sell_trades,
                            s.column_config.show_buy_trades,
                            s.column_config.show_ask_orders,
                        ]),
                        Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_type,
                            s.column_config.show_side,
                            s.column_config.show_price,
                            s.column_config.show_qty,
                        ]),
                        Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_price,
                            s.column_config.show_size,
                        ]),
                        Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_side,
                            s.column_config.show_price,
                            s.column_config.show_size,
                            s.column_config.show_notional,
                        ]),
                        _ => None,
                    }
                },
            );

            Some(sidebar_result)
        } else {
            None
        };

        // 8a-8d. Modals (color pickers, watchlist, clock popup) moved to render_modal_layer().

        let out_last_watchlist_modal_result: Option<mlc_layout::layout::modals::watchlist_modal::WatchlistModalResult> = None;
        let out_last_wl_group_name_result: Option<WlGroupNameInputResult> = None;

        // 9. End frame — dispatch widget responses (clicks / drag / scroll).
        //
        // The hover SNAPSHOT this bakes is no longer used to resolve the cursor
        // type or hover highlights — those are now hit-tested LIVE on the pointer
        // event via InputCoordinator::hit_test_now (see on_mouse_move/get_cursor),
        // which eliminates the one-frame lag and does not depend on which layers
        // registered before vs after end_frame. end_frame stays here (baseline
        // cadence: once per render pass) purely for response dispatch.
        let _rt4 = std::time::Instant::now(); // checkpoint: after sidebar + modals
        let out_render_timing_us = (
            _rt2.duration_since(_rt1).as_micros() as u64, // chart
            _rt3.duration_since(_rt2).as_micros() as u64, // toolbar
            _rt4.duration_since(_rt3).as_micros() as u64, // sidebar + modals
            _rt1.duration_since(_rt0).as_micros() as u64, // layout + setup
        );
        let _responses = self.input_coordinator.borrow_mut().end_frame();

        RenderOutput {
            scale_corner_zones_by_leaf: out_scale_corner_zones,
            last_toolbar_result: out_last_toolbar_result,
            frame_result: out_frame_result,
            search_modal_result: out_search_modal_result,
            context_menu_result: out_context_menu_result,
            last_sidebar_result: out_last_sidebar_result,
            last_watchlist_modal_result: out_last_watchlist_modal_result,
            last_wl_group_name_result: out_last_wl_group_name_result,
            leaf_tab_hit_zones: out_leaf_tab_hit_zones,
            render_timing_us: out_render_timing_us,
            content_rect,
            right_toolbar_left_x: out_right_toolbar_left_x,
            last_inline_bar_rect: out_last_inline_bar_rect,
            open_submenu_update: out_open_submenu_update,
            sub_pane_range_writebacks: out_sub_pane_range_writebacks,
            sub_pane_overlay_writebacks: out_sub_pane_overlay_writebacks,
        }
    }

    /// Applies the [`RenderOutput`] returned by [`render_to_scene`] back to
    /// `self`, persisting cached render results for input handlers.
    ///
    /// Call this immediately after `render_to_scene` completes.
    pub fn apply_render_output(&mut self, output: RenderOutput) {
        self.scale_corner_zones_by_leaf = output.scale_corner_zones_by_leaf;
        self.last_toolbar_result = output.last_toolbar_result;
        self.frame_result = output.frame_result;
        self.search_modal_result = output.search_modal_result;
        self.context_menu_result = output.context_menu_result;
        self.last_sidebar_result = output.last_sidebar_result;

        // Persist agent terminal rect on sidebar_state for hover-focus in CursorMoved.
        self.sidebar_state.agent_terminal_rect = self
            .last_sidebar_result
            .as_ref()
            .and_then(|r| r.agent_terminal_rect)
            .map(|r| (r.x as f32, r.y as f32, r.width as f32, r.height as f32));

        // Resize focused PTY leaf when the terminal content area changes grid dimensions.
        let new_size = self
            .last_sidebar_result
            .as_ref()
            .and_then(|r| r.agent_terminal_size);
        if new_size.is_some() && new_size != self.sidebar_state.agent_terminal_size {
            self.sidebar_state.agent_terminal_size = new_size;
            // Resize ALL visible PTY leaves using their actual rect dimensions.
            let leaf_rects: Vec<(uzor::panels::LeafId, uzor::panels::PanelRect)> = self
                .sidebar_state
                .agent_docking
                .inner()
                .panel_rects()
                .iter()
                .map(|(&id, &r)| (id, r))
                .collect();
            for (leaf_id, rect) in leaf_rects {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if desc.mode == gate4agent::InstanceMode::Pty {
                        let cols = ((rect.width / 7.0) as u16).max(1);
                        let rows = ((rect.height / 19.0) as u16).max(1);
                        self.bridge.runtime().block_on(self.agent.resize_instance(desc.instance_id, cols, rows));
                    }
                }
            }
        }

        self.last_watchlist_modal_result = output.last_watchlist_modal_result;
        self.last_wl_group_name_result = output.last_wl_group_name_result;
        self.leaf_tab_hit_zones = output.leaf_tab_hit_zones;
        self.render_timing_us = output.render_timing_us;
        self.content_rect = output.content_rect;
        self.right_toolbar_left_x = output.right_toolbar_left_x;
        self.last_inline_bar_rect = output.last_inline_bar_rect;
        if let Some(submenu_update) = output.open_submenu_update {
            self.panel_app.toolbar_state.open_submenu_id = submenu_update;
        }
        // Write back render-computed sub-pane ranges (symmetrization + padding already applied).
        for (leaf_id, pane_idx, min, max) in output.sub_pane_range_writebacks {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                let instance_id = mlc_core::state::chart_container::collect_inner_instance_ids(
                    &window.inner_docking,
                ).into_iter().nth(pane_idx);
                if let Some(iid) = instance_id {
                    let entry = window.sub_pane_y_state
                        .entry(iid)
                        .or_default();
                    if entry.auto_scale {
                        entry.price_min = min;
                        entry.price_max = max;
                    }
                }
            }
        }
        // Write back sub-pane overlay button rects for next frame's hit testing.
        for (leaf_id, overlays) in output.sub_pane_overlay_writebacks {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                window.sub_pane_overlay_results = overlays;
            }
        }
        // Update text-field geometry for the HexColor field whenever the
        // L2 color picker is visible.  This gives on_drag_start accurate rect +
        // char positions so cursor-from-click works correctly.
        if let Some(ref fr) = self.frame_result {
            if let Some(ref cp) = fr.color_picker {
                if let Some(ref l2) = cp.l2_result {
                    let r = &l2.hex_input_rect;
                    let hex_id = WidgetId::from(text_input::HEX_COLOR);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &hex_id,
                        (r.x, r.y, r.width, r.height),
                        l2.hex_char_positions.clone(),
                    );
                }
            }
        }
        // Update text-field geometry for the agent chat input field so
        // on_drag_start can compute cursor-from-click correctly.
        if let Some(ref sidebar_result) = self.last_sidebar_result {
            if let (Some(rect), Some(char_positions)) = (
                sidebar_result.agent_input_rect,
                sidebar_result.agent_input_char_positions.clone(),
            ) {
                let chat_id = WidgetId::from(text_input::AGENT_CHAT);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &chat_id,
                    (rect.x, rect.y, rect.width, rect.height),
                    char_positions,
                );
            }
        }

        // Auto-snap chat scroll to bottom when new messages arrive.
        // Check ALL chat leaves (not just focused) so switching to a leaf
        // that received messages while unfocused shows the latest content.
        {
            let chat_leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_leaves.iter()
                .filter(|(_, d)| d.mode == gate4agent::InstanceMode::Chat)
                .map(|(id, _)| *id)
                .collect();
            for leaf_id in chat_leaf_ids {
                let new_len = self.sidebar_state.agent_leaf_snapshots.get(&leaf_id)
                    .and_then(|snap| {
                        if let mlc_sidebar_content::agent_types::AgentSnapshotMode::Chat(ref msgs) = snap.mode {
                            Some(msgs.len())
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if new_len == 0 { continue; }
                // For the focused leaf use current-frame dimensions from sidebar_result.
                // For non-focused leaves we don't have exact dimensions — snap
                // offset to a large value; render will clamp to actual max_scroll.
                let is_focused = self.sidebar_state.focused_agent_leaf == Some(leaf_id);
                let scroll = self.sidebar_state.agent_chat_scrolls.entry(leaf_id).or_default();
                if is_focused {
                    if let Some(ref sidebar_result) = self.last_sidebar_result {
                        let content_h = sidebar_result.agent_chat_content_height;
                        let viewport_h = sidebar_result.agent_chat_viewport_h;
                        let max_scroll = (content_h - viewport_h).max(0.0);
                        // Resolve the f64::MAX sentinel to the real clamped value so
                        // that the was_at_bottom check reflects where the renderer
                        // actually placed the viewport.  Without this, offset stays
                        // at f64::MAX permanently and the user can never scroll up.
                        if scroll.offset >= 1e18 {
                            scroll.offset = max_scroll;
                        }
                        let was_at_bottom = scroll.offset >= (max_scroll - 1.0).max(0.0);
                        if was_at_bottom {
                            scroll.offset = max_scroll;
                        }
                    }
                } else {
                    // Non-focused: resolve the sentinel using per-leaf dimensions
                    // when available, so that was_at_bottom detection works the
                    // same way as for the focused leaf.  This prevents the offset
                    // from staying at f64::MAX forever and reverting after new
                    // messages arrive.
                    if let Some((content_h, vp_h)) = self.last_sidebar_result
                        .as_ref()
                        .and_then(|sr| sr.agent_leaf_content_heights.get(&leaf_id).copied())
                    {
                        let max_scroll = (content_h - vp_h).max(0.0);
                        if scroll.offset >= 1e18 {
                            scroll.offset = max_scroll;
                        }
                        let was_at_bottom = scroll.offset >= (max_scroll - 1.0).max(0.0);
                        if was_at_bottom {
                            scroll.offset = max_scroll;
                        }
                    } else {
                        // No dimensions yet (first frame / leaf not rendered).
                        // Keep sentinel so render will show bottom on first display.
                        if scroll.offset == 0.0 {
                            scroll.offset = f64::MAX;
                        }
                    }
                }
            }
        }
    }

    /// Render ONLY the toolbar vector graphics into `ctx`.
    ///
    /// This is used by the dirty-caching path in `chart-app-vello`: when
    /// `toolbar_dirty` is set, the renderer calls this method with a context
    /// wrapping the dedicated `toolbar_scene`, caches it, and composites it
    /// on top of the main scene every frame.
    ///
    /// Updates `self.last_toolbar_result` so that subsequent frames with
    /// `skip_toolbar_draw=true` still have correct hit zones.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call.
    pub fn render_toolbar_only(&mut self, ctx: &mut dyn RenderContext) {
        let w = self.width as f64;
        let h = self.height as f64;
        let sidebar_w = self.sidebar_state.right_width();

        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        let selected_config: Option<mlc_core::state::selected_config::SelectedPrimitiveConfig> = {
            let pid2 = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            pid2.and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .and_then(|panel| panel.drawing_manager.get_selected_config())
        };

        let clock_time = {
            let utc_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                if !time_fmt.show_utc_prefix {
                    time
                } else if time_fmt.timezone_offset_hours >= 0 {
                    format!("[UTC+{}] {}", time_fmt.timezone_offset_hours, time)
                } else {
                    format!("[UTC{}] {}", time_fmt.timezone_offset_hours, time)
                }
            } else {
                let hours = (utc_secs / 3600) % 24;
                let minutes = (utc_secs / 60) % 60;
                let seconds = utc_secs % 60;
                format!("[UTC+0] {:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        };

        // S2.7.1: read active symbol/timeframe from branch_meta.
        let (active_sym_str, active_tf_str, active_gid) = {
            let cid = self.panel_app.panel_grid.active_container_id();
            let sym = cid.and_then(|c| self.managers.bubble.symbol_for_chart(c))
                .map(String::from).unwrap_or_default();
            let tf = cid.and_then(|c| self.managers.bubble.timeframe_for_chart(c))
                .map(|tf| tf.name.clone())
                .unwrap_or_default();
            let gid = cid.and_then(|c| self.managers.bubble.bubble_for_chart(c));
            (sym, tf, gid)
        };

        let toolbar_result = self.panel_app.render_toolbars_with_theme(
            ctx,
            &panel_layout,
            selected_config.as_ref(),
            Some(clock_time.as_str()),
            self.split_without_group,
            Some(active_sym_str.as_str()),
            Some(active_tf_str.as_str()),
            sidebar_w,
            active_gid,
            &self.managers.bubble,
        );

        self.last_toolbar_result = Some(toolbar_result);
    }

    /// Render ONLY the sidebar vector graphics into `ctx`.
    ///
    /// This is used by the dirty-caching path in `chart-app-vello`: when
    /// `sidebar_dirty_scene` is set, the renderer calls this method with a
    /// context wrapping the dedicated `sidebar_scene` to rebuild the cached
    /// scene.  It must be called AFTER `render()` so that the widget
    /// registrations go into the frame already opened by `render()`'s
    /// `input_coordinator.begin_frame()`.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call (same contract as
    /// `render_toolbar_only`).
    pub fn render_sidebar_only(&mut self, ctx: &mut dyn RenderContext) {
        let w = self.width as f64;
        let h = self.height as f64;
        let sidebar_w = self.sidebar_state.right_width();

        if !self.sidebar_state.is_right_open() {
            return;
        }

        // Sync cursor blink state — render() does this in prepare_frame,
        // but render_sidebar_only() skips prepare_frame so we must do it here.
        let chat_id = WidgetId::from(text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.sidebar_state.agent_input_cursor_visible =
                self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            self.sidebar_state.agent_input_focused_leaf = self.sidebar_state.focused_agent_leaf;
        }

        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Compute content_rect (same logic as in render()) so sidebar_x is correct.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };

        let top_h = panel_layout.top_toolbar_rect.height;
        let bottom_h = panel_layout.bottom_toolbar_rect.height;
        let sidebar_x = content_rect.x + content_rect.width;
        let sidebar_y = top_h;
        let sidebar_h = h - top_h - bottom_h;

        let sidebar_rect = LayoutRect::new(sidebar_x, sidebar_y, sidebar_w, sidebar_h);
        let sidebar_toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let panel_theme = panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());

        // Provide current agent state to sidebar for the Agents panel.
        // Snapshot each registered leaf instance.
        {
            let leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_leaves.keys().copied().collect();
            for leaf_id in leaf_ids {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if let Some(snap) = self.agent.snapshot_instance(desc.instance_id) {
                        self.sidebar_state.agent_leaf_snapshots.insert(leaf_id, snap);
                    }
                }
            }
        }

        let panels_store = &self.panels_store;
        let sidebar_result = mlc_sidebar_content::render::render_right_sidebar(
            ctx,
            &sidebar_rect,
            &mut self.sidebar_state,
            &sidebar_toolbar_theme,
            &mut self.input_coordinator.borrow_mut(),
            &mut |item, rect, ctx, coord, slot_prefix| {
                panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme, coord, slot_prefix);
            },
            &|item: &mlc_core::state::Panel| -> Option<String> {
                use mlc_core::state::Panel;
                use mlc_panels::trading::SymbolSource;
                // Account-bound panels still use SymbolSource for display labels.
                fn label_from_source(source: &SymbolSource, symbol: &str, exchange: &str, account_type: &str) -> Option<String> {
                    match source {
                        SymbolSource::Fixed { symbol, exchange, account_type } => {
                            Some(format!("{}:{}:{}", exchange, symbol, account_type))
                        }
                        SymbolSource::BoundToChart { leaf_id } => Some(format!("L#{}", leaf_id)),
                        SymbolSource::HyperFocus => Some(format!("{}:{}:{}", exchange, symbol, account_type)),
                    }
                }
                // Market-data panels: label directly from stored fields (no SymbolSource).
                fn label_from_fields(symbol: &str, exchange: &str, account_type: &str) -> Option<String> {
                    if symbol.is_empty() { return None; }
                    Some(format!("{}:{}:{}", exchange, symbol, account_type))
                }
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::Footprint(id) => panels_store.footprint.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                        .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                    Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                        .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                    Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    Panel::PositionManager(_)
                    | Panel::TradeLog(_)
                    | Panel::RiskCalculator(_)
                    // Chart renders via dedicated path — no symbol-source label.
                    | Panel::Chart(_) => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter)),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> {
                use mlc_core::tag_manager::SyncMemberId;
                let panel_id = item.panel_id().0;
                let member = SyncMemberId::Panel(panel_id);
                let gid = self.managers.bubble.group_for_member(member)?;
                let group = self.managers.bubble.bubble(gid)?;
                if group.auto_created { None } else { Some(group.color) }
            },
            &|item: &mlc_core::state::Panel| -> Option<mlc_sidebar_content::render::PanelSyncFlagsSnapshot> {
                use mlc_core::tag_manager::SyncMemberId;
                let panel_id = item.panel_id().0;
                let member = SyncMemberId::Panel(panel_id);
                let gid = self.managers.bubble.group_for_member(member)?;
                let group = self.managers.bubble.bubble(gid)?;
                Some(mlc_sidebar_content::render::PanelSyncFlagsSnapshot {
                    sync_symbol: group.effective_sync_symbol(member),
                    sync_crosshair: group.effective_sync_crosshair(member),
                })
            },
            &|item: &mlc_core::state::Panel| -> Option<(String, String, String)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::Footprint(id) => panels_store.footprint.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    Panel::PositionManager(_)
                    | Panel::TradeLog(_)
                    | Panel::RiskCalculator(_)
                    // Chart renders via dedicated path — no overlay info.
                    | Panel::Chart(_) => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                        s.column_config.show_bid_orders,
                        s.column_config.show_sell_trades,
                        s.column_config.show_buy_trades,
                        s.column_config.show_ask_orders,
                    ]),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_type,
                        s.column_config.show_side,
                        s.column_config.show_price,
                        s.column_config.show_qty,
                    ]),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_price,
                        s.column_config.show_size,
                    ]),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_side,
                        s.column_config.show_price,
                        s.column_config.show_size,
                        s.column_config.show_notional,
                    ]),
                    _ => None,
                }
            },
        );

        // Persist agent terminal rect for hover-focus.
        self.sidebar_state.agent_terminal_rect = sidebar_result
            .agent_terminal_rect
            .map(|r| (r.x as f32, r.y as f32, r.width as f32, r.height as f32));

        // Resize focused PTY leaf when the terminal content area changes grid dimensions.
        let new_size = sidebar_result.agent_terminal_size;
        if new_size.is_some() && new_size != self.sidebar_state.agent_terminal_size {
            self.sidebar_state.agent_terminal_size = new_size;
            // Resize ALL visible PTY leaves using their actual rect dimensions.
            let leaf_rects: Vec<(uzor::panels::LeafId, uzor::panels::PanelRect)> = self
                .sidebar_state
                .agent_docking
                .inner()
                .panel_rects()
                .iter()
                .map(|(&id, &r)| (id, r))
                .collect();
            for (leaf_id, rect) in leaf_rects {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if desc.mode == gate4agent::InstanceMode::Pty {
                        let cols = ((rect.width / 7.0) as u16).max(1);
                        let rows = ((rect.height / 19.0) as u16).max(1);
                        self.bridge.runtime().block_on(self.agent.resize_instance(desc.instance_id, cols, rows));
                    }
                }
            }
        }

        // Update text-field geometry for agent chat input field.
        if let (Some(rect), Some(char_positions)) = (
            sidebar_result.agent_input_rect,
            sidebar_result.agent_input_char_positions.clone(),
        ) {
            let chat_id = WidgetId::from(text_input::AGENT_CHAT);
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &chat_id,
                (rect.x, rect.y, rect.width, rect.height),
                char_positions,
            );
        }

        // Register panel overlay zones (color tag + gear) on InputCoordinator
        // so on_click routes through process_click → dispatch_panel_click.
        for (panel_id, _leaf_id, zones) in &sidebar_result.panel_overlay_zones {
            let [cx, cy, cw, ch] = zones.color_tag_rect;
            if cw > 0.0 && ch > 0.0 {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("panel_overlay:{}:color_tag", panel_id),
                    uzor::types::Rect::new(cx, cy, cw, ch),
                    uzor::input::Sense::CLICK,
                    &uzor::input::LayerId::new("toolbar"),
                );
            }
            let [dx, dy, dw, dh] = zones.dots_rect;
            if dw > 0.0 && dh > 0.0 {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("panel_overlay:{}:gear", panel_id),
                    uzor::types::Rect::new(dx, dy, dw, dh),
                    uzor::input::Sense::CLICK,
                    &uzor::input::LayerId::new("toolbar"),
                );
            }
        }

        self.last_sidebar_result = Some(sidebar_result);
    }

    /// Render panel-specific overlay popups that must appear above the sidebar.
    ///
    /// Renders the sync color grid popup when it targets a trading panel.
    /// Skipped in `render_to_scene` and rendered here instead so that it is
    /// composited after the sidebar scene and is never covered by it.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call (same contract as
    /// `render_toolbar_only` and `render_sidebar_only`).
    /// Cheap predicate: is there any panel-overlay popup to draw this frame?
    /// Lets the scene builder skip the per-frame `Scene::new()` + render + append
    /// when nothing is open (the common case).
    pub fn has_panel_overlay_popups(&self) -> bool {
        self.panel_app.sync_color_grid.is_open() && self.panel_app.sync_color_grid.target_is_panel()
    }

    pub fn render_panel_overlay_popups(&self, ctx: &mut dyn RenderContext) {
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let w = self.width as f64;
        let h = self.height as f64;

        // Panel-targeting sync color grid popup.
        if self.panel_app.sync_color_grid.is_open() && self.panel_app.sync_color_grid.target_is_panel() {
            use mlc_layout::layout::ChartModalLayout;
            let modal_layout = ChartModalLayout {
                prim_screen_w: w,
                prim_screen_h: h,
                prim_modal_y: 60.0,
                ind_screen_w: w,
                ind_screen_h: h,
                chart_x: 0.0,
                chart_y: 0.0,
                content_w: w,
                content_h: h,
            };
            self.panel_app.render_color_picker_popups(
                ctx,
                &modal_layout,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                &self.managers.bubble,
            );
        }

    }

    /// Warm up all enabled connectors exactly once, via the shared bridge.
    ///
    /// The one-shot guard lives in the BRIDGE (which outlives the skeleton→live
    /// promote), not on ChartApp (which is dropped+recreated by the promote). So
    /// the dials fire once — during the empty skeleton/loading screen — and the
    /// live window's call is a no-op. This replaces the old per-ChartApp batch
    /// cursor, which reset on promote and re-walked all 21 exchanges in the
    /// visible chart (the source of the 30-40s startup churn). The dials run
    /// concurrently on the bridge's own runtime, off the UI thread.
    pub fn warmup_connectors(&self) {
        let enabled: Vec<digdigdig3::ExchangeId> = ALL_EXCHANGE_IDS
            .iter()
            .copied()
            .filter(|eid| {
                self.sidebar_state.connector_enabled
                    .get(eid.as_str())
                    .copied()
                    .unwrap_or(true)
            })
            .collect();
        self.bridge.warmup_all_connectors(&enabled);
    }

    // -------------------------------------------------------------------------
    // Modal layer (drawn ABOVE the crosshair overlay)
    // -------------------------------------------------------------------------

    /// Draw all modal / popup / dialog layers into `ctx` and store their results
    /// directly into `self` (no `RenderOutput` — same contract as
    /// `render_toolbar_only` / `render_sidebar_only`).
    ///
    /// # Drawing order
    /// chart → overlay(crosshair) → **modals** (this method)
    ///
    /// The VelloGpu compositor appends the scene produced here AFTER the overlay
    /// scene, ensuring modals always draw on top of the crosshair.
    ///
    /// # Input-frame contract
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()`.
    /// Widget registrations made here accumulate in `self.input_coordinator.widgets`
    /// and are cleared by the NEXT frame's `begin_frame()` — exactly the same
    /// contract as `render_panel_overlay_popups`.  Input events that arrive between
    /// renders will hit-test against these registrations, which is correct because
    /// modal hit-zones must beat chart/toolbar zones (modals are the topmost layer).
    pub fn render_modal_layer(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64) {
        let w = self.width as f64;
        let h = self.height as f64;

        // Recompute layout locals that the modal blocks depend on.
        // This mirrors what render_to_scene computes; the cost is negligible
        // (pure arithmetic, no allocation).
        let sidebar_w = if self.panel_app.user_settings_state.show_profile_manager
            || self.panel_app.user_settings_state.show_welcome_wizard
        {
            0.0
        } else {
            self.sidebar_state.right_width()
        };
        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        let modal_right_edge = content_rect.x + content_rect.width;
        let modal_layout = ChartModalLayout {
            prim_screen_w: modal_right_edge,
            prim_screen_h: h,
            prim_modal_y: 60.0,
            ind_screen_w: modal_right_edge,
            ind_screen_h: h,
            chart_x: content_rect.x,
            chart_y: content_rect.y,
            content_w: content_rect.width,
            content_h: content_rect.height,
        };
        let frame_theme = self.panel_app.frame_theme_for_render();
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let toolbar_state = ToolbarState::default();

        // 5. Render chart-level modals (primitive settings, indicator settings,
        //    chart settings, compare settings, etc.)
        //
        // render_modals reads panel_app modal state and needs &DrawingManager for the
        // active window.  After S2.8 the drawing_manager lives in panels_store.chart_panel
        // (a disjoint field from panel_app), so no raw pointer trick is needed.
        let active_dm_pid = self.panel_app.panel_grid
            .active_container()
            .map(|w| w.chart_panel_id);
        let dm_ref: Option<&mlc_core::drawing::DrawingManager> = active_dm_pid
            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .map(|panel| &panel.drawing_manager);
        let chart_settings_data = self.build_chart_settings_data();
        let theme_manager_ptr: *const mlc_core::theme::ThemeManager =
            &self.panel_app.theme_manager as *const _;

        let modal_result = self.panel_app.render_modals(
            ctx,
            &modal_layout,
            dm_ref,
            Some(&self.indicator_manager as &dyn IndicatorSource),
            Some(&chart_settings_data),
            // SAFETY: theme_manager lives inside panel_app which render_modals
            // does not mutate (it only reads modal state fields).
            Some(unsafe { &*theme_manager_ptr }),
            &frame_theme,
            &toolbar_theme,
            &toolbar_state,
            current_time_ms,
            &mut self.input_coordinator.borrow_mut(),
            &self.managers.bubble,
        );
        let mut frame_result: Option<ChartModalRenderResult> = Some(modal_result);

        // 5b. Search overlay (indicator / symbol / compare)
        let search_modal_result: Option<mlc_layout::layout::ModalSearchResult> =
            if self.modal_state.current.is_search_overlay() {
                let screen = ChartScreenArea { x: 0.0, y: 0.0, width: modal_right_edge, height: h };
                let indicator_catalog = self.build_indicator_catalog();
                let hovered = self.modal_state.hovered_item_id.as_deref();
                let toolbar_theme_search = self.panel_app.toolbar_theme_for_render();
                let indicator_sets = &self.panel_app.template_manager.indicator_sets;
                Some(render_search_overlay(
                    ctx,
                    screen,
                    &self.modal_state,
                    &indicator_catalog,
                    indicator_sets,
                    hovered,
                    &frame_theme,
                    &toolbar_theme_search,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 5c. Preset name input (CreateIndicatorSet mode)
        if self.panel_app.preset_name_input.is_open
            && self.panel_app.preset_name_input.mode
                == mlc_layout::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
        {
            use mlc_layout::layout::modals::preset_name_input::render_preset_name_input;
            let pni_result = render_preset_name_input(
                ctx,
                w, h,
                &self.panel_app.preset_name_input,
                &frame_theme,
                &toolbar_theme,
                current_time_ms,
                &mut self.input_coordinator.borrow_mut(),
            );
            if let Some(ref mut fr) = frame_result {
                fr.preset_name_input = Some(pni_result);
            }
        }

        // 6. Context menu
        let context_menu_result: Option<ContextMenuResult> =
            if self.panel_app.context_menu_state.is_open() {
                let dropdown_theme = self.panel_app.dropdown_theme_for_render();
                let hovered_id = self.hovered_context_menu_item_id.as_deref();
                Some(render_context_menu(
                    ctx,
                    &self.panel_app.context_menu_state,
                    &dropdown_theme,
                    hovered_id,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8a. Color picker popups (non-panel-targeting ones).
        // Panel-targeting sync_color_grid is still handled by render_panel_overlay_popups().
        if !self.panel_app.sync_color_grid.target_is_panel() {
            let cp_result = self.panel_app.render_color_picker_popups(
                ctx,
                &modal_layout,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                &self.managers.bubble,
            );
            if let Some(ref mut fr) = frame_result {
                if cp_result.color_picker.is_some() {
                    fr.color_picker = cp_result.color_picker;
                }
                if cp_result.sync_color_grid.is_some() {
                    fr.sync_color_grid = cp_result.sync_color_grid;
                }
            }
        }

        // 8b. Watchlist modal
        let last_watchlist_modal_result: Option<mlc_layout::layout::modals::watchlist_modal::WatchlistModalResult> =
            if self.watchlist_modal.is_open() {
                let color_flags: Vec<String> = self.sidebar_state.watchlist_items.iter()
                    .map(|item| {
                        self.sidebar_state.watchlist_manager.active_list()
                            .and_then(|l| l.get_color_flag(&item.symbol, &item.exchange, &item.account_type))
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                let entries: Vec<WatchlistEntry> = self.sidebar_state.watchlist_items.iter()
                    .zip(color_flags.iter())
                    .map(|(item, flag)| WatchlistEntry {
                        symbol: item.symbol.clone(),
                        exchange: item.exchange.clone(),
                        price: item.last_price,
                        change_pct: item.change_percent,
                        change_abs: item.last_price - (item.last_price / (1.0 + item.change_percent / 100.0)),
                        high_24h: item.high_24h,
                        low_24h: item.low_24h,
                        volume_24h: item.volume_24h,
                        color_flag: flag.clone(),
                        account_type: item.account_type.clone(),
                    })
                    .collect();
                let groups_info: Vec<WatchlistGroupInfo> = self.sidebar_state.watchlist_manager.lists.iter()
                    .map(|list| WatchlistGroupInfo {
                        id: list.id,
                        name: list.name.clone(),
                        color: if list.groups.is_empty() {
                            String::new()
                        } else {
                            list.groups[0].color.clone()
                        },
                        symbol_count: list.all_symbols().len(),
                        is_active: list.id == self.sidebar_state.watchlist_manager.active_list_id,
                    })
                    .collect();
                Some(render_watchlist_modal(
                    ctx,
                    modal_right_edge,
                    h,
                    &self.watchlist_modal,
                    &entries,
                    &groups_info,
                    &frame_theme,
                    &toolbar_theme,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8c. Watchlist group name input modal
        let last_wl_group_name_result: Option<WlGroupNameInputResult> =
            if self.wl_group_name_input.is_open() {
                Some(render_wl_group_name_input(
                    ctx,
                    modal_right_edge,
                    h,
                    &self.wl_group_name_input,
                    &frame_theme,
                    &toolbar_theme,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8d. Clock popup (timezone selector + 24h toggle)
        if self.panel_app.clock_popup_state.is_open {
            let bottom_toolbar_y = h - panel_layout.bottom_toolbar_rect.height;
            let (active_offset, use_24h, show_utc) = self.panel_app.panel_grid.active_container()
                .map(|w| (
                    w.scale_settings.time_format.timezone_offset_hours,
                    w.scale_settings.time_format.use_24h,
                    w.scale_settings.time_format.show_utc_prefix,
                ))
                .unwrap_or((0, false, true));
            let hovered = self.panel_app.clock_popup_state.hovered_item.as_deref();
            render_clock_popup(
                ctx,
                w,
                h,
                bottom_toolbar_y,
                active_offset,
                use_24h,
                show_utc,
                hovered,
                &frame_theme,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
            );
        }

        // Store results directly into self — input handlers read these fields
        // between renders.  apply_render_output() may set them to None when the
        // chart scene is rebuilt (chart_dirty), but render_modal_layer() always
        // runs on the same frame and overwrites them here before any input event
        // could read stale values.
        self.frame_result = frame_result;
        self.search_modal_result = search_modal_result;
        self.context_menu_result = context_menu_result;
        self.last_watchlist_modal_result = last_watchlist_modal_result;
        self.last_wl_group_name_result = last_wl_group_name_result;
    }

    // -------------------------------------------------------------------------
    // Agent hover-focus
    // -------------------------------------------------------------------------

    /// Update `agent_pty_hover_focused` based on the current mouse position.
    ///
    /// Call from the `CursorMoved` handler in `chart-app-vello` with the
    /// chart-relative coordinates (i.e. after subtracting chrome height from
    /// the window `y`).  Returns `true` if the hover state changed, indicating
    /// that the sidebar scene should be marked dirty.
    ///
    /// The hover focus is *transient* — it is cleared automatically when the
    /// cursor leaves the terminal rect.  A click inside the rect should promote
    /// to a persistent click-focus (handled separately in the click handler).
    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Apply the group's current instrument key (symbol / exchange / account_type)
    /// to all panel members of the given sync group.
    ///
    /// Called when the active chart changes (after `reassign_synced_panels`) and
    /// when a chart changes its symbol (from `propagate_symbol_to_sync_group`).
    pub(crate) fn apply_key_to_panels_in_group(
        &mut self,
        group_id: mlc_core::tag_manager::BubbleId,
    ) {
        let (symbol, exchange, account_type, panel_ids) = {
            let group = match self.managers.bubble.bubble(group_id) {
                Some(g) => g,
                None => return,
            };
            (
                group.symbol.clone(),
                group.exchange.clone(),
                group.account_type.clone(),
                self.managers.bubble.panel_members(group_id),
            )
        };

        // Track which (exchange, symbol, account_type) combos need depth subscription
        // so panels that receive orderbook data start getting the new symbol's stream.
        let mut needs_depth_sub = false;

        // Collect old depth subscriptions that differ from the new symbol so we
        // can unsubscribe them after updating panel state.  Each entry is
        // (exchange_string, symbol_string, account_type_string).
        let mut old_depth_subs: Vec<(String, String, String)> = Vec::new();

        for pid in &panel_ids {
            let panel_id = mlc_core::state::PanelId(*pid);
            // DOM symbol change: unsubscribe old orderbook + trades, subscribe new.
            // Two-pass to avoid holding a mutable borrow while calling bridge.
            let dom_change = self.panels_store.dom.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = dom_change {
                // Unsubscribe old orderbook and trades.
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.bridge.unsubscribe_orderbook(eid, &old_sym, old_at);
                        self.bridge.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                // Subscribe new orderbook and trades.
                let (new_ob_handle, new_trade_handle) = if !symbol.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&exchange) {
                        let new_at = account_type_from_label(&account_type);
                        let ob = self.bridge.subscribe_orderbook(eid, &symbol, new_at);
                        let tr = self.bridge.subscribe_trades(eid, &symbol, new_at);
                        (ob, tr)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                // Update state.
                if let Some(s) = self.panels_store.dom.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_orderbook = new_ob_handle;
                    s.last_seen_update_id = None;
                    s.level_qty_bid.clear();
                    s.level_qty_ask.clear();
                    s.shared_trades = new_trade_handle;
                    s.volume_by_price.clear();
                    s.max_volume = 0.0;
                    s.recent_fills.clear();
                }
            }
            if let Some(s) = self.panels_store.footprint.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            if let Some(s) = self.panels_store.volume_profile.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            if let Some(s) = self.panels_store.liquidity_heatmap.get_mut(&panel_id) {
                if s.symbol != symbol && !s.symbol.is_empty() {
                    old_depth_subs.push((s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
                let symbol_changed = s.symbol != symbol;
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
                if symbol_changed {
                    s.snapshots.clear();
                    s.max_depth = 0.0;
                }
                needs_depth_sub = true;
            }
            // BigTrades symbol change: unsubscribe old, subscribe new.
            // We do this in two passes to avoid holding a mutable borrow on
            // panels_store at the same time as calling bridge methods.
            let bt_change = self.panels_store.big_trades.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = bt_change {
                // Unsubscribe old.
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.bridge.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                // Subscribe new.
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.bridge.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                // Update state.
                if let Some(s) = self.panels_store.big_trades.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.big_trades.clear();
                }
            }
            // VolumeProfile symbol change: unsubscribe old, subscribe new.
            let vp_change = self.panels_store.volume_profile.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = vp_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.bridge.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.bridge.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.volume_profile.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.volume_by_price.clear();
                    s.buy_sell_by_price.clear();
                    s.total_volume = 0.0;
                    s.max_volume_at_price = 0.0;
                    s.poc = 0.0;
                }
            }
            // Footprint symbol change: unsubscribe old, subscribe new.
            let fp_change = self.panels_store.footprint.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = fp_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.bridge.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.bridge.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.footprint.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.footprints.clear();
                    s.poc_by_candle.clear();
                    s.imbalances.clear();
                }
            }
            // TradeTape symbol change: unsubscribe old, subscribe new.
            let tt_change = self.panels_store.trade_tape.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = tt_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.bridge.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.bridge.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.trade_tape.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.trades.clear();
                }
            }
            if let Some(s) = self.panels_store.l2_tape.get_mut(&panel_id) {
                if s.symbol != symbol && !s.symbol.is_empty() {
                    old_depth_subs.push((s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
                let symbol_changed = s.symbol != symbol;
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
                if symbol_changed {
                    s.events.clear();
                    s.previous_book.clear();
                    s.spoof_alerts.clear();
                }
                needs_depth_sub = true;
            }
            if let Some(s) = self.panels_store.order_entry.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            if let Some(s) = self.panels_store.trading_container.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
        }

        // Unsubscribe old depth streams that are no longer needed.  We deduplicate
        // so that multiple panels sharing the same old (exchange, symbol) only send
        // one RemoveSymbol command.
        old_depth_subs.sort_unstable();
        old_depth_subs.dedup();
        for (old_exch, old_sym, old_at_label) in &old_depth_subs {
            // Skip if this old key is the same as the new key — nothing to remove.
            if old_sym == &symbol && old_exch == &exchange && old_at_label == &account_type {
                continue;
            }
            let old_eid = self.exchange_symbols
                .keys()
                .find(|e| e.as_str() == old_exch.as_str())
                .copied()
                .unwrap_or(self.active_exchange);
            let old_at = crate::account_type_from_label(old_at_label);
            self.bridge.unsubscribe_depth(old_eid, old_sym, old_at);
            dlog!(
                "[BubbleStore] unsubscribed depth for panel group {} ← {} @ {} ({})",
                group_id.0, old_sym, old_exch, old_at_label
            );
        }

        // If any panel needs orderbook data for the new symbol, subscribe the depth
        // WebSocket stream so live updates start arriving for it.
        if needs_depth_sub && !symbol.is_empty() {
            let eid = self.exchange_symbols
                .keys()
                .find(|e| e.as_str() == exchange)
                .copied()
                .unwrap_or(self.active_exchange);
            let at = crate::account_type_from_label(&account_type);
            self.bridge.subscribe_depth(eid, &symbol, at);
            dlog!(
                "[BubbleStore] subscribed depth for panel group {} → {} @ {} ({})",
                group_id.0, symbol, exchange, account_type
            );
        }
    }

    /// Request bars for every non-active open preset so the bridge cache is
    /// populated before the user clicks a background tab.
    ///
    /// Safe to call repeatedly: `request_bars` is deduped by the bridge's
    /// in-flight guard, so a double-call never creates a duplicate fetch.
    /// Call this once per startup after presets and open_tabs are populated
    /// (i.e. after `LoadPreset` completes in `new_window`).
    pub fn warm_background_tabs(&mut self) {
        let active_id = self.panel_app.active_preset_id.clone();
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        let warm_targets: Vec<(String, String, mlc_core::state::Timeframe, String)> = self
            .panel_app
            .open_tabs
            .iter()
            .filter(|tab| **tab != active_id)
            .filter_map(|tab| self.panel_app.presets.get(tab))
            .flat_map(|preset| {
                preset.windows.iter().map(move |w| {
                    // Prefer the owning group's identity so the warm-up
                    // request matches the key the live restore will use.
                    // A window snapshot may carry stale account_type (e.g.
                    // "S") when the group's is "F" — using the window's
                    // snapshot would produce a cache-miss on restore.
                    if let Some(bid) = w.bubble_id {
                        if let Some(b) = preset.bubbles.iter().find(|b| b.id == bid) {
                            if !b.symbol.is_empty() {
                                return (
                                    b.symbol.clone(),
                                    b.exchange.clone(),
                                    b.timeframe.clone(),
                                    b.account_type.clone(),
                                );
                            }
                        }
                    }
                    (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone())
                })
            })
            .filter(|(sym, _, _, _)| !sym.is_empty())
            .collect();
        let mut warmed = 0usize;
        for (sym, exch, tf, at_label) in warm_targets {
            let eid = digdigdig3::ExchangeId::from_str(&exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            let at = account_type_from_label(&at_label);
            self.bridge.ensure_connector(eid);
            self.bridge.request_bars(eid, &sym, &tf, at, None, Some(bar_count), false);
            warmed += 1;
        }
        eprintln!("[ChartApp] startup: warmed {} background tab series", warmed);
    }

    /// Amortized variant of [`warm_background_tabs`] for the post-promote
    /// path: return the target list without executing anything, so the
    /// caller can warm ONE series per frame instead of blocking the main
    /// thread for the whole batch (~120 ms per series measured).
    pub fn warm_background_tab_targets(&self) -> Vec<(String, String, mlc_core::state::Timeframe, String)> {
        let active_id = self.panel_app.active_preset_id.clone();
        self.panel_app
            .open_tabs
            .iter()
            .filter(|tab| **tab != active_id)
            .filter_map(|tab| self.panel_app.presets.get(tab))
            .flat_map(|preset| {
                preset.windows.iter().map(move |w| {
                    if let Some(bid) = w.bubble_id {
                        if let Some(b) = preset.bubbles.iter().find(|b| b.id == bid) {
                            if !b.symbol.is_empty() {
                                return (
                                    b.symbol.clone(),
                                    b.exchange.clone(),
                                    b.timeframe.clone(),
                                    b.account_type.clone(),
                                );
                            }
                        }
                    }
                    (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone())
                })
            })
            .filter(|(sym, _, _, _)| !sym.is_empty())
            .collect()
    }

    /// Warm a single background-tab series (one step of the amortized
    /// post-promote warm-up).
    pub fn warm_one_background_tab(&mut self, target: &(String, String, mlc_core::state::Timeframe, String)) {
        let (sym, exch, tf, at_label) = target;
        let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
        let eid = digdigdig3::ExchangeId::from_str(exch)
            .unwrap_or(digdigdig3::ExchangeId::Binance);
        let at = account_type_from_label(at_label);
        self.bridge.ensure_connector(eid);
        self.bridge.request_bars(eid, sym, tf, at, None, Some(bar_count), false);
    }

    /// Synchronise `window.inner_docking` with the external `IndicatorManager`.
    ///
    /// Reads desired sub-pane instance ids from `indicator_manager`, walks the
    /// inner docking tree, adds missing leaves via `split_leaf`, hides vanished
    /// leaves via `hide_leaf`, restores re-appeared leaves via `show_leaf`.
    /// Also seeds `sub_pane_y_state` for brand-new panes from the visible range
    /// and drains pending preset restore state into the tree.
    ///
    /// Call after any operation that adds/removes indicators or recalculates
    /// indicator values (init, tick, indicator create/delete).
    pub fn sync_sub_panes_from_manager(&mut self) {
        use mlc_core::state::chart_container::collect_inner_instance_ids;
        use mlc_core::state::chart_container_leaf::ChartContainerLeaf;

        // Step 1: collect window-level read-only data before taking mutable borrows.
        let window_data: Vec<(mlc_core::LeafId, u64, String, usize, usize)> = self
            .panel_app
            .panel_grid
            .iter_containers()
            .map(|(leaf_id, window)| {
                let chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                let chart_id_val = chart_id.map(|cid| cid.0).unwrap_or(0);
                let symbol = chart_id
                    .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let (vs, ve) = window.viewport.visible_range();
                let ve = ve.min(window.bars.len());
                (leaf_id, chart_id_val, symbol, vs, ve)
            })
            .collect();

        // Step 2: for each window, collect desired instance_ids from indicator_manager.
        for (leaf_id, chart_id_val, symbol, visible_start, visible_end) in &window_data {
            let sub_pane_data: Vec<(u64, Option<(f64, f64)>)> = self
                .indicator_manager
                .get_instances_for_symbol_in_window(symbol, *chart_id_val)
                .into_iter()
                .filter(|i| i.pane > 0)
                .map(|i| {
                    let range = self.indicator_manager.calculate_pane_range(
                        i.id,
                        *visible_start,
                        *visible_end,
                    );
                    (i.id, range)
                })
                .collect();

            let window = match self.panel_app.panel_grid.container_for_leaf_mut(*leaf_id) {
                Some(w) => w,
                None => continue,
            };

            let pending_ratios = self.pending_sub_pane_ratios.get(&window.id.0).cloned();
            let pending_order = self.pending_sub_pane_order.get(&window.id.0).cloned();
            let pending_hidden = self.pending_sub_pane_hidden.get(&window.id.0).cloned();
            let pending_above = self.pending_sub_pane_above_main.get(&window.id.0).cloned();

            // Apply above-main set from preset restore (one-shot via pending map).
            if let Some(ref above_set) = pending_above {
                window.sub_pane_above_main = above_set.clone();
            }

            // Determine the desired order (from preset restore or indicator_manager order).
            let desired_ids: Vec<u64> = if let Some(ref order) = pending_order {
                // Preset order first, then append newly-discovered ids.
                let mut ids: Vec<u64> = order.iter()
                    .filter(|&&iid| sub_pane_data.iter().any(|(id, _)| *id == iid))
                    .copied()
                    .collect();
                for (iid, _) in &sub_pane_data {
                    if !ids.contains(iid) {
                        ids.push(*iid);
                    }
                }
                ids
            } else {
                sub_pane_data.iter().map(|(id, _)| *id).collect()
            };

            let range_map: std::collections::HashMap<u64, Option<(f64, f64)>> =
                sub_pane_data.iter().map(|(id, r)| (*id, *r)).collect();

            // Step 3: reconcile inner_docking with desired_ids.
            // Get current leaf set (excluding MainChart leaf at index 0).
            let current_ids = collect_inner_instance_ids(&window.inner_docking);

            // Find the main chart leaf id (always first child).
            let main_leaf_id = window.inner_docking.tree().root().children
                .first()
                .and_then(|n| if let uzor::panels::PanelNode::Leaf(l) = n { Some(l.id) } else { None });

            // Add leaves for desired_ids not yet in the tree (or hidden).
            for &iid in &desired_ids {
                let existing_leaf = window.inner_docking.tree().leaves().iter()
                    .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(iid))
                    .map(|l| (l.id, l.hidden));
                match existing_leaf {
                    None => {
                        // New indicator: append as a flat sibling under the root branch.
                        // split_leaf would nest the new leaf inside a fresh sub-branch,
                        // breaking the [MainChart, SubPane1, SubPane2..] sibling list
                        // that the render path's collect_inner_* helpers depend on.
                        let last_leaf_id = window.inner_docking.tree().root().children
                            .last()
                            .and_then(|n| if let uzor::panels::PanelNode::Leaf(l) = n { Some(l.id) } else { None })
                            .or(main_leaf_id);
                        if let Some(anchor) = last_leaf_id {
                            window.inner_docking.tree_mut().add_leaf_near(
                                ChartContainerLeaf::SubPane { instance_id: iid },
                                anchor,
                            );
                        }
                        // Seed y_state for the new pane.
                        let entry = window.sub_pane_y_state.entry(iid).or_default();
                        if let Some(Some((p_min, p_max))) = range_map.get(&iid) {
                            entry.price_min = *p_min;
                            entry.price_max = *p_max;
                        }
                    }
                    Some((leaf_id_inner, true)) => {
                        // Leaf exists but was hidden — restore it.
                        window.inner_docking.tree_mut().show_leaf(leaf_id_inner);
                    }
                    Some((_, false)) => {
                        // Already present and visible — nothing to do.
                    }
                }
            }

            // Hide leaves for instance_ids that have vanished from desired set.
            let current_ids_after = collect_inner_instance_ids(&window.inner_docking);
            for iid in &current_ids_after {
                if !desired_ids.contains(iid) {
                    if let Some(leaf_id_inner) = window.inner_docking.tree().leaves().iter()
                        .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(*iid))
                        .map(|l| l.id)
                    {
                        window.inner_docking.tree_mut().hide_leaf(leaf_id_inner);
                    }
                }
            }

            // Apply preset-restore hidden set.
            if let Some(ref hidden_set) = pending_hidden {
                for &iid in hidden_set {
                    if let Some(leaf_id_inner) = window.inner_docking.tree().leaves().iter()
                        .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(iid))
                        .map(|l| l.id)
                    {
                        window.inner_docking.tree_mut().hide_leaf(leaf_id_inner);
                    }
                }
            }

            // Set preserve_if_empty on root branch so splits aren't collapsed when empty.
            let root_id = window.inner_docking.tree().root().id;
            window.inner_docking.tree_mut().set_branch_preserve_if_empty(root_id, true);

            // Re-pin the vertical stack layout after the add/hide/show
            // reconcile above: uzor re-infers HORIZONTAL layouts from child
            // count on every structural change, which breaks separator drag
            // math (see pin_vertical_stack_layout docs).
            mlc_core::state::chart_container::pin_vertical_stack_layout(&mut window.inner_docking);

            // Apply preset-restore proportions if available.
            if let Some(ref ratios) = pending_ratios {
                let current_visible_ids = collect_inner_instance_ids(&window.inner_docking);
                if !current_visible_ids.is_empty() {
                    let sub_total: f64 = current_visible_ids.iter()
                        .map(|iid| ratios.get(iid).copied().unwrap_or(0.15) as f64)
                        .sum::<f64>()
                        .min(0.75);
                    let n = current_visible_ids.len();
                    let default_sub = sub_total / n as f64;
                    let main_share = (1.0 - sub_total).max(0.1);
                    let mut props = vec![main_share];
                    for iid in &current_visible_ids {
                        props.push(ratios.get(iid).copied().unwrap_or(default_sub as f32) as f64);
                    }
                    let total: f64 = props.iter().sum();
                    if total > 0.0 {
                        for v in &mut props { *v /= total; }
                    }
                    let root_branch_id = window.inner_docking.tree().root().id;
                    window.inner_docking.tree_mut().set_branch_proportions(root_branch_id, props);
                }
            } else {
                // Seed proportions when none have been set yet (e.g. after
                // add_leaf_near clears them on every structural change).
                // Without seeding, drag_separator falls back to equal 1/N
                // shares which differs from the rendered layout (main large,
                // sub-panes ~0.2 each) causing a visible jump on first grab.
                //
                // Rule: each sub-pane gets 0.2 of total, main gets the rest,
                // clamped so main is never below 0.4. This mirrors
                // InnerLayout::add_indicator which also carves 0.2 per pane.
                let root_child_count = window.inner_docking.tree().root().children.len();
                let stored_props_len = window.inner_docking.tree().root().proportions.len();
                if stored_props_len != root_child_count && root_child_count >= 2 {
                    let n_panes = root_child_count - 1; // subtract main-chart leaf
                    // 0.2 per sub-pane, main = remainder, clamped to [0.4, 1.0].
                    let sub_share_each = 0.2_f64;
                    let main_share = (1.0 - sub_share_each * n_panes as f64).max(0.4);
                    let mut props = vec![main_share];
                    for _ in 0..n_panes {
                        props.push(sub_share_each);
                    }
                    // Normalize so sum == 1.0.
                    let total: f64 = props.iter().sum();
                    if total > 0.0 {
                        for v in &mut props {
                            *v /= total;
                        }
                    }
                    let root_branch_id = window.inner_docking.tree().root().id;
                    window.inner_docking.tree_mut().set_branch_proportions(root_branch_id, props);
                }
            }

            // Suppress "current_ids" unused warning from the step before reconcile.
            let _ = current_ids;
        }

        // Clear all pending restore state once applied (one-shot).
        self.pending_sub_pane_ratios.clear();
        self.pending_sub_pane_above_main.clear();
        self.pending_sub_pane_order.clear();
        self.pending_sub_pane_hidden.clear();
    }

    /// Sync sub-pane pixel geometry (height, y_offset, chart_width) from the
    /// computed layout into each window's sub_pane structs.
    ///
    /// Must be called with `&mut self` so we can write to sub_panes.  Call
    /// once per frame inside `prepare_frame`, after the layout has been
    /// computed, so that `PanSubPane` action handlers and other code that
    /// reads these fields see up-to-date values.
    fn sync_sub_pane_geometry(&mut self) {
        // Phase 1: compute (leaf_id, leaf_rect, extended_layout) using &self.
        // `build_extended_layout_for_leaf` borrows self immutably, so we must
        // finish all those borrows before the mutable writes in phase 2.
        let content_rect = self.content_rect;

        let leaf_rects: Vec<(mlc_core::LeafId, LayoutRect)> = self
            .panel_app
            .panel_grid
            .panel_rects()
            .iter()
            .map(|(&leaf_id, &sub_rect)| {
                let leaf_rect = LayoutRect {
                    x: content_rect.x + sub_rect.x as f64,
                    y: content_rect.y + sub_rect.y as f64,
                    width: sub_rect.width as f64,
                    height: sub_rect.height as f64,
                };
                (leaf_id, leaf_rect)
            })
            .collect();

        let extended_layouts: Vec<(mlc_core::LeafId, mlc_core::ExtendedFrameLayout)> =
            leaf_rects
                .iter()
                .filter_map(|&(leaf_id, leaf_rect)| {
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect)?;
                    Some((leaf_id, extended))
                })
                .collect();

        // Phase 2: geometry writeback was previously to SubPane structs.
        // After S4 the inner_docking tree drives layout; no per-pane geometry
        // fields need to be written back here. Leave loop body empty.
        let _ = &extended_layouts;

        // Phase 3: store per-leaf layouts as state so input handlers can read
        // them without rebuilding (producer/consumer split).  Rebuild fresh each
        // call so removed leaves never linger.
        self.frame_layouts = extended_layouts.into_iter().collect();

        // Build and cache the single/active-leaf layout used by non-split input
        // paths and process_output_actions.  This is the correct place because
        // sync_sub_pane_geometry runs in the layout phase (prepare_frame) once
        // per frame, before any input events for that frame are processed.
        self.active_frame_layout = Some(self.build_extended_layout());
    }

    /// Build indicator catalog items from the IndicatorManager definitions.
    fn build_indicator_catalog(&self) -> Vec<IndicatorCatalogItem> {
        let mut items: Vec<IndicatorCatalogItem> = self.indicator_manager.get_definitions().iter().map(|def| {
            IndicatorCatalogItem::new(&def.type_id, &def.name, &def.short_name, def.category)
                .with_description(&def.description)
                .with_overlay(def.overlay)
        }).collect();
        items.sort_by(|a, b| a.short_name.cmp(&b.short_name));
        items
    }

    /// Build `SearchResult` list from exchange symbols, filtered by `query`.
    ///
    /// Iterates all exchanges and collects up to 100 results per exchange
    /// (200 total). Returns empty list if no symbols are loaded yet.
    pub(crate) fn build_symbol_search_results(
        query: &str,
        watchlist_manager: &mlc_sidebar_content::watchlist::WatchlistManager,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<mlc_live_data::SymbolInfo>>,
    ) -> Vec<mlc_core::state::modal_state::SearchResult> {
        use mlc_core::state::modal_state::SearchResult;
        use std::collections::HashMap;

        let q_full = query.to_lowercase();

        // Composite query syntax: "SYMBOL[:EXCHANGE[:AT]]" — TradingView-style.
        // Part 1 filters exchanges (full slug, slug prefix, or a short alias),
        // part 2 filters account type ("s"/"f", also "spot"/"fut"/"futures"/"perp").
        let mut q_parts = q_full.splitn(3, ':');
        let q = q_parts.next().unwrap_or("").to_string();
        let q_exchange = q_parts.next().map(str::trim).filter(|p| !p.is_empty());
        let q_at = q_parts.next().map(str::trim).filter(|p| !p.is_empty());

        /// Short exchange aliases for the composite search syntax.
        fn exchange_alias(alias: &str) -> Option<&'static str> {
            Some(match alias {
                "bn" => "binance",
                "bb" => "bybit",
                "kc" => "kucoin",
                "gt" => "gateio",
                "hl" => "hyperliquid",
                "ltr" => "lighter",
                "dr" => "deribit",
                "up" => "upbit",
                "cc" => "cryptocom",
                "cb" => "coinbase",
                "bg" => "bitget",
                "bf" => "bitfinex",
                "bs" => "bitstamp",
                "bx" => "bingx",
                "gm" => "gemini",
                "kr" => "kraken",
                "dy" => "dydx",
                "mx" => "mexc",
                _ => return None,
            })
        }

        /// Does the exchange slug match the query's exchange part?
        /// Accepts an exact alias ("hl") or any prefix of the full slug ("hyp").
        fn exchange_matches(slug: &str, part: &str) -> bool {
            if let Some(full) = exchange_alias(part) {
                return slug == full;
            }
            slug.starts_with(part)
        }

        /// Does the account-type short label ("S"/"F") match the query's AT part?
        fn at_matches(short_label: &str, part: &str) -> bool {
            match part {
                "s" | "spot" => short_label.eq_ignore_ascii_case("S"),
                "f" | "fut" | "futures" | "perp" => short_label.eq_ignore_ascii_case("F"),
                other => short_label.eq_ignore_ascii_case(other),
            }
        }

        /// Compute the canonical key for a symbol when base+quote are non-empty.
        /// Returns `None` when either asset field is empty (no canonicalization).
        fn canonical_key(s: &mlc_live_data::SymbolInfo) -> Option<String> {
            if s.base_asset.is_empty() || s.quote_asset.is_empty() {
                return None;
            }
            Some(format!("{}{}", s.base_asset, s.quote_asset).to_uppercase())
        }

        if !exchange_symbols.is_empty() {
            let mut results: Vec<SearchResult> = Vec::new();

            for (exchange_id, symbols) in exchange_symbols {
                // Canonical lowercase slug — used for both storage and display.
                let exchange_slug = exchange_id.as_str();

                // Composite-query exchange filter.
                if let Some(part) = q_exchange {
                    if !exchange_matches(&exchange_slug.to_lowercase(), part) {
                        continue;
                    }
                }

                // Build uniqueness count: (account_type_label, canonical_form) → count.
                // Only symbols with a computable canonical form are counted.
                // Symbols whose canonical form appears more than once within this
                // (exchange, account_type) group are NOT canonicalized — the raw
                // venue-native string keeps them distinct.
                let mut canonical_counts: HashMap<(String, String), usize> = HashMap::new();
                for s in symbols.iter() {
                    if let Some(canon) = canonical_key(s) {
                        *canonical_counts
                            .entry((s.account_type.short_label().to_string(), canon))
                            .or_insert(0) += 1;
                    }
                }

                let filtered: Vec<&mlc_live_data::SymbolInfo> = symbols
                    .iter()
                    .filter(|s| {
                        // Composite-query account-type filter ("...:S" / "...:F").
                        if let Some(part) = q_at {
                            if !at_matches(s.account_type.short_label(), part) {
                                return false;
                            }
                        }
                        if q.is_empty() {
                            true
                        } else {
                            // Match against raw symbol, base asset, OR the canonical
                            // form so that e.g. "btcusdt" finds OKX "BTC-USDT".
                            let canon_match = canonical_key(s)
                                .map(|c| c.to_lowercase().contains(&q))
                                .unwrap_or(false);
                            s.symbol.to_lowercase().contains(&q)
                                || s.base_asset.to_lowercase().contains(&q)
                                || canon_match
                        }
                    })
                    .collect();

                for s in filtered.iter().take(100) {
                    // Decide the canonical symbol key for this search result.
                    // Use the canonical form only when it is unique within this
                    // (exchange, account_type) group; otherwise fall back to raw.
                    let symbol_key = canonical_key(s)
                        .filter(|c| {
                            canonical_counts
                                .get(&(s.account_type.short_label().to_string(), c.clone()))
                                .copied()
                                .unwrap_or(0)
                                == 1
                        })
                        .unwrap_or_else(|| s.symbol.clone());

                    let in_watchlist = watchlist_manager.contains_with_type(
                        &symbol_key,
                        exchange_slug,
                        s.account_type.short_label(),
                    );
                    let asset_type = if exchange_slug == "moex" { "Stock" } else { "Crypto" };
                    let category_icon = if exchange_slug == "moex" { "S" } else { "C" };
                    results.push(SearchResult {
                        symbol: symbol_key,
                        name: format!("{}/{}", s.base_asset, s.quote_asset),
                        exchange: exchange_slug.to_string(),
                        exchange_id: exchange_slug.to_string(),
                        asset_type: asset_type.to_string(),
                        category_icon: category_icon.to_string(),
                        in_watchlist,
                        account_type: s.account_type.short_label().to_string(),
                    });
                }
            }

            // Sort: query matches first by relevance, then by exchange.
            if !q.is_empty() {
                results.sort_by(|a, b| {
                    // Exact symbol match first.
                    let a_exact = a.symbol.to_lowercase() == q;
                    let b_exact = b.symbol.to_lowercase() == q;
                    b_exact.cmp(&a_exact)
                        .then_with(|| {
                            // USDT pairs before others.
                            let a_usdt = a.name.contains("/USDT");
                            let b_usdt = b.name.contains("/USDT");
                            b_usdt.cmp(&a_usdt)
                        })
                        .then(a.symbol.cmp(&b.symbol))
                });
            }

            results.truncate(200);
            results
        } else {
            // No symbols loaded yet — return empty list.
            Vec::new()
        }
    }


    /// Alias for `build_symbol_search_results` — kept for compatibility with callers
    /// in `input.rs` that still reference the old name.
    #[inline]
    pub(crate) fn build_demo_symbol_results(
        query: &str,
        watchlist_manager: &mlc_sidebar_content::watchlist::WatchlistManager,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<mlc_live_data::SymbolInfo>>,
    ) -> Vec<mlc_core::state::modal_state::SearchResult> {
        Self::build_symbol_search_results(query, watchlist_manager, exchange_symbols)
    }

    /// Apply `ChartOutputAction` results to the active `ChartContainer`.
    ///
    /// Mirrors `TerminalApp::process_output_actions` but simplified:
    /// no multi-window panel manager, no frame layout offset.
    pub(crate) fn process_output_actions(&mut self, actions: Vec<ChartOutputAction>) {
        fn calc_visible_price_range(window: &mut mlc_core::ChartContainer) {
            window.calc_auto_scale();
        }

        // Compute chart layout ONCE for coordinate conversion.
        // All actions from DefaultChartInputHandler use screen-absolute coords;
        // viewport/crosshair expect chart-local coords (0,0 = top-left of chart canvas).
        // Clone the frame-cached layout so the borrow ends here, allowing the mutable
        // &mut self calls (propagate_viewport_to_sync_group, etc.) that follow.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
        let chart_x = extended.main_chart.chart.x;
        let chart_y = extended.main_chart.chart.y;
        let _chart_w = extended.main_chart.chart.width;
        let chart_h = extended.main_chart.chart.height;

        let drag_mode = self.input_handler.state.drag_mode;

        for action in actions {
            match action {
                ChartOutputAction::Pan { bar_delta, price_delta } => {
                    // Block viewport panning when a click-based drawing tool is active.
                    // This ensures accidental micro-drags don't move the chart,
                    // keeping mouse release position close enough for click detection.
                    if self.has_click_drawing_tool() { continue; }
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    if window.viewport.is_time_axis() {
                        // Time-axis chart: pan in the time domain (bar_delta is a
                        // pixel delta here, same as the divisor below uses it).
                        window.viewport.pan_time(bar_delta);
                    } else {
                        let bar_delta_bars = bar_delta / window.viewport.bar_spacing;
                        window.viewport.pan(bar_delta_bars);
                    }
                    if let mlc_core::engine::input::DragMode::SubPaneChart { pane_index } = drag_mode {
                        let instance_id = mlc_core::state::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        let pane_height = self.active_frame_layout.as_ref()
                            .and_then(|ext| ext.sub_panes.get(pane_index))
                            .map(|sp| sp.content.height)
                            .unwrap_or(100.0);
                        if let Some(iid) = instance_id {
                            if let Some(s) = window.sub_pane_y_state.get_mut(&iid) {
                                if !s.auto_scale {
                                    let price_range = s.price_max - s.price_min;
                                    let price_delta_scaled = price_delta * price_range / pane_height;
                                    s.price_min += price_delta_scaled;
                                    s.price_max += price_delta_scaled;
                                }
                            }
                        }
                    } else if window.price_scale.scale_mode.is_auto_y() {
                        calc_visible_price_range(window);
                    } else {
                        let price_range = window.price_scale.price_max - window.price_scale.price_min;
                        let price_delta_scaled = price_delta * price_range / window.viewport.chart_height;
                        window.price_scale.price_min += price_delta_scaled;
                        window.price_scale.price_max += price_delta_scaled;
                    }
                    // Propagate viewport change to sync group.
                    let view_start = window.viewport.view_start;
                    let bar_spacing = window.viewport.bar_spacing;
                    // End the mutable borrow so we can call propagate_viewport_to_sync_group.
                    let _ = window;
                    let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf_opt {
                        self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, None);
                    }
                }
                ChartOutputAction::Zoom { center_x, factor_x, factor_y, .. } => {
                    // Convert screen-absolute center_x to chart-local
                    let local_center_x = center_x - chart_x;
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    if factor_x != 1.0 {
                        if window.viewport.is_time_axis() {
                            window.viewport.zoom_time_at(local_center_x, factor_x);
                        } else {
                            window.viewport.zoom_at(local_center_x, factor_x);
                        }
                        if window.price_scale.scale_mode.is_auto_y() {
                            calc_visible_price_range(window);
                        }
                    }
                    if factor_y != 1.0 {
                        window.price_scale.scale_mode = ScaleMode::Manual;
                        let center = (window.price_scale.price_min + window.price_scale.price_max) / 2.0;
                        let half_range = (window.price_scale.price_max - window.price_scale.price_min) / 2.0 * factor_y;
                        window.price_scale.price_min = center - half_range;
                        window.price_scale.price_max = center + half_range;
                    }
                    // Propagate viewport change to sync group.
                    let view_start = window.viewport.view_start;
                    let bar_spacing = window.viewport.bar_spacing;
                    // End the mutable borrow so we can call propagate_viewport_to_sync_group.
                    let _ = window;
                    if factor_x != 1.0 {
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, None);
                        }
                    }
                    if factor_y != 1.0 {
                        // Y-scale drag: mirror the A/M button click pattern — update
                        // group.scale_mode then propagate so sync-group peers switch to M.
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            let viewport_sync_on = self.panel_app.panel_grid
                                .container_id_for_leaf(active_leaf)
                                .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
                                .and_then(|gid| {
                                    self.managers.bubble.bubble_mut(gid).map(|g| {
                                        g.scale_mode = ScaleMode::Manual;
                                        g.sync_flags.sync_viewport
                                    })
                                })
                                .unwrap_or(false);
                            if viewport_sync_on {
                                self.propagate_viewport_to_sync_group(
                                    active_leaf,
                                    view_start,
                                    bar_spacing,
                                    Some(ScaleMode::Manual),
                                );
                            }
                        }
                    }
                }
                ChartOutputAction::ResetPriceScale => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    if let mlc_core::engine::input::DragMode::SubPanePriceScale { pane_index } = drag_mode {
                        let instance_id = mlc_core::state::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        if let Some(iid) = instance_id {
                            window.sub_pane_y_state.entry(iid).or_default().auto_scale = true;
                        }
                        // Restore main chart to Auto so the A/M button reflects the true state.
                        window.price_scale.scale_mode = ScaleMode::Auto;
                        let view_start = window.viewport.view_start;
                        let bar_spacing = window.viewport.bar_spacing;
                        let _ = window;
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            self.propagate_viewport_to_sync_group(
                                active_leaf,
                                view_start,
                                bar_spacing,
                                Some(ScaleMode::Auto),
                            );
                        }
                    } else {
                        window.price_scale.scale_mode = ScaleMode::Auto;
                        calc_visible_price_range(window);
                    }
                }
                ChartOutputAction::ResetTimeScale => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    window.viewport.reset_to_default();
                    window.price_scale.scale_mode = ScaleMode::Auto;
                    calc_visible_price_range(window);
                }
                ChartOutputAction::TogglePriceScaleMode => {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.price_scale.toggle_mode();
                    }
                }
                ChartOutputAction::UpdateCrosshair { .. } => {
                    // Crosshair updates are handled by update_crosshair_from_global
                    // (called in on_mouse_move / on_drag_move), which is
                    // sub-pane-aware and uses the correct coordinate space.
                    // Applying main-chart-only coords here would conflict with
                    // that logic and produce wrong crosshair positions in sub-panes.
                }
                ChartOutputAction::HideCrosshair => {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.crosshair.visible = false;
                    }
                }

                // ── Undo recording ─────────────────────────────────────────
                ChartOutputAction::RecordUndo(undo_action) => {
                    let cmd = match undo_action {
                        UndoAction::ViewportChange {
                            old_view_start,
                            old_bar_spacing,
                            old_price_min,
                            old_price_max,
                            new_view_start,
                            new_bar_spacing,
                            new_price_min,
                            new_price_max,
                        } => {
                            // Only push if the viewport actually changed.
                            let changed = (new_view_start - old_view_start).abs() > 0.001
                                || (new_bar_spacing - old_bar_spacing).abs() > 0.001
                                || (new_price_min - old_price_min).abs() > 0.001
                                || (new_price_max - old_price_max).abs() > 0.001;
                            if changed {
                                Some(Command::ViewportChange {
                                    previous: ViewportState::new(
                                        old_view_start,
                                        old_bar_spacing,
                                        old_price_min,
                                        old_price_max,
                                    ),
                                    new: ViewportState::new(
                                        new_view_start,
                                        new_bar_spacing,
                                        new_price_min,
                                        new_price_max,
                                    ),
                                })
                            } else {
                                None
                            }
                        }
                        UndoAction::PrimitiveMoved { index, old_points, new_points } => {
                            self.autosave_snapshot();
                            Some(Command::MovePrimitive {
                                index,
                                previous_points: old_points,
                                new_points,
                            })
                        }
                        UndoAction::PrimitiveCreated { index } => {
                            // For creation we need full primitive data from the drawing manager.
                            // Extract it before mutably borrowing the window.
                            let active_pid_oa = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                            let snapshot = active_pid_oa
                                .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                                .and_then(|panel| {
                                    let type_id = panel.drawing_manager.get_type_id_at(index)?;
                                    let points = panel.drawing_manager.get_points_at(index)?;
                                    let data = panel.drawing_manager.get_data_at(index)?;
                                    Some((type_id, points, data))
                                });
                            if let Some((type_id, points, data)) = snapshot {
                                self.autosave_snapshot();
                                Some(Command::CreatePrimitive { index, type_id, points, data })
                            } else {
                                eprintln!("[Undo] PrimitiveCreated: could not read primitive at index {}", index);
                                None
                            }
                        }
                        UndoAction::PrimitiveDeleted { index, data: _data } => {
                            // Deletion undo is handled externally when the delete action fires.
                            // We log it here rather than silently drop it.
                            dlog!("[Undo] PrimitiveDeleted at index {} — deletion undo handled at call site", index);
                            None
                        }
                    };
                    if let Some(command) = cmd {
                        self.push_undo_command(command);
                    }
                }

                // ── Drawing interaction ─────────────────────────────────────
                ChartOutputAction::SelectPrimitive { id } => {
                    let active_pid_sp = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = active_pid_sp {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            match id {
                                Some(primitive_id) => {
                                    if let Some(idx) = panel.drawing_manager.find_index_by_id(primitive_id) {
                                        panel.drawing_manager.select_by_index(idx);
                                    }
                                }
                                None => {
                                    panel.drawing_manager.deselect();
                                }
                            }
                        }
                    }
                }
                ChartOutputAction::StartPrimitiveDrag { id, bar: screen_x, price: screen_y } => {
                    let active_pid_pd = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // `bar` and `price` from DefaultChartInputHandler are raw screen (x, y).
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    let primitive_pane_id = active_pid_pd
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| {
                            let idx = panel.drawing_manager.find_index_by_id(id)?;
                            panel.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
                        })
                        .flatten();

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    let data_price = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport.x_to_bar(local_x);
                                let (snapped, _) = w.calculate_magnet_snap(
                                    bar_idx, data_price, chart_h,
                                    w.price_scale.price_min, w.price_scale.price_max,
                                );
                                snapped
                            } else {
                                data_price
                            }
                        } else {
                            data_price
                        }
                    } else {
                        data_price
                    };

                    // Capture the primitive's points BEFORE drag.
                    let pre_drag = active_pid_pd
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| {
                            let idx = panel.drawing_manager.find_index_by_id(id)?;
                            let pts = panel.drawing_manager.get_points_at(idx)?;
                            Some((idx, pts))
                        });
                    self.drag_start_points = pre_drag;

                    if let Some(window) = self.panel_app.panel_grid.active_container() {
                        if let Some(pid) = active_pid_pd {
                            if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                if let Some(idx) = panel.drawing_manager.find_index_by_id(id) {
                                    let drag_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&window.bars, data_bar);
                                    panel.drawing_manager.start_drag(idx, drag_ts_ms, data_price);
                                }
                            }
                        }
                    }
                }
                ChartOutputAction::StartControlPointDrag { primitive_id, control_point, bar: screen_x, price: screen_y } => {
                    let active_pid_cpd = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    let primitive_pane_id = active_pid_cpd
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| {
                            let idx = panel.drawing_manager.find_index_by_id(primitive_id)?;
                            panel.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
                        })
                        .flatten();

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    // Call calculate_magnet_snap() directly, like the terminal does.
                    let data_price = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport.x_to_bar(local_x);
                                let (snapped, _) = w.calculate_magnet_snap(
                                    bar_idx, data_price, chart_h,
                                    w.price_scale.price_min, w.price_scale.price_max,
                                );
                                snapped
                            } else {
                                data_price
                            }
                        } else {
                            data_price
                        }
                    } else {
                        data_price
                    };

                    // Capture points before control-point reshape too.
                    let pre_drag = active_pid_cpd
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| {
                            let idx = panel.drawing_manager.find_index_by_id(primitive_id)?;
                            let pts = panel.drawing_manager.get_points_at(idx)?;
                            Some((idx, pts))
                        });
                    self.drag_start_points = pre_drag;

                    // Use start_control_point_drag (not start_drag) so DragType is ControlPoint.
                    if let Some(window) = self.panel_app.panel_grid.active_container() {
                        if let Some(pid) = active_pid_cpd {
                            if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                if let Some(idx) = panel.drawing_manager.find_index_by_id(primitive_id) {
                                    let cp_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&window.bars, data_bar);
                                    panel.drawing_manager.start_control_point_drag(idx, control_point, cp_ts_ms, data_price);
                                }
                            }
                        }
                    }
                }
                ChartOutputAction::UpdatePrimitiveDrag { bar: screen_x, price: screen_y } => {
                    let active_pid_ud = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // Use the dragging primitive's pane_id for coordinate conversion.
                    let primitive_pane_id = active_pid_ud
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| {
                            let idx = panel.drawing_manager.dragging()?;
                            panel.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
                        })
                        .flatten();

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    // Call calculate_magnet_snap() directly on every drag update, like the terminal does.
                    let data_price = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport.x_to_bar(local_x);
                                let (snapped, _) = w.calculate_magnet_snap(
                                    bar_idx, data_price, chart_h,
                                    w.price_scale.price_min, w.price_scale.price_max,
                                );
                                snapped
                            } else {
                                data_price
                            }
                        } else {
                            data_price
                        }
                    } else {
                        data_price
                    };

                    // Get the dragged primitive id BEFORE update_drag (id doesn't change).
                    let dragged_id = active_pid_ud
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| panel.drawing_manager.dragging_primitive_id());

                    if let Some(window) = self.panel_app.panel_grid.active_container() {
                        if let Some(pid) = active_pid_ud {
                            if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                let drag_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&window.bars, data_bar);
                                panel.drawing_manager.update_drag(drag_ts_ms, data_price);
                            }
                        }
                    }
                    // Propagate live drag position to sync-group peer leaves.
                    if let Some(prim_id) = dragged_id {
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            // TagManager path: update group in real-time during drag.
                            let group_updated = self.update_group_primitive_after_drag(active_leaf, prim_id);
                            if !group_updated {
                                self.propagate_primitive_update_to_sync_group(active_leaf, prim_id);
                            }
                        }
                    }
                }
                ChartOutputAction::EndPrimitiveDrag => {
                    let active_pid_ed = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // Snapshot the dragged primitive's id BEFORE end_drag() clears it.
                    let dragged_prim_id = active_pid_ed
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|panel| panel.drawing_manager.dragging_primitive_id());

                    // Collect move data BEFORE ending the drag.
                    let move_cmd = if let Some((idx, ref previous_points)) = self.drag_start_points {
                        active_pid_ed
                            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                            .and_then(|panel| panel.drawing_manager.get_points_at(idx))
                            .and_then(|new_points| {
                                if new_points != *previous_points {
                                    Some(Command::MovePrimitive {
                                        index: idx,
                                        previous_points: previous_points.clone(),
                                        new_points,
                                    })
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    };
                    self.drag_start_points = None;

                    if let Some(pid) = active_pid_ed {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.end_drag();
                        }
                    }
                    if let Some(cmd) = move_cmd {
                        self.push_undo_command(cmd);
                    }
                    // Propagate final primitive position to sync-group peer leaves.
                    if let Some(prim_id) = dragged_prim_id {
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            // TagManager path: write updated primitive back to the group.
                            let group_updated = self.update_group_primitive_after_drag(active_leaf, prim_id);
                            if !group_updated {
                                // Legacy clone-based sync fallback.
                                self.propagate_primitive_update_to_sync_group(active_leaf, prim_id);
                            }
                        }
                        self.autosave_snapshot();
                    }
                }
                ChartOutputAction::FinishMultipointDrawing => {
                    let active_pid_fm = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // Record CreatePrimitive if finish_multipoint creates a new primitive.
                    let prev_count = active_pid_fm
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|panel| panel.drawing_manager.count())
                        .unwrap_or(0);

                    if let Some(pid) = active_pid_fm {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.finish_multipoint();
                        }
                    }

                    let new_count = active_pid_fm
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|panel| panel.drawing_manager.count())
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let snapshot = active_pid_fm
                            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                            .and_then(|panel| {
                                let idx = panel.drawing_manager.last_index()?;
                                let type_id = panel.drawing_manager.get_type_id_at(idx)?;
                                let points = panel.drawing_manager.get_points_at(idx)?;
                                let data = panel.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = snapshot {
                            self.push_undo_command(Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            self.autosave_snapshot();
                        }
                    }
                }
                ChartOutputAction::DrawingClick { bar, price, .. } => {
                    let active_pid_dc = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    // Record CreatePrimitive if the click completes a new primitive.
                    let prev_count = active_pid_dc
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|panel| panel.drawing_manager.count())
                        .unwrap_or(0);

                    let click_ts_ms = self.panel_app.panel_grid.active_container()
                        .map(|w| mlc_core::bar_f64_to_timestamp_ms(&w.bars, bar))
                        .unwrap_or(0);
                    if let Some(pid) = active_pid_dc {
                        if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                            panel.drawing_manager.on_click(click_ts_ms, price);
                        }
                    }

                    let new_count = active_pid_dc
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|panel| panel.drawing_manager.count())
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let snapshot = active_pid_dc
                            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                            .and_then(|panel| {
                                let idx = panel.drawing_manager.last_index()?;
                                let type_id = panel.drawing_manager.get_type_id_at(idx)?;
                                let points = panel.drawing_manager.get_points_at(idx)?;
                                let data = panel.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = snapshot {
                            self.push_undo_command(Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            self.autosave_snapshot();
                        }
                    }
                }

                ChartOutputAction::ZoomSubPane { pane_index, delta_y } => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    {
                        let instance_id = mlc_core::state::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        let pane_h = self.active_frame_layout.as_ref()
                            .and_then(|ext| ext.sub_panes.get(pane_index))
                            .map(|sp| sp.content.height)
                            .unwrap_or(100.0);
                        if let Some(iid) = instance_id {
                            let entry = window.sub_pane_y_state.entry(iid).or_default();
                            entry.auto_scale = false;
                            if pane_h > 0.0 {
                                let factor = 1.0 + delta_y / pane_h;
                                let center = (entry.price_min + entry.price_max) / 2.0;
                                let half_range = (entry.price_max - entry.price_min) / 2.0 * factor;
                                entry.price_min = center - half_range;
                                entry.price_max = center + half_range;
                            }
                        }
                    }
                    // Dragging a sub-pane price scale switches the whole window to
                    // Manual mode so the A/M button reflects the true state.
                    window.price_scale.scale_mode = ScaleMode::Manual;
                    let view_start = window.viewport.view_start;
                    let bar_spacing = window.viewport.bar_spacing;
                    let _ = window;
                    let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf_opt {
                        self.propagate_viewport_to_sync_group(
                            active_leaf,
                            view_start,
                            bar_spacing,
                            Some(ScaleMode::Manual),
                        );
                    }
                }

                // ── Tooltip / cursor / kinetic — not applicable in chart-app ──
                ChartOutputAction::ShowTooltip { .. }
                | ChartOutputAction::HideTooltip
                | ChartOutputAction::UpdateCursor(_)
                | ChartOutputAction::OpenContextMenu { .. }
                | ChartOutputAction::StartKinetic { .. }
                | ChartOutputAction::StopKinetic
                | ChartOutputAction::Repaint
                | ChartOutputAction::None => {
                    // These are either handled elsewhere or not applicable in the
                    // standalone chart-app context.
                }
            }
        }
    }

    /// Propagate viewport state to all leaves in the same sync color group.
    ///
    /// Propagate updated primitive points to sync-group peer leaves.
    ///
    /// Called after a drag move (`UpdatePrimitiveDrag`) and after drag end
    /// After a drag ends on a grouped window, replace the corresponding
    /// primitive in the TagManager group with the updated version from the
    /// window's drawing_manager.  Returns `true` if the window was grouped
    /// and the group was updated.
    pub(crate) fn update_group_primitive_after_drag(
        &mut self,
        source_leaf: mlc_core::LeafId,
        primitive_id: u64,
    ) -> bool {
        let group_id = self.panel_app.panel_grid
            .container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
        let group_id = match group_id {
            Some(gid) => gid,
            None => return false,
        };

        // Respect the sync_drawings flag — if disabled, do not write drag result back to group.
        if let Some(group) = self.managers.bubble.bubble(group_id) {
            if !group.sync_flags.sync_drawings {
                return false;
            }
        }

        // Clone the updated primitive from the window's drawing_manager.
        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return false,
        };
        let updated_prim = self.panel_app.panel_grid
            .containers()
            .get(&chart_id)
            .and_then(|w| self.panels_store.chart_panel.get(&w.chart_panel_id))
            .and_then(|panel| {
                panel.drawing_manager.primitives().iter()
                    .find(|p| p.data().id == primitive_id)
                    .map(|p| p.clone_box())
            });
        let updated_prim = match updated_prim {
            Some(p) => p,
            None => return false,
        };

        // Replace the primitive in the group by id.
        if let Some(group) = self.managers.bubble.bubble_mut(group_id) {
            if let Some(pos) = group.primitives.iter().position(|p| p.data().id == primitive_id) {
                group.primitives[pos] = updated_prim;
                return true;
            }
        }
        false
    }

    /// DEPRECATED: Legacy clone-based drag propagation for non-grouped windows.
    /// For grouped windows, `update_group_primitive_after_drag` handles this via TagManager.
    /// Used as fallback when `update_group_primitive_after_drag` returns false.
    pub(crate) fn propagate_primitive_update_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        primitive_id: u64,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<mlc_core::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && input::sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Phase 1: read the current points from the source window (immutable borrow).
        let source_chart_id = match self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return,
        };
        let src_pid_pp = self.panel_app.panel_grid.containers().get(&source_chart_id).map(|w| w.chart_panel_id);
        let new_points: Vec<(i64, f64)> = match src_pid_pp
            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .and_then(|panel| panel.drawing_manager.get_points_by_id(primitive_id))
        {
            Some(pts) => pts,
            None => return,
        };

        // Phase 2: update each peer window's synced clone (mutable borrows, one at a time).
        for peer_leaf in peer_leaves {
            let peer_chart_id = match self.panel_app.panel_grid.container_id_for_leaf(peer_leaf) {
                Some(id) => id,
                None => continue,
            };
            if let Some(peer_pid) = self.panel_app.panel_grid.containers().get(&peer_chart_id).map(|w| w.chart_panel_id) {
                if let Some(peer_panel) = self.panels_store.chart_panel.get_mut(&peer_pid) {
                    peer_panel.drawing_manager.update_synced_primitive_points(primitive_id, &new_points);
                }
            }
        }
    }

    /// `source_leaf` is the leaf whose viewport was just changed.
    /// All other leaves sharing the same color tag receive the same
    /// visible bar count, time-aligned `view_start`, and optionally `scale_mode`.
    ///
    /// Instead of copying raw `bar_spacing`, we sync the NUMBER of visible bars.
    /// Each peer recalculates its own `bar_spacing = peer.chart_width / visible_bars`
    /// so that a small 85px window and a large 271px window both show the same
    /// number of candles (just at different pixel densities).
    pub(crate) fn propagate_viewport_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        view_start: f64,
        bar_spacing: f64,
        scale_mode: Option<mlc_core::ScaleMode>,
    ) {
        let should_sync = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid))
            .and_then(|gid| self.managers.bubble.bubble(gid))
            .map(|g| g.sync_flags.sync_viewport)
            .unwrap_or(true);
        if !should_sync { return; }

        // Compute source visible bar count — the metric we actually sync.
        let (source_chart_width, anchor_ts, frac) = match self.panel_app.panel_grid.container_for_leaf(source_leaf) {
            Some(w) => {
                let ts = if w.bars.is_empty() {
                    None
                } else {
                    let last_idx = w.bars.len() - 1;
                    let fidx = view_start.floor();
                    if fidx > last_idx as f64 {
                        // Dragged past the last bar: extrapolate the anchor
                        // timestamp by the bar interval so sync peers follow
                        // into the future (find_bar_for_timestamp extrapolates
                        // right) instead of pinning at their last bar.
                        let interval = mlc_core::bar_interval_seconds(&w.bars);
                        Some(w.bars[last_idx].timestamp
                            + (fidx as i64 - last_idx as i64) * interval)
                    } else {
                        let idx = (fidx.max(0.0) as usize).min(last_idx);
                        Some(w.bars[idx].timestamp)
                    }
                };
                (w.viewport.chart_width, ts, view_start - view_start.floor())
            }
            None => return,
        };
        let visible_bars = if bar_spacing > 0.0 { source_chart_width / bar_spacing } else { 100.0 };

        // Enumerate peers via TagManager group membership so untagged groups propagate too.
        let source_chart_id_vp = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_vp = source_chart_id_vp
            .and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_vp {
            self.managers.bubble
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect()
        } else {
            match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(source_color) => self.panel_app.leaf_color_tags.iter()
                    .filter(|(&lid, &c)| lid != source_leaf && input::sync_colors_match(c, source_color))
                    .map(|(&lid, _)| lid)
                    .collect(),
                None => return,
            }
        };
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                // Apply scale_mode BEFORE the auto-scale check so the new mode takes effect.
                if let Some(mode) = scale_mode {
                    window.price_scale.scale_mode = mode;
                    let is_auto = mode.is_auto_y();
                    for state in window.sub_pane_y_state.values_mut() {
                        state.auto_scale = is_auto;
                    }
                }
                // Time-based sync: convert source timestamp to peer's bar index so
                // windows with different bar counts land on the same calendar date.
                let peer_view_start = anchor_ts
                    .and_then(|ts| mlc_core::find_bar_for_timestamp(&window.bars, ts))
                    .map(|idx| idx as f64 + frac)
                    .unwrap_or(view_start);
                // Sync visible bar count: peer gets the same number of bars in view,
                // but its own bar_spacing adapts to its chart_width.
                let peer_spacing = if visible_bars > 0.0 && window.viewport.chart_width > 0.0 {
                    (window.viewport.chart_width / visible_bars).clamp(
                        window.viewport.min_bar_spacing(),
                        window.viewport.max_bar_spacing(),
                    )
                } else {
                    bar_spacing
                };
                // Only write + rescale when the peer's viewport actually moves.
                // An unconditional per-frame write made calc_auto_scale here race
                // the per-tick TradeUpdate autoscale on the same peer: the two
                // paths compute slightly different price ranges, the last-price
                // projection lands alternately inside/outside the range, and the
                // line + scale chip blink (owner report: right window of a
                // 2-chart sync group ripples).
                let moved = (window.viewport.view_start - peer_view_start).abs() > 1e-9
                    || (window.viewport.bar_spacing - peer_spacing).abs() > 1e-9;
                if moved {
                    window.viewport.view_start = peer_view_start;
                    window.viewport.bar_spacing = peer_spacing;
                    if window.price_scale.scale_mode.is_auto_y() {
                        window.calc_auto_scale();
                    }
                } else if scale_mode.is_some() && window.price_scale.scale_mode.is_auto_y() {
                    // Mode flip without viewport movement still needs one rescale.
                    window.calc_auto_scale();
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // User profile persistence
    // -------------------------------------------------------------------------

    /// Map a [`mlc_sidebar_content::state::RightSidebarPanel`] to a string name.
    pub fn panel_to_str(panel: mlc_sidebar_content::state::RightSidebarPanel) -> Option<String> {
        use mlc_sidebar_content::state::RightSidebarPanel;
        match panel {
            RightSidebarPanel::None => None,
            RightSidebarPanel::Watchlist => Some("watchlist".to_string()),
            RightSidebarPanel::Alerts => Some("alerts".to_string()),
            RightSidebarPanel::ObjectTree => Some("object_tree".to_string()),
            RightSidebarPanel::Signals => Some("signals".to_string()),
            RightSidebarPanel::Connectors => Some("connectors".to_string()),
            RightSidebarPanel::Performance => Some("performance".to_string()),
            RightSidebarPanel::Agents => Some("agents".to_string()),
            RightSidebarPanel::Slot1 => Some("slot1".to_string()),
            RightSidebarPanel::Slot2 => Some("slot2".to_string()),
            RightSidebarPanel::Slot3 => Some("slot3".to_string()),
            RightSidebarPanel::Slot4 => Some("slot4".to_string()),
        }
    }

    /// Parse a string name into a [`mlc_sidebar_content::state::RightSidebarPanel`].
    pub fn str_to_panel(s: &str) -> mlc_sidebar_content::state::RightSidebarPanel {
        use mlc_sidebar_content::state::RightSidebarPanel;
        match s {
            "watchlist" => RightSidebarPanel::Watchlist,
            "alerts" => RightSidebarPanel::Alerts,
            "object_tree" => RightSidebarPanel::ObjectTree,
            "signals" => RightSidebarPanel::Signals,
            "connectors" => RightSidebarPanel::Connectors,
            "performance" => RightSidebarPanel::Performance,
            "agents" => RightSidebarPanel::Agents,
            "slot1" => RightSidebarPanel::Slot1,
            "slot2" => RightSidebarPanel::Slot2,
            "slot3" => RightSidebarPanel::Slot3,
            "slot4" => RightSidebarPanel::Slot4,
            _ => RightSidebarPanel::None,
        }
    }

    /// Build indicator display name with ALL numeric params in definition order.
    ///
    /// Examples: "SMA(20)", "RSI(14)", "MACD(26, 12, 9)", "BB(20, 2)"
    fn format_indicator_display_name(
        mgr: &mlc_indicators::IndicatorManager,
        inst: &mlc_indicators::IndicatorMgrInstance,
    ) -> String {
        use mlc_indicators::{IndicatorParamType, IndicatorParamValue};

        if let Some(def) = mgr.get_definition(&inst.type_id) {
            // Collect numeric param values in definition order
            let numeric_vals: Vec<String> = def.params.iter()
                .filter(|p| matches!(p.param_type, IndicatorParamType::Int { .. } | IndicatorParamType::Float { .. }))
                .filter_map(|p| {
                    inst.params.get(&p.name).map(|v| match v {
                        IndicatorParamValue::Int(i) => i.to_string(),
                        IndicatorParamValue::Float(f) => {
                            if f.fract() == 0.0 { format!("{:.0}", f) } else { format!("{}", f) }
                        }
                        _ => String::new(),
                    })
                })
                .filter(|s| !s.is_empty())
                .collect();

            if numeric_vals.is_empty() {
                def.short_name.clone()
            } else {
                format!("{}({})", def.short_name, numeric_vals.join(", "))
            }
        } else {
            inst.name.clone()
        }
    }

    /// Build a [`ChartSettingsData`] snapshot from the current chart state.
    ///
    /// This is the same logic used during rendering so that templates saved via
    /// the "Save As…" button capture exactly what the render pass would show.
    pub fn build_chart_settings_data(&self) -> mlc_layout::layout::modals::chart_settings::ChartSettingsData {
        use mlc_layout::layout::modals::chart_settings::{
            ChartSettingsData, InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
        };

        let rt = self.panel_app.theme_manager.current();
        let series = &rt.series;

        // Phase 4 C: collect window-only fields first, then panel fields separately.
        let settings_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
        let (auto_scale,
             price_scale_width, time_scale_height,
             time_fmt_use_24h, time_fmt_show_dow, time_fmt_show_utc, tz_label, date_fmt_label, precision_lbl) =
            self.panel_app
            .panel_grid
            .active_container()
            .map(|w| {
                let tf = &w.scale_settings.time_format;
                (
                    w.price_scale.scale_mode.is_auto_y(),
                    w.scale_settings.price_scale_width,
                    w.scale_settings.time_scale_height,
                    tf.use_24h,
                    tf.show_day_of_week,
                    tf.show_utc_prefix,
                    tf.timezone_label(),
                    tf.date_format.label().to_string(),
                    mlc_core::scale_settings::precision_label(
                        w.scale_settings.user_precision,
                    ).to_string(),
                )
            })
            .unwrap_or_else(|| (
                true, 70.0, 30.0,
                true, false, true,
                "(UTC+0) Лондон".to_string(),
                "21.01.2026".to_string(),
                "Авто".to_string(),
            ));
        // Panel-owned display fields.
        let settings_default_panel = settings_pid.map(mlc_core::ChartPanelState::new);
        let settings_panel = settings_pid
            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .or(settings_default_panel.as_ref());
        let (vert_lines, horz_lines, legend_show_ohlc, legend_show_change, legend_show_percent,
             tooltip_visible, tooltip_follow_cursor) = settings_panel
            .map(|p| (
                p.grid_options.vert_lines.visible,
                p.grid_options.horz_lines.visible,
                p.legend.show_ohlc,
                p.legend.show_change,
                p.legend.show_percent,
                p.tooltip.visible,
                p.tooltip.follow_cursor,
            ))
            .unwrap_or((true, true, true, true, true, false, false));

        let css = &self.panel_app.chart_settings_state;

        ChartSettingsData {
            instrument: InstrumentSettings {
                use_prev_close_color: css.instrument_use_prev_close,
                body_enabled:   css.instrument_body_enabled,
                body_up_color:  series.candle_up_body.clone(),
                body_down_color: series.candle_down_body.clone(),
                border_enabled: css.instrument_border_enabled,
                border_up_color: series.candle_up_body.clone(),
                border_down_color: series.candle_down_body.clone(),
                wick_enabled:   css.instrument_wick_enabled,
                wick_up_color:  series.candle_up_wick.clone(),
                wick_down_color: series.candle_down_wick.clone(),
                precision_label: precision_lbl.clone(),
                timezone_label: tz_label.clone(),
                use_24h: time_fmt_use_24h,
                show_utc_prefix: time_fmt_show_utc,
                date_format_label: date_fmt_label.clone(),
                show_day_of_week: time_fmt_show_dow,
                show_bar_countdown: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.show_bar_countdown)
                    .unwrap_or(true),
                price_tick_style: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_style.clone())
                    .unwrap_or_else(|| "dotted".to_string()),
                price_tick_extend_right: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_extend_right)
                    .unwrap_or(true),
                price_tick_extend_left: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_extend_left)
                    .unwrap_or(true),
            },
            status_line: StatusLineSettings {
                legend_show_ohlc,
                legend_show_change,
                legend_show_percent,
                tooltip_visible,
                tooltip_follow_cursor,
                ..Default::default()
            },
            scales: ScalesLinesSettings {
                show_grid: vert_lines || horz_lines,
                vert_lines,
                horz_lines,
                auto_scale,
                price_scale_right: true,
                time_scale_bottom: true,
                price_scale_width,
                time_scale_height,
                crosshair_mode: "Normal".to_string(),
                crosshair_line_style: "Dashed".to_string(),
                crosshair_line_width: 1.0,
                crosshair_line_color: rt.chart.crosshair_line.clone(),
                price_scale_position: "right".to_string(),
                time_scale_position: "bottom".to_string(),
                corner_visibility: "on_hover".to_string(),
                date_format: "day_month_year".to_string(),
                use_24h: time_fmt_use_24h,
                show_day_of_week: time_fmt_show_dow,
                timezone_label: tz_label,
                ..Default::default()
            },
        }
    }

    /// Capture the current chart settings into the app-level snapshot store.
    ///
    /// Pushes a [`SnapshotAction::ChartSettings`] that is drained by App each
    /// frame and applied to the shared `AppState.snapshots`.
    pub fn snapshot_chart_settings_to_user_manager(&mut self) {
        let data = self.build_chart_settings_data();
        if let Ok(val) = serde_json::to_value(&data) {
            self.snapshot_actions.push(SnapshotAction::ChartSettings(val));
        }
    }

    /// Capture primitive settings for the active primitive into the app-level
    /// snapshot store, keyed by the primitive's `type_id`.
    ///
    /// `idx` is the index of the primitive in the active window's drawing manager.
    pub fn snapshot_primitive_settings_to_user_manager(&mut self, idx: usize) {
        let active_pid_snap = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
        if let Some(data) = active_pid_snap
            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .and_then(|panel| panel.drawing_manager.get_data_at(idx))
        {
            let type_id = data.type_id.clone();
            if let Ok(val) = serde_json::to_value(&data) {
                self.snapshot_actions.push(SnapshotAction::PrimitiveSettings {
                    type_id: type_id.clone(),
                    data: val,
                });
            }
            // Remember this style (including extended style_properties) so the next
            // primitive of the same type is pre-populated with these settings.
            if let Some(pid) = active_pid_snap {
                if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                    panel.drawing_manager.save_last_style_at_index(idx);
                    // Persist the last-used style globally so it survives app restarts.
                    if let Some(style) = panel.drawing_manager.last_style_for(&type_id) {
                        if let Ok(style_val) = serde_json::to_value(&style) {
                            self.snapshot_actions.push(SnapshotAction::DrawingStyle {
                                type_id,
                                data: style_val,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Capture indicator settings for the currently-open indicator into the
    /// app-level snapshot store, keyed by the indicator's `type_id`.
    pub fn snapshot_indicator_settings_to_user_manager(&mut self) {
        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
            if let Some(inst) = self.indicator_manager.get_instance(ind_id) {
                let type_id = inst.type_id.to_string();
                if let Ok(val) = serde_json::to_value(inst) {
                    self.snapshot_actions.push(SnapshotAction::IndicatorSettings { type_id, data: val });
                }
            }
        }
    }

    /// Capture compare series settings for the currently-open compare modal into
    /// the app-level snapshot store.
    pub fn snapshot_compare_settings_to_user_manager(&mut self) {
        let idx = self.panel_app.compare_settings_state.series_index;
        if let Some(series) = self.panel_app.panel_grid.active_container()
            .and_then(|w| w.compare_overlay.series.get(idx))
        {
            if let Ok(val) = serde_json::to_value(series) {
                self.snapshot_actions.push(SnapshotAction::CompareSettings(val));
            }
        }
    }
}

impl Default for ChartApp {
    fn default() -> Self {
        Self::new("Chart")
    }
}
