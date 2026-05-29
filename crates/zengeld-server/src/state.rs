//! Shared state for the Agent API server.
//!
//! [`AgentState`] is wrapped in `Arc` and injected into every axum handler via
//! `extract::State`. The main thread updates snapshots on state-change events;
//! HTTP handlers read them via `RwLock`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use serde::{Deserialize, Serialize};

use live_data::DataBridge;

// ===========================================================================
// AgentState — root shared state
// ===========================================================================

/// Shared state accessible by the Agent API server.
pub struct AgentState {
    /// Reference to the live-data bridge — provides access to the bar cache
    /// via [`DataBridge::get_cached_bars`] and [`DataBridge::cached_bar_keys`].
    pub bridge: Arc<DataBridge>,

    /// Snapshot of indicator data, updated by the main (render) thread.
    pub indicator_snapshot: RwLock<IndicatorSnapshot>,

    /// Snapshot of terminal structure (windows, tabs, charts, layout).
    pub terminal_snapshot: RwLock<TerminalSnapshot>,

    /// Indicator catalog — populated once at startup, rarely changes.
    pub indicator_catalog: RwLock<IndicatorCatalogSnapshot>,

    /// Drawing primitive catalog — populated once at startup.
    pub primitive_catalog: RwLock<PrimitiveCatalogSnapshot>,

    /// Watchlist snapshot — updated when watchlists change.
    pub watchlist_snapshot: RwLock<WatchlistSnapshot>,

    /// Connector status snapshot — updated periodically.
    pub connector_snapshot: RwLock<ConnectorSnapshot>,

    /// Command queue — HTTP handlers push, render thread drains.
    pub command_queue: Mutex<Vec<AgentCommand>>,

    /// Wall-clock time when the server was created (used to compute uptime).
    pub start_time: std::time::Instant,

    /// Milliseconds since `start_time` when an Agent API request was last served.
    ///
    /// Bumped by the access-tracking middleware on every HTTP request. The render
    /// thread reads this to skip building the (expensive) indicator/terminal/
    /// watchlist/connector snapshots when no agent has talked to us recently —
    /// those snapshots clone every indicator's full output series, which is a
    /// 20-30ms-per-second frame stall when nothing is even listening.
    /// `0` = never accessed.
    pub last_agent_access_ms: AtomicU64,

    /// Application version string (e.g. `"0.1.0"`).
    pub version: String,
}

impl AgentState {
    /// Create a new [`AgentState`] wrapping the given bridge.
    pub fn new(bridge: Arc<DataBridge>, version: impl Into<String>) -> Self {
        Self {
            bridge,
            indicator_snapshot: RwLock::new(IndicatorSnapshot::default()),
            terminal_snapshot: RwLock::new(TerminalSnapshot::default()),
            indicator_catalog: RwLock::new(IndicatorCatalogSnapshot::default()),
            primitive_catalog: RwLock::new(PrimitiveCatalogSnapshot::default()),
            watchlist_snapshot: RwLock::new(WatchlistSnapshot::default()),
            connector_snapshot: RwLock::new(ConnectorSnapshot::default()),
            command_queue: Mutex::new(Vec::new()),
            start_time: std::time::Instant::now(),
            last_agent_access_ms: AtomicU64::new(0),
            version: version.into(),
        }
    }

    /// Record that an Agent API request was just served (called by middleware).
    pub fn bump_access(&self) {
        let ms = self.start_time.elapsed().as_millis() as u64;
        self.last_agent_access_ms.store(ms, Ordering::Relaxed);
    }

    /// Whether an Agent API request was served within the last `within` window.
    ///
    /// Used by the render thread to gate snapshot rebuilds: if no agent has
    /// queried recently, skip the expensive snapshot cloning entirely.
    pub fn accessed_within(&self, within: std::time::Duration) -> bool {
        let last = self.last_agent_access_ms.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        let now = self.start_time.elapsed().as_millis() as u64;
        now.saturating_sub(last) <= within.as_millis() as u64
    }

    /// Push an agent command into the queue (called from HTTP handlers).
    pub fn push_command(&self, cmd: AgentCommand) {
        if let Ok(mut q) = self.command_queue.lock() {
            q.push(cmd);
        }
    }

    /// Drain all pending commands (called from render thread each frame).
    pub fn drain_commands(&self) -> Vec<AgentCommand> {
        if let Ok(mut q) = self.command_queue.lock() {
            std::mem::take(&mut *q)
        } else {
            Vec::new()
        }
    }
}

// ===========================================================================
// Terminal snapshot types (Phase 1 — read-only discovery)
// ===========================================================================

/// Point-in-time snapshot of the entire terminal structure.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TerminalSnapshot {
    pub windows: Vec<WindowSnapshot>,
}

/// One OS window.
#[derive(Debug, Clone, Serialize)]
pub struct WindowSnapshot {
    pub window_id: String,
    pub tabs: Vec<TabSnapshot>,
    pub active_tab_id: String,
    pub charts: Vec<ChartSnapshot>,
    pub layout: LayoutNode,
}

/// One tab (preset) in a window.
#[derive(Debug, Clone, Serialize)]
pub struct TabSnapshot {
    pub preset_id: String,
    pub name: String,
    pub active: bool,
}

/// One chart leaf in a split layout.
#[derive(Debug, Clone, Serialize)]
pub struct ChartSnapshot {
    pub chart_id: u64,
    pub leaf_id: u64,
    pub symbol: String,
    pub exchange: String,
    pub timeframe: String,
    pub bar_count: usize,
    pub viewport: ViewportSnapshot,
    pub indicator_count: usize,
    pub primitive_count: usize,
    /// Summary of indicators (id + type_id + name only).
    pub indicators: Vec<IndicatorSummary>,
    /// Summary of primitives (id + type_id only).
    pub primitives: Vec<PrimitiveSummary>,
}

/// Viewport state of a chart.
#[derive(Debug, Clone, Serialize)]
pub struct ViewportSnapshot {
    pub view_start: f64,
    pub bar_spacing: f64,
    pub chart_width: f64,
    pub chart_height: f64,
    pub bars_visible: usize,
}

/// Compact indicator reference (no computed values).
#[derive(Debug, Clone, Serialize)]
pub struct IndicatorSummary {
    pub id: u64,
    pub type_id: String,
    pub name: String,
}

/// Compact primitive reference.
#[derive(Debug, Clone, Serialize)]
pub struct PrimitiveSummary {
    pub id: u64,
    pub type_id: String,
}

/// Recursive layout tree node.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "leaf")]
    Leaf { chart_id: u64, leaf_id: u64 },
    #[serde(rename = "split")]
    Split {
        axis: String,
        proportions: Vec<f64>,
        children: Vec<LayoutNode>,
    },
}

impl Default for LayoutNode {
    fn default() -> Self {
        LayoutNode::Leaf { chart_id: 0, leaf_id: 0 }
    }
}

// ===========================================================================
// Agent commands (Phase 2 — write operations)
// ===========================================================================

/// Raw pixel data for a captured screenshot, returned by the render thread
/// to an HTTP handler via a oneshot channel.
pub struct ScreenshotData {
    /// PNG-encoded bytes ready for base64 encoding or direct delivery.
    pub png_bytes: Vec<u8>,
    /// Width of the captured region in pixels.
    pub width: u32,
    /// Height of the captured region in pixels.
    pub height: u32,
}

/// Commands pushed by HTTP handlers, drained by the render thread.
pub enum AgentCommand {
    // -- Viewport / navigation --
    SetViewport {
        window_id: String,
        chart_id: u64,
        view_start: Option<f64>,
        bar_spacing: Option<f64>,
        mode: Option<String>,
    },
    SwitchSymbol {
        window_id: String,
        chart_id: u64,
        symbol: String,
        exchange: String,
        timeframe: String,
        account_type: String,
    },

    // -- Indicator CRUD (Phase 3) --
    AddIndicator {
        window_id: String,
        chart_id: u64,
        type_id: String,
        params: HashMap<String, serde_json::Value>,
        agent_id: Option<String>,
    },
    UpdateIndicator {
        window_id: String,
        chart_id: u64,
        indicator_id: u64,
        params: HashMap<String, serde_json::Value>,
        agent_id: Option<String>,
    },
    RemoveIndicator {
        window_id: String,
        chart_id: u64,
        indicator_id: u64,
        agent_id: Option<String>,
    },

    // -- Primitive CRUD (Phase 4) --
    AddPrimitive {
        window_id: String,
        chart_id: u64,
        type_id: String,
        points: Vec<[f64; 2]>,
        style: PrimitiveStyleDto,
        agent_id: Option<String>,
    },
    UpdatePrimitive {
        window_id: String,
        chart_id: u64,
        primitive_id: u64,
        points: Option<Vec<[f64; 2]>>,
        style: Option<PrimitiveStyleDto>,
        agent_id: Option<String>,
    },
    RemovePrimitive {
        window_id: String,
        chart_id: u64,
        primitive_id: u64,
        agent_id: Option<String>,
    },

    // -- Screenshot (Phase 5) --
    /// Request a PNG screenshot of a specific chart in a window.
    ///
    /// The render thread captures the frame, encodes it to PNG, and sends
    /// the result back via `response_tx`.  The HTTP handler awaits the
    /// oneshot receiver with a timeout.
    RequestScreenshot {
        window_id: String,
        chart_id: u64,
        agent_id: Option<String>,
        response_tx: tokio::sync::oneshot::Sender<Result<ScreenshotData, String>>,
    },

    // -- Key management (UI-initiated) --
    /// Create a new managed API key with the given label and tier.
    ///
    /// Pushed by the UI key manager "Create" button.  The render thread
    /// generates the raw key, calls `agent_state.add_key()`, and stores the
    /// raw key in `UserSettingsState.last_created_key` for the one-time
    /// display box.
    CreateKey {
        label: String,
        tier: String,
    },
    /// Delete all managed API keys whose label matches `label`.
    ///
    /// Pushed by the UI key manager delete [X] button.
    DeleteKey {
        label: String,
    },
}

/// Style fields for a drawing primitive (used in commands and responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveStyleDto {
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_width")]
    pub width: f64,
    #[serde(default = "default_style")]
    pub style: String,
    pub fill_color: Option<String>,
    pub fill_opacity: Option<f64>,
}

fn default_color() -> String { "#e74c3c".to_string() }
fn default_width() -> f64 { 2.0 }
fn default_style() -> String { "solid".to_string() }

// ===========================================================================
// Indicator snapshot types (existing)
// ===========================================================================

/// Point-in-time snapshot of all active indicator instances.
///
/// Updated by the main thread; read by HTTP handlers.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndicatorSnapshot {
    /// Indicator instances grouped by symbol.
    pub symbols: HashMap<String, Vec<IndicatorInstanceSnapshot>>,
}

/// A single indicator instance snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct IndicatorInstanceSnapshot {
    /// Opaque instance id (monotonically increasing within a session).
    pub id: u64,
    /// Machine-readable type identifier (e.g. `"ema"`).
    pub type_id: String,
    /// Human-readable type name (e.g. `"Exponential Moving Average"`).
    pub type_name: String,
    /// Symbol the indicator is attached to (e.g. `"BTCUSDT"`).
    pub symbol: String,
    /// Window/panel id this indicator lives in, if applicable.
    pub window_id: Option<u64>,
    /// Parameter map (key → JSON value).
    pub params: HashMap<String, serde_json::Value>,
    /// Computed output series.
    pub outputs: Vec<IndicatorOutputSnapshot>,
}

/// One output series of a computed indicator.
#[derive(Debug, Clone, Serialize)]
pub struct IndicatorOutputSnapshot {
    /// Output name (e.g. `"value"`, `"signal"`, `"histogram"`).
    pub name: String,
    /// Series values aligned with the bar series (oldest first).
    pub values: Vec<f64>,
}

// ===========================================================================
// Catalog snapshot types (Phase 6 — metadata and discovery)
// ===========================================================================

/// Static catalog of all available indicator types.
///
/// Populated once at startup from the indicator registry.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndicatorCatalogSnapshot {
    pub indicators: Vec<CatalogIndicator>,
}

/// Definition of one indicator type available in the catalog.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogIndicator {
    /// Machine-readable identifier (e.g. `"sma"`).
    pub type_id: String,
    /// Full display name (e.g. `"Simple Moving Average"`).
    pub name: String,
    /// Short abbreviation shown in chart legends (e.g. `"SMA"`).
    pub short_name: String,
    /// Category group (e.g. `"trend"`, `"oscillator"`, `"volume"`).
    pub category: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this indicator renders as an overlay on the price chart.
    pub overlay: bool,
    /// Configurable parameters for this indicator.
    pub params: Vec<CatalogParam>,
    /// Output series produced by this indicator.
    pub outputs: Vec<CatalogOutput>,
}

/// One configurable parameter of a catalog indicator.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogParam {
    /// Machine-readable parameter name (e.g. `"period"`).
    pub name: String,
    /// Human-readable label shown in the UI (e.g. `"Period"`).
    pub display_name: String,
    /// Type tag: `"int"`, `"float"`, `"bool"`, `"select"`, `"color"`, `"source"`.
    pub param_type: String,
    /// Default value as a JSON value (int, float, bool, or string).
    pub default_value: serde_json::Value,
    /// Minimum numeric value, if applicable.
    pub min: Option<f64>,
    /// Maximum numeric value, if applicable.
    pub max: Option<f64>,
}

/// One output series produced by a catalog indicator.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogOutput {
    /// Machine-readable output name (e.g. `"value"`, `"signal"`).
    pub name: String,
    /// Suggested default hex color for rendering (e.g. `"#2196f3"`).
    pub color: Option<String>,
}

/// Static catalog of all available drawing primitive types.
///
/// Populated once at startup from the drawing registry.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PrimitiveCatalogSnapshot {
    pub primitives: Vec<CatalogPrimitive>,
}

/// Definition of one drawing primitive type.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogPrimitive {
    /// Machine-readable identifier (e.g. `"trend_line"`).
    pub type_id: String,
    /// Human-readable label (e.g. `"Trend Line"`).
    pub display_name: String,
    /// Visual kind group (e.g. `"lines"`, `"shapes"`, `"fibonacci"`, `"text"`).
    pub kind: String,
    /// How many chart clicks are required to place this primitive
    /// (e.g. `"TwoPoint"`, `"OnePoint"`, `"FreeForm"`).
    pub click_behavior: String,
    /// Default hex color applied when placing a new instance.
    pub default_color: String,
    /// Whether this primitive supports an attached text label.
    pub supports_text: bool,
    /// Whether this primitive supports multiple horizontal price levels
    /// (e.g. Fibonacci retracement, Pitchfork).
    pub has_levels: bool,
}

// ===========================================================================
// Watchlist snapshot types (Phase 6)
// ===========================================================================

/// Point-in-time snapshot of all user watchlists.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WatchlistSnapshot {
    pub watchlists: Vec<WatchlistEntry>,
}

/// One watchlist.
#[derive(Debug, Clone, Serialize)]
pub struct WatchlistEntry {
    /// Stable numeric id.
    pub id: u64,
    /// User-visible name.
    pub name: String,
    /// Whether this is the currently selected watchlist.
    pub active: bool,
    /// Ordered list of symbols in this watchlist.
    pub items: Vec<WatchlistItemEntry>,
}

/// One symbol entry inside a watchlist.
#[derive(Debug, Clone, Serialize)]
pub struct WatchlistItemEntry {
    /// Trading pair symbol (e.g. `"BTCUSDT"`).
    pub symbol: String,
    /// Exchange identifier (e.g. `"binance"`).
    pub exchange: String,
    /// Asset category (e.g. `"crypto"`, `"stock"`, `"forex"`).
    pub category: String,
}

// ===========================================================================
// Connector status snapshot types (Phase 6)
// ===========================================================================

/// Point-in-time snapshot of all configured exchange connectors.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorSnapshot {
    pub connectors: Vec<ConnectorEntry>,
}

/// Status of one exchange connector.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorEntry {
    /// Exchange identifier (e.g. `"binance"`).
    pub exchange_id: String,
    /// Whether the connector is currently active (REST polling enabled).
    pub active: bool,
    /// Whether the WebSocket feed is currently connected.
    pub ws_active: bool,
    /// Number of symbols tracked by this connector.
    pub symbol_count: usize,
}
