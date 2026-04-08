//! Sidebar state management — faithful clone of terminal core's SidebarState.
//!
//! Stripped to the 4 panels used by chart-app (no ThemeSettings, no Indicators,
//! no left/bottom sidebars, no color picker).

use std::collections::{HashSet, HashMap, VecDeque};
use std::time::Instant;
use zengeld_chart::ui::scroll_state::ScrollState;
use crate::types::{ObjectTreeItem, AlertItem, IndicatorsTabData, WatchlistItem, ConnectorStatusItem};
use crate::watchlist::WatchlistManager;
use crate::agents_dock::AgentLeafDescriptor;
use crate::sidebar_panel::SidebarPanel;

// =============================================================================
// MetricsSnapshot
// =============================================================================

/// A single point-in-time snapshot of connector metrics, captured at ~1 Hz.
///
/// Up to 60 snapshots are kept per exchange, providing ~60 seconds of history
/// for sparkline graphs rendered in the Connectors sidebar panel.
#[derive(Clone, Debug)]
pub struct MetricsSnapshot {
    /// Total HTTP requests made since connector creation (cumulative counter).
    pub http_requests: u64,
    /// Total HTTP errors since connector creation (cumulative counter).
    pub http_errors: u64,
    /// Latency of the most recently completed HTTP request in milliseconds.
    pub latency_ms: u64,
    /// Current consumed rate-limiter weight for this window.
    pub rate_used: u32,
    /// Maximum rate-limiter weight allowed per window.
    pub rate_max: u32,
    /// Number of active WebSocket connections.
    pub ws_count: usize,
    /// WebSocket ping round-trip time in milliseconds (0 = not measured yet).
    pub ws_ping_rtt_ms: u64,
}

// =============================================================================
// Constants
// =============================================================================

/// Default width of the right sidebar panel content area.
///
/// Matches `RIGHT_SIDEBAR_WIDTH` in terminal core (340.0 px).
pub const RIGHT_SIDEBAR_WIDTH: f64 = 340.0;

/// Minimum allowed right sidebar width (px).
pub const MIN_SIDEBAR_WIDTH: f64 = 280.0;
/// Hard upper bound — the real cap is enforced dynamically by the caller
/// based on the available window width minus the minimum chart width.
/// We just keep this absurdly large so it never bites in practice.

/// Maximum allowed right sidebar width (px).
pub const MAX_SIDEBAR_WIDTH: f64 = 4000.0;

// =============================================================================
// SidebarDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype wrapper around `DockingManager<SidebarPanel>` that provides manual
/// `Clone` and `Debug` impls so it can be a field of `#[derive]`-d `SidebarState`.
///
/// `Clone` creates a fresh empty manager — structural cloning of the panel tree
/// is not needed for the snapshot/undo use cases that drive `SidebarState::clone()`.
pub struct SidebarDockingManager(pub uzor::panels::DockingManager<SidebarPanel>);

impl SidebarDockingManager {
    /// Create an empty manager with a single Watchlist leaf (the default layout).
    pub fn default_layout() -> Self {
        Self(uzor::panels::DockingManager::with_panel(SidebarPanel::Watchlist))
    }

    /// Borrow the inner manager immutably.
    pub fn inner(&self) -> &uzor::panels::DockingManager<SidebarPanel> {
        &self.0
    }

    /// Borrow the inner manager mutably.
    pub fn inner_mut(&mut self) -> &mut uzor::panels::DockingManager<SidebarPanel> {
        &mut self.0
    }
}

impl Default for SidebarDockingManager {
    fn default() -> Self {
        Self::default_layout()
    }
}

impl Clone for SidebarDockingManager {
    /// Returns a **fresh default** manager. Structural cloning of the panel
    /// tree is intentionally omitted.
    fn clone(&self) -> Self {
        Self::default_layout()
    }
}

impl std::fmt::Debug for SidebarDockingManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidebarDockingManager").finish_non_exhaustive()
    }
}

// =============================================================================
// Panel enum
// =============================================================================

/// Which right sidebar panel is currently open (if any).
///
/// This enum is kept for backward compatibility with external callers that
/// inspect `right_panel` to decide whether the sidebar is open and which
/// tab is active.  New code should prefer querying `sidebar_workspace` directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RightSidebarPanel {
    #[default]
    None,
    Watchlist,
    Alerts,
    ObjectTree,
    /// Strategy signals panel — separate from user primitives.
    Signals,
    /// Exchange connector control panel — health status and capabilities.
    Connectors,
    /// Application performance monitoring and resource control panel.
    Performance,
    /// AI agent control panel — terminal / chat modes.
    Agents,
}

impl RightSidebarPanel {
    /// Returns `true` when any panel is open.
    pub fn is_open(self) -> bool {
        self != RightSidebarPanel::None
    }

    /// Returns a stable string key used to index per-panel scroll offsets.
    pub fn scroll_key(self) -> &'static str {
        match self {
            RightSidebarPanel::None       => "none",
            RightSidebarPanel::Watchlist  => "watchlist",
            RightSidebarPanel::Alerts     => "alerts",
            RightSidebarPanel::ObjectTree => "object_tree",
            RightSidebarPanel::Signals    => "signals",
            RightSidebarPanel::Connectors => "connectors",
            RightSidebarPanel::Performance => "performance",
            RightSidebarPanel::Agents     => "agents",
        }
    }
}

// =============================================================================
// Agent panel types
// =============================================================================

/// Which AI CLI agent to use — re-exported from `gate4agent`.
pub use gate4agent::snapshot::AgentCli;

// =============================================================================
// State struct
// =============================================================================

/// Centralized sidebar state for chart-app.
///
/// Mirrors `SidebarState` from `zengeld-terminal-core` but scoped to the 4
/// panels that are available in the standalone chart application.
#[derive(Clone, Debug)]
pub struct SidebarState {
    /// Which right sidebar panel is open (None = closed).
    pub right_panel: RightSidebarPanel,

    /// Current right sidebar width in pixels (user-resizable via drag).
    ///
    /// Clamped to `[MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH]`.
    pub right_sidebar_width: f64,
    /// Items for the Watchlist panel (populated by app before render).
    pub watchlist_items: Vec<WatchlistItem>,
    /// Items for rendering in the Object Tree sidebar (populated by app before render).
    pub object_tree_items: Vec<ObjectTreeItem>,
    /// Alert items for the Alerts panel (populated by app before render).
    pub alert_items: Vec<AlertItem>,
    /// Currently hovered object tree item ID (for hover effects and delete button).
    pub hovered_object_tree_id: Option<String>,
    /// Currently hovered alert ID (for hover effects and buttons).
    pub hovered_alert_id: Option<u64>,
    /// Right sidebar scroll state (for scrollable content).
    ///
    /// Kept for backward compatibility with external code that reads `.right_scroll` directly.
    /// Internal rendering uses `panel_scroll` for per-panel isolation.
    pub right_scroll: ScrollState,

    /// Per-panel scroll states, keyed by `RightSidebarPanel::scroll_key()`.
    ///
    /// Each panel keeps its own independent scroll offset so that switching
    /// between panels does not reset (or leak) scroll position.
    pub panel_scroll: HashMap<String, ScrollState>,

    /// Whether a drag-to-scroll gesture is active on the sidebar content area.
    pub sidebar_drag_active: bool,

    /// Screen Y position of the last mouse event during a sidebar drag-to-scroll.
    pub sidebar_drag_last_y: f64,
    /// Collapsed signal groups (by instance_id).
    pub collapsed_signal_groups: HashSet<u64>,
    /// Per-group scroll state for the Signals panel (instance_id → ScrollState).
    ///
    /// Keyed by `IndicatorSignalGroup::instance_id`.  The renderer uses these
    /// to clip and translate each group's signal list independently.
    /// Also carries drag state for scrollbar handle dragging.
    pub signal_group_scroll: HashMap<u64, ScrollState>,
    /// Currently hovered indicator signal group ID (for collapse toggle).
    pub hovered_signal_group_id: Option<u64>,
    /// Indicator signals data for the Signals panel.
    pub indicator_signals: IndicatorsTabData,

    /// Items for the Connectors panel (populated by app before render).
    pub connector_items: Vec<ConnectorStatusItem>,

    /// Persistent expand/collapse state for each connector card (keyed by exchange_id).
    pub connector_expanded: HashMap<String, bool>,

    /// Persistent enabled/disabled state for each connector (keyed by exchange_id).
    pub connector_enabled: HashMap<String, bool>,

    /// Persistent metrics section visibility for each connector (keyed by exchange_id).
    ///
    /// When `true` the extended metrics section is shown in the connector card.
    /// Toggled by clicking the metrics toggle widget in the connector panel.
    pub connector_metrics_visible: HashMap<String, bool>,

    /// Persistent collapse state for connector group sections.
    /// Keyed by group label string (e.g. "NO API KEY", "REQUIRES API KEY", "NON-CHART DATA").
    /// When `true`, the group is collapsed (items hidden).
    pub connector_group_collapsed: HashMap<String, bool>,

    /// Watchlist manager — tracks which symbols have been starred by the user.
    ///
    /// Used by the symbol search overlay to render filled/empty star icons
    /// and by the star toggle input handler to add/remove symbols.
    pub watchlist_manager: WatchlistManager,

    /// Whether the watchlist column-config dropdown is open.
    ///
    /// Toggled by clicking the `watchlist_column_config` header button.
    /// The dropdown renders as an overlay panel over the watchlist content.
    pub watchlist_config_dropdown_open: bool,

    /// Index of the watchlist row currently being drag-reordered (`None` when idle).
    pub watchlist_drag_index: Option<usize>,

    /// Current Y screen position of the dragged watchlist row.
    pub watchlist_drag_y: f64,

    /// Computed drop target index during a drag-reorder operation.
    ///
    /// `None` when no drag is active or the drop position hasn't been computed yet.
    pub watchlist_drop_index: Option<usize>,

    /// Active column-separator drag state.
    ///
    /// `Some((sep_index, drag_start_x, sep_offset_at_start))` while the user
    /// drags a watchlist column separator.  `sep_index` is the 0-based
    /// separator index (separator 0 sits between column 0 and column 1).
    /// `drag_start_x` is the screen X coordinate where the drag began.
    /// `sep_offset_at_start` is the separator's absolute X offset from the
    /// left edge of the usable area at the moment the drag started.
    pub watchlist_sep_drag: Option<(usize, f64, f64)>,

    /// Currently open color flag picker: `Some((row_index, screen_x, screen_y))`.
    ///
    /// `row_index` is the watchlist item index that owns the flag stripe that
    /// was clicked.  `screen_x / screen_y` are the popup anchor coordinates
    /// (bottom-left corner of the flag stripe for the relevant row).
    pub watchlist_color_picker_open: Option<(usize, f64, f64)>,

    /// Current sort mode for the watchlist sort-by-color button.
    ///
    /// - 0 = no sort (symbols stay in their current order)
    /// - 1 = flagged first, by color order (red first)
    /// - 2 = flagged first, reverse color order (gray/last first)
    pub watchlist_sort_mode: u8,

    /// Rolling 60-sample metrics history per exchange, keyed by exchange_id.
    ///
    /// Populated by `push_metrics_sample` at ~1 Hz from the connector panel
    /// update path.  Used by the sparkline renderer.
    pub metrics_history: HashMap<String, VecDeque<MetricsSnapshot>>,

    /// Timestamp of the last metrics sample push, used to throttle sampling
    /// to approximately once per second.
    pub metrics_last_sample: Option<Instant>,

    /// Performance monitoring data for the Performance panel.
    pub performance_data: PerformanceData,

    // ── Agents panel state ────────────────────────────────────────────────────

    /// Default CLI used when creating a new terminal pane (cycles via the CLI button).
    pub agent_default_cli: AgentCli,

    /// Bounding rect of the agent terminal content area for the focused leaf,
    /// in sidebar-local coordinates.  `None` when the Agents panel is not open
    /// or no pane is focused.
    ///
    /// Used by `chart-app`'s `CursorMoved` handler to auto-focus the PTY terminal on hover.
    pub agent_terminal_rect: Option<(f32, f32, f32, f32)>,

    /// Last known PTY terminal size (cols, rows) for the focused leaf.
    ///
    /// Persisted here so `apply_render_output` can detect changes and call
    /// `resize_instance` only when the grid size actually changes.
    pub agent_terminal_size: Option<(u16, u16)>,

    /// Per-leaf input buffer (text typed but not yet sent), keyed by LeafId.
    pub agent_input_buffers: HashMap<uzor::panels::LeafId, String>,

    /// Per-leaf input cursor position (mirrored from TIM), keyed by LeafId.
    pub agent_input_cursors: HashMap<uzor::panels::LeafId, usize>,

    /// Per-leaf selection range, keyed by LeafId.
    pub agent_input_selections: HashMap<uzor::panels::LeafId, (Option<usize>, Option<usize>)>,

    /// Whether the blinking cursor is currently visible (shared across all fields).
    pub agent_input_cursor_visible: bool,

    /// Which leaf's input field is focused (None = no leaf focused).
    pub agent_input_focused_leaf: Option<uzor::panels::LeafId>,

    /// Per-leaf scroll state for chat messages.
    pub agent_chat_scrolls: HashMap<uzor::panels::LeafId, ScrollState>,

    /// Per-leaf scroll state for PTY terminal.
    pub agent_pty_scrolls: HashMap<uzor::panels::LeafId, ScrollState>,

    /// Per-leaf host-side PTY text selection.
    pub agent_pty_selections: HashMap<uzor::panels::LeafId, PtySelection>,

    /// Per-leaf render snapshots (set each frame by chart-app before render).
    pub agent_leaf_snapshots: HashMap<uzor::panels::LeafId, crate::agent_types::AgentRenderSnapshot>,

    // ── Sidebar workspace docking (Phase 1) ──────────────────────────────────

    /// Top-level docking workspace for the entire right sidebar.
    ///
    /// All 7 current tabs (Watchlist, Alerts, ObjectTree, Signals, Connectors,
    /// Performance, Agents) are first-class `SidebarPanel` leaves inside this
    /// manager.  The old `RightSidebarPanel::current_tab` + nested
    /// `AgentDockingManager` have been replaced by this single workspace.
    ///
    /// Default layout: one leaf containing `SidebarPanel::Watchlist`.
    pub sidebar_workspace: SidebarDockingManager,

    /// Which top-level sidebar leaf currently has keyboard / input focus.
    ///
    /// Replaces the old `focused_agent_leaf`.  When the focused leaf holds an
    /// `Agents(id)` panel, agent input routing still works — just keyed to
    /// this field instead.
    pub focused_sidebar_leaf: Option<uzor::panels::LeafId>,

    /// Full descriptor for each live agent pane, keyed by the top-level
    /// sidebar workspace `LeafId`.
    ///
    /// Consulted when routing input, reading snapshots, or persisting layout.
    pub agent_leaves: HashMap<uzor::panels::LeafId, AgentLeafDescriptor>,
}

/// A host-side PTY text selection in cell coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySelection {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

impl PtySelection {
    pub fn new(row: u16, col: u16) -> Self {
        Self { start_row: row, start_col: col, end_row: row, end_col: col }
    }

    /// Returns (lo, hi) ordered so lo precedes hi in reading order.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let a = (self.start_row, self.start_col);
        let b = (self.end_row, self.end_col);
        if a <= b { (a, b) } else { (b, a) }
    }

    pub fn is_empty(&self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }
}

// =============================================================================
// RenderBackend
// =============================================================================

/// Available render backends for the chart application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    VelloGpu,
    InstancedWgpu,
    VelloCpu,
    VelloHybrid,
    TinySkia,
}

impl RenderBackend {
    /// Human-readable label for this backend.
    pub fn label(&self) -> &'static str {
        match self {
            Self::VelloGpu => "Vello GPU",
            Self::InstancedWgpu => "Instanced wGPU",
            Self::VelloCpu => "Vello CPU",
            Self::VelloHybrid => "Vello Hybrid",
            Self::TinySkia => "Tiny-Skia CPU",
        }
    }

    /// Returns a slice of all available backends in cycle order.
    pub fn all() -> &'static [Self] {
        &[
            Self::VelloGpu,
            Self::InstancedWgpu,
            Self::VelloCpu,
            Self::VelloHybrid,
            Self::TinySkia,
        ]
    }
}

// =============================================================================
// PerformanceData
// =============================================================================

/// Performance metrics and control state for the Performance sidebar panel.
#[derive(Clone, Debug)]
pub struct PerformanceData {
    /// Rolling FPS value (updated from main.rs frame timing).
    pub fps: f64,
    /// Last frame time in milliseconds.
    pub frame_time_ms: f64,
    /// Total number of bars across all windows.
    pub total_bars: usize,
    /// Number of active WebSocket connections.
    pub ws_connections: usize,
    /// Number of broadcast lag events since last reset.
    pub lag_events: u64,
    /// Current FPS limit setting (0 = unlimited).
    pub fps_limit: u32,
    /// Current MSAA sample count (0=off, 4, 8, 16).
    pub msaa_samples: u8,
    /// Current RecalcMode label.
    pub recalc_mode: String,
    /// Number of open windows.
    pub window_count: usize,
    /// Number of active connectors.
    pub active_connectors: usize,
    /// System-wide CPU usage percentage (0-100).
    pub cpu_usage: f32,
    /// Process CPU usage (sum of all threads, can exceed 100% on multicore).
    pub process_cpu: f32,
    /// Process CPU normalized to total machine capacity (0–100%).
    ///
    /// Computed as `process_cpu / num_cores`.  E.g. 154% raw on a 16-core
    /// machine becomes ~9.6% normalized — directly comparable with System CPU.
    pub process_cpu_normalized: f32,
    /// Process memory (RSS) in megabytes.
    pub ram_mb: f64,
    /// Total system RAM in megabytes.
    pub ram_total_mb: f64,
    /// GPU adapter name (e.g. "NVIDIA GeForce RTX 4090").
    pub gpu_name: String,
    /// GPU driver info string.
    pub gpu_driver: String,
    /// Whether frame timing logs are printed to stderr.
    pub perf_log_enabled: bool,
    /// Current render backend selection.
    pub render_backend: RenderBackend,
    /// Scene build time in microseconds (CPU).
    pub scene_build_us: u64,
    /// GPU render-to-texture time in microseconds.
    pub gpu_render_us: u64,
    /// GPU present time in microseconds.
    pub gpu_present_us: u64,
    /// Per-core CPU usage percentages (0–100).
    pub per_core_cpu: Vec<f32>,
    /// GPU memory usage in MB (0 if unavailable).
    pub gpu_mem_mb: f64,

    // ── Internal CPU profiling (microseconds, updated every frame) ────────────
    /// Total time spent inside ChartApp::tick() this frame.
    pub tick_us: u64,
    /// Time spent in indicator recalculation (calculate_for_window calls) this frame.
    pub indicator_recalc_us: u64,
    /// Total number of indicator instances across all windows.
    pub indicator_recalc_count: u32,
    /// How many indicator instances used the O(1) incremental path last recalc.
    pub indicator_incremental_count: u32,
    /// How many indicator instances used the O(N) full-recalc path last recalc.
    pub indicator_full_count: u32,
    /// Time spent processing LiveUpdate events (the drain loop) this frame.
    pub event_process_us: u64,
    /// Accumulated time spent in calc_auto_scale() calls this frame.
    pub auto_scale_us: u64,
    /// Accumulated time spent in calc_moving_averages() calls this frame.
    pub moving_avg_us: u64,
}

impl Default for PerformanceData {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            total_bars: 0,
            ws_connections: 0,
            lag_events: 0,
            fps_limit: 60,
            msaa_samples: 16,
            recalc_mode: "PerFrame".to_string(),
            window_count: 0,
            active_connectors: 0,
            cpu_usage: 0.0,
            process_cpu: 0.0,
            process_cpu_normalized: 0.0,
            ram_mb: 0.0,
            ram_total_mb: 0.0,
            gpu_name: String::new(),
            gpu_driver: String::new(),
            perf_log_enabled: false,
            render_backend: RenderBackend::VelloGpu,
            scene_build_us: 0,
            gpu_render_us: 0,
            gpu_present_us: 0,
            per_core_cpu: Vec::new(),
            gpu_mem_mb: 0.0,
            tick_us: 0,
            indicator_recalc_us: 0,
            indicator_recalc_count: 0,
            indicator_incremental_count: 0,
            indicator_full_count: 0,
            event_process_us: 0,
            auto_scale_us: 0,
            moving_avg_us: 0,
        }
    }
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            right_panel: RightSidebarPanel::default(),
            right_sidebar_width: RIGHT_SIDEBAR_WIDTH,
            watchlist_items: Vec::new(),
            object_tree_items: Vec::new(),
            alert_items: Vec::new(),
            connector_items: Vec::new(),
            connector_expanded: HashMap::new(),
            connector_enabled: HashMap::new(),
            connector_metrics_visible: HashMap::new(),
            connector_group_collapsed: HashMap::new(),
            hovered_object_tree_id: None,
            hovered_alert_id: None,
            right_scroll: ScrollState::default(),
            panel_scroll: HashMap::new(),
            sidebar_drag_active: false,
            sidebar_drag_last_y: 0.0,
            collapsed_signal_groups: std::collections::HashSet::new(),
            signal_group_scroll: HashMap::new(),
            hovered_signal_group_id: None,
            indicator_signals: IndicatorsTabData::default(),
            watchlist_manager: WatchlistManager::default(),
            watchlist_config_dropdown_open: false,
            watchlist_drag_index: None,
            watchlist_drag_y: 0.0,
            watchlist_drop_index: None,
            watchlist_sep_drag: None,
            watchlist_color_picker_open: None,
            watchlist_sort_mode: 0,
            metrics_history: HashMap::new(),
            metrics_last_sample: None,
            performance_data: PerformanceData::default(),
            agent_default_cli: AgentCli::Claude,
            agent_terminal_rect: None,
            agent_terminal_size: None,
            agent_input_buffers: HashMap::new(),
            agent_input_cursors: HashMap::new(),
            agent_input_selections: HashMap::new(),
            agent_input_cursor_visible: false,
            agent_input_focused_leaf: None,
            agent_chat_scrolls: HashMap::new(),
            agent_pty_scrolls: HashMap::new(),
            agent_pty_selections: HashMap::new(),
            agent_leaf_snapshots: HashMap::new(),
            sidebar_workspace: SidebarDockingManager::default_layout(),
            focused_sidebar_leaf: None,
            agent_leaves: HashMap::new(),
        }
    }
}

impl SidebarState {
    /// Create new sidebar state (all closed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when the right sidebar is open.
    pub fn is_right_open(&self) -> bool {
        self.right_panel.is_open()
    }

    /// Returns the right sidebar width in pixels, or 0.0 when closed.
    pub fn right_width(&self) -> f64 {
        if self.is_right_open() { self.right_sidebar_width } else { 0.0 }
    }

    /// Set the right sidebar width, clamping to `[MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH]`.
    pub fn set_right_width(&mut self, w: f64) {
        self.right_sidebar_width = w.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }

    // =========================================================================
    // Panel control — matching core API exactly
    // =========================================================================

    /// Set the right sidebar panel.
    ///
    /// Returns `Some((opening, width))` if the open/closed state changed, or
    /// `None` if we are just switching between panels.
    ///
    /// - `opening = true`  → sidebar was closed, now opening
    /// - `opening = false` → sidebar was open, now closing
    ///
    /// The caller uses this to call `viewport.compensate_right_sidebar()` when `Some`.
    pub fn set_right_panel(&mut self, panel: RightSidebarPanel) -> Option<(bool, f64)> {
        let was_open = self.is_right_open();
        let will_open = panel.is_open();
        self.right_panel = panel;

        if was_open != will_open {
            Some((will_open, self.right_sidebar_width))
        } else {
            None // just switching panels — no viewport compensation needed
        }
    }

    /// Toggle a right sidebar panel.
    ///
    /// If the **same** panel is already open, close it.
    /// If a **different** panel is open (or none), switch to (or open) the new one.
    ///
    /// Returns `Some((opening, width))` when the open/closed state changes,
    /// `None` when only the active panel changes.
    pub fn toggle_right_panel(&mut self, panel: RightSidebarPanel) -> Option<(bool, f64)> {
        if self.right_panel == panel {
            // Same panel — close it.
            self.set_right_panel(RightSidebarPanel::None)
        } else {
            // Different panel — open/switch to it.
            // Each panel keeps its own scroll offset in panel_scroll, so there
            // is nothing to reset here.  (right_scroll is kept for legacy callers.)
            self.set_right_panel(panel)
        }
    }

    /// Close the right sidebar.
    ///
    /// Returns `Some((false, width))` if it was open, `None` if already closed.
    pub fn close_right(&mut self) -> Option<(bool, f64)> {
        self.set_right_panel(RightSidebarPanel::None)
    }

    // =========================================================================
    // Compatibility accessors — Phase 1 migration
    // =========================================================================

    /// Returns the currently focused sidebar leaf id.
    ///
    /// Backward-compatible alias for `focused_sidebar_leaf` — used by all the
    /// existing call-sites in `chart-app/src/input.rs` and `lib.rs` that
    /// previously read `focused_agent_leaf`.  Agents-specific code should call
    /// this and then check whether the leaf's active panel is `Agents(_)`.
    #[inline(always)]
    pub fn focused_agent_leaf(&self) -> Option<uzor::panels::LeafId> {
        self.focused_sidebar_leaf
    }

    /// Set the focused leaf (backward-compatible setter used by input.rs).
    #[inline(always)]
    pub fn set_focused_agent_leaf(&mut self, id: Option<uzor::panels::LeafId>) {
        self.focused_sidebar_leaf = id;
    }

    // =========================================================================
    // Phase 1 sidebar workspace helpers
    // =========================================================================

    /// Show a pinned panel (Category A/B) in the sidebar workspace.
    ///
    /// Behaviour:
    /// - If the panel already exists in a leaf, focus that leaf (activate its tab).
    /// - If the panel is absent, spawn a new leaf containing it.
    ///
    /// Returns `true` when the sidebar open/closed state changes so the caller
    /// can trigger viewport compensation.  For Agents, callers should check
    /// `sidebar_workspace.tree()` for existing Agents leaves rather than
    /// calling this helper (since Agents is multi-instance).
    pub fn show_or_focus_panel(&mut self, panel: SidebarPanel) -> bool {
        let was_open = self.is_right_open();

        // Sync the legacy right_panel field so existing callers still work.
        let legacy = match &panel {
            SidebarPanel::Watchlist   => RightSidebarPanel::Watchlist,
            SidebarPanel::Alerts      => RightSidebarPanel::Alerts,
            SidebarPanel::ObjectTree  => RightSidebarPanel::ObjectTree,
            SidebarPanel::Signals     => RightSidebarPanel::Signals,
            SidebarPanel::Connectors  => RightSidebarPanel::Connectors,
            SidebarPanel::Performance => RightSidebarPanel::Performance,
            SidebarPanel::Agents(_)   => RightSidebarPanel::Agents,
            _                         => RightSidebarPanel::Watchlist, // migratable panels default
        };

        // Check if panel already exists in any leaf.
        let existing_leaf = self.find_panel_leaf(&panel);

        if let Some(leaf_id) = existing_leaf {
            // Already present — focus its leaf.
            self.sidebar_workspace.inner_mut().set_active_leaf(leaf_id);
            self.focused_sidebar_leaf = Some(leaf_id);
        } else {
            // Not present — spawn as a new leaf.
            let leaf_id = self.sidebar_workspace.inner_mut()
                .tree_mut()
                .add_leaf(panel);
            self.sidebar_workspace.inner_mut().set_active_leaf(leaf_id);
            self.focused_sidebar_leaf = Some(leaf_id);
        }

        // Keep legacy field in sync.
        self.right_panel = legacy;

        let now_open = true; // workspace always has at least one leaf after this
        !was_open && now_open
    }

    /// Toggle a pinned panel (Category A).
    ///
    /// - If panel is the currently focused active panel AND it's the only leaf:
    ///   close the sidebar (hide it by setting `right_panel = None`).
    /// - Otherwise: show or focus it.
    pub fn toggle_panel(&mut self, panel: SidebarPanel) -> Option<(bool, f64)> {
        // Check whether this panel is already the sole focused leaf.
        let active_is_this_panel = self.sidebar_workspace
            .inner()
            .active_leaf()
            .and_then(|lid| self.sidebar_workspace.inner().tree().leaf(lid))
            .and_then(|leaf| leaf.active_panel())
            .map(|p| p.variant_eq(&panel))
            .unwrap_or(false);

        let leaf_count = self.sidebar_workspace.inner().tree().visible_leaf_count();

        if active_is_this_panel && self.right_panel != RightSidebarPanel::None {
            if leaf_count <= 1 {
                // Only panel visible — close the sidebar.
                return self.set_right_panel(RightSidebarPanel::None);
            }
        }

        let opened = self.show_or_focus_panel(panel);
        if opened {
            Some((true, self.right_sidebar_width))
        } else {
            None
        }
    }

    /// Find the first leaf that contains a panel matching `panel` (by variant).
    fn find_panel_leaf(&self, panel: &SidebarPanel) -> Option<uzor::panels::LeafId> {
        let tree = self.sidebar_workspace.inner().tree();
        self.find_panel_leaf_in_branch(tree.root(), panel)
    }

    fn find_panel_leaf_in_branch(
        &self,
        branch: &uzor::panels::Branch<SidebarPanel>,
        panel: &SidebarPanel,
    ) -> Option<uzor::panels::LeafId> {
        for child in &branch.children {
            match child {
                uzor::panels::PanelNode::Leaf(leaf) => {
                    if leaf.panels.iter().any(|p| p.variant_eq(panel)) {
                        return Some(leaf.id);
                    }
                }
                uzor::panels::PanelNode::Branch(b) => {
                    if let Some(id) = self.find_panel_leaf_in_branch(b, panel) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    // =========================================================================
    // Metrics history helpers
    // =========================================================================

    /// Push a new metrics snapshot for `exchange_id`.
    ///
    /// Keeps a rolling window of at most 60 snapshots (~60 s at 1 Hz).
    /// Older snapshots are dropped from the front when the window is full.
    pub fn push_metrics_sample(&mut self, exchange_id: &str, snapshot: MetricsSnapshot) {
        let history = self.metrics_history
            .entry(exchange_id.to_string())
            .or_insert_with(|| VecDeque::with_capacity(61));
        if history.len() >= 60 {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    // =========================================================================
    // Per-panel scroll helpers
    // =========================================================================

    /// Returns the scroll state for the currently open right panel.
    ///
    /// Each panel has its own independent scroll offset stored in `panel_scroll`.
    /// Falls back to `right_scroll` when the panel key is `"none"`.
    pub fn current_right_scroll(&self) -> &ScrollState {
        let key = self.right_panel.scroll_key();
        self.panel_scroll.get(key).unwrap_or(&self.right_scroll)
    }

    /// Returns a mutable reference to the scroll state for the currently open right panel.
    ///
    /// Inserts a default entry if none exists yet, so the reference is always valid.
    pub fn current_right_scroll_mut(&mut self) -> &mut ScrollState {
        let key = self.right_panel.scroll_key().to_owned();
        self.panel_scroll.entry(key).or_default()
    }
}
