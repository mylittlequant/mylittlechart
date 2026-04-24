//! chart-app — standalone chart application crate
//!
//! A minimal, self-contained chart application that owns one `ChartPanelApp`
//! (which itself holds one `ChartWindow` via `ChartPanelGrid`) plus an
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
pub mod input;
pub mod panels_render;
pub mod panels_store;
pub mod preset_cache;
pub mod scroll_dispatch;
pub mod text_input;
use uzor::input::TextFieldConfig;
use uzor::WidgetId;
pub mod workspace;

pub use panels_store::TradingPanelsStore;

pub use input::KeyPress;
pub use digdigdig3::ExchangeId;
pub use sidebar_content::watchlist::{WatchlistManager, WatchlistSymbol};
pub use zengeld_terminal_indicators::RecalcMode;
pub use zengeld_chart::{Language, set_language};

use std::cell::RefCell;

use zengeld_chart::{
    ChartPanelRenderData, render_full_chart_panel,
    ChartModalLayout, ChartModalRenderResult,
    ChartPanelLayout, ToolbarConfig,
    LayoutRect,
    ExtendedFrameLayout,
    ScaleCornerHitZones,
    layout::ToolbarState,
    render::RenderContext,
    layout::ChartRenderConfig,
    DefaultChartInputHandler, ChartOutputAction, UndoAction,
    Command, ViewportState,
    data_provider::SharedDataProvider,
    ContextMenuResult,
    ScaleMode,
    ChartId,
};
use zengeld_chart::layout::modals::context_menu::render_context_menu;
use zengeld_chart::layout::modals::indicator_overlay::render_indicator_overlay;
use zengeld_chart::layout::modals::search_overlay::render_search_overlay;
use zengeld_chart::ModalSearchResult;
use zengeld_chart::layout::render_ui::IndicatorOverlayInfo;
use zengeld_chart::indicator_source::{IndicatorSource, AlertRenderData, AlertRenderStatus};
use zengeld_chart::ui::modal_state::{ModalState, OpenModal, IndicatorCatalogItem};
use zengeld_chart::ui::modal_settings::{ChartScreenArea, WatchlistModalState, WatchlistGroupNameInputState};
use zengeld_chart::layout::modals::watchlist_modal::{render_watchlist_modal, WatchlistEntry, WatchlistGroupInfo, render_wl_group_name_input, WlGroupNameInputResult};
use zengeld_terminal_indicators::IndicatorManager;
use live_data::{DataBridge, LiveUpdate, LiveDataProvider};

use zengeld_chart::panel_app::ChartPanelApp;
use uzor::input::{InputCoordinator, InputState};

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
fn parse_timeframe_name(name: &str) -> Option<zengeld_chart::state::Timeframe> {
    use zengeld_chart::state::Timeframe;
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
/// Owns a single `ChartPanelApp` (which holds exactly one `ChartWindow` and
/// its own modal state structs), an `InputCoordinator` for widget routing,
/// and a `DefaultChartInputHandler` for chart canvas input.
///
/// There is no sync bridge — with only one window the state flows directly.
pub struct ChartApp {
    /// The chart panel — owns ChartWindow, toolbar state, split grid, and modal state.
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
    pub(crate) scale_corner_zones: ScaleCornerHitZones,

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
    drag_start_points: Option<(usize, Vec<(f64, f64)>)>,

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
    pub(crate) leaf_tab_hit_zones: std::collections::HashMap<zengeld_chart::LeafId, zengeld_chart::LeafTabHitZones>,

    /// Currently hovered overlay-tab zone (shared across all tabs — only one can be hovered at a time).
    pub(crate) leaf_tab_hover: zengeld_chart::LeafTabHoverZone,

    /// LeafId of the tab currently being hovered (if any).
    pub(crate) leaf_tab_hovered_leaf: Option<zengeld_chart::LeafId>,

    /// Toolbar render result from the last frame — used to read chevron rects and
    /// max_scroll values when dispatching chevron clicks.
    pub(crate) last_toolbar_result: Option<zengeld_chart::ChartToolbarRenderResult>,

    /// User's preferred default scale mode, synced from profile settings.
    /// Applied to windows on initial bar load.
    pub default_scale_mode: ScaleMode,

    /// Alert manager — owns all alert items, crossing detection, and ID generation.
    alert_manager: alerts::AlertManager,

    /// Triggered alert events waiting for delivery (Telegram, toast, etc.)
    pub pending_delivery_events: Vec<alert_delivery::DeliveryEvent>,

    /// Whether an alert-triggered screenshot capture is needed.
    /// Set by tick() when an alert fires. Cleared by the renderer after capture.
    pub pending_alert_screenshot: bool,

    /// Sidebar panel state — tracks which right-side panel is open and its content.
    pub sidebar_state: sidebar_content::state::SidebarState,

    /// Right sidebar render result from the last frame (hit zones for click dispatch).
    pub last_sidebar_result: Option<sidebar_content::render::RightSidebarResult>,

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
    pub(crate) last_watchlist_modal_result: Option<zengeld_chart::layout::modals::watchlist_modal::WatchlistModalResult>,

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

    /// Shared trade ring map — clone of `bridge.trade_map()`, shared between the
    /// bridge (write path from WS events) and `ChartApp` (read/write path from
    /// `TradeUpdate` events and panel subscriptions).
    trade_map: trade_service::SharedTradeMap,

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
    exchange_symbols: std::collections::HashMap<digdigdig3::ExchangeId, Vec<live_data::SymbolInfo>>,

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
    /// Set to true when backfill or scroll-load added bars to the bridge cache.
    /// Checked and cleared by App in about_to_wait() to trigger a bar-store flush.
    pub bars_cache_dirty: bool,
    /// Per-window tracked series handles from BarService.
    /// Key = (chart_id, BarSeriesKey) → handle.
    /// Used for version-based change detection in future phases.
    series_handles: std::collections::HashMap<(u64, bar_service::BarSeriesKey), bar_service::TrackedSeriesHandle>,
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

    /// Cached connector registry — built once on first sidebar open, reused every frame.
    ///
    /// `ConnectorRegistry::new()` allocates a HashMap from a 50+ entry static array.
    /// The registry data is static and never changes at runtime, so we cache it here
    /// to avoid a per-frame heap allocation when the connectors sidebar panel is open.
    connector_registry: Option<digdigdig3::connector_manager::ConnectorRegistry>,

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
    last_active_leaf: Option<zengeld_chart::LeafId>,

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

    // ── Internal CPU profiling timers (updated each tick, read by sidebar) ────
    /// Total time spent in the last tick() call, in microseconds.
    pub last_tick_us: u64,
    /// Time spent in indicator recalculation (calculate_for_window) in last tick.
    pub last_indicator_recalc_us: u64,
    /// Time spent processing LiveUpdate events in the last tick.
    pub last_event_process_us: u64,
    /// Accumulated time in calc_auto_scale() calls during the last tick.
    pub last_auto_scale_us: u64,
    /// Accumulated time in calc_moving_averages() calls during the last tick.
    pub last_moving_avg_us: u64,

    /// Heavy state for all trading panels docked into the free-slot sidebars.
    ///
    /// Keyed by `PanelId`. The matching `FreeItem` leaves in each slot's
    /// `DockingManager<FreeItem>` carry only the `PanelId`, keeping
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

    /// Active DOM drag-to-scroll.
    ///
    /// `(slot_index, leaf_id, dom_panel_id, last_y, row_height)`.
    pub(crate) slot_dom_drag: Option<(usize, uzor::panels::LeafId, sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active LiquidityHeatmap drag-to-pan.
    ///
    /// `(heatmap_panel_id, last_x, last_y)`.
    pub(crate) slot_heatmap_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active L2Tape drag-to-scroll.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_l2tape_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active Footprint drag-to-pan.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_footprint_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active BigTrades drag-to-scroll.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_bigtrades_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active VolumeProfile drag-to-pan.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_volprofile_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Active TradeTape drag-to-scroll.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_tradetape_drag: Option<(sidebar_content::free_slot::PanelId, f64, f64)>,

    /// Trading manager — order routing, position tracking, paper engine.
    pub trading_manager: Option<trading_manager::TradingManager>,
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
    Upsert(zengeld_chart::preset::preset::ChartPreset),
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
    AddPrimitive(zengeld_chart::templates::primitive_template::PrimitiveTemplate),
    /// Remove a primitive template by id.
    RemovePrimitive { id: String },
    /// Add an indicator template.
    AddIndicator(zengeld_chart::templates::indicator_template::IndicatorTemplate),
    /// Remove an indicator template by id.
    RemoveIndicator { id: String },
    /// Add a compare overlay template.
    AddCompare(zengeld_chart::templates::compare_template::CompareTemplate),
    /// Remove a compare template by id.
    RemoveCompare { id: String },
    /// Add a chart settings template.
    AddChart(zengeld_chart::templates::chart_template::ChartTemplate),
    /// Remove a chart template by id.
    RemoveChart { id: String },
    /// Add an indicator set.
    AddIndicatorSet(zengeld_chart::templates::indicator_set::IndicatorSet),
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
    pub orientation: zengeld_chart::SeparatorOrientation,
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
    /// Scale-corner hit zones computed during chart content rendering.
    pub scale_corner_zones: ScaleCornerHitZones,
    /// Toolbar render result (hit zones, dropdown state) for this frame.
    pub last_toolbar_result: Option<zengeld_chart::ChartToolbarRenderResult>,
    /// Modal render result (settings panels, color pickers).
    pub frame_result: Option<ChartModalRenderResult>,
    /// Search-overlay render result (indicator / symbol search).
    pub search_modal_result: Option<zengeld_chart::ModalSearchResult>,
    /// Context-menu render result.
    pub context_menu_result: Option<ContextMenuResult>,
    /// Right-sidebar render result.
    pub last_sidebar_result: Option<sidebar_content::render::RightSidebarResult>,
    /// Watchlist-modal render result.
    pub last_watchlist_modal_result: Option<zengeld_chart::layout::modals::watchlist_modal::WatchlistModalResult>,
    /// Watchlist group-name input result.
    pub last_wl_group_name_result: Option<WlGroupNameInputResult>,
    /// Per-leaf tab hit zones, rebuilt every frame.
    pub leaf_tab_hit_zones: std::collections::HashMap<zengeld_chart::LeafId, zengeld_chart::LeafTabHitZones>,
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
    pub sub_pane_range_writebacks: Vec<(zengeld_chart::LeafId, usize, f64, f64)>,
    /// Sub-pane overlay result writebacks: `(leaf_id, overlay_results)`.
    ///
    /// Produced each frame by `render_full_chart_panel`.  Applied in
    /// `apply_render_output` so the hit tester can read button rects next frame.
    pub sub_pane_overlay_writebacks: Vec<(zengeld_chart::LeafId, Vec<zengeld_chart::SubPaneOverlayResult>)>,
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
        let shared_series: bar_service::SharedSeriesMap =
            std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        let shared_trades: trade_service::SharedTradeMap =
            std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        let shared_orderbook: orderbook_service::SharedOrderbookMap =
            std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        let (bridge, live_update_rx, _connector_ready_rx) = DataBridge::new(shared_series, shared_trades, shared_orderbook);
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
            scale_corner_zones: ScaleCornerHitZones::default(),
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
            leaf_tab_hover: zengeld_chart::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            alert_manager: alerts::AlertManager::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: sidebar_content::state::SidebarState::new(),
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
            trade_map: bridge.trade_map(),
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
            bars_cache_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
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
            connector_registry: None,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            series_handles: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_moving_avg_us: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            slot_dom_drag: None,
            slot_heatmap_drag: None,
            slot_l2tape_drag: None,
            slot_footprint_drag: None,
            slot_bigtrades_drag: None,
            slot_volprofile_drag: None,
            slot_tradetape_drag: None,
            trading_manager: trading_manager::TradingManager::new(
                bridge.clone(),
                std::path::PathBuf::from("."),
            ).ok(),
        };

        if let Some(tm) = &app.trading_manager {
            app.panels_store.set_trading_snapshot(tm.snapshot());
        }

        // Initialize WatchlistManager with a minimal default.
        // The full list is restored from persisted user state by load_user_state().
        app.sidebar_state.watchlist_manager = sidebar_content::watchlist::WatchlistManager::new(
            vec![
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
            ],
        );

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
            if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();

            // Trigger the full preset restore pipeline (layout, windows, indicators, etc.).
            let preset_id = app.panel_app.active_preset_id.clone();
            app.process_chart_out_event(
                zengeld_chart::events::ChartOutEvent::LoadPreset { id: preset_id },
            );

            // Ensure all windows have a data_provider and toolbar config.
            // Each window uses its own exchange from the saved preset.
            let window_data: Vec<(String, String, zengeld_chart::state::Timeframe, String)> = app
                .panel_app
                .panel_grid
                .iter_windows()
                .map(|(_, w)| (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone()))
                .collect();

            for window in app.panel_app.panel_grid.windows_mut().values_mut() {
                let win_exchange_id = digdigdig3::ExchangeId::from_str(&window.exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let win_exchange_name = win_exchange_id.as_str().to_string();
                let win_at = account_type_from_label(&window.account_type);
                let win_provider: SharedDataProvider = std::sync::Arc::new(
                    LiveDataProvider::new(win_exchange_id, win_exchange_name, win_at, bridge.clone()),
                );
                window.data_provider = win_provider;
                window.toolbar_config = ToolbarConfig::standalone();
            }

            // Set active_exchange from the active window's exchange.
            if let Some(active_win) = app.panel_app.panel_grid.active_window() {
                app.active_exchange = digdigdig3::ExchangeId::from_str(&active_win.exchange)
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
            if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
                window.data_provider = data_provider.clone();
                window.exchange = exchange_name.clone();
                window.toolbar_config = ToolbarConfig::standalone();
                // Use BTCUSDT as the default symbol.
                window.symbol = "BTCUSDT".to_string();
                window.timeframe = zengeld_chart::state::Timeframe::new("1H", 60);
                window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();

            // Ensure Binance connector and request paginated bars.
            bridge.ensure_connector(exchange_id);
            bridge.request_bars(exchange_id, "BTCUSDT", &zengeld_chart::state::Timeframe::new("1H", 60), digdigdig3::AccountType::Spot, None, Some(app.panel_app.user_manager.profile.bar_count as usize), false);
        }

        // Ensure connectors for all registered exchanges.
        // New connectors default to disabled — only explicitly enabled ones start.
        {
            use digdigdig3::connector_manager::ConnectorRegistry;
            let registry = ConnectorRegistry::new();
            for meta in registry.list_all() {
                // Original 24 exchanges default to enabled; new ones default to disabled.
                // l3/open connectors with real capabilities — enabled by default
                let default_enabled = matches!(meta.id,
                    digdigdig3::ExchangeId::Binance | digdigdig3::ExchangeId::Bybit |
                    digdigdig3::ExchangeId::OKX | digdigdig3::ExchangeId::KuCoin |
                    digdigdig3::ExchangeId::GateIO | digdigdig3::ExchangeId::Bitget |
                    digdigdig3::ExchangeId::MEXC | digdigdig3::ExchangeId::HTX |
                    digdigdig3::ExchangeId::Kraken | digdigdig3::ExchangeId::Coinbase |
                    digdigdig3::ExchangeId::BingX | digdigdig3::ExchangeId::Bitfinex |
                    digdigdig3::ExchangeId::Bitstamp | digdigdig3::ExchangeId::Gemini |
                    digdigdig3::ExchangeId::CryptoCom | digdigdig3::ExchangeId::Lighter |
                    digdigdig3::ExchangeId::Upbit |
                    digdigdig3::ExchangeId::Deribit | digdigdig3::ExchangeId::HyperLiquid |
                    digdigdig3::ExchangeId::Dydx |
                    digdigdig3::ExchangeId::Moex
                );
                if !app.sidebar_state.connector_enabled.get(meta.id.as_str()).copied().unwrap_or(default_enabled) {
                    continue;
                }
                bridge.ensure_connector(meta.id);
            }
        }

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
            panel_app: ChartPanelApp::new("BTCUSDT"),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones: ScaleCornerHitZones::default(),
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
            leaf_tab_hover: zengeld_chart::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            alert_manager: alerts::AlertManager::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: sidebar_content::state::SidebarState::new(),
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
            trade_map: bridge.trade_map(),
            live_update_rx,
            mini_ticker_cache: std::collections::HashMap::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            exchange_symbols: std::collections::HashMap::new(),
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
            bars_cache_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
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
            connector_registry: None,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            series_handles: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_moving_avg_us: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            slot_dom_drag: None,
            slot_heatmap_drag: None,
            slot_l2tape_drag: None,
            slot_footprint_drag: None,
            slot_bigtrades_drag: None,
            slot_volprofile_drag: None,
            slot_tradetape_drag: None,
            trading_manager: None,
        };

        app.sidebar_state.watchlist_manager = sidebar_content::watchlist::WatchlistManager::new(
            vec![
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
            ],
        );

        // Fresh state — Binance default
        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
            window.data_provider = data_provider.clone();
            window.exchange = exchange_name.clone();
            window.toolbar_config = ToolbarConfig::standalone();
            window.symbol = "BTCUSDT".to_string();
            window.timeframe = zengeld_chart::state::Timeframe::new("1H", 60);
            window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
        }
        app.panel_app.toolbar_config = ToolbarConfig::standalone();

        // Create a single "Untitled" preset tab.
        let untitled_preset = zengeld_chart::preset::preset::ChartPreset::new("Untitled".to_string());
        let untitled_id = untitled_preset.id.clone();
        app.panel_app.presets.insert(untitled_id.clone(), untitled_preset);
        app.panel_app.open_tabs = vec![untitled_id.clone()];
        app.panel_app.active_preset_id = untitled_id;

        // Ensure Binance connector and request bars.
        bridge.ensure_connector(exchange_id);
        bridge.request_bars(
            exchange_id,
            "BTCUSDT",
            &zengeld_chart::state::Timeframe::new("1H", 60),
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
        restore: Option<&zengeld_chart::WindowState>,
        profile: &zengeld_chart::UserProfile,
        user_manager: &zengeld_chart::ProfileManager,
        skeleton: bool,
    ) -> Self {
        let mut app = Self {
            panel_app: ChartPanelApp::new("BTCUSDT"),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones: ScaleCornerHitZones::default(),
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
            leaf_tab_hover: zengeld_chart::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            default_scale_mode: ScaleMode::Auto,
            alert_manager: alerts::AlertManager::new(),
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            sidebar_state: sidebar_content::state::SidebarState::new(),
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
            trade_map: bridge.trade_map(),
            live_update_rx,
            mini_ticker_cache: std::collections::HashMap::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            exchange_symbols: std::collections::HashMap::new(),
            window_id,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            bars_cache_dirty: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
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
            connector_registry: None,
            sidebar_data_dirty: true,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            series_handles: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_moving_avg_us: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            slot_dom_drag: None,
            slot_heatmap_drag: None,
            slot_l2tape_drag: None,
            slot_footprint_drag: None,
            slot_bigtrades_drag: None,
            slot_volprofile_drag: None,
            slot_tradetape_drag: None,
            trading_manager: None,
        };

        // Initialize watchlist with a minimal default — overwritten by load_user_state below.
        app.sidebar_state.watchlist_manager = sidebar_content::watchlist::WatchlistManager::new(
            vec![
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("ETHUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("SOLUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BNBUSDT".to_string(), "binance".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "bybit".to_string()),
                sidebar_content::watchlist::WatchlistSymbol::new("BTCUSDT".to_string(), "okx".to_string()),
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

        eprintln!(
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
            if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();
        } else {
            let has_saved_preset = app.panel_app.presets.contains_key(&app.panel_app.active_preset_id);

            if has_saved_preset {
                // Restore from saved preset.
                if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
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
                    zengeld_chart::events::ChartOutEvent::LoadPreset { id: preset_id },
                );

                // Ensure all windows have data_provider and toolbar config.
                let window_data: Vec<(String, String, zengeld_chart::state::Timeframe, String)> = app
                    .panel_app.panel_grid.iter_windows()
                    .map(|(_, w)| (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone()))
                    .collect();

                for window in app.panel_app.panel_grid.windows_mut().values_mut() {
                    let win_exchange_id = digdigdig3::ExchangeId::from_str(&window.exchange)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    let win_exchange_name = win_exchange_id.as_str().to_string();
                    let win_at = account_type_from_label(&window.account_type);
                    let win_provider: SharedDataProvider = std::sync::Arc::new(
                        LiveDataProvider::new(win_exchange_id, win_exchange_name, win_at, bridge.clone()),
                    );
                    window.data_provider = win_provider;
                    window.toolbar_config = ToolbarConfig::standalone();
                }

                if let Some(active_win) = app.panel_app.panel_grid.active_window() {
                    app.active_exchange = digdigdig3::ExchangeId::from_str(&active_win.exchange)
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
            } else {
                // Fresh state — Binance default.
                if let Some(window) = app.panel_app.panel_grid.active_window_mut() {
                    window.data_provider = data_provider.clone();
                    window.exchange = exchange_name.clone();
                    window.toolbar_config = ToolbarConfig::standalone();
                    window.symbol = "BTCUSDT".to_string();
                    window.timeframe = zengeld_chart::state::Timeframe::new("1H", 60);
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
                    let untitled_preset = zengeld_chart::preset::preset::ChartPreset::new(format!("Untitled {}", max_n + 1));
                    let untitled_id = untitled_preset.id.clone();
                    app.panel_app.presets.insert(untitled_id.clone(), untitled_preset);
                    app.panel_app.open_tabs = vec![untitled_id.clone()];
                    app.panel_app.active_preset_id = untitled_id;
                }

                bridge.ensure_connector(exchange_id);
                bridge.request_bars(
                    exchange_id,
                    "BTCUSDT",
                    &zengeld_chart::state::Timeframe::new("1H", 60),
                    digdigdig3::AccountType::Spot,
                    None,
                    Some(app.panel_app.user_manager.profile.bar_count as usize),
                    false,
                );
            }
        }

        // Warm up all enabled connectors (idempotent — ensure_connector is a no-op if started).
        // New connectors default to disabled — only explicitly enabled ones start.
        if !skeleton {
            use digdigdig3::connector_manager::ConnectorRegistry;
            let registry = ConnectorRegistry::new();
            for meta in registry.list_all() {
                // Original 24 exchanges default to enabled; new ones default to disabled.
                // l3/open connectors with real capabilities — enabled by default
                let default_enabled = matches!(meta.id,
                    digdigdig3::ExchangeId::Binance | digdigdig3::ExchangeId::Bybit |
                    digdigdig3::ExchangeId::OKX | digdigdig3::ExchangeId::KuCoin |
                    digdigdig3::ExchangeId::GateIO | digdigdig3::ExchangeId::Bitget |
                    digdigdig3::ExchangeId::MEXC | digdigdig3::ExchangeId::HTX |
                    digdigdig3::ExchangeId::Kraken | digdigdig3::ExchangeId::Coinbase |
                    digdigdig3::ExchangeId::BingX | digdigdig3::ExchangeId::Bitfinex |
                    digdigdig3::ExchangeId::Bitstamp | digdigdig3::ExchangeId::Gemini |
                    digdigdig3::ExchangeId::CryptoCom | digdigdig3::ExchangeId::Lighter |
                    digdigdig3::ExchangeId::Upbit |
                    digdigdig3::ExchangeId::Deribit | digdigdig3::ExchangeId::HyperLiquid |
                    digdigdig3::ExchangeId::Dydx |
                    digdigdig3::ExchangeId::Moex
                );
                if !app.sidebar_state.connector_enabled.get(meta.id.as_str()).copied().unwrap_or(default_enabled) {
                    continue;
                }
                bridge.ensure_connector(meta.id);
            }
        }

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
        profile: &zengeld_chart::UserProfile,
        window_state: Option<&zengeld_chart::WindowState>,
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
                "Top" => zengeld_chart::InlineDockEdge::Top,
                "Free" => zengeld_chart::InlineDockEdge::Free,
                _ => zengeld_chart::InlineDockEdge::Bottom,
            };
        }

        // Notification settings — load from profile into alert settings state.
        eprintln!(
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
                        let by_id: std::collections::HashMap<u64, &zengeld_chart::PersistedAgentLeaf> =
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
                                    zengeld_chart::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                    zengeld_chart::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                    zengeld_chart::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                    zengeld_chart::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                };
                                let mode = match persisted.mode {
                                    zengeld_chart::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                    zengeld_chart::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                };
                                (cli, mode)
                            } else {
                                (fallback_cli, fallback_mode)
                            };
                            Some(sidebar_content::AgentPaneLeaf {
                                instance_id: gate4agent::InstanceId::new(),
                                cli,
                                mode,
                            })
                        });

                        match restore_result {
                            Ok(tree) => {
                                // Replace the docking manager with the restored tree.
                                *self.sidebar_state.agent_docking.inner_mut() =
                                    uzor::panels::DockingManager::from_tree(tree);

                                // Rebuild the agent_leaves map from persisted descriptors.
                                // Each leaf gets a registered instance via create_instance so
                                // the Start button can find the ID in MultiCliManager's map.
                                self.sidebar_state.agent_leaves.clear();
                                for persisted in &ws.agents_tab_leaves {
                                    let cli = match persisted.cli {
                                        zengeld_chart::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                        zengeld_chart::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                        zengeld_chart::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                        zengeld_chart::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                    };
                                    let mode = match persisted.mode {
                                        zengeld_chart::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                        zengeld_chart::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                    };
                                    let leaf_id = uzor::panels::LeafId(persisted.leaf_id);
                                    let workdir = persisted.workdir.clone();
                                    let _ = std::fs::create_dir_all(&workdir);
                                    match self.agent.create_instance(cli, mode, workdir.clone()) {
                                        Ok(instance_id) => {
                                            let desc = sidebar_content::agents_dock::AgentLeafDescriptor {
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
                                eprintln!(
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
        eprintln!("[Screenshot] Capture requested");
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
            eprintln!("[ChartApp] Exchange {} is disabled, skipping", exchange_id.as_str());
            return;
        }
        // Capture old trade-stream identity BEFORE updating active_exchange or window state.
        let old_trade_exchange = self.active_exchange;
        let old_trade_symbol = self.panel_app.panel_grid.active_window()
            .map(|w| w.symbol.clone())
            .unwrap_or_default();
        let old_trade_at = self.panel_app.panel_grid.active_window()
            .map(|w| account_type_from_label(&w.account_type))
            .unwrap_or(digdigdig3::AccountType::Spot);

        let bridge = self.bridge.clone();
        self.active_exchange = exchange_id;

        // Start the connector (no-op if already running).
        bridge.ensure_connector(exchange_id);

        // Human-readable exchange name used by the window header.
        let exchange_name = exchange_id.as_str().to_string();

        // Use the active window's account_type for the new provider.
        let switch_at = self.panel_app.panel_grid.active_window()
            .map(|w| account_type_from_label(&w.account_type))
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

        // Attach the provider to the active window and request paginated bars.
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.data_provider = provider;
            window.exchange = exchange_name;
            let symbol = window.symbol.clone();
            let timeframe = window.timeframe.clone();
            let at = account_type_from_label(&window.account_type);
            bridge.request_bars(exchange_id, &symbol, &timeframe, at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
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
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::LoadPreset {
            id: id.to_string(),
        });
    }

    /// Close a tab without deleting the preset from disk.
    ///
    /// Fires `CloseTab` which removes the ID from `open_tabs` and switches
    /// to an adjacent tab if the closed tab was active.
    pub fn close_tab(&mut self, id: &str) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::CloseTab {
            id: id.to_string(),
        });
    }

    /// Open the "new chart" flow (shows the preset-name input modal).
    pub fn new_chart(&mut self) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::NewChart);
    }

    /// Open an existing (closed) preset as a new tab.
    pub fn open_tab(&mut self, id: &str) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenTab {
            id: id.to_string(),
        });
    }

    /// Open the chart browser modal (list of all saved presets).
    pub fn open_chart_browser(&mut self) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenChartBrowser);
    }

    /// Open the chart settings modal.
    pub fn open_settings(&mut self) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenChartSettings);
    }

    /// Open (or toggle) the user settings modal.
    ///
    /// Called when the chrome gear button is clicked.
    pub fn open_user_settings(&mut self) {
        self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenUserSettings);
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
        self.sync_viewport_from_layout();

        // On the very first resize after a preset restore, chart_width is now
        // known. Reposition every window so the last bar has 5 empty bars of
        // right margin (set_bars() couldn't do this because chart_width was 0).
        if self.needs_initial_viewport_fit {
            self.needs_initial_viewport_fit = false;
            for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                // Only snap if chart dimensions are valid and bars are loaded.
                if window.viewport.chart_width > 0.0 && window.viewport.bar_spacing > 0.0 && !window.bars.is_empty() {
                    window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
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
        let extended = self.build_extended_layout();
        let new_width = extended.main_chart.chart.width;
        let new_height = extended.main_chart.chart.height;
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
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
        for window in self.panel_app.panel_grid.windows().values() {
            if window.symbol.is_empty() {
                continue;
            }
            let eid = digdigdig3::ExchangeId::from_str(&window.exchange)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            let at = account_type_from_label(&window.account_type);
            // Passing `None` for both limit and total_bars lets the bridge
            // pick up incremental mode from its bar cache.
            self.bridge.request_bars(eid, &window.symbol, &window.timeframe, at, None, None, false);
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
        let matching_ids: Vec<u64> = self
            .panel_app
            .panel_grid
            .windows()
            .iter()
            .filter(|(_id, w)| w.symbol == symbol)
            .map(|(id, _w)| id.0)
            .collect();

        // Split-borrow: `panel_app.panel_grid` and `indicator_manager` are
        // distinct fields, so Rust allows simultaneous borrows of both.
        for window_id in matching_ids {
            let chart_id = ChartId(window_id);
            if let Some(w) = self.panel_app.panel_grid.windows().get(&chart_id) {
                self.indicator_manager.calculate_for_window(symbol, window_id, &w.bars);
            }
        }
        self.recalc_count += 1;
        self.sync_sub_panes_from_manager();
    }

    // -------------------------------------------------------------------------
    // Live tick
    // -------------------------------------------------------------------------

    /// Called every frame with the current wall-clock time in milliseconds.
    ///
    /// Drains the async `LiveUpdate` channel (bar loads, WebSocket bar updates,
    /// connector-ready events) and runs the alert crossing checker.
    pub fn tick(&mut self, current_time_ms: u64, bar_svc: &mut bar_service::BarService) {
        let _ = current_time_ms;
        let tick_start = std::time::Instant::now();

        // Load chat history for any restored chat leaves on the very first tick.
        // PTY sessions spawn-on-demand only (user must click [Start]).
        if !self.agent_autostarted {
            self.agent_autostarted = true;
            // Load chat history per leaf — use the persisted session_id when available
            // so each leaf resumes its own conversation, not just the latest one.
            let chat_leaves: Vec<(gate4agent::InstanceId, Option<String>)> = self.sidebar_state.agent_leaves
                .values()
                .filter(|d| d.mode == gate4agent::InstanceMode::Chat)
                .map(|d| (d.instance_id, d.chat_session_id.clone()))
                .collect();
            for (id, session_id) in chat_leaves {
                if let Some(ref sid) = session_id {
                    self.agent.load_history_instance(id, sid);
                } else {
                    self.agent.load_latest_history_instance(id);
                }
            }
        }

        // Reset per-tick accumulators for profiling.
        self.last_auto_scale_us = 0;
        self.last_moving_avg_us = 0;
        self.last_indicator_recalc_us = 0;

        // ── Live data: drain the async update channel ─────────────────────
        // The channel is a broadcast — handle Lagged by continuing to drain.
        // Track whether at least one trade arrived this tick so the alert
        // crossing checker can be skipped on quiet (no-trade) frames.
        let mut had_trade_update = false;
        let mut _drain_count = 0u32;
        let mut trading_updates: Vec<LiveUpdate> = Vec::new();
        let events_start = std::time::Instant::now();
        loop {
            let update = match self.live_update_rx.try_recv() {
                Ok(u) => { _drain_count += 1; u },
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    self.lag_event_count += 1;
                    eprintln!("[ChartApp:{}] broadcast LAGGED — skipped {} messages (total lag events: {})",
                        self.panel_app.panel_grid.windows().values().next()
                            .map(|w| w.symbol.as_str()).unwrap_or("?"),
                        n, self.lag_event_count);
                    // Always trigger backfill — the receiver has jumped forward
                    // and missed Trade updates are permanently lost.
                    if self.last_backfill_time.elapsed() > std::time::Duration::from_millis(500) {
                        self.last_backfill_time = std::time::Instant::now();
                        self.trigger_backfill_after_lag();
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    eprintln!("[ChartApp] broadcast receiver CLOSED — no more updates possible!");
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            };
            // Only mark sidebar dirty for panels that display live trade data.
            // Performance uses its own 1-second timer; Alerts, ObjectTree, and
            // Signals are not affected by individual price ticks.
            {
                use sidebar_content::state::RightSidebarPanel;
                match self.sidebar_state.right_panel {
                    RightSidebarPanel::Watchlist | RightSidebarPanel::Connectors => {
                        self.sidebar_data_dirty = true;
                    }
                    _ => {}
                }
            }
            match update {
                LiveUpdate::BarsLoaded { exchange_id, symbol, timeframe: tf_name, bars, account_type } => {
                    let loaded_tf = parse_timeframe_name(&tf_name);
                    eprintln!("[ChartApp] BarsLoaded: {:?} {} tf={} bars={} first_ts={} last_ts={}",
                        exchange_id, symbol, tf_name, bars.len(),
                        bars.first().map(|b| b.timestamp).unwrap_or(0),
                        bars.last().map(|b| b.timestamp).unwrap_or(0));

                    // Obtain/update TrackedSeriesHandle for matched windows.
                    {
                        let period_secs = loaded_tf.as_ref().map_or(60, |tf| tf.minutes as i64) * 60;
                        let bs_key = bar_service::BarSeriesKey::new(exchange_id, account_type, symbol.clone(), tf_name.clone());
                        let matched_cids: Vec<u64> = self.panel_app.panel_grid.windows().iter()
                            .filter(|(_cid, window)| {
                                window.symbol == symbol
                                    && window.exchange == exchange_id.as_str()
                                    && window.timeframe.name == tf_name
                                    && window.account_type == account_type.short_label()
                            })
                            .map(|(cid, _window)| cid.0)
                            .collect();
                        for cid_val in matched_cids {
                            let handle_key = (cid_val, bs_key.clone());
                            self.series_handles.entry(handle_key).or_insert_with(|| {
                                let arc = bar_svc.get_or_create(bs_key.clone(), period_secs);
                                bar_service::TrackedSeriesHandle::new(arc)
                            });
                        }
                    }

                    let mut any_matched = false;
                    // Collect (symbol, timeframe, account_type) for windows that received
                    // an initial load so we can trigger background backfill after the loop
                    // (can't borrow self.bridge while windows_mut() is held).
                    let mut backfill_requests: Vec<(String, zengeld_chart::state::Timeframe, digdigdig3::AccountType)> = Vec::new();
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        let matched = window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == account_type.short_label();
                        if matched {
                            any_matched = true;
                            // Use update_bars for backfill (preserves viewport),
                            // set_bars for initial load (resets viewport to end).
                            // pending_symbol_load forces the initial-load path even if a
                            // stray TradeUpdate inserted a synthetic bar before bars arrived.
                            let is_backfill = if window.pending_symbol_load {
                                false // force initial-load path, ignore any stray bars
                            } else {
                                !window.bars.is_empty()
                            };
                            eprintln!("[ChartApp]   -> window matched: sym={} exch={} tf={} is_backfill={} bars_len={} pending_sym={}",
                                window.symbol, window.exchange, window.timeframe.name,
                                is_backfill, window.bars.len(), window.pending_symbol_load);
                            if is_backfill {
                                window.update_bars(bars.clone());
                                // Also schedule backfill for cached windows that haven't
                                // reached the target bar count yet.
                                let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                                if target > 0 && (window.bars.len() as u32) < target {
                                    backfill_requests.push((
                                        window.symbol.clone(),
                                        window.timeframe.clone(),
                                        account_type_from_label(&window.account_type),
                                    ));
                                }
                            } else {
                                // Apply scale mode BEFORE set_bars so calc_auto_scale()
                                // runs with the correct mode (not stale Manual from previous symbol).
                                window.price_scale.scale_mode = self.default_scale_mode;
                                window.set_bars(bars.clone());
                                window.pending_symbol_load = false;
                                // Schedule a Layer 2 background backfill to extend history
                                // beyond the initial 300 bars.
                                let target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                                if target > 300 {
                                    backfill_requests.push((
                                        window.symbol.clone(),
                                        window.timeframe.clone(),
                                        account_type_from_label(&window.account_type),
                                    ));
                                }
                            }
                            // Force-fill empty timestamps BEFORE recalculate.
                            // Old presets may have primitives with empty point_timestamps
                            // (timestamps were not saved in earlier versions).  Without this
                            // call, recalculate_all_bar_caches skips those primitives and
                            // they render at wrong positions after a symbol/TF switch.
                            window.drawing_manager.ensure_timestamps_populated(&window.bars);
                            // Recalculate bar-index caches for all drawings so primitives
                            // render at correct positions now that real bars are available.
                            window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                            // Belt-and-suspenders: ensure every primitive has timestamps
                            // populated (catches any created without sync, e.g. via undo
                            // restore or deserialization without timestamp migration).
                            window.drawing_manager.update_all_timestamps_from_bars(&window.bars);
                            eprintln!("[BarsLoaded] after set_bars: view_start={} chart_width={} bar_spacing={}",
                                window.viewport.view_start, window.viewport.chart_width, window.viewport.bar_spacing);
                        }
                    }

                    // Trigger Layer 2 background backfill for initial loads.
                    // Done after the window loop to avoid borrow conflicts with self.bridge.
                    let bg_target = self.panel_app.user_manager.profile.data_load.background_bar_count;
                    for (sym, tf, at) in backfill_requests {
                        eprintln!("[ChartApp] Scheduling background backfill: {} {} tf={} target={}", exchange_id.as_str(), sym, tf.name, bg_target);
                        self.bridge.request_background_backfill(exchange_id, &sym, &tf, at, bg_target);
                    }

                    // Populate data_provider cache so future LoadPreset calls
                    // can serve bars synchronously via get_bars().
                    if any_matched {
                        if let Some(w) = self.panel_app.panel_grid.windows().values()
                            .find(|w| w.symbol == symbol && w.exchange == exchange_id.as_str() && w.timeframe.name == tf_name && w.account_type == account_type.short_label())
                        {
                            w.data_provider.insert_bars(&symbol, &tf_name, bars.clone());
                        }
                    }

                    // Recalculate indicators only for windows that match this
                    // BarsLoaded event (symbol + exchange + timeframe).  Using
                    // calculate_for_window instead of calculate_all_for_symbol
                    // prevents leaking bars from one TF into another window's
                    // indicators.
                    let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .windows()
                        .iter()
                        .filter(|(_, w)| {
                            w.symbol == symbol
                                && w.exchange == exchange_id.as_str()
                                && w.timeframe.name == tf_name
                                && w.account_type == account_type.short_label()
                        })
                        .map(|(cid, w)| (cid.0, w.bars.clone()))
                        .collect();
                    for (wid, bars_for_window) in &matched_ids {
                        self.indicator_manager.calculate_for_window(&symbol, *wid, bars_for_window);
                    }

                    // Only autosave and subscribe trades if at least one window matched.
                    if any_matched {
                        // Auto-subscribe to WebSocket trade stream for live updates after bars load.
                        if self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
                            self.bridge.subscribe_trades(exchange_id, &symbol, account_type);
                        }

                        // Bars are kept in-memory (window.bars) for tab-switch UX.
                        // No disk write or sync needed — bars are re-fetchable cache.
                    }
                }
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    eprintln!("[ChartApp] BackfillComplete: {} {} tf={} bars={}", exchange_id.as_str(), symbol, tf_name, bars.len());
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        if !(window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == account_type.short_label())
                        {
                            continue;
                        }
                        // Backfill always uses update_bars — viewport is never reset.
                        window.update_bars(bars.clone());
                        window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                        window.drawing_manager.update_all_timestamps_from_bars(&window.bars);
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
                    }

                    // Recalculate indicators for matched windows.
                    let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .windows()
                        .iter()
                        .filter(|(_, w)| {
                            w.symbol == symbol
                                && w.exchange == exchange_id.as_str()
                                && w.timeframe.name == tf_name
                                && w.account_type == account_type.short_label()
                        })
                        .map(|(cid, w)| (cid.0, w.bars.clone()))
                        .collect();
                    for (wid, bars_for_window) in &matched_ids {
                        self.indicator_manager.calculate_for_window(&symbol, *wid, bars_for_window);
                    }
                    // Backfill wrote new bars into the bridge cache — mark for disk flush.
                    self.bars_cache_dirty = true;
                }
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    eprintln!("[ChartApp] ScrollBarsLoaded: {} {} tf={} bars={} prepend={}",
                        exchange_id.as_str(), symbol, tf_name, bars.len(), prepend_count);

                    let at_label = account_type.short_label();
                    let mut any_matched = false;

                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        let tf_matches = window.timeframe.name == tf_name;
                        if !(window.symbol == symbol
                            && window.exchange == exchange_id.as_str()
                            && tf_matches
                            && window.account_type == at_label)
                        {
                            continue;
                        }

                        any_matched = true;
                        window.scroll_fetch_in_flight = false;
                        window.scroll_fetch_started = None;

                        // Viewport shift: prepending N bars pushes all existing indices up by N.
                        window.viewport.view_start += prepend_count as f64;

                        // Replace bars with the full merged set from the bridge.
                        window.update_bars(bars.clone());

                        // Enforce max_loaded_bars: evict oldest bars if over limit.
                        let max = self.panel_app.user_manager.profile.data_load.max_loaded_bars as usize;
                        if max > 0 && window.bars.len() > max {
                            let excess = window.bars.len() - max;
                            window.bars.drain(..excess);
                            window.viewport.view_start = (window.viewport.view_start - excess as f64).max(0.0);
                            window.viewport.bar_count = window.bars.len();
                        }

                        window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                        window.drawing_manager.update_all_timestamps_from_bars(&window.bars);
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
                    }

                    if any_matched {
                        let matched_ids: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                            .panel_app
                            .panel_grid
                            .windows()
                            .iter()
                            .filter(|(_, w)| {
                                w.symbol == symbol
                                    && w.exchange == exchange_id.as_str()
                                    && w.timeframe.name == tf_name
                            })
                            .map(|(cid, w)| (cid.0, w.bars.clone()))
                            .collect();
                        for (wid, bars_for_window) in &matched_ids {
                            self.indicator_manager.calculate_for_window(&symbol, *wid, bars_for_window);
                        }
                    }
                    // Scroll-load wrote new bars into the bridge cache — mark for disk flush.
                    if any_matched {
                        self.bars_cache_dirty = true;
                    }
                }
                LiveUpdate::BarUpdate { .. } => {
                    // BarUpdate is superseded by TradeUpdate — no-op.
                }
                LiveUpdate::TradeUpdate { exchange_id, symbol, price, quantity, timestamp, account_type, is_buyer_maker } => {
                    self.trade_count += 1;
                    had_trade_update = true;
                    // Track whether any window formed a new bar for this symbol.
                    let mut is_new_bar = false;
                    // Track whether a multi-bar gap was detected (needs REST backfill).
                    let mut needs_backfill = false;

                    // Feed trade into BarService for each active timeframe.
                    {
                        let mut seen_tfs: Vec<String> = Vec::new();
                        for window in self.panel_app.panel_grid.windows().values() {
                            if window.pending_symbol_load { continue; }
                            if window.symbol == symbol && window.account_type == account_type.short_label() {
                                let tf_name = &window.timeframe.name;
                                if !seen_tfs.contains(tf_name) {
                                    seen_tfs.push(tf_name.clone());
                                    let key = bar_service::BarSeriesKey::new(exchange_id, account_type, symbol.clone(), tf_name.clone());
                                    bar_svc.apply_trade(&key, price, quantity, timestamp);
                                }
                            }
                        }
                    }

                    // Update the last bar of every window matching this symbol.
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        if window.pending_symbol_load {
                            // Skip trade updates while waiting for initial bars —
                            // otherwise a stray bar inserted here would cause BarsLoaded
                            // to treat the load as a backfill and skip viewport repositioning.
                            continue;
                        }
                        if window.symbol == symbol && window.account_type == account_type.short_label() {
                            // Period in seconds derived from minutes field of Timeframe.
                            let period_secs = (window.timeframe.minutes as i64) * 60;
                            let trade_ts_secs = timestamp / 1000;

                            if let Some(last_ts) = window.bars.last().map(|b| b.timestamp) {
                                let candle_end = last_ts + period_secs;

                                if trade_ts_secs >= candle_end {
                                    // Detect multi-bar gap (>1 bar skipped → need REST backfill).
                                    if trade_ts_secs >= candle_end + period_secs {
                                        needs_backfill = true;
                                    }
                                    // Trade belongs to a new candle — push a fresh bar.
                                    let new_candle_start = (trade_ts_secs / period_secs) * period_secs;
                                    window.bars.push(zengeld_chart::Bar {
                                        timestamp: new_candle_start,
                                        open: price,
                                        high: price,
                                        low: price,
                                        close: price,
                                        volume: quantity,
                                    });
                                    is_new_bar = true;
                                } else if let Some(last) = window.bars.last_mut() {
                                    // Same candle — update OHLCV in-place.
                                    last.close = price;
                                    if price > last.high { last.high = price; }
                                    if price < last.low { last.low = price; }
                                    last.volume += quantity;
                                }
                            } else {
                                // No bars yet — create first bar from trade.
                                let candle_start = (trade_ts_secs / period_secs) * period_secs;
                                window.bars.push(zengeld_chart::Bar {
                                    timestamp: candle_start,
                                    open: price,
                                    high: price,
                                    low: price,
                                    close: price,
                                    volume: quantity,
                                });
                                is_new_bar = true;
                            }

                            // Update bar count.
                            window.viewport.bar_count = window.bars.len();

                            // Auto-scale if enabled.
                            if window.price_scale.scale_mode.is_auto_y() {
                                let _as_start = std::time::Instant::now();
                                window.calc_auto_scale();
                                self.last_auto_scale_us += _as_start.elapsed().as_micros() as u64;
                            }

                            let count = window.bars.len();
                            let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;

                            // Follow mode: keep last bar visible with standard margin.
                            if window.price_scale.scale_mode.is_follow() {
                                window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                            }

                            // Auto mode guard: if a new bar appeared and it would
                            // be off-screen or at the very edge, nudge viewport by
                            // exactly 1 bar so it stays visible.  No margin — A-mode
                            // just keeps the last bar in view without adding space.
                            // If the user scrolled far away, don't disturb.
                            if is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
                                let right_edge_bar = window.viewport.view_start + visible_f;
                                let last_bar = count as f64; // one past last bar index
                                // Nudge if the new bar is at or beyond the right edge
                                if last_bar >= right_edge_bar {
                                    window.viewport.view_start += 1.0;
                                }
                            }
                            // Manual mode: no viewport adjustments.

                            let _ma_start = std::time::Instant::now();
                            window.calc_moving_averages();
                            self.last_moving_avg_us += _ma_start.elapsed().as_micros() as u64;
                        }
                    }

                    // Multi-bar gap detected — trigger REST backfill to fill missing candles.
                    if needs_backfill {
                        eprintln!("[ChartApp] Multi-bar gap detected for {} — requesting REST backfill", symbol);
                        let bridge = self.bridge.clone();
                        for window in self.panel_app.panel_grid.windows().values() {
                            if window.symbol == symbol && window.account_type == account_type.short_label() {
                                let at = account_type_from_label(&window.account_type);
                                bridge.request_bars(exchange_id, &window.symbol, &window.timeframe, at, None, None, false);
                            }
                        }
                    }

                    // Write trade into the shared TradeSeries ring so that
                    // BigTrades (and future panels) can pull from it via tick().
                    {
                        let trade_key = trade_service::TradeKey::new(
                            exchange_id,
                            account_type,
                            symbol.clone(),
                        );
                        if let Ok(map) = self.trade_map.read() {
                            if let Some(series_arc) = map.get(&trade_key) {
                                if let Ok(mut series) = series_arc.write() {
                                    let trade = trade_service::Trade {
                                        timestamp_ms: timestamp,
                                        price,
                                        quantity,
                                        trade_id: 0,
                                        is_buyer_maker: if is_buyer_maker { 1 } else { 0 },
                                        _pad: [0u8; 7],
                                    };
                                    series.trades.push_back(trade);
                                    series.version += 1;
                                    series.dirty = true;
                                    if timestamp > series.last_ts_ms {
                                        series.last_ts_ms = timestamp;
                                    }
                                    // Enforce ring buffer capacity (simple eviction, no disk flush here).
                                    if series.trades.len() > series.capacity {
                                        series.trades.pop_front();
                                    }
                                }
                            }
                        }
                    }

                    // Pull new trades from the shared ring into all order-flow panels.
                    // All four panel kinds now read from shared_trades via tick().
                    for state in self.panels_store.big_trades.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.volume_profile.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.footprint.values_mut() {
                        state.tick();
                    }
                    for state in self.panels_store.trade_tape.values_mut() {
                        state.tick();
                    }

                    // Schedule indicator recalculation according to the current mode.
                    match self.indicator_manager.recalc_mode {
                        RecalcMode::PerTick => {
                            // Immediate recalc — pull bars from ALL windows with this symbol
                            // (fixes the bug where only the active window was considered).
                            let _ri_start = std::time::Instant::now();
                            self.recalc_indicators_for_symbol(&symbol);
                            self.last_indicator_recalc_us += _ri_start.elapsed().as_micros() as u64;
                        }
                        RecalcMode::PerFrame => {
                            // Defer to end-of-tick flush; all trades in this frame are batched.
                            self.indicator_manager.mark_dirty(&symbol);
                        }
                        RecalcMode::PerBar => {
                            if is_new_bar {
                                eprintln!("[ChartApp] PerBar: new bar detected for {}", symbol);
                                self.indicator_manager.mark_new_bar(&symbol);
                            } else {
                                // Still mark dirty so the flag exists; drain_pending_recalc
                                // will ignore it in PerBar mode unless a new bar formed.
                                self.indicator_manager.mark_dirty(&symbol);
                            }
                        }
                    }

                    // Update orderbook last_trade_price so the ghost-level filter
                    // has an authoritative mid to work with.
                    {
                        let ob_key = orderbook_service::OrderbookKey::new(exchange_id, account_type, &symbol);
                        let series_arc = {
                            let ob_map = self.bridge.orderbook_map();
                            ob_map.read().ok().and_then(|map| map.get(&ob_key).cloned())
                        };
                        if let Some(arc) = series_arc {
                            if let Ok(mut s) = arc.write() {
                                s.set_last_trade_price(price);
                            }
                        }
                    }
                }
                LiveUpdate::MiniTickerUpdate { exchange_id, symbol, last_price, price_change_percent, high_price, low_price, volume, account_type } => {
                    // Cache the 24h ticker stats keyed by symbol:exchange:account_type so that
                    // the same symbol on different exchanges or account types gets separate entries.
                    //
                    // Stats fields (price_change_percent, high, low, volume) are
                    // Option: BBO-only events (e.g. KuCoin `trade.ticker`) carry
                    // None for those fields and must not overwrite the values that
                    // a prior full-snapshot event already wrote into the cache.
                    let cache_key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type.short_label());
                    let entry = self.mini_ticker_cache
                        .entry(cache_key)
                        .or_insert(MiniTickerData {
                            last_price,
                            price_change_percent: 0.0,
                            high_price: 0.0,
                            low_price: 0.0,
                            volume: 0.0,
                        });
                    // Always update last_price — it is always present.
                    entry.last_price = last_price;
                    // Only update stats fields when the event carries them.
                    if let Some(v) = price_change_percent { entry.price_change_percent = v; }
                    if let Some(v) = high_price           { entry.high_price = v; }
                    if let Some(v) = low_price            { entry.low_price = v; }
                    if let Some(v) = volume               { entry.volume = v; }

                    // Mirror last_price into the matching OrderbookSeries so the ghost
                    // filter has an authoritative mid even on slow markets with no trades.
                    {
                        let ob_key = orderbook_service::OrderbookKey::new(exchange_id, account_type, &symbol);
                        let series_arc = {
                            let ob_map = self.bridge.orderbook_map();
                            ob_map.read().ok().and_then(|map| map.get(&ob_key).cloned())
                        };
                        if let Some(arc) = series_arc {
                            if let Ok(mut s) = arc.write() {
                                s.set_last_trade_price(last_price);
                            }
                        }
                    }
                }
                LiveUpdate::ConnectorReady { exchange_id } => {
                    let eid_str = exchange_id.as_str();
                    if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                        eprintln!("[ChartApp] ConnectorReady for disabled {}, ignoring", eid_str);
                        continue;
                    }
                    // Always load symbols for any connector that becomes ready.
                    let bridge = self.bridge.clone();
                    bridge.request_symbols(exchange_id);

                    // Subscribe mini-tickers for watchlist symbols on this exchange.
                    let exchange_str = exchange_id.as_str();
                    if let Some(wl) = self.sidebar_state.watchlist_manager.active_list() {
                        for ws in wl.all_symbols() {
                            if ws.exchange == exchange_str {
                                let ws_at = account_type_from_label(&ws.account_type);
                                bridge.subscribe_mini_ticker(exchange_id, &ws.symbol, ws_at);
                            }
                        }
                    }

                    // Backfill bars for ALL windows on this exchange (covers reconnect gaps).
                    // force=true bypasses the cache_is_fresh guard so a reconnect always
                    // fetches fresh data even if the last cached bar is recent.
                    for window in self.panel_app.panel_grid.windows().values() {
                        if window.symbol.is_empty() { continue; }
                        let win_eid = digdigdig3::ExchangeId::from_str(&window.exchange)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if win_eid == exchange_id {
                            let at = account_type_from_label(&window.account_type);
                            bridge.request_bars(exchange_id, &window.symbol, &window.timeframe, at, None, None, true);
                        }
                    }
                    if let Some(tm) = &mut self.trading_manager {
                        tm.on_connector_ready(exchange_id);
                    }
                }
                LiveUpdate::SymbolsLoaded { exchange_id, symbols } => {
                    self.exchange_symbols.insert(exchange_id, symbols);
                }
                LiveUpdate::Error { exchange_id, message } => {
                    eprintln!("[ChartApp] live-data error ({:?}): {}", exchange_id, message);
                }
                LiveUpdate::OrderbookSnapshot { exchange_id, account_type, symbol, bids: _, asks: _, timestamp: _, source: _ } => {
                    let ex_str = exchange_id.as_str();
                    let at_str = account_type.short_label();
                    // All orderbook panels now read from the shared OrderbookSeries via tick().
                    // The bridge already wrote the new data into the shared series before
                    // broadcasting this LiveUpdate, so tick() will see the version bump.
                    for state in self.panels_store.dom.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                    for state in self.panels_store.l2_tape.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                            state.prune_flash();
                        }
                    }
                    for state in self.panels_store.liquidity_heatmap.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                }
                LiveUpdate::OrderbookDelta { exchange_id, account_type, symbol, bids: _, asks: _, timestamp: _ } => {
                    let ex_str = exchange_id.as_str();
                    let at_str = account_type.short_label();
                    // All orderbook panels read from shared OrderbookSeries via tick().
                    for state in self.panels_store.dom.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                    for state in self.panels_store.l2_tape.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                            state.prune_flash();
                        }
                    }
                    for state in self.panels_store.liquidity_heatmap.values_mut() {
                        if state.symbol == symbol && state.exchange == ex_str && state.account_type == at_str {
                            state.tick();
                        }
                    }
                }
                LiveUpdate::ConnectorMetrics { .. } => {
                    // Metrics snapshots are collected on-demand by the metrics panel.
                    // No action needed in the main update loop.
                }
                LiveUpdate::OrderUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
                LiveUpdate::BalanceUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
                LiveUpdate::PositionUpdate { .. } => {
                    trading_updates.push(update.clone());
                }
            }
        }
        self.last_event_process_us = events_start.elapsed().as_micros() as u64;

        if !trading_updates.is_empty() {
            if let Some(tm) = &mut self.trading_manager {
                tm.tick(&trading_updates);
            }
            for state in self.panels_store.order_entry.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.position_manager.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.trade_log.values_mut() {
                state.sync_from_snapshot();
            }
        }

        // ── Alert checker: detect price crossings for every visible symbol ────
        // Skip entirely when no trade arrived this tick — nothing changed.
        if had_trade_update {
            // Collect one entry per unique (symbol, exchange, account_type) triple across all windows.
            // Multiple windows on the same triple share the same bar data, so one check
            // per triple is sufficient.
            let mut seen_pairs: std::collections::HashSet<(String, String, String)> = std::collections::HashSet::new();
            struct WindowAlertData {
                symbol: String,
                exchange: String,
                account_type: String,
                current_price: f64,
                current_bar: f64,
                drawing_points: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)>,
            }
            let window_data: Vec<WindowAlertData> = self.panel_app.panel_grid.windows()
                .values()
                .filter_map(|window| {
                    let triple = (window.symbol.clone(), window.exchange.clone(), window.account_type.clone());
                    if seen_pairs.contains(&triple) {
                        return None;
                    }
                    seen_pairs.insert(triple);
                    let current_price = window.bars.last().map(|b| b.close).unwrap_or(0.0);
                    let current_bar = window.bars.len().saturating_sub(1) as f64;
                    let drawing_points: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> = window
                        .drawing_manager
                        .primitives()
                        .iter()
                        .map(|p| (p.data().id, p.points(), alerts::DrawingExtendMode::from_u8(p.extend_mode_raw())))
                        .collect();
                    Some(WindowAlertData {
                        symbol: window.symbol.clone(),
                        exchange: window.exchange.clone(),
                        account_type: window.account_type.clone(),
                        current_price,
                        current_bar,
                        drawing_points,
                    })
                })
                .collect();

            let indicator_values = Self::build_indicator_values_for_alerts(
                &self.alert_manager,
                &self.indicator_manager,
            );

            let mut all_triggered_ids: Vec<u64> = Vec::new();
            for wd in &window_data {
                let triggered = self.alert_manager.check_crossings_dynamic(
                    wd.current_price,
                    wd.current_bar,
                    &wd.symbol,
                    &wd.exchange,
                    &wd.account_type,
                    &wd.drawing_points,
                    &indicator_values,
                );
                all_triggered_ids.extend(triggered);
            }

            // ── Signal alert checker ───────────────────────────────────────────────
            // Gather all signals from all indicator instances, then check signal alerts
            // per window (symbol/exchange/account_type context).
            {
                use zengeld_terminal_indicators::signals::signal::BarConfirmation;

                let signal_batch: Vec<(u64, usize, i8, u8, String)> = self
                    .indicator_manager
                    .instances_iter()
                    .flat_map(|inst| {
                        let ind_id = inst.id;
                        inst.signals.iter().map(move |s| {
                            let conf_u8 = match s.confirmation {
                                BarConfirmation::Pending => 0u8,
                                BarConfirmation::Closed => 1u8,
                                BarConfirmation::WickOnly => 2u8,
                            };
                            (ind_id, s.bar_index, s.direction.as_i8(), conf_u8, s.kind.description().to_string())
                        })
                    })
                    .collect();

                for wd in &window_data {
                    let triggered = self.alert_manager.check_signal_alerts(
                        &wd.symbol,
                        &wd.exchange,
                        &wd.account_type,
                        &signal_batch,
                    );
                    all_triggered_ids.extend(triggered);
                }
            }

            // Deduplicate in case the same alert matched multiple windows.
            all_triggered_ids.sort_unstable();
            all_triggered_ids.dedup();
            let triggered_ids = all_triggered_ids;

            // Use the active window's price for delivery event messages.
            let current_price = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.bars.last())
                .map(|b| b.close)
                .unwrap_or(0.0);

            // Build delivery events for triggered alerts.
            if !triggered_ids.is_empty() {
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                for id in &triggered_ids {
                    if let Some(alert) = self.alert_manager.get(*id) {
                        self.pending_delivery_events.push(alert_delivery::DeliveryEvent {
                            alert_name: alert.name.clone(),
                            symbol: symbol.clone(),
                            message: format!("{} {} @ {:.8}",
                                alert.source.display_name(),
                                alert.condition.display_name(),
                                current_price),
                            price: current_price,
                            timestamp: now,
                            screenshot: None,
                        });
                    }
                }

                // Request a screenshot capture from the render layer.
                // The renderer will attach PNG bytes to all pending_delivery_events
                // before they are drained and dispatched.
                self.pending_alert_screenshot = true;

                // Sidebar alert list needs to reflect the new Triggered status.
                self.sidebar_data_dirty = true;
            }
        }

        // ── Deferred indicator recalculation flush (PerFrame / PerBar) ────────
        // For PerTick mode drain_pending_recalc returns an empty Vec, so this
        // block is a no-op — PerTick was already handled inline above.
        let pending = self.indicator_manager.drain_pending_recalc();
        if !pending.is_empty() {
            let _deferred_start = std::time::Instant::now();
            for symbol in &pending {
                // Collect only the ChartId values (cheap u64 copies) for every
                // window showing this symbol.  Each window may have different
                // bars (different timeframes), so indicator instances are
                // per-window.  Bars are borrowed by reference below — no clone.
                let matching_ids: Vec<u64> = self
                    .panel_app
                    .panel_grid
                    .windows()
                    .iter()
                    .filter(|(_id, w)| w.symbol == *symbol)
                    .map(|(id, _w)| id.0)
                    .collect();

                // Split-borrow: `panel_app.panel_grid` and `indicator_manager`
                // are distinct struct fields, so both can be used simultaneously.
                for window_id in matching_ids {
                    let chart_id = ChartId(window_id);
                    if let Some(w) = self.panel_app.panel_grid.windows().get(&chart_id) {
                        self.indicator_manager.calculate_for_window(symbol, window_id, &w.bars);
                    }
                }
                // Count one recalc per symbol (regardless of window count).
                self.recalc_count += 1;
            }
            self.last_indicator_recalc_us += _deferred_start.elapsed().as_micros() as u64;
            self.sync_sub_panes_from_manager();
        }

        // ── Periodic RecalcMode diagnostic log (every 5 seconds) ─────────────
        if self.diagnostics_enabled
            && self.recalc_log_timer.elapsed() >= std::time::Duration::from_secs(5)
        {
            let mode = match self.indicator_manager.recalc_mode {
                RecalcMode::PerTick => "PerTick",
                RecalcMode::PerFrame => "PerFrame",
                RecalcMode::PerBar => "PerBar",
            };
            eprintln!(
                "[ChartApp] RecalcMode={} | trades={} recalcs={} in 5s",
                mode, self.trade_count, self.recalc_count
            );
            self.trade_count = 0;
            self.recalc_count = 0;
            self.recalc_log_timer = std::time::Instant::now();
        }

        // ── Layer 3: trigger scroll-fetch when viewport approaches left edge ───
        // Collect (symbol, exchange, tf_name, at_label) for windows that need a
        // historical extension fetch.  Two-pass to avoid a mutable/immutable
        // borrow conflict: first collect while mutating `scroll_fetch_in_flight`,
        // then re-borrow immutably to read `oldest_ts` for each request.
        {
            let mut scroll_requests: Vec<(String, String, String, String)> = Vec::new();

            let max_loaded = self.panel_app.user_manager.profile.data_load.max_loaded_bars;

            for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                if window.scroll_fetch_in_flight {
                    if let Some(started) = window.scroll_fetch_started {
                        if started.elapsed() > std::time::Duration::from_secs(10) {
                            eprintln!("[ChartApp] scroll_fetch_in_flight timeout, resetting for {}", window.symbol);
                            window.scroll_fetch_in_flight = false;
                            window.scroll_fetch_started = None;
                        }
                    }
                    if window.scroll_fetch_in_flight { continue; }
                }
                if window.bars.is_empty() { continue; }
                if window.pending_symbol_load { continue; }

                let visible = window.viewport.visible_bars() as f64;
                let threshold = (visible * 0.20).max(5.0);
                if window.viewport.view_start > threshold { continue; }

                if max_loaded > 0 && window.bars.len() >= max_loaded as usize { continue; }

                window.scroll_fetch_in_flight = true;
                window.scroll_fetch_started = Some(std::time::Instant::now());
                scroll_requests.push((
                    window.symbol.clone(),
                    window.exchange.clone(),
                    window.timeframe.name.clone(),
                    window.account_type.clone(),
                ));
            }

            for (symbol, exchange, tf_name, at_label) in scroll_requests {
                let oldest_ts = self.panel_app.panel_grid.windows()
                    .values()
                    .find(|w| {
                        w.symbol == symbol
                            && w.exchange == exchange
                            && w.timeframe.name == tf_name
                            && w.account_type == at_label
                    })
                    .and_then(|w| w.bars.first().map(|b| b.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = account_type_from_label(&at_label);

                if let Some(tf) = parse_timeframe_name(&tf_name) {
                    self.bridge.request_scroll_bars(eid, &symbol, &tf, at, oldest_ts, 500);
                }
            }
        }

        if self.agent.drain_events() {
            self.sidebar_data_dirty = true;
        }
        // Sync pipe_session_id from gate4agent into sidebar descriptors for persistence.
        {
            let updates: Vec<(uzor::panels::LeafId, Option<String>)> = self
                .sidebar_state
                .agent_leaves
                .iter()
                .filter(|(_, desc)| desc.mode == gate4agent::InstanceMode::Chat)
                .filter_map(|(&leaf_id, desc)| {
                    self.agent
                        .snapshot_instance(desc.instance_id)
                        .and_then(|snap| {
                            if snap.pipe_session_id != desc.chat_session_id {
                                Some((leaf_id, snap.pipe_session_id))
                            } else {
                                None
                            }
                        })
                })
                .collect();
            for (leaf_id, session_id) in updates {
                if let Some(desc) = self.sidebar_state.agent_leaves.get_mut(&leaf_id) {
                    desc.chat_session_id = session_id;
                }
                self.profile_dirty = true;
            }
        }
        self.last_tick_us = tick_start.elapsed().as_micros() as u64;
    }

    // -------------------------------------------------------------------------
    // Alert indicator-value helper
    // -------------------------------------------------------------------------

    /// Build the `indicator_values` slice required by
    /// [`alerts::AlertManager::check_crossings_dynamic`] and
    /// [`alerts::AlertManager::resolve_price_static`].
    ///
    /// Iterates all active `AlertSource::Indicator` alerts, de-duplicates
    /// `(indicator_id, output_index)` pairs, looks up the corresponding
    /// `IndicatorRenderInstance` from `indicator_manager`, and returns the
    /// output's value buffer.
    fn build_indicator_values_for_alerts(
        alert_manager: &alerts::AlertManager,
        indicator_manager: &IndicatorManager,
    ) -> Vec<(u64, usize, Vec<f64>)> {
        use std::collections::HashSet;
        let mut result: Vec<(u64, usize, Vec<f64>)> = Vec::new();
        let mut seen: HashSet<(u64, usize)> = HashSet::new();

        for alert in alert_manager.items() {
            if alert.status != alerts::AlertStatus::Active {
                continue;
            }
            if let alerts::AlertSource::Indicator { indicator_id, output_index, .. } = &alert.source {
                if !seen.insert((*indicator_id, *output_index)) {
                    continue;
                }
                if let Some(render_inst) = indicator_manager.get_render_instance(*indicator_id) {
                    if let Some(output_def) = render_inst.output_defs.get(*output_index) {
                        if let Some(values) = render_inst.values.get(&output_def.name) {
                            result.push((*indicator_id, *output_index, values.clone()));
                        }
                    }
                }
            }
        }
        result
    }

    // -------------------------------------------------------------------------
    // Alert bell rendering
    // -------------------------------------------------------------------------

    /// Draw small bell icons at the rightmost endpoint of drawing primitives and
    /// indicators that have bound Active alerts.
    ///
    /// Returns a list of `(widget_id, x, y, size)` tuples so the caller can
    /// register each bell as a clickable zone with `input_coordinator`.
    ///
    /// # Parameters
    /// * `ctx` - Render context to draw into.
    /// * `chart_area_rect` - The corrected main chart area rectangle (excluding
    ///   price/time scales).  Used to convert bar/price coordinates to screen
    ///   pixels and to clip bell icons that fall outside the visible area.
    /// * `viewport` - Viewport for bar→X conversions (must already be corrected
    ///   to match `chart_area_rect` dimensions).
    /// * `price_min` / `price_max` - Visible price range for price→Y conversion.
    /// * `drawing_manager` - Access to drawing primitives.
    /// * `window_id` - Active chart window id (for filtering primitives).
    fn draw_alert_bell_icons(
        ctx: &mut dyn RenderContext,
        chart_area_rect: LayoutRect,
        viewport: &zengeld_chart::Viewport,
        price_min: f64,
        price_max: f64,
        drawing_manager: &zengeld_chart::DrawingManager,
        indicator_manager: &IndicatorManager,
        alert_manager: &alerts::AlertManager,
        window_id: Option<u64>,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) -> Vec<(String, f64, f64, f64)> {
        use zengeld_chart::indicator_source::IndicatorSource;

        const BELL_SIZE: f64 = 12.0;
        const BELL_MARGIN: f64 = 3.0; // gap between right edge and bell center

        let chart_x = chart_area_rect.x;
        let chart_y = chart_area_rect.y;
        let chart_w = chart_area_rect.width;
        let chart_h = chart_area_rect.height;

        let mut bells: Vec<(String, f64, f64, f64)> = Vec::new();

        // Helpers to clamp a bell position inside the visible chart area.
        let clamp_bell_x = |x: f64| -> f64 {
            x.min(chart_x + chart_w - BELL_SIZE / 2.0 - BELL_MARGIN)
             .max(chart_x + BELL_SIZE / 2.0)
        };
        let clamp_bell_y = |y: f64| -> f64 {
            y.min(chart_y + chart_h - BELL_SIZE / 2.0)
             .max(chart_y + BELL_SIZE / 2.0)
        };

        for alert in alert_manager.items() {
            if alert.status != alerts::AlertStatus::Active {
                continue;
            }
            if !alert.matches_window(symbol, exchange, account_type) {
                continue;
            }

            match &alert.source {
                alerts::AlertSource::Drawing { primitive_id, .. } => {
                    // Find the primitive.
                    let prim = drawing_manager
                        .primitives()
                        .iter()
                        .find(|p| {
                            p.data().id == *primitive_id
                                && p.data().window_id == window_id
                        });

                    let prim = match prim {
                        Some(p) => p,
                        None => continue,
                    };

                    let points = prim.points();
                    if points.is_empty() {
                        continue;
                    }

                    // Use point 2 (index 1) as the anchor; fall back to point 1 if only one exists.
                    let (bar2, price2) = if points.len() >= 2 { points[1] } else { points[0] };

                    // Convert point 2 to screen coordinates (relative to chart origin).
                    let rel_x2 = viewport.bar_to_x_f64(bar2);
                    let rel_y2 = viewport.price_to_y(price2, price_min, price_max);

                    let type_id = prim.type_id();

                    let (raw_bell_x, raw_bell_y) = if (type_id == "ray" || type_id == "extended_line")
                        && points.len() >= 2
                    {
                        // For projecting primitives, extrapolate to the right edge of the chart.
                        let (bar1, price1) = points[0];
                        let rel_x1 = viewport.bar_to_x_f64(bar1);
                        let rel_y1 = viewport.price_to_y(price1, price_min, price_max);

                        let dx = rel_x2 - rel_x1;
                        let dy = rel_y2 - rel_y1;

                        if dx > 0.001 {
                            // Project to the right edge.
                            let t_right = (chart_w - rel_x1) / dx;
                            let proj_x = chart_w; // at the right edge in relative coords
                            let proj_y = rel_y1 + dy * t_right;
                            (
                                chart_x + proj_x.min(chart_w) - BELL_MARGIN,
                                chart_y + proj_y,
                            )
                        } else {
                            // Non-rightward ray: fall back to point 2 position.
                            (chart_x + rel_x2, chart_y + rel_y2)
                        }
                    } else {
                        // Trend line and all other types: bell at point 2.
                        (chart_x + rel_x2, chart_y + rel_y2)
                    };

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    // Skip if the price anchor is outside the visible price range.
                    if price2 < price_min || price2 > price_max {
                        continue;
                    }

                    // Determine if the primitive body arrives from above (screen Y
                    // decreases toward point 2) — if so, flip the bell below the
                    // anchor so it doesn't overlap the line.
                    let flip_below = if points.len() >= 2 {
                        let rel_y1 = viewport.price_to_y(points[0].1, price_min, price_max);
                        // Line goes downward on screen (y1 < y2) → body is above → bell below.
                        rel_y1 < rel_y2
                    } else {
                        false
                    };

                    let color = &prim.data().color.stroke;
                    let widget_id = format!("alert_bell_drw_{}", primitive_id);

                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, flip_below);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                alerts::AlertSource::Indicator { indicator_id, output_index, .. } => {
                    // Get render instance for this indicator.
                    let render_inst = indicator_manager.get_render_instance(*indicator_id);
                    let render_inst = match render_inst {
                        Some(ri) => ri,
                        None => continue,
                    };

                    // Indicators on sub-panes (pane > 0) are not on the main chart — skip.
                    if render_inst.pane > 0 {
                        continue;
                    }

                    // Check that indicator belongs to this symbol and window.
                    let symbol_instances = match window_id {
                        Some(wid) => indicator_manager.get_instances_for_symbol_in_window(symbol, wid),
                        None => indicator_manager.get_instances_for_symbol(symbol),
                    };
                    if !symbol_instances.iter().any(|i| i.id == *indicator_id) {
                        continue;
                    }

                    // Find the output by index.
                    let output_def = match render_inst.output_defs.get(*output_index) {
                        Some(def) => def,
                        None => continue,
                    };

                    let values = match render_inst.values.get(&output_def.name) {
                        Some(v) => v,
                        None => continue,
                    };

                    // Find the last non-NaN value within the visible range.
                    let (vis_start, vis_end) = viewport.visible_range();
                    let search_end = vis_end.min(values.len());

                    let last_valid = (vis_start..search_end)
                        .rev()
                        .find(|&i| !values[i].is_nan());

                    let (bar_idx, price) = match last_valid {
                        Some(i) => (i, values[i]),
                        None => continue,
                    };

                    // Bell X is at the bar of the last valid value.
                    let rel_x = viewport.bar_to_x_f64(bar_idx as f64);
                    let raw_bell_x = chart_x + rel_x;

                    // Convert price to screen Y.
                    let rel_y = viewport.price_to_y(price, price_min, price_max);
                    let raw_bell_y = chart_y + rel_y;

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    // Clip.
                    if price < price_min || price > price_max {
                        continue;
                    }

                    // Determine slope at the last bar: if indicator is rising
                    // (prev value < current → line comes from below on screen)
                    // the body approaches from above on screen → flip bell below.
                    let flip_below = if bar_idx > 0 {
                        let prev_val = values[bar_idx - 1];
                        !prev_val.is_nan() && prev_val > price // price dropped → line goes down screen → body above
                    } else {
                        false
                    };

                    let color = render_inst.output_defs
                        .get(*output_index)
                        .map(|d| d.color.as_str())
                        .unwrap_or("#FF9800");

                    let widget_id = format!("alert_bell_ind_{}", indicator_id);
                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, flip_below);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                alerts::AlertSource::Signal { indicator_id, .. } => {
                    // Position the bell near the last visible signal marker for this indicator,
                    // or fall back to the right edge of the first output line.
                    let render_inst = indicator_manager.get_render_instance(*indicator_id);
                    let render_inst = match render_inst {
                        Some(ri) => ri,
                        None => continue,
                    };

                    // Indicators on sub-panes (pane > 0) are not on the main chart — skip.
                    if render_inst.pane > 0 {
                        continue;
                    }

                    // Check that indicator belongs to this symbol and window.
                    let symbol_instances = match window_id {
                        Some(wid) => indicator_manager.get_instances_for_symbol_in_window(symbol, wid),
                        None => indicator_manager.get_instances_for_symbol(symbol),
                    };
                    if !symbol_instances.iter().any(|i| i.id == *indicator_id) {
                        continue;
                    }

                    // Try to find the last visible signal position for this indicator.
                    let (vis_start, vis_end) = viewport.visible_range();

                    let signal_pos = render_inst.signals.iter()
                        .filter(|s| s.bar_index >= vis_start && s.bar_index < vis_end)
                        .max_by_key(|s| s.bar_index)
                        .map(|s| (s.bar_index, s.price));

                    // Fall back to the last non-NaN value of the first output line.
                    let anchor = signal_pos.or_else(|| {
                        render_inst.output_defs.first().and_then(|def| {
                            render_inst.values.get(&def.name).and_then(|vals| {
                                let search_end = vis_end.min(vals.len());
                                (vis_start..search_end)
                                    .rev()
                                    .find(|&i| !vals[i].is_nan())
                                    .map(|i| (i, vals[i]))
                            })
                        })
                    });

                    let (bar_idx, price) = match anchor {
                        Some(p) => p,
                        None => continue,
                    };

                    if price < price_min || price > price_max {
                        continue;
                    }

                    let rel_x = viewport.bar_to_x_f64(bar_idx as f64);
                    let raw_bell_x = chart_x + rel_x;
                    let rel_y = viewport.price_to_y(price, price_min, price_max);
                    let raw_bell_y = chart_y + rel_y;

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    let color = render_inst.output_defs
                        .first()
                        .map(|d| d.color.as_str())
                        .unwrap_or("#FF9800");

                    let widget_id = format!("alert_bell_ind_{}", indicator_id);
                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, false);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                _ => {}
            }
        }

        bells
    }

    /// Draw a small bell icon near `(cx, cy)`.
    ///
    /// `flip_below` — when `true` the bell is placed *below* the anchor
    /// instead of above, so it never overlaps the primitive/indicator body.
    ///
    /// Returns `(icon_center_x, icon_center_y)` for the clickable-zone.
    fn draw_bell_icon(
        ctx: &mut dyn RenderContext,
        cx: f64,
        cy: f64,
        size: f64,
        color: &str,
        flip_below: bool,
    ) -> (f64, f64) {
        const OFFSET_X: f64 = -12.0; // left of anchor
        const OFFSET_Y_UP: f64 = -7.0; // above anchor (default)
        const OFFSET_Y_DOWN: f64 = 7.0; // below anchor (flipped)

        let offset_y = if flip_below { OFFSET_Y_DOWN } else { OFFSET_Y_UP };

        let icon_x = cx + OFFSET_X - size / 2.0;
        let icon_y = cy + offset_y - size / 2.0;

        zengeld_chart::render::draw_svg_icon(
            ctx,
            zengeld_chart::ui::Icon::Alert.svg(),
            icon_x,
            icon_y,
            size,
            size,
            color,
        );

        // Return the visual center for clickable-zone registration.
        (cx + OFFSET_X, cy + offset_y)
    }

    // -------------------------------------------------------------------------
    // Render
    // -------------------------------------------------------------------------

    /// Pre-render mutations — call once per frame on the mutable self BEFORE
    /// calling `render_to_scene`.
    ///
    /// Handles:
    /// - Layout computation and `content_rect` / `right_toolbar_left_x` sync
    /// - `indicator_manager.recalc_mode_label` sync
    /// - `diagnostics_enabled` sync
    /// - Viewport dimensions sync via `sync_viewport_from_layout()`
    /// - Alert-settings modal sync
    /// - Sidebar data rebuild (when `sidebar_data_dirty` is set)
    pub fn prepare_frame(&mut self, width: f64, height: f64) {
        // Advance the text-field store's frame counter so stale field geometry
        // from a previous frame is expired before new update_field calls arrive.
        // NOTE: render_to_scene also calls coordinator.begin_frame() which calls
        // text_fields.begin_frame() internally.  We call it here too so that
        // prepare_frame (called before render) stamps the correct frame on
        // update_field calls that follow render.
        self.input_coordinator.borrow_mut().text_fields_mut().begin_frame();

        // Sync text-field cursor → picker.hex_cursor before rendering.
        // The renderer reads hex_cursor from ColorPickerState, but the text-field
        // store owns the authoritative cursor position after mouse/keyboard events.
        let hex_id = WidgetId::new(text_input::HEX_COLOR);
        if self.input_coordinator.borrow().text_fields().is_focused(&hex_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&hex_id);
            let text = tf.text(&hex_id).to_string();
            let sel = tf.selection_range(&hex_id);
            drop(coord);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if picker.hex_editing {
                    picker.hex_cursor = cursor;
                    picker.hex_input = text.clone();
                    picker.hex_selection_start = sel.map(|(s, _)| s);
                    picker.hex_selection_end = sel.map(|(_, e)| e);
                    picker.hex_cursor_visible = cursor_vis;
                }
            }
        }

        // Sync text-field store → sidebar_state for agent chat input rendering.
        let chat_id = WidgetId::new(text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&chat_id);
            let text = tf.text(&chat_id).to_string();
            let sel = tf.selection_range(&chat_id);
            drop(coord);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            self.sidebar_state.agent_input_cursor_visible = cursor_vis;
            self.sidebar_state.agent_input_focused_leaf = self.sidebar_state.focused_agent_leaf;
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                self.sidebar_state.agent_input_buffers.insert(leaf_id, text);
                self.sidebar_state.agent_input_cursors.insert(leaf_id, cursor);
                self.sidebar_state.agent_input_selections.insert(
                    leaf_id, (sel.map(|(s, _)| s), sel.map(|(_, e)| e))
                );
            }
        } else {
            self.sidebar_state.agent_input_focused_leaf = None;
        }

        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Sync content_rect and right_toolbar_left_x so input handlers have
        // correct coordinates before the frame is rendered.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        self.content_rect = content_rect;
        self.right_toolbar_left_x = panel_layout.right_toolbar_rect.x;

        // Clamp open color picker popups to content_rect so they never
        // overlap toolbars or the right sidebar.
        {
            let cr = &content_rect;
            let margin = 4.0;
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if !picker.is_open() { continue; }
                let (pw, ph) = match picker.level {
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L1 => picker.l1_config().calculate_size(),
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L2 => picker.l2_config().calculate_size(),
                    _ => continue,
                };
                let min_x = cr.x + margin;
                let min_y = cr.y + margin;
                let max_x = (cr.x + cr.width - pw - margin).max(min_x);
                let max_y = (cr.y + cr.height - ph - margin).max(min_y);
                picker.origin.0 = picker.origin.0.clamp(min_x, max_x);
                picker.origin.1 = picker.origin.1.clamp(min_y, max_y);
            }
        }

        // Sync recalc_mode_label into user_settings_state so the modal can display it.
        self.panel_app.user_settings_state.recalc_mode_label = match self.indicator_manager.recalc_mode {
            RecalcMode::PerTick  => "Per Tick".to_string(),
            RecalcMode::PerFrame => "Per Frame".to_string(),
            RecalcMode::PerBar   => "Per Bar".to_string(),
        };
        // Sync diagnostics flag so the checkbox reflects the current state.
        self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
        // Sync data_load settings into user_settings_state for the DATA & CACHE sliders.
        // Only update the cached values when the slider is not being dragged so the
        // handle does not snap back to the committed value on every frame during drag.
        if !self.panel_app.user_settings_state.is_data_slider_dragging() {
            let dl = &self.panel_app.user_manager.profile.data_load;
            self.panel_app.user_settings_state.data_bg_bars      = dl.background_bar_count;
            self.panel_app.user_settings_state.data_max_bars     = dl.max_loaded_bars;
            self.panel_app.user_settings_state.data_store_size_mb = dl.max_store_size_mb;
            self.panel_app.user_settings_state.data_cleanup_days  = dl.store_cleanup_days;
        }

        // Sync viewport dimensions.
        // In split mode, viewport sync is handled later in the split-pane
        // layout block (after panel_grid.layout() computes up-to-date rects).
        // Running it here too would read stale panel_rects from the previous
        // frame and apply an incorrect bar_shift to view_start.
        if !self.panel_app.panel_grid.is_split() {
            self.sync_viewport_from_layout();
        }

        // Deferred viewport snap: set_bars() defers snap-to-end + auto-scale
        // to here where layout dimensions are guaranteed valid.
        // In split mode this runs AFTER the split-layout block sets real
        // chart_width values (see below).
        if !self.panel_app.panel_grid.is_split() {
            let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new(); // (chart_id, view_start, bar_spacing)
            for (&chart_id, window) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
                    window.needs_auto_scale_after_bars = false;
                    // Snap to end with standard margin.
                    window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                    // No snap_cooldown needed: snap fires in prepare_frame AFTER
                    // sync_viewport_from_layout, so bar_shift cannot undo it this frame.
                    // Next frame old_width == new_width → bar_shift = 0.
                    window.calc_auto_scale();
                    // restore_scale_mode is already consumed inside set_bars() for the eager
                    // path; for the deferred path (chart_width was 0 at set_bars time),
                    // consume it here.
                    if let Some(mode) = window.restore_scale_mode.take() {
                        window.price_scale.scale_mode = mode;
                    }
                    snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                }
            }

            // Propagate viewport snap to sync-group peers so all synced windows
            // align to the same TIME position after bar load (not just user pan/zoom).
            for (chart_id, view_start, bar_spacing) in snapped_windows {
                if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_chart_id(chart_id) {
                    self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                }
            }
        }

        // Keep the alert-settings modal's alerts list always in sync.
        if self.panel_app.alert_settings_state.is_open() {
            self.panel_app.alert_settings_state.all_alerts =
                self.alert_manager.items().to_vec();
        }

        // Auto-dirty sidebar when active leaf changes (for object tree refresh).
        let current_leaf = self.panel_app.panel_grid.docking().active_leaf();
        if current_leaf != self.last_active_leaf {
            self.last_active_leaf = current_leaf;
            self.sidebar_data_dirty = true;

            // When the active chart changes, reassign all Synced panels to the new
            // chart's group and apply the group's instrument key to their states.
            if let Some(new_active_group) = self.panel_app.panel_grid
                .active_chart_id()
                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            {
                // Update the group's exchange/account_type from the new active window so that
                // the group carries the full instrument key (symbol already tracked by TagManager).
                if let Some(w) = self.panel_app.panel_grid.active_window() {
                    let exch = w.exchange.clone();
                    let at = w.account_type.clone();
                    if let Some(g) = self.panel_app.tag_manager.group_mut(new_active_group) {
                        g.exchange = exch;
                        g.account_type = at;
                    }
                }
                self.panel_app.tag_manager.reassign_synced_panels(new_active_group);
                self.apply_key_to_panels_in_group(new_active_group);
            }
        }

        // Populate sidebar data from chart state (guarded by dirty flag).
        if self.sidebar_state.is_right_open() && self.sidebar_data_dirty {
            // --- ObjectTree: drawing primitives + indicators ---
            self.sidebar_state.object_tree_items.clear();

            let active_cid = self.panel_app.panel_grid.active_chart_id();

            // Determine whether the active window is in a real (non-auto_created) tag group.
            let tagged_group = active_cid
                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
                .and_then(|gid| self.panel_app.tag_manager.group(gid))
                .filter(|g| !g.auto_created);

            // Helper: convert a PrimitiveKind into an ObjectCategory.
            let prim_category = |kind: zengeld_chart::PrimitiveKind| match kind {
                zengeld_chart::PrimitiveKind::Annotation => zengeld_chart::ObjectCategory::Text,
                zengeld_chart::PrimitiveKind::Measurement => zengeld_chart::ObjectCategory::Measurement,
                zengeld_chart::PrimitiveKind::Trading => zengeld_chart::ObjectCategory::Position,
                zengeld_chart::PrimitiveKind::Signal => zengeld_chart::ObjectCategory::Signal,
                _ => zengeld_chart::ObjectCategory::Drawing,
            };

            // Collect active window key fields before any borrows.
            let (active_window_sym, active_window_exchange, active_window_account_type) =
                self.panel_app.panel_grid.active_window()
                    .map(|w| (w.symbol.clone(), w.exchange.clone(), w.account_type.clone()))
                    .unwrap_or_default();

            if let Some(group) = tagged_group {
                // ----------------------------------------------------------------
                // TAGGED window: two sections — "Group" and (optionally) "Window"
                // ----------------------------------------------------------------

                // --- Section "Group": primitives from group.primitives ---
                if group.sync_flags.sync_drawings {
                    for p in group.primitives.iter() {
                        let data = p.data();
                        let kind = p.kind();
                        let display = p.display_name().to_string();
                        let name = if display.is_empty() { data.type_id.as_str() } else { display.as_str() };
                        // Group primitives inherit the window's exchange/account_type since
                        // PrimitiveData has no exchange/account_type fields. If multi-exchange
                        // groups are added in the future, PrimitiveData would need those fields.
                        let prim_sym = data.symbol.clone();
                        let is_active_sym = prim_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::GroupOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            data.id, name, prim_category(kind), &data.type_id,
                        )
                        .with_visible(data.visible)
                        .with_locked(data.locked)
                        .with_color(Some(data.color.stroke.clone()))
                        .with_section("Group")
                        .with_key(&prim_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }

                // --- Section "Group": indicators from group.indicator_configs ---
                if group.sync_flags.sync_indicators {
                    let active_window_id = active_cid.map(|cid| cid.0);
                    for cfg in group.indicator_configs.iter() {
                        // Resolve to the active window's own instance so that widget
                        // actions (visibility, delete, settings) use the correct ID.
                        let local = active_window_id.and_then(|wid| {
                            self.indicator_manager.instances_iter()
                                .find(|i| i.window_id == Some(wid) && i.type_id == cfg.type_id)
                        });
                        let (id, name, type_id, visible, locked) = match local {
                            Some(inst) => (inst.id, inst.name.clone(), inst.type_id.clone(), inst.visible, inst.locked),
                            None => (cfg.id, cfg.name.clone(), cfg.type_id.clone(), cfg.visible, false),
                        };
                        let cfg_sym = cfg.symbol.clone();
                        let is_active_sym = cfg_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::GroupIndicatorOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            id, &name, zengeld_chart::ObjectCategory::Indicator, &type_id,
                        )
                        .with_visible(visible)
                        .with_locked(locked)
                        .with_section("Group")
                        .with_key(&cfg_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }

                // --- Section "Window": window-local stashed primitives ---
                // Collect stashed primitive data first so we don't hold an active_window borrow
                // while also needing indicator_manager (which is not behind the same ref).
                // Stashed primitives are always shown regardless of sync_drawings state —
                // they represent objects that were on the window before joining the tag group.
                let stashed_prim_data: Vec<_> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.stashed_primitives.iter()
                        .map(|p| {
                            let data = p.data();
                            let kind = p.kind();
                            let display = p.display_name().to_string();
                            (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone())
                        })
                        .collect())
                    .unwrap_or_default();

                // Collect window-local indicator IDs before releasing the window borrow.
                let pre_tag_ids: Vec<u64> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.pre_tag_indicator_ids.clone())
                    .unwrap_or_default();

                let has_window_section = !stashed_prim_data.is_empty() || !pre_tag_ids.is_empty();

                if has_window_section {
                    for (id, display, type_id, kind, visible, locked, stroke, prim_symbol) in &stashed_prim_data {
                        let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            *id, name, prim_category(*kind), type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_color(Some(stroke.clone()))
                        .with_section("Window")
                        .with_key(prim_symbol, &active_window_exchange, &active_window_account_type)
                        .with_item_state(sidebar_content::types::ObjectItemState::Memory)
                        .with_memory_kind(sidebar_content::types::MemoryKind::WindowStash);
                        self.sidebar_state.object_tree_items.push(item);
                    }

                    for &iid in &pre_tag_ids {
                        if let Some(inst) = self.indicator_manager.instances_iter()
                            .find(|i| i.id == iid)
                        {
                            let item = sidebar_content::types::ObjectTreeItem::new(
                                inst.id,
                                &inst.name,
                                zengeld_chart::ObjectCategory::Indicator,
                                &inst.type_id,
                            )
                            .with_visible(inst.visible)
                            .with_locked(inst.locked)
                            .with_section("Window")
                            .with_key(&active_window_sym, &active_window_exchange, &active_window_account_type)
                            .with_item_state(sidebar_content::types::ObjectItemState::Active);
                            self.sidebar_state.object_tree_items.push(item);
                        }
                    }
                }
            } else {
                // ----------------------------------------------------------------
                // UNTAGGED window (auto_created group): flat list, no section headers
                // ----------------------------------------------------------------

                // Primitives from window-local drawing_manager — all symbols, annotated by state.
                let local_prims: Vec<_> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.primitives().iter()
                        .map(|p| {
                            let data = p.data();
                            let kind = p.kind();
                            let display = p.display_name().to_string();
                            (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone())
                        })
                        .collect())
                    .unwrap_or_default();

                for (id, display, type_id, kind, visible, locked, stroke, prim_sym) in &local_prims {
                    let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                    let is_active_sym = *prim_sym == active_window_sym;
                    let item_state = if is_active_sym {
                        sidebar_content::types::ObjectItemState::Active
                    } else {
                        sidebar_content::types::ObjectItemState::Memory
                    };
                    let memory_kind = if is_active_sym {
                        sidebar_content::types::MemoryKind::None
                    } else {
                        sidebar_content::types::MemoryKind::WindowOtherKey
                    };
                    let item = sidebar_content::types::ObjectTreeItem::new(
                        *id, name, prim_category(*kind), type_id,
                    )
                    .with_visible(*visible)
                    .with_locked(*locked)
                    .with_color(Some(stroke.clone()))
                    .with_key(prim_sym, &active_window_exchange, &active_window_account_type)
                    .with_item_state(item_state)
                    .with_memory_kind(memory_kind);
                    self.sidebar_state.object_tree_items.push(item);
                }

                // Indicators from indicator_manager for this window — all symbols, annotated by state.
                let window_id = active_cid.map(|cid| cid.0);
                if let Some(wid) = window_id {
                    let insts: Vec<_> = self.indicator_manager.instances_iter()
                        .filter(|i| i.window_id == Some(wid))
                        .map(|i| (i.id, i.name.clone(), i.type_id.clone(), i.visible, i.locked, i.symbol.clone()))
                        .collect();
                    for (id, name, type_id, visible, locked, inst_sym) in &insts {
                        let is_active_sym = *inst_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::WindowOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            *id,
                            name,
                            zengeld_chart::ObjectCategory::Indicator,
                            type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_key(inst_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }
            }

            // Sort within sections: Active first, then Memory (stable to preserve sub-order).
            self.sidebar_state.object_tree_items.sort_by_key(|item| {
                let section_order: u8 = match item.section.as_deref() {
                    Some("Group") => 0,
                    Some("Window") => 1,
                    _ => 2,
                };
                let state_order: u8 = match item.item_state {
                    sidebar_content::types::ObjectItemState::Active => 0,
                    sidebar_content::types::ObjectItemState::Memory => 1,
                };
                (section_order, state_order)
            });

            // --- ObjectTree: compare overlay series ---
            if let Some(window) = self.panel_app.panel_grid.active_window() {
                for (i, series) in window.compare_overlay.series.iter().enumerate() {
                    let item = sidebar_content::types::ObjectTreeItem::new(
                        i as u64,
                        &series.symbol,
                        zengeld_chart::ObjectCategory::Compare,
                        "Compare",
                    )
                    .with_visible(series.visible)
                    .with_color(Some(series.color.clone()));
                    self.sidebar_state.object_tree_items.push(item);
                }
            }

            // --- Signals panel: collect per-instance SignalEvents ---
            use sidebar_content::types::{IndicatorsTabData, IndicatorSignalGroup, IndicatorSignalRow};

            // Only show signals for indicator instances that belong to the active window.
            let active_window_id_for_signals = active_cid.map(|cid| cid.0);
            let signal_groups: Vec<IndicatorSignalGroup> = self
                .indicator_manager
                .instances_iter()
                .filter(|inst| {
                    !inst.signals.is_empty()
                        && active_window_id_for_signals
                            .map(|wid| inst.window_id == Some(wid))
                            .unwrap_or(false)
                })
                .map(|inst| {
                    let mut rows: Vec<IndicatorSignalRow> = inst
                        .signals
                        .iter()
                        .map(|ev| IndicatorSignalRow {
                            bar_index: ev.bar_index as i64,
                            signal_type: format!("{:?}", ev.kind),
                            price: ev.price,
                            strength: 0.0,
                            direction: ev.direction.as_i8() as i32,
                        })
                        .collect();
                    rows.sort_by(|a, b| b.bar_index.cmp(&a.bar_index));
                    IndicatorSignalGroup {
                        instance_id: inst.id,
                        indicator_name: inst.name.clone(),
                        collapsed: self
                            .sidebar_state
                            .collapsed_signal_groups
                            .contains(&inst.id),
                        signals: rows,
                    }
                })
                .collect();

            let total_count = signal_groups.iter().map(|g| g.signals.len()).sum();
            self.sidebar_state.indicator_signals = IndicatorsTabData {
                groups: signal_groups,
                total_count,
            };

            // --- Watchlist: populate from WatchlistManager symbol list ---
            {
                use sidebar_content::types::WatchlistItem;

                self.sidebar_state.watchlist_items.clear();
                let watchlist_entries: Vec<(String, String, String)> = self
                    .sidebar_state
                    .watchlist_manager
                    .active_list()
                    .map(|list| {
                        list.all_symbols()
                            .iter()
                            .map(|ws| (ws.symbol.clone(), ws.exchange.clone(), ws.account_type.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

                for (sym_name, sym_exchange, sym_account_type) in &watchlist_entries {
                    let price_data = self.panel_app.panel_grid.iter_windows()
                        .find(|(_, w)| w.symbol == *sym_name && w.exchange == *sym_exchange && w.account_type == *sym_account_type)
                        .and_then(|(_, w)| w.bars.last())
                        .map(|bar| (bar.close, bar.open, bar.high, bar.low, bar.volume));

                    if let Some((price, open, high, low, volume)) = price_data {
                        let change_pct = if open != 0.0 {
                            (price - open) / open * 100.0
                        } else {
                            0.0
                        };
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: price,
                            change_percent: change_pct,
                            high_24h: high,
                            low_24h: low,
                            volume_24h: volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else if let Some(ticker) = self.mini_ticker_cache.get(&format!("{}:{}:{}", sym_name, sym_exchange, sym_account_type)) {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: ticker.last_price,
                            change_percent: ticker.price_change_percent,
                            high_24h: ticker.high_price,
                            low_24h: ticker.low_price,
                            volume_24h: ticker.volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: 0.0,
                            change_percent: 0.0,
                            high_24h: 0.0,
                            low_24h: 0.0,
                            volume_24h: 0.0,
                            account_type: sym_account_type.clone(),
                        });
                    }
                }
            }

            // --- Connectors: populate from ConnectorRegistry + active pool ---
            {
                use digdigdig3::connector_manager::{ConnectorRegistry, AuthType};
                use sidebar_content::types::ConnectorStatusItem;
                use sidebar_content::types::ConnectorGroup;

                self.sidebar_state.connector_items.clear();
                let registry = self.connector_registry.get_or_insert_with(ConnectorRegistry::new);
                let active_ids = self.bridge.pool().ids();

                let metrics_map: std::collections::HashMap<String, (digdigdig3::core::types::ConnectorStats, usize)> =
                    self.bridge.collect_metrics()
                        .into_iter()
                        .map(|(eid, stats, ws)| (eid.as_str().to_string(), (stats, ws)))
                        .collect();

                {
                    use sidebar_content::MetricsSnapshot;
                    let now = std::time::Instant::now();
                    let should_sample = self.sidebar_state.metrics_last_sample
                        .is_none_or(|last| now.duration_since(last).as_secs_f64() >= 1.0);
                    if should_sample {
                        self.sidebar_state.metrics_last_sample = Some(now);
                        for (exchange_id, (stats, ws_count)) in &metrics_map {
                            self.sidebar_state.push_metrics_sample(exchange_id, MetricsSnapshot {
                                http_requests: stats.http_requests,
                                http_errors: stats.http_errors,
                                latency_ms: stats.last_latency_ms,
                                rate_used: stats.rate_used,
                                rate_max: stats.rate_max,
                                ws_count: *ws_count,
                                ws_ping_rtt_ms: stats.ws_ping_rtt_ms,
                            });
                        }
                    }
                }

                for meta in registry.list_all() {
                    let is_active = active_ids.contains(&meta.id);
                    if !is_active {
                        continue; // connector not in pool = not shown
                    }

                    let mut item = ConnectorStatusItem::new(
                        meta.id.as_str(),
                        meta.name,
                    );

                    let pool = self.bridge.pool();
                    let at = digdigdig3::AccountType::Spot;
                    let md_caps = pool.market_data_capabilities(&meta.id, at);
                    let tr_caps = pool.trading_capabilities(&meta.id, at);
                    let ac_caps = pool.account_capabilities(&meta.id, at);

                    item.enabled = *self.sidebar_state.connector_enabled
                        .get(meta.id.as_str())
                        .unwrap_or(&true);
                    item.expanded = *self.sidebar_state.connector_expanded
                        .get(meta.id.as_str())
                        .unwrap_or(&false);
                    item.rest_healthy = item.enabled;

                    let has_ws = md_caps.map_or(false, |md| md.has_ws_klines || md.has_ws_trades || md.has_ws_orderbook);
                    item.ws_connected = item.enabled && has_ws;

                    item.auth_type = match meta.authentication {
                        AuthType::ApiKey => "API Key".to_string(),
                        AuthType::OAuth2 => "OAuth2".to_string(),
                        AuthType::TOTP => "TOTP".to_string(),
                        AuthType::BasicAuth => "Basic Auth".to_string(),
                        AuthType::BearerToken => "Bearer Token".to_string(),
                        AuthType::None => "None".to_string(),
                    };
                    item.requires_api_key = meta.requires_api_key_for_data;
                    item.free_tier = meta.free_tier;
                    item.group = if md_caps.map_or(true, |md| !md.has_klines) {
                        ConnectorGroup::NonChartData
                    } else if meta.requires_api_key_for_data {
                        ConnectorGroup::RequiresApiKey
                    } else {
                        ConnectorGroup::NoApiKey
                    };

                    if let Some(md) = md_caps {
                        item.has_klines = md.has_klines;
                        item.has_trades = md.has_recent_trades;
                        item.has_orderbook = md.has_orderbook;
                        item.has_aggregated_bars = md.has_klines;
                    }
                    if let Some(md) = md_caps {
                        item.has_ws_klines = md.has_ws_klines;
                        item.has_ws_trades = md.has_ws_trades;
                        item.has_ws_orderbook = md.has_ws_orderbook;
                    }
                    if let Some(tr) = tr_caps {
                        item.has_trading = tr.has_market_order || tr.has_limit_order;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_account = ac.has_balances;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_positions = ac.has_positions;
                    }

                    // Derive legacy UI fields from RateLimitCapabilities
                    if let Some(pool) = meta.rate_limits.rest_pools.first() {
                        if pool.is_weight {
                            item.weight_per_minute = Some(pool.max_budget * 60 / pool.window_seconds.max(1));
                        } else {
                            let rps = pool.max_budget / pool.window_seconds.max(1);
                            item.rate_limit_per_second = if rps > 0 { Some(rps) } else { None };
                            item.rate_limit_per_minute = Some(pool.max_budget * 60 / pool.window_seconds.max(1));
                        }
                    }

                    item.base_url = meta.base_url.to_string();
                    item.ws_url = meta.websocket_url.unwrap_or("").to_string();

                    item.rest_status = "active".to_string();
                    item.ws_status = if has_ws {
                        "available".to_string()
                    } else {
                        "n/a".to_string()
                    };

                    item.kline_batch_size = md_caps
                        .and_then(|md| md.max_kline_limit)
                        .unwrap_or(0);

                    item.supported_timeframes = md_caps
                        .map(|md| md.supported_intervals.iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default();

                    if let Some((stats, ws_count)) = metrics_map.get(meta.id.as_str()) {
                        item.ws_active_count = *ws_count;
                        item.http_requests_total = stats.http_requests;
                        item.http_errors_total = stats.http_errors;
                        item.last_latency_ms = stats.last_latency_ms;
                        item.rate_used = stats.rate_used;
                        item.rate_max = stats.rate_max;
                        item.rate_groups = stats.rate_groups.clone();
                        item.rate_window_seconds = meta.rate_limits.rest_pools.first().map(|p| p.window_seconds).unwrap_or(60);
                        item.ws_ping_rtt_ms = stats.ws_ping_rtt_ms;
                    }

                    item.show_metrics = *self.sidebar_state.connector_metrics_visible
                        .get(meta.id.as_str())
                        .unwrap_or(&false);

                    if let Some(history) = self.sidebar_state.metrics_history.get(meta.id.as_str()) {
                        item.metrics_history = history.iter().cloned().collect();
                    }

                    self.sidebar_state.connector_items.push(item);
                }
            }

            // --- Alerts: copy from alert manager ---
            self.sidebar_state.alert_items = self.alert_manager.items().to_vec();

            // --- ObjectTree: mark items that have bound alerts ---
            {
                let alert_items = self.alert_manager.items();
                for tree_item in &mut self.sidebar_state.object_tree_items {
                    tree_item.has_alert = alert_items.iter().any(|a| match &a.source {
                        alerts::AlertSource::Drawing { primitive_id, .. } => {
                            *primitive_id == tree_item.id
                        }
                        alerts::AlertSource::Indicator { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        alerts::AlertSource::Signal { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        _ => false,
                    });
                }
            }

            self.sidebar_data_dirty = false;
        }

        // Keep symbol / compare search results filtered by current query so
        // render_to_scene (which takes &self) can read the pre-filtered list.
        if self.modal_state.current == OpenModal::SymbolSearch
            || self.modal_state.current == OpenModal::CompareSearch
        {
            let query = self.modal_state.search_query.clone();
            self.modal_state.symbol_search_results =
                Self::build_demo_symbol_results(&query, &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
        }

        // --- Split-pane layout and viewport sync ---
        //
        // When the grid is split, call panel_grid.layout() so sub-chart rects
        // are computed, then sync each leaf window's viewport dimensions so that
        // bar_to_x, visible_range, and crosshair calculations are correct.
        // Also sync group primitives into each window's drawing_manager.
        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout_pf = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let content_rect_pf = {
            let mut r = panel_layout_pf.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };

        if self.panel_app.panel_grid.is_split() {
            let split_rect = zengeld_chart::PanelRect {
                x: 0.0,
                y: 0.0,
                width: content_rect_pf.width as f32,
                height: content_rect_pf.height as f32,
            };
            self.panel_app.panel_grid.layout(split_rect);

            let leaf_rects: Vec<_> = self.panel_app.panel_grid.panel_rects()
                .iter()
                .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
                .collect();

            // Sync viewport.chart_width/chart_height for all split windows.
            // Pre-compute target dimensions using immutable borrows (build_extended_layout_for_leaf
            // needs &self), then apply with mutable borrows in a second pass.
            let leaf_dims: Vec<(zengeld_chart::LeafId, f64, f64)> = leaf_rects.iter()
                .filter_map(|&(leaf_id, sub_rect)| {
                    let leaf_layout_rect = LayoutRect {
                        x: content_rect_pf.x + sub_rect.x as f64,
                        y: content_rect_pf.y + sub_rect.y as f64,
                        width: sub_rect.width as f64,
                        height: sub_rect.height as f64,
                    };
                    // build_extended_layout_for_leaf accounts for sub-panes so that
                    // chart_height reflects only the main chart area, matching the
                    // render path and eliminating the hit-test Y offset.
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_layout_rect)?;
                    Some((leaf_id, extended.main_chart.chart.width, extended.main_chart.chart.height))
                })
                .collect();

            for (leaf_id, new_chart_w, new_chart_h) in leaf_dims {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    let old_w = window.viewport.chart_width;
                    // Skip bar_shift when window hasn't been snapped yet (still has
                    // placeholder chart_width from Viewport::default).  The deferred
                    // snap below will compute view_start with the real chart_width.
                    // chart_width/chart_height are always updated regardless.
                    if !window.needs_auto_scale_after_bars
                        && (old_w - new_chart_w).abs() > 0.5
                        && window.viewport.bar_spacing > 0.0
                        && old_w > 0.0
                    {
                        let bar_shift = (old_w - new_chart_w) / window.viewport.bar_spacing;
                        window.viewport.view_start += bar_shift;
                    }
                    window.viewport.chart_width = new_chart_w;
                    window.viewport.chart_height = new_chart_h;
                }
            }

            // Deferred viewport snap for split mode: now chart_width is real.
            {
                let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new();
                for (&chart_id, window) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                    if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
                        window.needs_auto_scale_after_bars = false;
                        // Snap to end with standard margin,
                        // using CURRENT bar_spacing (restored from preset).
                        window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                        window.calc_auto_scale();
                        if let Some(mode) = window.restore_scale_mode.take() {
                            window.price_scale.scale_mode = mode;
                        }
                        snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                    }
                }
                // (diagnostic logging removed — snap-to-end confirmed working)
                for (chart_id, view_start, bar_spacing) in snapped_windows {
                    if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_chart_id(chart_id) {
                        self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                    }
                }
            }

            // Sync group primitives into split windows, filtered to each
            // window's current symbol so stale drawings don't bleed through
            // after a symbol switch.
            let group_prim_sync: Vec<(zengeld_chart::ChartId, Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>>)> = {
                let mut syncs = Vec::new();
                for &(leaf_id, _) in &leaf_rects {
                    if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                        let window_symbol = self.panel_app.panel_grid
                            .window_for_leaf(leaf_id)
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        if let Some(group_id) = self.panel_app.panel_grid
                            .window_for_leaf(leaf_id)
                            .and_then(|w| w.group_id)
                        {
                            if let Some(group) = self.panel_app.tag_manager.group(group_id) {
                                if group.sync_flags.sync_drawings && group.members.len() > 1 {
                                    let cloned: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                        group.primitives.iter()
                                            .filter(|p| {
                                                let sym = &p.data().symbol;
                                                sym.is_empty() || sym == &window_symbol
                                            })
                                            .map(|p| p.clone_box())
                                            .collect();
                                    syncs.push((chart_id, cloned));
                                }
                            }
                        }
                    }
                }
                syncs
            };
            for (chart_id, cloned_prims) in group_prim_sync {
                if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                    if !window.drawing_manager.is_dragging() {
                        window.drawing_manager.sync_from_group_primitives(&cloned_prims);
                    }
                }
            }

            // Sync indicator overlay visibility per leaf.
            for &(leaf_id, _) in &leaf_rects {
                let symbol = self.panel_app.panel_grid.window_for_leaf(leaf_id)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let has_compare = self.panel_app.panel_grid.window_for_leaf(leaf_id)
                    .map(|w| !w.compare_overlay.series.is_empty())
                    .unwrap_or(false);
                let has_indicators = if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                    !self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0).is_empty()
                } else {
                    !self.indicator_manager.get_instances_for_symbol(&symbol).is_empty()
                };
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.visible = has_indicators || has_compare;
            }
        } else {
            // Single pane: sync group primitives.
            if let Some(active_window) = self.panel_app.panel_grid.active_window() {
                let group_id_opt = active_window.group_id;
                let is_dragging = active_window.drawing_manager.is_dragging();
                let chart_id_opt = self.panel_app.panel_grid.active_chart_id();
                if let (Some(group_id), Some(chart_id)) = (group_id_opt, chart_id_opt) {
                    if !is_dragging {
                        // Respect the sync_drawings flag — skip forward sync if disabled.
                        let (drawings_on, is_mono) = self.panel_app.tag_manager
                            .group(group_id)
                            .map(|g| (g.sync_flags.sync_drawings, g.members.len() <= 1))
                            .unwrap_or((true, false));
                        if drawings_on && !is_mono {
                            // Capture the window's current symbol so we can filter
                            // primitives — stale drawings from the previous symbol
                            // must not be re-injected by the forward sync.
                            let window_symbol = self.panel_app.panel_grid
                                .windows()
                                .get(&chart_id)
                                .map(|w| w.symbol.clone())
                                .unwrap_or_default();
                            let cloned: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                self.panel_app.tag_manager
                                    .group(group_id)
                                    .map(|g| g.primitives.iter()
                                        .filter(|p| {
                                            let sym = &p.data().symbol;
                                            sym.is_empty() || sym == &window_symbol
                                        })
                                        .map(|p| p.clone_box())
                                        .collect())
                                    .unwrap_or_default();
                            if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                                window.drawing_manager.sync_from_group_primitives(&cloned);
                            }
                        }
                    }
                }
            }

            // Single pane: sync indicator overlay visibility.
            let (symbol, has_compare) = self.panel_app.panel_grid.active_window()
                .map(|w| (w.symbol.clone(), !w.compare_overlay.series.is_empty()))
                .unwrap_or_default();
            let has_indicators = if let Some(chart_id) = self.panel_app.panel_grid.active_chart_id() {
                !self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0).is_empty()
            } else {
                !self.indicator_manager.get_instances_for_symbol(&symbol).is_empty()
            };
            self.panel_app.indicator_overlay_state.visible = has_indicators || has_compare;
        }

        // Sync sub-pane pixel geometry into window.sub_panes so that
        // PanSubPane handlers and other &mut code see up-to-date values.
        self.sync_sub_pane_geometry();

        // Snapshot agent state for the sidebar renderer (agents panel).
        // Done here in prepare_frame (&mut self) because render_to_scene takes &self.
        // Iterate all registered agent leaves and snapshot each instance.
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
    }

    /// Convenience wrapper: calls `prepare_frame`, `render_to_scene`, then
    /// `apply_render_output` in sequence.
    ///
    /// Existing call sites can continue using this method unchanged.
    pub fn render(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64, skip_toolbar_draw: bool) {
        self.prepare_frame(self.width as f64, self.height as f64);
        let output = self.render_to_scene(ctx, current_time_ms, skip_toolbar_draw);
        self.apply_render_output(output);
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
    pub fn render_to_scene(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64, skip_toolbar_draw: bool) -> RenderOutput {
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
        let mut out_leaf_tab_hit_zones: std::collections::HashMap<zengeld_chart::LeafId, zengeld_chart::LeafTabHitZones> = std::collections::HashMap::new();
        // Submenu state update from toolbar dropdown rendering.
        let mut out_open_submenu_update: Option<Option<String>> = None;
        // Sub-pane range writebacks: render computes symmetrized+padded ranges,
        // applied by apply_render_output() via window_for_leaf_mut.
        let mut out_sub_pane_range_writebacks: Vec<(zengeld_chart::LeafId, usize, f64, f64)> = Vec::new();
        // Sub-pane overlay result writebacks: cached per-window for hit testing.
        let mut out_sub_pane_overlay_writebacks: Vec<(zengeld_chart::LeafId, Vec<zengeld_chart::SubPaneOverlayResult>)> = Vec::new();

        let _rt1 = std::time::Instant::now(); // checkpoint: before chart render
        let frame_theme = self.panel_app.frame_theme_for_render();
        let leaf_tab_toolbar_theme = self.panel_app.toolbar_theme_for_render();
        // main_chart_y is set in the single-window branch below; used by the
        // indicator overlay chevron rendered after the chart block.
        let mut main_chart_y_single = content_rect.y;
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

            let chart_theme = self.panel_app.chart_theme_for_render();
            let no_toolbar = zengeld_chart::ToolbarConfig::minimal();

            // Compute alert indicator values ONCE per render — they are
            // window-independent (only depend on alert_manager + indicator_manager)
            // and can be reused across all split-pane leaves.
            let alert_indicator_values_cache = Self::build_indicator_values_for_alerts(
                &self.alert_manager,
                &self.indicator_manager,
            );

            // Pre-build drawing points per chart window. In split-pane mode the
            // same window may appear as multiple leaves; computing points once
            // and reusing avoids redundant Vec allocations per leaf.
            let drawing_points_cache: std::collections::HashMap<
                zengeld_chart::ChartId,
                Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)>,
            > = self.panel_app.panel_grid.windows()
                .iter()
                .map(|(&chart_id, w)| {
                    let pts: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> = w
                        .drawing_manager
                        .primitives()
                        .iter()
                        .map(|p| (p.data().id, p.points(), alerts::DrawingExtendMode::from_u8(p.extend_mode_raw())))
                        .collect();
                    (chart_id, pts)
                })
                .collect();

            let mut sub_pane_writebacks: Vec<(zengeld_chart::LeafId, Vec<(usize, f64, f64)>)> = Vec::new();
            let mut overlay_writebacks: Vec<(zengeld_chart::LeafId, Vec<zengeld_chart::SubPaneOverlayResult>)> = Vec::new();

            for (leaf_id, sub_rect) in leaf_rects {
                let window = match self.panel_app.panel_grid.window_for_leaf(leaf_id) {
                    Some(w) => w,
                    None => continue,
                };

                // Convert sub_rect to absolute LayoutRect.
                let leaf_rect = LayoutRect {
                    x: content_rect.x + sub_rect.x as f64,
                    y: content_rect.y + sub_rect.y as f64,
                    width: sub_rect.width as f64,
                    height: sub_rect.height as f64,
                };

                use zengeld_chart::chart::render::ChartRect;
                let chart_rect = ChartRect::new(
                    leaf_rect.x, leaf_rect.y,
                    leaf_rect.width, leaf_rect.height,
                );
                let scale_corner_state = window.to_corner_state();
                let render_state = window.to_render_state(
                    chart_rect,
                    &chart_theme,
                    Some(window.timeframe.name.as_str()),
                    Some(&window.scale_settings.time_format),
                );
                let crosshair_config = if self.pending_alert_screenshot {
                    zengeld_chart::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
                } else {
                    zengeld_chart::chart::CrosshairConfig::default()
                };
                let render_config = ChartRenderConfig {
                    scale_theme: self.panel_app.scale_theme_for_render(),
                    chart_type: window.chart_type,
                    crosshair_config,
                    ..ChartRenderConfig::default()
                };

                // Scope indicator queries to this window so each split pane only
                // renders the indicators that belong to it.
                if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                    self.indicator_manager.current_render_window_id.set(Some(chart_id.0));
                }

                let alert_current_bar = window.bars.len().saturating_sub(1) as f64;
                // Look up pre-built drawing points from cache (computed once per
                // chart window before this loop to avoid per-leaf reallocation).
                let empty_pts: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> = Vec::new();
                let chart_id_for_cache = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id);
                let alert_drawing_points: &Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> =
                    chart_id_for_cache
                        .and_then(|cid| drawing_points_cache.get(&cid))
                        .unwrap_or(&empty_pts);
                // Reuse the indicator values computed once before this loop.
                let alert_indicator_values = &alert_indicator_values_cache;
                let alert_render_data: Vec<AlertRenderData> = self.alert_manager.items()
                    .iter()
                    .filter(|a| a.status == alerts::AlertStatus::Active)
                    .filter(|a| matches!(a.source, alerts::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(&window.symbol, &window.exchange, &window.account_type))
                    .filter_map(|alert| {
                        let price = alerts::AlertManager::resolve_price_static(
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

                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&window.drawing_manager),
                    indicator_source: Some(&self.indicator_manager),
                    symbol: Some(&window.symbol),
                    sub_panes: Some(&window.sub_panes),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: window.watermark.as_ref(),
                    tooltip: Some(&window.tooltip),
                    alert_render_data: &alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &no_toolbar,
                    is_split: true,
                };

                let render_result = render_full_chart_panel(ctx, &leaf_rect, &panel_data);
                if !render_result.sub_pane_ranges.is_empty() {
                    sub_pane_writebacks.push((leaf_id, render_result.sub_pane_ranges));
                }
                // Store overlay results for next frame's hit testing.
                overlay_writebacks.push((leaf_id, render_result.sub_pane_overlays));

                // Post-render: draw bell icons for alerts bound to drawing
                // primitives and overlay indicators in this leaf.
                let main_chart_y;
                {
                    let sub_pane_ids: Vec<u64> = if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                        self.indicator_manager
                            .get_instances_for_symbol_in_window(&window.symbol, chart_id.0)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    } else {
                        self.indicator_manager
                            .get_instances_for_symbol(&window.symbol)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    };
                    // Build heights matching sub_pane_ids (visible only), not all sub_panes.
                    let sub_pane_heights: Vec<f64> = sub_pane_ids.iter().map(|&id| {
                        let ratio = window.sub_panes.iter()
                            .find(|p| p.instance_id == id)
                            .map(|p| p.height_ratio)
                            .unwrap_or(0.0);
                        if ratio > 0.0 { (ratio as f64 * leaf_rect.height).max(30.0) } else { 100.0 }
                    }).collect();
                    let maximized_instance_id: Option<u64> = window.sub_panes.iter()
                        .find(|p| p.maximized && sub_pane_ids.contains(&p.instance_id))
                        .map(|p| p.instance_id);
                    let above_main_flags_sub: Vec<bool> = sub_pane_ids.iter().map(|&id| {
                        window.sub_panes.iter()
                            .find(|p| p.instance_id == id)
                            .map(|p| p.above_main)
                            .unwrap_or(false)
                    }).collect();
                    let extended = zengeld_chart::ExtendedFrameLayout::compute_from_chart_panel(
                        &leaf_rect,
                        &sub_pane_ids,
                        &window.scale_settings,
                        &sub_pane_heights,
                        1.0,
                        maximized_instance_id,
                        &above_main_flags_sub,
                    );
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

                    let window_id = self.panel_app.panel_grid
                        .chart_id_for_leaf(leaf_id)
                        .map(|cid| cid.0);

                    let bells = Self::draw_alert_bell_icons(
                        ctx,
                        chart_area,
                        &corrected_vp,
                        window.price_scale.price_min,
                        window.price_scale.price_max,
                        &window.drawing_manager,
                        &self.indicator_manager,
                        &self.alert_manager,
                        window_id,
                        &window.symbol,
                        &window.exchange,
                        &window.account_type,
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
                    main_chart_y = extended.main_chart.chart.y;
                }

                // Reset render scope after this leaf is done.
                self.indicator_manager.current_render_window_id.set(None);

                // Render overlay tab header at the top-left of this leaf.
                let is_active_leaf = self.panel_app.panel_grid.docking()
                    .active_leaf() == Some(leaf_id);
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(leaf_id) {
                    self.leaf_tab_hover
                } else {
                    zengeld_chart::LeafTabHoverZone::None
                };
                let color_tag = self.panel_app.leaf_color_tags.get(&leaf_id).copied();
                let hit_zones = zengeld_chart::render_leaf_tab(
                    ctx,
                    leaf_rect.x + 2.0,
                    main_chart_y + 2.0,
                    leaf_rect.width - 4.0,
                    &window.symbol,
                    &window.timeframe.name,
                    &window.exchange,
                    &window.account_type,
                    is_active_leaf,
                    hover_zone,
                    color_tag,
                    &leaf_tab_toolbar_theme,
                );
                let tab_rect = hit_zones.tab_rect;
                out_leaf_tab_hit_zones.insert(leaf_id, hit_zones);

                // Register overlay tab as a UI widget so crosshair hides and cursor is default.
                {
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        use uzor::input::Sense;
                        self.input_coordinator.borrow_mut().register(
                            format!("leaf_tab:{}", leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                }

                // Render per-leaf indicator overlay (chevron) for this leaf.
                {
                    let symbol = window.symbol.clone();
                    // Collect compare series info before any mutable borrow of panel_app.
                    let compare_series_info: Vec<(String, bool, String)> = window
                        .compare_overlay
                        .series
                        .iter()
                        .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                        .collect();
                    let instances = if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
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

                    if overlay_state.visible || overlay_state.is_open {
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
                                use zengeld_chart::ui::z_order::ZLayer;

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

            use zengeld_chart::SeparatorOrientation;
            for (orientation, position, start, length, thickness) in separators {
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

            ScaleCornerHitZones::default()
        } else {
            let scale_corner_state = self.panel_app.panel_grid.active_window()
                .map(|w| w.to_corner_state())
                .unwrap_or_default();
            let active_chart_type = self.panel_app.panel_grid.active_window()
                .map(|w| w.chart_type)
                .unwrap_or("candles");
            let crosshair_config = if self.pending_alert_screenshot {
                zengeld_chart::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
            } else {
                zengeld_chart::chart::CrosshairConfig::default()
            };
            let render_config = ChartRenderConfig {
                scale_theme: self.panel_app.scale_theme_for_render(),
                chart_type: active_chart_type,
                crosshair_config,
                ..ChartRenderConfig::default()
            };

            let chart_theme = self.panel_app.chart_theme_for_render();

            // Snapshot symbol/timeframe/exchange/account_type before the window borrow for the overlay tab.
            let single_window_info: Option<(String, String, String, String)> = self.panel_app.panel_grid
                .active_window()
                .map(|w| (w.symbol.clone(), w.timeframe.name.clone(), w.exchange.clone(), w.account_type.clone()));

            // Scope indicator queries to the active window for single-window mode.
            if let Some(chart_id) = self.panel_app.panel_grid.active_chart_id() {
                self.indicator_manager.current_render_window_id.set(Some(chart_id.0));
            }

            // Group primitive sync was done in prepare_frame; use the already-synced drawing_manager here.

            let (single_alert_current_bar, single_alert_drawing_points) = if let Some(window) = self.panel_app.panel_grid.active_window() {
                let cb = window.bars.len().saturating_sub(1) as f64;
                let pts: Vec<(u64, Vec<(f64, f64)>, alerts::DrawingExtendMode)> = window
                    .drawing_manager
                    .primitives()
                    .iter()
                    .map(|p| (p.data().id, p.points(), alerts::DrawingExtendMode::from_u8(p.extend_mode_raw())))
                    .collect();
                (cb, pts)
            } else {
                (0.0, Vec::new())
            };
            let single_alert_indicator_values = Self::build_indicator_values_for_alerts(
                &self.alert_manager,
                &self.indicator_manager,
            );
            let single_alert_render_data: Vec<AlertRenderData> = {
                // single_window_info = (symbol, timeframe, exchange, account_type) captured above.
                let (single_sym, _, single_exch, single_at) = single_window_info
                    .as_ref()
                    .map(|(s, tf, e, at)| (s.as_str(), tf.as_str(), e.as_str(), at.as_str()))
                    .unwrap_or(("", "", "", ""));
                self.alert_manager.items()
                    .iter()
                    .filter(|a| a.status == alerts::AlertStatus::Active)
                    .filter(|a| matches!(a.source, alerts::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(single_sym, single_exch, single_at))
                    .filter_map(|alert| {
                        let price = alerts::AlertManager::resolve_price_static(
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
            let mut single_sub_pane_overlays: Vec<zengeld_chart::SubPaneOverlayResult> = Vec::new();

            let window_opt = self.panel_app.panel_grid.active_window();
            let corner_zones_single = if let Some(window) = window_opt {
                use zengeld_chart::chart::render::ChartRect;
                let chart_rect = ChartRect::new(
                    content_rect.x,
                    content_rect.y,
                    content_rect.width,
                    content_rect.height,
                );
                let render_state = window.to_render_state(
                    chart_rect,
                    &chart_theme,
                    Some(window.timeframe.name.as_str()),
                    Some(&window.scale_settings.time_format),
                );

                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&window.drawing_manager),
                    indicator_source: Some(&self.indicator_manager),
                    symbol: Some(&window.symbol),
                    sub_panes: Some(&window.sub_panes),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: window.watermark.as_ref(),
                    tooltip: Some(&window.tooltip),
                    alert_render_data: &single_alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &self.panel_app.toolbar_config,
                    is_split: false,
                };

                // Render chart into content_rect (shrunk by sidebar).
                // Use content_rect as the full rect with minimal toolbar config
                // so render_full_chart_panel treats the entire rect as chart area
                // (toolbars are rendered separately via render_toolbars_with_theme).
                let chart_render_rect = content_rect;
                let mut chart_panel_data = panel_data;
                let no_toolbar = zengeld_chart::ToolbarConfig::minimal();
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
                    let sub_pane_ids: Vec<u64> = if let Some(chart_id) = self.panel_app.panel_grid.active_chart_id() {
                        self.indicator_manager
                            .get_instances_for_symbol_in_window(&window.symbol, chart_id.0)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    } else {
                        self.indicator_manager
                            .get_instances_for_symbol(&window.symbol)
                            .into_iter()
                            .filter(|i| i.visible && i.pane > 0)
                            .map(|i| i.id)
                            .collect()
                    };
                    // Build heights matching sub_pane_ids (visible only), not all sub_panes.
                    let sub_pane_heights: Vec<f64> = sub_pane_ids.iter().map(|&id| {
                        let ratio = window.sub_panes.iter()
                            .find(|p| p.instance_id == id)
                            .map(|p| p.height_ratio)
                            .unwrap_or(0.0);
                        if ratio > 0.0 { (ratio as f64 * chart_render_rect.height).max(30.0) } else { 100.0 }
                    }).collect();
                    let maximized_instance_id: Option<u64> = window.sub_panes.iter()
                        .find(|p| p.maximized && sub_pane_ids.contains(&p.instance_id))
                        .map(|p| p.instance_id);
                    let above_main_flags_sub: Vec<bool> = sub_pane_ids.iter().map(|&id| {
                        window.sub_panes.iter()
                            .find(|p| p.instance_id == id)
                            .map(|p| p.above_main)
                            .unwrap_or(false)
                    }).collect();
                    let extended = zengeld_chart::ExtendedFrameLayout::compute_from_chart_panel(
                        &chart_render_rect,
                        &sub_pane_ids,
                        &window.scale_settings,
                        &sub_pane_heights,
                        1.0,
                        maximized_instance_id,
                        &above_main_flags_sub,
                    );
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
                        .active_chart_id()
                        .map(|cid| cid.0);

                    let bells = Self::draw_alert_bell_icons(
                        ctx,
                        chart_area,
                        &corrected_vp,
                        window.price_scale.price_min,
                        window.price_scale.price_max,
                        &window.drawing_manager,
                        &self.indicator_manager,
                        &self.alert_manager,
                        window_id,
                        &window.symbol,
                        &window.exchange,
                        &window.account_type,
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
                    main_chart_y_single = extended.main_chart.chart.y;
                }

                corner_zones_ret
            } else {
                main_chart_y_single = content_rect.y;
                ScaleCornerHitZones::default()
            };

            // Collect single-window sub-pane ranges for writeback via RenderOutput.
            if !single_sub_pane_ranges.is_empty() {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(zengeld_chart::LeafId(0));
                for (pane_idx, min, max) in single_sub_pane_ranges {
                    out_sub_pane_range_writebacks.push((single_leaf_id, pane_idx, min, max));
                }
            }

            // Collect single-window overlay results for writeback via RenderOutput.
            {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(zengeld_chart::LeafId(0));
                out_sub_pane_overlay_writebacks.push((single_leaf_id, single_sub_pane_overlays));
            }

            // Reset render scope after single-window render is complete.
            self.indicator_manager.current_render_window_id.set(None);

            // Render overlay tab header (always, even in single mode).
            if let Some((symbol, timeframe, exchange, account_type_label)) = single_window_info {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(zengeld_chart::LeafId(0));
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(single_leaf_id) {
                    self.leaf_tab_hover
                } else {
                    zengeld_chart::LeafTabHoverZone::None
                };
                let single_color_tag = self.panel_app.leaf_color_tags.get(&single_leaf_id).copied();
                let hit_zones = zengeld_chart::render_leaf_tab(
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
                out_leaf_tab_hit_zones.insert(single_leaf_id, hit_zones);

                // Register overlay tab as a UI widget so crosshair hides and cursor is default.
                {
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        use uzor::input::Sense;
                        self.input_coordinator.borrow_mut().register(
                            format!("leaf_tab:{}", single_leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                }
            }

            corner_zones_single
        };
        let out_scale_corner_zones = corner_zones;
        let _rt2 = std::time::Instant::now(); // checkpoint: after chart render
        // Build selected primitive config for the inline config toolbar.
        //
        // DrawingManager::get_selected_config() reads the primitive registry and
        // the selected primitive's data in one call, returning None when nothing
        // is selected.  The result is a cheap owned value so no borrow escapes.
        let selected_config: Option<zengeld_chart::state::selected_config::SelectedPrimitiveConfig> =
            self.panel_app
                .panel_grid
                .active_window()
                .and_then(|w| w.drawing_manager.get_selected_config());

        // Generate clock time from active window's timezone settings
        let clock_time = {
            let utc_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some(window) = self.panel_app.panel_grid.active_window() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                let offset = time_fmt.timezone_offset_hours;
                if offset >= 0 {
                    format!("[UTC+{}] {}", offset, time)
                } else {
                    format!("[UTC{}] {}", offset, time)
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
        if !self.panel_app.panel_grid.is_split() {
            // overlay visibility was set in prepare_frame; read the state here.

            let toolbar_theme_for_overlay = self.panel_app.toolbar_theme_for_render();
            let overlay_state = &self.panel_app.indicator_overlay_state;
            if overlay_state.visible || overlay_state.is_open {
                let (symbol, compare_series): (String, Vec<(String, bool, String)>) =
                    self.panel_app.panel_grid.active_window()
                        .map(|w| {
                            let cs = w.compare_overlay.series.iter()
                                .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                                .collect();
                            (w.symbol.clone(), cs)
                        })
                        .unwrap_or_default();

                let instances = if let Some(chart_id) = self.panel_app.panel_grid.active_chart_id() {
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
                        use zengeld_chart::ui::z_order::ZLayer;

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
        let out_last_toolbar_result: Option<zengeld_chart::ChartToolbarRenderResult> = if !skip_toolbar_draw {
            let (active_sym_str, active_tf_str) = self.panel_app.panel_grid.active_window()
                .map(|w| (w.symbol.clone(), w.timeframe.name.clone()))
                .unwrap_or_default();
            let toolbar_result = self.panel_app.render_toolbars_with_theme(
                ctx,
                &panel_layout,
                selected_config.as_ref(),
                Some(clock_time.as_str()),
                self.split_without_group,
                Some(active_sym_str.as_str()),
                Some(active_tf_str.as_str()),
                sidebar_w,
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
            use zengeld_chart::ui::z_order::ZLayer;

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
                use zengeld_chart::ui::z_order::ZLayer;

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

        // 5. Render modals

        // Modal layout uses content_rect bounds so settings modals stay
        // within the chart area and never overlap the right sidebar.
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

        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let toolbar_state = ToolbarState::default();

        // Use a raw pointer to work around the borrow checker:
        // render_modals reads self.panel_app's modal state but also needs
        // &window.drawing_manager which lives in panel_grid (another field of panel_app).
        // Both fields are independent — no actual aliasing occurs.
        let dm_ptr: *const zengeld_chart::drawing::DrawingManager = self.panel_app.panel_grid
            .active_window()
            .map(|w| &w.drawing_manager as *const _)
            .unwrap_or(std::ptr::null::<zengeld_chart::drawing::DrawingManager>());

        // SAFETY: dm_ptr points into panel_grid which render_modals does not mutate.
        let dm_ref = if dm_ptr.is_null() {
            None
        } else {
            Some(unsafe { &*dm_ptr })
        };

        // Build ChartSettingsData from current panel_app state and theme.
        // Uses build_chart_settings_data() so legend/tooltip fields are read
        // from the actual window state instead of being hardcoded.
        let chart_settings_data = self.build_chart_settings_data();

        // Snapshot the theme_manager pointer so render_modals can borrow it.
        let theme_manager_ptr: *const zengeld_chart::theme::ThemeManager =
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
        );
        let mut out_frame_result: Option<ChartModalRenderResult> = Some(modal_result);

        // 5b. Render indicator / symbol / compare search modal (if open)
        let out_search_modal_result: Option<zengeld_chart::ModalSearchResult> = if self.modal_state.current.is_search_overlay() {
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

        // 5c. Render preset name input for CreateIndicatorSet mode AFTER search
        // overlay so it draws on top visually.
        if self.panel_app.preset_name_input.is_open
            && self.panel_app.preset_name_input.mode
                == zengeld_chart::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
        {
            use zengeld_chart::layout::modals::preset_name_input::render_preset_name_input;
            let pni_result = render_preset_name_input(
                ctx,
                w, h,
                &self.panel_app.preset_name_input,
                &frame_theme,
                &toolbar_theme,
                current_time_ms,
                &mut self.input_coordinator.borrow_mut(),
            );
            // Store in frame_result so click handlers can access it
            if let Some(ref mut fr) = out_frame_result {
                fr.preset_name_input = Some(pni_result);
            }
        }

        // 6. Render context menu (highest z-order after modals)
        let out_context_menu_result: Option<ContextMenuResult> = if self.panel_app.context_menu_state.is_open() {
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
        let out_last_sidebar_result: Option<sidebar_content::render::RightSidebarResult> = if self.sidebar_state.is_right_open() && !skeleton_active {
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
            let sidebar_result = sidebar_content::render::render_right_sidebar(
                ctx,
                &sidebar_rect,
                &mut self.sidebar_state,
                &sidebar_toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                &mut |item, rect, ctx| {
                    panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme);
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<String> {
                    use sidebar_content::free_slot::FreeItem;
                    use zengeld_panels::trading::SymbolSource;
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
                        FreeItem::Dom(id) => panels_store.dom.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::Footprint(id) => panels_store.footprint.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::VolumeProfile(id) => panels_store.volume_profile.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::BigTrades(id) => panels_store.big_trades.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::L2Tape(id) => panels_store.l2_tape.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::TradeTape(id) => panels_store.trade_tape.get(id)
                            .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::OrderEntry(id) => panels_store.order_entry.get(id)
                            .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::TradingContainer(id) => panels_store.trading_container.get(id)
                            .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                        FreeItem::PositionManager(_)
                        | FreeItem::TradeLog(_)
                        | FreeItem::RiskCalculator(_) => None,
                    }
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<(bool, f64, f64)> {
                    use sidebar_content::free_slot::FreeItem;
                    match item {
                        FreeItem::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter)),
                        _ => None,
                    }
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<[f32; 4]> {
                    use zengeld_chart::tag_manager::SyncMemberId;
                    let panel_id = item.panel_id().0;
                    let member = SyncMemberId::Panel(panel_id);
                    let gid = self.panel_app.tag_manager.group_for_member(member)?;
                    let group = self.panel_app.tag_manager.group(gid)?;
                    if group.auto_created { None } else { Some(group.color) }
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<sidebar_content::render::PanelSyncFlagsSnapshot> {
                    use zengeld_chart::tag_manager::SyncMemberId;
                    let panel_id = item.panel_id().0;
                    let member = SyncMemberId::Panel(panel_id);
                    let gid = self.panel_app.tag_manager.group_for_member(member)?;
                    let group = self.panel_app.tag_manager.group(gid)?;
                    Some(sidebar_content::render::PanelSyncFlagsSnapshot {
                        sync_symbol: group.effective_sync_symbol(member),
                        sync_crosshair: group.effective_sync_crosshair(member),
                    })
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<(String, String, String)> {
                    use sidebar_content::free_slot::FreeItem;
                    match item {
                        FreeItem::Dom(id) => panels_store.dom.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::Footprint(id) => panels_store.footprint.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::VolumeProfile(id) => panels_store.volume_profile.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::BigTrades(id) => panels_store.big_trades.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::L2Tape(id) => panels_store.l2_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::TradeTape(id) => panels_store.trade_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::OrderEntry(id) => panels_store.order_entry.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::TradingContainer(id) => panels_store.trading_container.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                        FreeItem::PositionManager(_)
                        | FreeItem::TradeLog(_)
                        | FreeItem::RiskCalculator(_) => None,
                    }
                },
                &|item: &sidebar_content::free_slot::FreeItem| -> Option<Vec<bool>> {
                    use sidebar_content::free_slot::FreeItem;
                    match item {
                        FreeItem::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                            s.column_config.show_bid_orders,
                            s.column_config.show_sell_trades,
                            s.column_config.show_buy_trades,
                            s.column_config.show_ask_orders,
                        ]),
                        FreeItem::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_type,
                            s.column_config.show_side,
                            s.column_config.show_price,
                            s.column_config.show_qty,
                        ]),
                        FreeItem::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_price,
                            s.column_config.show_size,
                        ]),
                        FreeItem::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
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

        // 8a. Render color picker popups AFTER the sidebar so they draw on top of it.
        // Panel-targeting sync color grid is skipped here — it is rendered in
        // render_panel_overlay_popups() after the sidebar scene is composited.
        if !self.panel_app.sync_color_grid.target_is_panel() {
            let cp_result = self.panel_app.render_color_picker_popups(
                ctx,
                &modal_layout,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
            );
            // Merge into frame_result — color picker and sync_color_grid fields only.
            if let Some(ref mut fr) = out_frame_result {
                if cp_result.color_picker.is_some() {
                    fr.color_picker = cp_result.color_picker;
                }
                if cp_result.sync_color_grid.is_some() {
                    fr.sync_color_grid = cp_result.sync_color_grid;
                }
            }
        }

        // 8b. Render watchlist modal if open (above sidebar, below context menu).
        let out_last_watchlist_modal_result: Option<zengeld_chart::layout::modals::watchlist_modal::WatchlistModalResult> = if self.watchlist_modal.is_open() {
            // Build WatchlistEntry items from sidebar_state.watchlist_items.
            // Pre-collect color flags so the iterator closure doesn't double-borrow self.
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

            let wl_modal_result = render_watchlist_modal(
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
            );
            Some(wl_modal_result)
        } else {
            None
        };

        // 8c. Render watchlist group name input modal (on top of watchlist modal)
        let out_last_wl_group_name_result: Option<WlGroupNameInputResult> = if self.wl_group_name_input.is_open() {
            let result = render_wl_group_name_input(
                ctx,
                modal_right_edge,
                h,
                &self.wl_group_name_input,
                &frame_theme,
                &toolbar_theme,
                current_time_ms,
                &mut self.input_coordinator.borrow_mut(),
            );
            Some(result)
        } else {
            None
        };

        // 9. End frame — collect widget responses (ignored for now)
        let _rt4 = std::time::Instant::now(); // checkpoint: after sidebar + modals
        let out_render_timing_us = (
            _rt2.duration_since(_rt1).as_micros() as u64, // chart
            _rt3.duration_since(_rt2).as_micros() as u64, // toolbar
            _rt4.duration_since(_rt3).as_micros() as u64, // sidebar + modals
            _rt1.duration_since(_rt0).as_micros() as u64, // layout + setup
        );
        let _responses = self.input_coordinator.borrow_mut().end_frame();

        RenderOutput {
            scale_corner_zones: out_scale_corner_zones,
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
        self.scale_corner_zones = output.scale_corner_zones;
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
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                if let Some(sp) = window.sub_panes.get_mut(pane_idx) {
                    if sp.auto_scale {
                        sp.price_min = min;
                        sp.price_max = max;
                    }
                }
            }
        }
        // Write back sub-pane overlay button rects for next frame's hit testing.
        for (leaf_id, overlays) in output.sub_pane_overlay_writebacks {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
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
                    let hex_id = WidgetId::new(text_input::HEX_COLOR);
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
                let chat_id = WidgetId::new(text_input::AGENT_CHAT);
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
                        if let sidebar_content::agent_types::AgentSnapshotMode::Chat(ref msgs) = snap.mode {
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

        let selected_config: Option<zengeld_chart::state::selected_config::SelectedPrimitiveConfig> =
            self.panel_app
                .panel_grid
                .active_window()
                .and_then(|w| w.drawing_manager.get_selected_config());

        let clock_time = {
            let utc_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Some(window) = self.panel_app.panel_grid.active_window() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                let offset = time_fmt.timezone_offset_hours;
                if offset >= 0 {
                    format!("[UTC+{}] {}", offset, time)
                } else {
                    format!("[UTC{}] {}", offset, time)
                }
            } else {
                let hours = (utc_secs / 3600) % 24;
                let minutes = (utc_secs / 60) % 60;
                let seconds = utc_secs % 60;
                format!("[UTC+0] {:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        };

        let (active_sym_str, active_tf_str) = self.panel_app.panel_grid.active_window()
            .map(|w| (w.symbol.clone(), w.timeframe.name.clone()))
            .unwrap_or_default();

        let toolbar_result = self.panel_app.render_toolbars_with_theme(
            ctx,
            &panel_layout,
            selected_config.as_ref(),
            Some(clock_time.as_str()),
            self.split_without_group,
            Some(active_sym_str.as_str()),
            Some(active_tf_str.as_str()),
            sidebar_w,
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
        let chat_id = WidgetId::new(text_input::AGENT_CHAT);
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
        let sidebar_result = sidebar_content::render::render_right_sidebar(
            ctx,
            &sidebar_rect,
            &mut self.sidebar_state,
            &sidebar_toolbar_theme,
            &mut self.input_coordinator.borrow_mut(),
            &mut |item, rect, ctx| {
                panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme);
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<String> {
                use sidebar_content::free_slot::FreeItem;
                use zengeld_panels::trading::SymbolSource;
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
                    FreeItem::Dom(id) => panels_store.dom.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::Footprint(id) => panels_store.footprint.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::VolumeProfile(id) => panels_store.volume_profile.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::BigTrades(id) => panels_store.big_trades.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::L2Tape(id) => panels_store.l2_tape.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::TradeTape(id) => panels_store.trade_tape.get(id)
                        .and_then(|s| label_from_fields(&s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::OrderEntry(id) => panels_store.order_entry.get(id)
                        .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::TradingContainer(id) => panels_store.trading_container.get(id)
                        .and_then(|s| label_from_source(&s.source, &s.symbol, &s.exchange, &s.account_type)),
                    FreeItem::PositionManager(_)
                    | FreeItem::TradeLog(_)
                    | FreeItem::RiskCalculator(_) => None,
                }
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<(bool, f64, f64)> {
                use sidebar_content::free_slot::FreeItem;
                match item {
                    FreeItem::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter)),
                    _ => None,
                }
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<[f32; 4]> {
                use zengeld_chart::tag_manager::SyncMemberId;
                let panel_id = item.panel_id().0;
                let member = SyncMemberId::Panel(panel_id);
                let gid = self.panel_app.tag_manager.group_for_member(member)?;
                let group = self.panel_app.tag_manager.group(gid)?;
                if group.auto_created { None } else { Some(group.color) }
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<sidebar_content::render::PanelSyncFlagsSnapshot> {
                use zengeld_chart::tag_manager::SyncMemberId;
                let panel_id = item.panel_id().0;
                let member = SyncMemberId::Panel(panel_id);
                let gid = self.panel_app.tag_manager.group_for_member(member)?;
                let group = self.panel_app.tag_manager.group(gid)?;
                Some(sidebar_content::render::PanelSyncFlagsSnapshot {
                    sync_symbol: group.effective_sync_symbol(member),
                    sync_crosshair: group.effective_sync_crosshair(member),
                })
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<(String, String, String)> {
                use sidebar_content::free_slot::FreeItem;
                match item {
                    FreeItem::Dom(id) => panels_store.dom.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::Footprint(id) => panels_store.footprint.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::VolumeProfile(id) => panels_store.volume_profile.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::BigTrades(id) => panels_store.big_trades.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::L2Tape(id) => panels_store.l2_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::TradeTape(id) => panels_store.trade_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::OrderEntry(id) => panels_store.order_entry.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::TradingContainer(id) => panels_store.trading_container.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
                    FreeItem::PositionManager(_)
                    | FreeItem::TradeLog(_)
                    | FreeItem::RiskCalculator(_) => None,
                }
            },
            &|item: &sidebar_content::free_slot::FreeItem| -> Option<Vec<bool>> {
                use sidebar_content::free_slot::FreeItem;
                match item {
                    FreeItem::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                        s.column_config.show_bid_orders,
                        s.column_config.show_sell_trades,
                        s.column_config.show_buy_trades,
                        s.column_config.show_ask_orders,
                    ]),
                    FreeItem::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_type,
                        s.column_config.show_side,
                        s.column_config.show_price,
                        s.column_config.show_qty,
                    ]),
                    FreeItem::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_price,
                        s.column_config.show_size,
                    ]),
                    FreeItem::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
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
            let chat_id = WidgetId::new(text_input::AGENT_CHAT);
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
    pub fn render_panel_overlay_popups(&self, ctx: &mut dyn RenderContext) {
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let w = self.width as f64;
        let h = self.height as f64;

        // Panel-targeting sync color grid popup.
        if self.panel_app.sync_color_grid.is_open() && self.panel_app.sync_color_grid.target_is_panel() {
            use zengeld_chart::ChartModalLayout;
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
            );
        }

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
    pub fn check_agent_hover(&mut self, chart_x: f64, chart_y: f64) -> bool {
        use sidebar_content::state::RightSidebarPanel;

        let agents_open = self.sidebar_state.is_right_open()
            && self.sidebar_state.right_panel == RightSidebarPanel::Agents;

        if !agents_open {
            if self.agent_pty_hover_focused {
                self.agent_pty_hover_focused = false;
                return true;
            }
            return false;
        }

        let inside = self
            .sidebar_state
            .agent_terminal_rect
            .map(|(rx, ry, rw, rh)| {
                let rx = rx as f64;
                let ry = ry as f64;
                let rw = rw as f64;
                let rh = rh as f64;
                chart_x >= rx && chart_x < rx + rw && chart_y >= ry && chart_y < ry + rh
            })
            .unwrap_or(false);

        if inside != self.agent_pty_hover_focused {
            self.agent_pty_hover_focused = inside;
            // Focus PTY field on hover if focused leaf is in PTY mode.
            let is_pty_leaf = self.sidebar_state.focused_agent_leaf
                .and_then(|id| self.sidebar_state.agent_leaves.get(&id))
                .map(|d| d.mode == gate4agent::InstanceMode::Pty)
                .unwrap_or(false);
            if inside && is_pty_leaf {
                self.input_coordinator.borrow_mut().text_fields_mut().focus(text_input::AGENT_PTY);
            }
            // Do NOT blur on cursor-leave — blur only on click outside. Otherwise
            // any tiny mouse movement during typing steals PTY focus mid-keystroke.
            return true;
        }
        false
    }

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
        group_id: zengeld_chart::tag_manager::SyncGroupId,
    ) {
        let (symbol, exchange, account_type, panel_ids) = {
            let group = match self.panel_app.tag_manager.group(group_id) {
                Some(g) => g,
                None => return,
            };
            (
                group.symbol.clone(),
                group.exchange.clone(),
                group.account_type.clone(),
                self.panel_app.tag_manager.panel_members(group_id),
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
            let panel_id = sidebar_content::free_slot::PanelId(*pid);
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
                        (Some(ob), Some(tr))
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
                    s.last_seen_orderbook_version = 0;
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
                    digdigdig3::ExchangeId::from_str(&exchange).map(|eid| {
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
                    s.last_seen_trade_version = 0;
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
                    digdigdig3::ExchangeId::from_str(&exchange).map(|eid| {
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
                    s.last_seen_trade_version = 0;
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
                    digdigdig3::ExchangeId::from_str(&exchange).map(|eid| {
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
                    s.last_seen_trade_version = 0;
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
                    digdigdig3::ExchangeId::from_str(&exchange).map(|eid| {
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
                    s.last_seen_version = 0;
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
            eprintln!(
                "[TagManager] unsubscribed depth for panel group {} ← {} @ {} ({})",
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
            eprintln!(
                "[TagManager] subscribed depth for panel group {} → {} @ {} ({})",
                group_id.0, symbol, exchange, account_type
            );
        }
    }

    /// Synchronise `window.sub_panes` with the external `IndicatorManager`.
    ///
    /// `ChartWindow.indicator_source` is always `NullIndicatorSource`, so the
    /// built-in `ChartWindow::sync_sub_panes()` always produces an empty list.
    /// This method replicates that logic but reads from `self.indicator_manager`
    /// (the real source of truth) instead.
    ///
    /// Call after any operation that adds/removes indicators or recalculates
    /// indicator values (init, tick, indicator create/delete).
    pub fn sync_sub_panes_from_manager(&mut self) {
        // Step 1: collect (leaf_id, chart_id_u64, symbol, visible_start, visible_end)
        // for every window — before taking mutable borrows on anything.
        let window_data: Vec<(zengeld_chart::LeafId, u64, String, usize, usize)> = self
            .panel_app
            .panel_grid
            .iter_windows()
            .map(|(leaf_id, window)| {
                let chart_id_val = self
                    .panel_app
                    .panel_grid
                    .chart_id_for_leaf(leaf_id)
                    .map(|cid| cid.0)
                    .unwrap_or(0);
                let (vs, ve) = window.viewport.visible_range();
                let ve = ve.min(window.bars.len());
                (leaf_id, chart_id_val, window.symbol.clone(), vs, ve)
            })
            .collect();

        // Step 2: for each window, collect sub-pane data scoped to that window's
        // chart_id so we only get the indicator instances belonging to this window.
        for (leaf_id, chart_id_val, symbol, visible_start, visible_end) in &window_data {
            // `pane > 0` means it lives in a separate sub-pane (not the main chart).
            // Collect ALL pane>0 instances regardless of visible — hidden panes
            // must keep their SubPane struct (position, height_ratio, above_main).
            // Visibility filtering happens in the render pipeline, not here.
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

            // Step 3: apply to the window — no borrow on indicator_manager remains.
            let window = match self.panel_app.panel_grid.window_for_leaf_mut(*leaf_id) {
                Some(w) => w,
                None => continue,
            };

            // Grab any pending restore data for this window (from a preset restore).
            let pending_ratios = self.pending_sub_pane_ratios.get(&window.id.0).cloned();
            let pending_above_main = self.pending_sub_pane_above_main.get(&window.id.0).cloned();
            let pending_order = self.pending_sub_pane_order.get(&window.id.0).cloned();

            // Build a lookup from instance_id → price range for new panes.
            let range_map: std::collections::HashMap<u64, Option<(f64, f64)>> =
                sub_pane_data.iter().map(|(id, r)| (*id, *r)).collect();

            // Build new_sub_panes.
            //
            // When a saved order exists, honour it: iterate saved order first,
            // then append any new instance_ids not present in the saved order.
            // When no saved order exists, preserve current Vec order for existing
            // panes and append newly-appeared panes at the end (original logic).
            let mut new_sub_panes: Vec<zengeld_chart::state::SubPane> =
                Vec::with_capacity(sub_pane_data.len());
            let mut used_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

            // Helper: build / update a single SubPane.
            let build_pane = |instance_id: u64,
                              existing: Option<&zengeld_chart::state::SubPane>,
                              pending_ratios: &Option<std::collections::HashMap<u64, f32>>,
                              pending_above_main: &Option<std::collections::HashSet<u64>>,
                              range_map: &std::collections::HashMap<u64, Option<(f64, f64)>>|
             -> zengeld_chart::state::SubPane {
                let mut pane = existing
                    .cloned()
                    .unwrap_or_else(|| zengeld_chart::state::SubPane::new(instance_id));

                // Apply saved price range when creating a brand-new pane.
                if existing.is_none() {
                    if let Some(Some((p_min, p_max))) = range_map.get(&instance_id) {
                        pane.price_min = *p_min;
                        pane.price_max = *p_max;
                    }
                }
                // Apply saved height ratio.
                if let Some(ref ratios) = pending_ratios {
                    if let Some(&ratio) = ratios.get(&instance_id) {
                        pane.height_ratio = ratio;
                    }
                }
                // Apply saved above_main flag.
                if let Some(ref above_set) = pending_above_main {
                    pane.above_main = above_set.contains(&instance_id);
                }
                pane
            };

            // Build a quick lookup for existing sub_panes.
            let existing_map: std::collections::HashMap<u64, &zengeld_chart::state::SubPane> =
                window.sub_panes.iter().map(|p| (p.instance_id, p)).collect();

            if let Some(ref order) = pending_order {
                // Restore saved order first.
                for &iid in order {
                    if range_map.contains_key(&iid) {
                        let pane = build_pane(
                            iid,
                            existing_map.get(&iid).copied(),
                            &pending_ratios,
                            &pending_above_main,
                            &range_map,
                        );
                        new_sub_panes.push(pane);
                        used_ids.insert(iid);
                    }
                }
            } else {
                // No saved order: keep existing Vec order for panes that are
                // still present in sub_pane_data, or are hidden.
                for existing in &window.sub_panes {
                    if range_map.contains_key(&existing.instance_id) {
                        let pane = build_pane(
                            existing.instance_id,
                            Some(existing),
                            &pending_ratios,
                            &pending_above_main,
                            &range_map,
                        );
                        new_sub_panes.push(pane);
                        used_ids.insert(existing.instance_id);
                    }
                }
            }

            // Append any new panes not covered by the ordering pass.
            for (instance_id, _) in &sub_pane_data {
                if used_ids.contains(instance_id) {
                    continue;
                }
                let pane = build_pane(
                    *instance_id,
                    existing_map.get(instance_id).copied(),
                    &pending_ratios,
                    &pending_above_main,
                    &range_map,
                );
                new_sub_panes.push(pane);
            }

            // Update indices.
            for (i, pane) in new_sub_panes.iter_mut().enumerate() {
                pane.index = i;
            }
            window.sub_panes = new_sub_panes;
        }

        // Clear all pending restore state once applied (one-shot).
        self.pending_sub_pane_ratios.clear();
        self.pending_sub_pane_above_main.clear();
        self.pending_sub_pane_order.clear();
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

        let leaf_rects: Vec<(zengeld_chart::LeafId, LayoutRect)> = self
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

        let extended_layouts: Vec<(zengeld_chart::LeafId, zengeld_chart::ExtendedFrameLayout)> =
            leaf_rects
                .iter()
                .filter_map(|&(leaf_id, leaf_rect)| {
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect)?;
                    Some((leaf_id, extended))
                })
                .collect();

        // Phase 2: apply geometry to sub_panes using &mut self.
        for (leaf_id, extended) in &extended_layouts {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(*leaf_id) {
                for layout_sp in &extended.sub_panes {
                    if let Some(sp) = window
                        .sub_panes
                        .iter_mut()
                        .find(|s| s.instance_id == layout_sp.instance_id)
                    {
                        sp.height = layout_sp.content.height as f32;
                        sp.y_offset = layout_sp.content.y as f32;
                        sp.chart_width = layout_sp.content.width as f32;
                    }
                }
            }
        }
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
        watchlist_manager: &sidebar_content::watchlist::WatchlistManager,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<live_data::SymbolInfo>>,
    ) -> Vec<zengeld_chart::ui::modal_state::SearchResult> {
        use zengeld_chart::ui::modal_state::SearchResult;

        let q = query.to_lowercase();

        if !exchange_symbols.is_empty() {
            let mut results: Vec<SearchResult> = Vec::new();

            for (exchange_id, symbols) in exchange_symbols {
                // Canonical lowercase slug — used for both storage and display.
                // The display layer can uppercase it when rendering if desired.
                let exchange_slug = exchange_id.as_str();

                let filtered: Vec<&live_data::SymbolInfo> = symbols
                    .iter()
                    .filter(|s| {
                        if q.is_empty() {
                            true
                        } else {
                            s.symbol.to_lowercase().contains(&q)
                                || s.base_asset.to_lowercase().contains(&q)
                        }
                    })
                    .collect();

                for s in filtered.iter().take(100) {
                    let in_watchlist = watchlist_manager.contains_with_type(&s.symbol, exchange_slug, s.account_type.short_label());
                    let asset_type = if exchange_slug == "moex" { "Stock" } else { "Crypto" };
                    let category_icon = if exchange_slug == "moex" { "S" } else { "C" };
                    results.push(SearchResult {
                        symbol: s.symbol.clone(),
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
        watchlist_manager: &sidebar_content::watchlist::WatchlistManager,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<live_data::SymbolInfo>>,
    ) -> Vec<zengeld_chart::ui::modal_state::SearchResult> {
        Self::build_symbol_search_results(query, watchlist_manager, exchange_symbols)
    }

    /// Returns `true` when the drawing manager is mid-drawing (first point placed,
    /// waiting for the user to place the next point).
    ///
    /// Used by the winit runner to call `SetCapture` so that `CursorMoved` events
    /// continue arriving even when the cursor leaves the window boundary.
    pub fn is_drawing(&self) -> bool {
        self.panel_app
            .panel_grid
            .active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false)
    }

    /// Returns true when a click-based (non-freehand) drawing tool is selected.
    /// Used by the runner to always route mouse-release to `on_click` instead of
    /// `on_drag_end`, so accidental micro-drags don't swallow the click.
    pub fn has_click_drawing_tool(&self) -> bool {
        self.panel_app
            .panel_grid
            .active_window()
            .map(|w| {
                w.drawing_manager.current_tool().is_some()
                    && !w.drawing_manager.is_freehand_tool()
            })
            .unwrap_or(false)
    }

    /// Apply `ChartOutputAction` results to the active `ChartWindow`.
    ///
    /// Mirrors `TerminalApp::process_output_actions` but simplified:
    /// no multi-window panel manager, no frame layout offset.
    pub(crate) fn process_output_actions(&mut self, actions: Vec<ChartOutputAction>) {
        fn calc_visible_price_range(window: &mut zengeld_chart::ChartWindow) {
            window.calc_auto_scale();
        }

        // Compute chart layout ONCE for coordinate conversion.
        // All actions from DefaultChartInputHandler use screen-absolute coords;
        // viewport/crosshair expect chart-local coords (0,0 = top-left of chart canvas).
        let extended = self.build_extended_layout();
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
                    let Some(window) = self.panel_app.panel_grid.active_window_mut() else { continue; };
                    let bar_delta_bars = bar_delta / window.viewport.bar_spacing;
                    window.viewport.pan(bar_delta_bars);
                    if let zengeld_chart::engine::input::DragMode::SubPaneChart { pane_index } = drag_mode {
                        if let Some(sub_pane) = window.sub_panes.get_mut(pane_index) {
                            if !sub_pane.auto_scale {
                                let price_range = sub_pane.price_max - sub_pane.price_min;
                                let pane_height = sub_pane.height as f64;
                                let price_delta_scaled = price_delta * price_range / pane_height;
                                sub_pane.price_min += price_delta_scaled;
                                sub_pane.price_max += price_delta_scaled;
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
                    let Some(window) = self.panel_app.panel_grid.active_window_mut() else { continue; };
                    if factor_x != 1.0 {
                        window.viewport.zoom_at(local_center_x, factor_x);
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
                    // Propagate viewport change to sync group (horizontal zoom only).
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
                }
                ChartOutputAction::ResetPriceScale => {
                    let Some(window) = self.panel_app.panel_grid.active_window_mut() else { continue; };
                    if let zengeld_chart::engine::input::DragMode::SubPanePriceScale { pane_index } = drag_mode {
                        if let Some(sub_pane) = window.sub_panes.get_mut(pane_index) {
                            sub_pane.auto_scale = true;
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
                    let Some(window) = self.panel_app.panel_grid.active_window_mut() else { continue; };
                    window.viewport.reset_to_default();
                    window.price_scale.scale_mode = ScaleMode::Auto;
                    calc_visible_price_range(window);
                }
                ChartOutputAction::TogglePriceScaleMode => {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
                            let snapshot = self.panel_app
                                .panel_grid
                                .active_window()
                                .and_then(|w| {
                                    let type_id = w.drawing_manager.get_type_id_at(index)?;
                                    let points = w.drawing_manager.get_points_at(index)?;
                                    let data = w.drawing_manager.get_data_at(index)?;
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
                            eprintln!("[Undo] PrimitiveDeleted at index {} — deletion undo handled at call site", index);
                            None
                        }
                    };
                    if let Some(command) = cmd {
                        self.push_undo_command(command);
                    }
                }

                // ── Drawing interaction ─────────────────────────────────────
                ChartOutputAction::SelectPrimitive { id } => {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        match id {
                            Some(primitive_id) => {
                                if let Some(idx) = window.drawing_manager.find_index_by_id(primitive_id) {
                                    window.drawing_manager.select_by_index(idx);
                                }
                            }
                            None => {
                                window.drawing_manager.deselect();
                            }
                        }
                    }
                }
                ChartOutputAction::StartPrimitiveDrag { id, bar: screen_x, price: screen_y } => {
                    // `bar` and `price` from DefaultChartInputHandler are raw screen (x, y).
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    // For main-chart primitives use chart_x/chart_y/chart_h;
                    // for sub-pane primitives use the sub-pane content rect and price range.
                    let primitive_pane_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            let idx = w.drawing_manager.find_index_by_id(id)?;
                            w.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
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
                        if let Some(w) = self.panel_app.panel_grid.active_window() {
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

                    // Capture the primitive's points BEFORE drag so EndPrimitiveDrag
                    // can record a MovePrimitive command with before/after points.
                    let pre_drag = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            let idx = w.drawing_manager.find_index_by_id(id)?;
                            let pts = w.drawing_manager.get_points_at(idx)?;
                            Some((idx, pts))
                        });
                    self.drag_start_points = pre_drag;

                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                            window.drawing_manager.start_drag(idx, data_bar, data_price);
                        }
                    }
                }
                ChartOutputAction::StartControlPointDrag { primitive_id, control_point, bar: screen_x, price: screen_y } => {
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    let primitive_pane_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            let idx = w.drawing_manager.find_index_by_id(primitive_id)?;
                            w.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
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
                        if let Some(w) = self.panel_app.panel_grid.active_window() {
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
                    let pre_drag = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            let idx = w.drawing_manager.find_index_by_id(primitive_id)?;
                            let pts = w.drawing_manager.get_points_at(idx)?;
                            Some((idx, pts))
                        });
                    self.drag_start_points = pre_drag;

                    // Use start_control_point_drag (not start_drag) so DragType is
                    // ControlPoint — this makes update_drag resize the primitive
                    // instead of moving it.
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(idx) = window.drawing_manager.find_index_by_id(primitive_id) {
                            window.drawing_manager.start_control_point_drag(idx, control_point, data_bar, data_price);
                        }
                    }
                }
                ChartOutputAction::UpdatePrimitiveDrag { bar: screen_x, price: screen_y } => {
                    // Use the dragging primitive's pane_id for coordinate conversion.
                    let primitive_pane_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            let idx = w.drawing_manager.dragging()?;
                            w.drawing_manager.get_data_at(idx).map(|d| d.pane_id)
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
                        if let Some(w) = self.panel_app.panel_grid.active_window() {
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
                    let dragged_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.drawing_manager.dragging_primitive_id());

                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.update_drag(data_bar, data_price);
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
                    // Snapshot the dragged primitive's id BEFORE end_drag() clears it.
                    let dragged_prim_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.drawing_manager.dragging_primitive_id());

                    // Collect move data BEFORE ending the drag, while we still
                    // know which primitive was being dragged.
                    let move_cmd = if let Some((idx, ref previous_points)) = self.drag_start_points {
                        self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.get_points_at(idx))
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

                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.end_drag();
                    }
                    if let Some(cmd) = move_cmd {
                        self.push_undo_command(cmd);
                    }
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = window.bars.clone();
                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                    // Record CreatePrimitive if finish_multipoint creates a new primitive.
                    let prev_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.count())
                        .unwrap_or(0);

                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.finish_multipoint();
                    }

                    let new_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.count())
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = snapshot {
                            self.push_undo_command(Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let bars = window.bars.clone();
                                window.drawing_manager.update_all_timestamps_from_bars(&bars);
                            }
                            self.autosave_snapshot();
                        }
                    }
                }
                ChartOutputAction::DrawingClick { bar, price, .. } => {
                    // Record CreatePrimitive if the click completes a new primitive.
                    let prev_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.count())
                        .unwrap_or(0);

                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.on_click(bar, price);
                    }

                    let new_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.count())
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = snapshot {
                            self.push_undo_command(Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let bars = window.bars.clone();
                                window.drawing_manager.update_all_timestamps_from_bars(&bars);
                            }
                            self.autosave_snapshot();
                        }
                    }
                }

                ChartOutputAction::ZoomSubPane { pane_index, delta_y } => {
                    let Some(window) = self.panel_app.panel_grid.active_window_mut() else { continue; };
                    if let Some(sub_pane) = window.sub_panes.get_mut(pane_index) {
                        sub_pane.auto_scale = false;
                        let pane_h = sub_pane.height as f64;
                        if pane_h > 0.0 {
                            // 1:1 pixel-to-zoom: each pixel of drag = proportional range change
                            let factor = 1.0 + delta_y / pane_h;
                            let center = (sub_pane.price_min + sub_pane.price_max) / 2.0;
                            let half_range = (sub_pane.price_max - sub_pane.price_min) / 2.0 * factor;
                            sub_pane.price_min = center - half_range;
                            sub_pane.price_max = center + half_range;
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
        source_leaf: zengeld_chart::LeafId,
        primitive_id: u64,
    ) -> bool {
        let group_id = self.panel_app.panel_grid
            .window_for_leaf(source_leaf)
            .and_then(|w| w.group_id);
        let group_id = match group_id {
            Some(gid) => gid,
            None => return false,
        };

        // Respect the sync_drawings flag — if disabled, do not write drag result back to group.
        if let Some(group) = self.panel_app.tag_manager.group(group_id) {
            if !group.sync_flags.sync_drawings {
                return false;
            }
        }

        // Clone the updated primitive from the window's drawing_manager.
        let chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return false,
        };
        let updated_prim = self.panel_app.panel_grid
            .windows()
            .get(&chart_id)
            .and_then(|w| {
                w.drawing_manager.primitives().iter()
                    .find(|p| p.data().id == primitive_id)
                    .map(|p| p.clone_box())
            });
        let updated_prim = match updated_prim {
            Some(p) => p,
            None => return false,
        };

        // Replace the primitive in the group by id.
        if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
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
        source_leaf: zengeld_chart::LeafId,
        primitive_id: u64,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && input::sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Phase 1: read the current points from the source window (immutable borrow).
        let source_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return,
        };
        let new_points: Vec<(f64, f64)> = match self.panel_app.panel_grid
            .windows()
            .get(&source_chart_id)
            .and_then(|w| w.drawing_manager.get_points_by_id(primitive_id))
        {
            Some(pts) => pts,
            None => return,
        };

        // Phase 2: update each peer window's synced clone (mutable borrows, one at a time).
        for peer_leaf in peer_leaves {
            let peer_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(peer_leaf) {
                Some(id) => id,
                None => continue,
            };
            if let Some(peer_window) = self.panel_app.panel_grid.windows_mut().get_mut(&peer_chart_id) {
                peer_window.drawing_manager.update_synced_primitive_points(primitive_id, &new_points);
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
        source_leaf: zengeld_chart::LeafId,
        view_start: f64,
        bar_spacing: f64,
        scale_mode: Option<zengeld_chart::ScaleMode>,
    ) {
        let should_sync = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf)
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.sync_flags.sync_viewport)
            .unwrap_or(true);
        if !should_sync { return; }

        // Compute source visible bar count — the metric we actually sync.
        let (source_chart_width, anchor_ts, frac) = match self.panel_app.panel_grid.window_for_leaf(source_leaf) {
            Some(w) => {
                let ts = if w.bars.is_empty() {
                    None
                } else {
                    let idx = (view_start.floor() as usize).min(w.bars.len().saturating_sub(1));
                    Some(w.bars[idx].timestamp)
                };
                (w.viewport.chart_width, ts, view_start - view_start.floor())
            }
            None => return,
        };
        let visible_bars = if bar_spacing > 0.0 { source_chart_width / bar_spacing } else { 100.0 };

        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && input::sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                // Apply scale_mode BEFORE the auto-scale check so the new mode takes effect.
                if let Some(mode) = scale_mode {
                    window.price_scale.scale_mode = mode;
                    let is_auto = mode.is_auto_y();
                    for sp in &mut window.sub_panes {
                        sp.auto_scale = is_auto;
                    }
                }
                // Time-based sync: convert source timestamp to peer's bar index so
                // windows with different bar counts land on the same calendar date.
                let peer_view_start = anchor_ts
                    .and_then(|ts| zengeld_chart::find_bar_for_timestamp(&window.bars, ts))
                    .map(|idx| idx as f64 + frac)
                    .unwrap_or(view_start);
                window.viewport.view_start = peer_view_start;
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
                window.viewport.bar_spacing = peer_spacing;
                if window.price_scale.scale_mode.is_auto_y() {
                    window.calc_auto_scale();
                }
            }
        }
    }

    /// Convert raw screen coordinates to data coordinates (bar, price).
    ///
    /// When `pane_id` is `Some(instance_id)` the conversion uses the sub-pane's
    /// content rect (for local_y) and its price range.  When `pane_id` is `None`
    /// the main-chart coordinate system is used.
    ///
    /// The bar index is always derived from the main chart's viewport X-axis since
    /// all panes share the same time axis.
    fn screen_to_data_coords(
        &self,
        screen_x: f64,
        screen_y: f64,
        pane_id: Option<u64>,
        extended: &ExtendedFrameLayout,
        chart_x: f64,
        chart_y: f64,
        chart_h: f64,
    ) -> (f64, f64) {
        // X-axis is shared across all panes — always convert using main chart origin.
        let local_x = screen_x - chart_x;

        let Some(window) = self.panel_app.panel_grid.active_window() else {
            return (screen_x, screen_y);
        };
        // Snap to bar center (matching crosshair coordinate system).
        let bar = if let Some(idx) = window.viewport.x_to_bar(local_x) {
            idx as f64
        } else {
            window.viewport.x_to_bar_f64(local_x)
        };

        let price = if let Some(instance_id) = pane_id {
            // Sub-pane: find the pane layout rect and price range.
            if let Some(pane_layout) = extended.sub_panes.iter()
                .find(|p| p.instance_id == instance_id)
            {
                let content = pane_layout.content;
                let local_y = screen_y - content.y;
                let (p_min, p_max) = window.sub_panes.iter()
                    .find(|sp| sp.instance_id == instance_id)
                    .map(|sp| (sp.price_min, sp.price_max))
                    .unwrap_or((0.0, 100.0));
                let pane_h = content.height;
                if pane_h > 0.0 {
                    p_max - (local_y / pane_h) * (p_max - p_min)
                } else {
                    p_min
                }
            } else {
                // Fallback: use main chart coordinate system.
                let local_y = screen_y - chart_y;
                window.price_scale.y_to_price(local_y, chart_h)
            }
        } else {
            // Main chart.
            let local_y = screen_y - chart_y;
            window.price_scale.y_to_price(local_y, chart_h)
        };

        (bar, price)
    }

    // -------------------------------------------------------------------------
    // User profile persistence
    // -------------------------------------------------------------------------

    /// Map a [`sidebar_content::state::RightSidebarPanel`] to a string name.
    pub fn panel_to_str(panel: sidebar_content::state::RightSidebarPanel) -> Option<String> {
        use sidebar_content::state::RightSidebarPanel;
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

    /// Parse a string name into a [`sidebar_content::state::RightSidebarPanel`].
    pub fn str_to_panel(s: &str) -> sidebar_content::state::RightSidebarPanel {
        use sidebar_content::state::RightSidebarPanel;
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

    /// Collect the current app state into a [`UserProfile`] snapshot.
    ///
    /// Only lightweight metadata is captured here.  Heavy data (chart presets,
    /// templates, watchlists) are stored in separate files managed by their
    /// own sub-systems.
    pub fn build_user_profile(&self) -> zengeld_chart::UserProfile {
        // Preserve device identity and telemetry from the currently loaded
        // profile so that we don't clobber counters on every save.
        let existing = &self.panel_app.user_manager.profile;
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            zengeld_chart::InlineDockEdge::Bottom => "Bottom",
            zengeld_chart::InlineDockEdge::Top => "Top",
            zengeld_chart::InlineDockEdge::Free => "Free",
        };
        zengeld_chart::UserProfile {
            version: zengeld_chart::user_profile::profile::PROFILE_VERSION,
            active_preset_id: self.panel_app.active_preset_id.clone(),
            open_tabs: self.panel_app.open_tabs.clone(),
            active_theme: self.panel_app.theme_manager.preset_name().to_string(),
            sidebar_visible: self.sidebar_state.is_right_open(),
            sidebar_panel: Self::panel_to_str(self.sidebar_state.right_panel),
            sidebar_width: Some(self.sidebar_state.right_sidebar_width),
            inline_bar_x: Some(inline.x),
            inline_bar_y: Some(inline.y),
            inline_bar_dock: Some(inline_dock_str.to_string()),
            // Preserve profile identity fields managed by the profile system
            profile_id: existing.profile_id.clone(),
            display_name: existing.display_name.clone(),
            avatar: existing.avatar.clone(),
            profile_created_at: existing.profile_created_at,
            // Preserve fields managed by the profile itself
            device_name: existing.device_name.clone(),
            app_version: existing.app_version.clone(),
            linked_account: existing.linked_account.clone(),
            telemetry: existing.telemetry.clone(),
            bar_count: existing.bar_count,
            data_load: existing.data_load.clone(),
            recalc_mode: existing.recalc_mode.clone(),
            language: self.panel_app.user_settings_state.language.clone(),
            scale_mode: match self.default_scale_mode {
                ScaleMode::Auto   => "Auto".to_string(),
                ScaleMode::Focus  => "Focus".to_string(),
                ScaleMode::Manual => "Manual".to_string(),
            },
            cloud_enabled: existing.cloud_enabled,
            sync_level: existing.sync_level.clone(),
            ota_enabled: self.panel_app.user_settings_state.ota_enabled,
            server_enabled: self.panel_app.user_settings_state.server_enabled,
            server_port: self.panel_app.user_settings_state.server_port,
            legacy_single_agent_key: String::new(),
            local_agent_keys: existing.local_agent_keys.clone(),
            exchange_keys: existing.exchange_keys.clone(),
            connector_enabled: self.sidebar_state.connector_enabled.clone(),
            notification_settings: existing.notification_settings.clone(),
            windows: existing.windows.clone(),
            sync_state: {
                let ui = &self.panel_app.user_settings_state;
                zengeld_chart::user_profile::profile::SyncState {
                    enabled: ui.sync_enabled,
                    last_sync_timestamp: existing.sync_state.last_sync_timestamp,
                    sync_vault: true,
                    sync_presets: ui.sync_presets,
                    sync_templates: ui.sync_templates,
                    sync_watchlists: ui.sync_watchlists,
                    sync_theme: ui.sync_theme_toggle,
                    sync_recovery_key: true,
                    // Preserve the synced_items set — it is managed by the updater
                    // loop and must not be reset when the user changes settings.
                    synced_items: existing.sync_state.synced_items.clone(),
                    // Preserve the last-synced checksum map — managed by the updater
                    // loop and written back to the profile for cross-restart persistence.
                    last_synced_checksums: existing.sync_state.last_synced_checksums.clone(),
                }
            },
        }
    }

    /// Update in-memory profile state only.
    ///
    /// DEPRECATED: Disk writes are handled exclusively by `App::save_all()` in
    /// `main.rs`, which coordinates all windows before writing.  Calling this
    /// function from an individual window would write a stale `windows` list
    /// (because each window only knows its own state) and would clobber the
    /// correct multi-window state assembled by `save_all()`.
    ///
    /// This function now only refreshes the in-memory profile.  No file I/O is
    /// performed here.
    pub fn save_user_profile(&mut self) {
        // Only set dirty flags — actual disk writes are done by App
        // which has full context of all windows.
        self.profile_dirty = true;
        self.watchlists_dirty = true;
    }

    /// Set this window's unique identifier.  Call immediately after construction
    /// to override the auto-generated "win_<timestamp>" default.
    pub fn set_window_id(&mut self, id: String) {
        self.window_id = id;
    }

    /// Build a lightweight snapshot of this window's per-window state.
    ///
    /// Captures the `window_id`, the list of open tab preset IDs, and the
    /// currently active preset ID.  Used by the coordinated multi-window save
    /// in `main.rs` so that every OS window's state is stored in
    /// [`zengeld_chart::UserProfile::windows`] before the profile is written.
    pub fn build_window_state(&self) -> zengeld_chart::WindowState {
        let inline = &self.panel_app.toolbar_state.floating_inline_bar;
        let inline_dock_str = match inline.dock_edge {
            zengeld_chart::InlineDockEdge::Bottom => "Bottom",
            zengeld_chart::InlineDockEdge::Top => "Top",
            zengeld_chart::InlineDockEdge::Free => "Free",
        };
        let agents_tab_layout = {
            let tree = self.sidebar_state.agent_docking.inner().tree();
            uzor::panels::serialize::LayoutSnapshot::from_tree(tree, "agents")
                .to_json()
                .ok()
        };
        let agents_tab_leaves: Vec<zengeld_chart::PersistedAgentLeaf> = self
            .sidebar_state
            .agent_leaves
            .iter()
            .map(|(leaf_id, desc)| zengeld_chart::PersistedAgentLeaf {
                leaf_id: leaf_id.0,
                cli: match desc.cli {
                    gate4agent::AgentCli::Claude => zengeld_chart::PersistedAgentCli::Claude,
                    gate4agent::AgentCli::Codex => zengeld_chart::PersistedAgentCli::Codex,
                    gate4agent::AgentCli::Gemini => zengeld_chart::PersistedAgentCli::Gemini,
                    gate4agent::AgentCli::OpenCode => zengeld_chart::PersistedAgentCli::OpenCode,
                },
                mode: match desc.mode {
                    gate4agent::InstanceMode::Pty => zengeld_chart::PersistedInstanceMode::Pty,
                    gate4agent::InstanceMode::Chat => zengeld_chart::PersistedInstanceMode::Chat,
                },
                workdir: desc.workdir.clone(),
                chat_session_id: desc.chat_session_id.clone(),
            })
            .collect();
        // Log agents state for diagnostics (appears in structured log).
        log::info!(
            "[agents-diag] build_window_state: agents_layout={} agents_leaves={}",
            if agents_tab_layout.is_some() { "Some" } else { "None" },
            agents_tab_leaves.len(),
        );
        zengeld_chart::WindowState {
            window_id: self.window_id.clone(),
            open_tabs: self.panel_app.open_tabs.clone(),
            active_preset_id: self.panel_app.active_preset_id.clone(),
            x: self.window_x,
            y: self.window_y,
            width: self.window_width,
            height: self.window_height,
            sidebar_visible: self.sidebar_state.is_right_open(),
            sidebar_panel: Self::panel_to_str(self.sidebar_state.right_panel),
            sidebar_width: Some(self.sidebar_state.right_sidebar_width),
            inline_bar_x: Some(inline.x),
            inline_bar_y: Some(inline.y),
            inline_bar_dock: Some(inline_dock_str.to_string()),
            agents_tab_layout,
            agents_tab_leaves,
        }
    }

    /// Update the in-memory profile's windows list.  Call this before
    /// `save_user_profile()` when multiple OS windows are open.
    pub fn set_profile_windows(&mut self, windows: Vec<zengeld_chart::WindowState>) {
        self.panel_app.user_manager.profile.windows = windows;
    }

    // =========================================================================
    // Granular persistence helpers — call after each mutation
    // =========================================================================

    /// Persist the user profile (active_preset_id, sidebar state, inline bar, device, telemetry).
    ///
    /// Only sets the dirty flag — App monitors this and saves with full
    /// multi-window context.  Windows must never write profile.json
    /// directly because they don't know about other windows.
    pub fn persist_profile(&mut self) {
        self.profile_dirty = true;
    }

    /// Park the active preset's live state into the cache.
    /// Active fields are replaced with cheap placeholders.
    fn park_active_preset(&mut self, id: &str) {
        let state = preset_cache::LivePresetState {
            panel_grid: std::mem::replace(
                &mut self.panel_app.panel_grid,
                zengeld_chart::state::panel_grid::ChartPanelGrid::placeholder(),
            ),
            tag_manager: std::mem::replace(
                &mut self.panel_app.tag_manager,
                zengeld_chart::tag_manager::TagManager::new(),
            ),
            indicator_manager: std::mem::replace(
                &mut self.indicator_manager,
                IndicatorManager::new(),
            ),
            alert_manager: std::mem::replace(
                &mut self.alert_manager,
                alerts::AlertManager::new(),
            ),
            leaf_color_tags: std::mem::take(&mut self.panel_app.leaf_color_tags),
            indicator_overlay_states: std::mem::take(&mut self.panel_app.indicator_overlay_states),
            series_handles: std::mem::take(&mut self.series_handles),
            pending_sub_pane_ratios: std::mem::take(&mut self.pending_sub_pane_ratios),
            pending_sub_pane_above_main: std::mem::take(&mut self.pending_sub_pane_above_main),
            pending_sub_pane_order: std::mem::take(&mut self.pending_sub_pane_order),
            needs_initial_viewport_fit: self.needs_initial_viewport_fit,
            slot_dockings: std::mem::replace(
                &mut self.sidebar_state.slot_dockings,
                std::array::from_fn(|_| sidebar_content::SlotDockingManager::new()),
            ),
            panels_store: std::mem::replace(
                &mut self.panels_store,
                crate::panels_store::TradingPanelsStore::new(),
            ),
            focused_free_leaf: self.sidebar_state.focused_free_leaf.take(),
        };
        self.live_preset_cache.insert(id.to_string(), state);
        eprintln!("[ChartApp] Parked preset '{}' into live cache ({} total cached)", id, self.live_preset_cache.len());
    }

    /// Unpack a cached preset into active fields. Returns true on cache hit.
    fn unpark_preset(&mut self, id: &str) -> bool {
        let Some(state) = self.live_preset_cache.remove(id) else { return false; };
        self.panel_app.panel_grid = state.panel_grid;
        self.panel_app.tag_manager = state.tag_manager;
        self.indicator_manager = state.indicator_manager;
        self.alert_manager = state.alert_manager;
        self.panel_app.leaf_color_tags = state.leaf_color_tags;
        self.panel_app.indicator_overlay_states = state.indicator_overlay_states;
        self.series_handles = state.series_handles;
        self.pending_sub_pane_ratios = state.pending_sub_pane_ratios;
        self.pending_sub_pane_above_main = state.pending_sub_pane_above_main;
        self.pending_sub_pane_order = state.pending_sub_pane_order;
        self.needs_initial_viewport_fit = state.needs_initial_viewport_fit;
        self.sidebar_state.slot_dockings = state.slot_dockings;
        self.panels_store = state.panels_store;
        self.sidebar_state.focused_free_leaf = state.focused_free_leaf;
        self.sidebar_data_dirty = true;
        eprintln!("[ChartApp] Unpacked preset '{}' from live cache", id);
        true
    }

    /// Persist watchlists to disk.
    ///
    /// Only sets the dirty flag — App saves watchlists from AppState
    /// (the single source of truth shared across all windows).
    pub fn persist_watchlists(&mut self) {
        self.watchlists_dirty = true;
    }

    /// Persist templates to disk.
    pub fn persist_templates(&self) {
        if let Err(e) = self.panel_app.template_manager.save_to_default_dir(None) {
            eprintln!("[persist] templates: {:?}", e);
        }
    }

    /// Load and apply a previously saved user profile from `user_data/profile.json`.
    ///
    /// Also restores the [`sidebar_content::watchlist::WatchlistManager`] from
    /// `user_data/watchlists.json` when that file exists.
    ///
    /// Missing files are silently ignored so that a fresh install with no
    /// saved data still starts correctly.
    pub fn load_user_profile(&mut self) {
        // Load profile metadata.
        match zengeld_chart::load_profile(None) {
            Ok(profile) => {
                // Restore active preset id.
                self.panel_app.active_preset_id = profile.active_preset_id;

                // Restore sidebar width first (before opening, so the correct
                // width is applied when the panel is opened).
                if let Some(width) = profile.sidebar_width {
                    self.sidebar_state.set_right_width(width);
                }

                // Restore the open panel (or leave closed if None/unknown).
                if profile.sidebar_visible {
                    if let Some(panel_name) = &profile.sidebar_panel {
                        let panel = Self::str_to_panel(panel_name);
                        self.sidebar_state.set_right_panel(panel);
                    }
                }

                // Restore connector enabled/disabled state.
                if !profile.connector_enabled.is_empty() {
                    self.sidebar_state.connector_enabled = profile.connector_enabled.clone();
                }

                // Restore inline toolbar position.
                if let Some(x) = profile.inline_bar_x {
                    self.panel_app.toolbar_state.floating_inline_bar.x = x;
                }
                if let Some(y) = profile.inline_bar_y {
                    self.panel_app.toolbar_state.floating_inline_bar.y = y;
                }
                if let Some(ref dock) = profile.inline_bar_dock {
                    self.panel_app.toolbar_state.floating_inline_bar.dock_edge = match dock.as_str() {
                        "Top" => zengeld_chart::InlineDockEdge::Top,
                        "Free" => zengeld_chart::InlineDockEdge::Free,
                        _ => zengeld_chart::InlineDockEdge::Bottom,
                    };
                }
            }
            Err(e) => {
                eprintln!("[UserProfile] Failed to load profile: {}", e);
            }
        }

        // Restore watchlist manager.
        let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
        if watchlists_path.exists() {
            match zengeld_chart::load_json::<sidebar_content::watchlist::WatchlistManager>(&watchlists_path, None) {
                Ok(manager) => {
                    self.sidebar_state.watchlist_manager = manager;
                }
                Err(e) => {
                    eprintln!("[UserProfile] Failed to load watchlists: {}", e);
                }
            }
        }
    }

    /// Load watchlists from disk without touching the user profile.
    ///
    /// Called by `new_window()` so that each window starts with the persisted
    /// watchlist state.  Profile loading (`profile.json`) is NOT performed here
    /// — that is done once at startup in `main()` and passed in via `apply_profile_state`.
    pub fn load_watchlists(&mut self) {
        let watchlists_path = zengeld_chart::user_profile::storage::watchlists_path();
        if watchlists_path.exists() {
            match zengeld_chart::load_json::<sidebar_content::watchlist::WatchlistManager>(&watchlists_path, None) {
                Ok(manager) => {
                    self.sidebar_state.watchlist_manager = manager;
                }
                Err(e) => {
                    eprintln!("[UserProfile] Failed to load watchlists: {}", e);
                }
            }
        }
    }

    /// Build indicator display name with ALL numeric params in definition order.
    ///
    /// Examples: "SMA(20)", "RSI(14)", "MACD(26, 12, 9)", "BB(20, 2)"
    fn format_indicator_display_name(
        mgr: &zengeld_terminal_indicators::IndicatorManager,
        inst: &zengeld_terminal_indicators::IndicatorMgrInstance,
    ) -> String {
        use zengeld_terminal_indicators::{IndicatorParamType, IndicatorParamValue};

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
    pub fn build_chart_settings_data(&self) -> zengeld_chart::layout::modals::chart_settings::ChartSettingsData {
        use zengeld_chart::layout::modals::chart_settings::{
            ChartSettingsData, InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
        };

        let rt = self.panel_app.theme_manager.current();
        let series = &rt.series;

        let (auto_scale, vert_lines, horz_lines,
             price_scale_width, time_scale_height,
             time_fmt_use_24h, time_fmt_show_dow, tz_label, date_fmt_label, precision_lbl,
             legend_show_ohlc, legend_show_change, legend_show_percent,
             tooltip_visible, tooltip_follow_cursor) =
            self.panel_app
            .panel_grid
            .active_window()
            .map(|w| {
                let tf = &w.scale_settings.time_format;
                (
                    w.price_scale.scale_mode.is_auto_y(),
                    w.grid_options.vert_lines.visible,
                    w.grid_options.horz_lines.visible,
                    w.scale_settings.price_scale_width,
                    w.scale_settings.time_scale_height,
                    tf.use_24h,
                    tf.show_day_of_week,
                    tf.timezone_label(),
                    tf.date_format.label().to_string(),
                    zengeld_chart::scale_settings::precision_label(
                        w.scale_settings.user_precision,
                    ).to_string(),
                    w.legend.show_ohlc,
                    w.legend.show_change,
                    w.legend.show_percent,
                    w.tooltip.visible,
                    w.tooltip.follow_cursor,
                )
            })
            .unwrap_or_else(|| (
                true, true, true, 70.0, 30.0,
                true, false,
                "(UTC+0) Лондон".to_string(),
                "21.01.2026".to_string(),
                "Авто".to_string(),
                true, true, true,
                false, false,
            ));

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
                date_format_label: date_fmt_label.clone(),
                show_day_of_week: time_fmt_show_dow,
                show_bar_countdown: self.panel_app.panel_grid
                    .active_window()
                    .map(|w| w.scale_settings.show_bar_countdown)
                    .unwrap_or(true),
                price_tick_style: self.panel_app.panel_grid
                    .active_window()
                    .map(|w| w.scale_settings.price_tick_style.clone())
                    .unwrap_or_else(|| "dotted".to_string()),
                price_tick_extend_right: self.panel_app.panel_grid
                    .active_window()
                    .map(|w| w.scale_settings.price_tick_extend_right)
                    .unwrap_or(true),
                price_tick_extend_left: self.panel_app.panel_grid
                    .active_window()
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
        if let Some(data) = self.panel_app.panel_grid.active_window()
            .and_then(|w| w.drawing_manager.get_data_at(idx))
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
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.save_last_style_at_index(idx);
                // Persist the last-used style globally so it survives app restarts.
                if let Some(style) = window.drawing_manager.last_style_for(&type_id) {
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
        if let Some(series) = self.panel_app.panel_grid.active_window()
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
