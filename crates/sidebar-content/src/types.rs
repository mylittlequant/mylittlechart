//! Shared data types for sidebar panels.
//!
//! Mirrors the types used by `zengeld-terminal-core`'s sidebar definitions.

use zengeld_chart::state::command::ObjectCategory;
use serde::{Serialize, Deserialize};
use crate::state::MetricsSnapshot;

// =============================================================================
// Object Tree
// =============================================================================

/// An item rendered in the Object Tree sidebar panel.
///
/// Mirrors `ObjectTreeItem` from `zengeld-terminal-core::ui::definitions::sidebar`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectTreeItem {
    /// Unique object identifier.
    pub id: u64,
    /// Display name shown in the sidebar.
    pub name: String,
    /// Object category (Drawing, Indicator, etc.).
    pub category: ObjectCategory,
    /// Type name (e.g. "TrendLine", "SMA").
    pub type_name: String,
    /// Whether the object is visible on the chart.
    pub visible: bool,
    /// Whether the object is locked (not draggable).
    pub locked: bool,
    /// Whether the object is currently selected.
    pub selected: bool,
    /// Optional accent colour for the row indicator swatch.
    pub color: Option<String>,
    /// Whether this object has at least one alert bound to it.
    ///
    /// Set by `chart-app` after cross-referencing `alert_manager.items()`.
    /// Used by the renderer to highlight the bell icon in accent colour.
    pub has_alert: bool,
    /// Optional section label used to group items under a section header.
    ///
    /// Recognised values: `"Group"` (shared across synced windows) or
    /// `"Window"` (local to the active window only).  `None` means no
    /// section header is rendered above this item.
    pub section: Option<String>,
}

impl ObjectTreeItem {
    /// Construct a new item with defaults (visible, unlocked, not selected).
    pub fn new(id: u64, name: &str, category: ObjectCategory, type_name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            category,
            type_name: type_name.to_string(),
            visible: true,
            locked: false,
            selected: false,
            color: None,
            has_alert: false,
            section: None,
        }
    }

    pub fn with_visible(mut self, v: bool) -> Self { self.visible = v; self }
    pub fn with_locked(mut self, v: bool) -> Self { self.locked = v; self }
    pub fn with_selected(mut self, v: bool) -> Self { self.selected = v; self }
    pub fn with_color(mut self, c: Option<String>) -> Self { self.color = c; self }
    pub fn with_has_alert(mut self, v: bool) -> Self { self.has_alert = v; self }
    pub fn with_section(mut self, s: &str) -> Self { self.section = Some(s.to_string()); self }
}

// =============================================================================
// Alerts
// =============================================================================

/// Alert item for the Alerts sidebar panel.
///
/// Re-exported from the `alerts` crate so chart-level renderers
/// can use this type without depending on core.
pub use alerts::AlertItem;

// =============================================================================
// Signals
// =============================================================================

/// A single signal row from one indicator instance.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndicatorSignalRow {
    /// Bar index where the signal occurred.
    pub bar_index: i64,
    /// Human-readable signal type label.
    pub signal_type: String,
    /// Price level associated with the signal.
    pub price: f64,
    /// Signal strength (0.0–1.0).
    pub strength: f64,
    /// 1 = bullish, -1 = bearish, 0 = neutral.
    pub direction: i32,
}

impl IndicatorSignalRow {
    pub fn direction_symbol(&self) -> &str {
        match self.direction {
            1 => "▲",
            -1 => "▼",
            _ => "●",
        }
    }

    pub fn is_bullish(&self) -> bool { self.direction == 1 }
    pub fn is_bearish(&self) -> bool { self.direction == -1 }
}

/// A group of signals produced by a single indicator instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndicatorSignalGroup {
    /// Unique indicator instance ID.
    pub instance_id: u64,
    /// Human-readable indicator name.
    pub indicator_name: String,
    /// Whether the group is collapsed in the sidebar.
    pub collapsed: bool,
    /// Individual signal rows within this group.
    pub signals: Vec<IndicatorSignalRow>,
}

impl IndicatorSignalGroup {
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }
}

/// All indicator signals data for the Signals sidebar panel.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndicatorsTabData {
    /// Signal groups, one per indicator instance.
    pub groups: Vec<IndicatorSignalGroup>,
    /// Total signal count across all groups.
    pub total_count: usize,
}

// =============================================================================
// Watchlist
// =============================================================================

/// A single row in the Watchlist sidebar panel.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WatchlistItem {
    /// Trading symbol (e.g. "BTCUSDT").
    pub symbol: String,
    /// Exchange name (e.g. "Binance").
    pub exchange: String,
    /// Most recent trade price.
    pub last_price: f64,
    /// 24-hour price change as a percentage.
    pub change_percent: f64,
    /// 24-hour high price.
    pub high_24h: f64,
    /// 24-hour low price.
    pub low_24h: f64,
    /// 24-hour volume (in base asset).
    pub volume_24h: f64,
}

impl WatchlistItem {
    /// Create a new watchlist item with just a symbol (other fields default to 0).
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            ..Default::default()
        }
    }
}

// =============================================================================
// Connectors
// =============================================================================

/// Connector grouping for the Connectors panel UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectorGroup {
    /// Free data — no API key needed, provides chartable OHLCV data.
    NoApiKey,
    /// Requires API key — provides chartable OHLCV data but needs authentication.
    RequiresApiKey,
    /// Non-chart data — does not provide standard OHLCV candles.
    NonChartData,
}

impl ConnectorGroup {
    pub fn label(&self) -> &'static str {
        match self {
            ConnectorGroup::NoApiKey => "NO API KEY",
            ConnectorGroup::RequiresApiKey => "REQUIRES API KEY",
            ConnectorGroup::NonChartData => "NON-CHART DATA",
        }
    }

    /// Order for display in the panel (lower = first).
    pub fn sort_order(&self) -> u8 {
        match self {
            ConnectorGroup::NoApiKey => 0,
            ConnectorGroup::RequiresApiKey => 1,
            ConnectorGroup::NonChartData => 2,
        }
    }
}

/// Status and capability record for a single exchange connector.
///
/// Displayed in the Connectors sidebar panel as an expandable card.
#[derive(Debug, Clone)]
pub struct ConnectorStatusItem {
    /// Stable identifier used internally (e.g. "binance", "okx").
    pub exchange_id: String,
    /// Human-readable display name shown in the panel (e.g. "Binance", "OKX").
    pub display_name: String,
    /// Whether the connector is currently enabled (user-toggled).
    pub enabled: bool,
    /// Whether the card is expanded to show capability details.
    pub expanded: bool,
    /// Whether the REST API is reachable and responding.
    pub rest_healthy: bool,
    /// Whether the WebSocket connection is currently established.
    pub ws_connected: bool,
    /// Whether the connector supports fetching historical kline (OHLCV) data.
    pub has_klines: bool,
    /// Whether the connector supports streaming klines over WebSocket.
    pub has_ws_klines: bool,
    /// Whether the connector supports fetching trade data.
    pub has_trades: bool,
    /// Whether the connector supports streaming trades over WebSocket.
    pub has_ws_trades: bool,
    /// Whether the connector supports fetching order book data.
    pub has_orderbook: bool,
    /// Whether the connector supports streaming order book over WebSocket.
    pub has_ws_orderbook: bool,
    /// Maximum number of klines returned per REST request.
    pub kline_batch_size: u16,
    /// List of timeframe strings supported by this connector (e.g. ["1m", "5m", "1h"]).
    pub supported_timeframes: Vec<String>,
    /// Whether the connector returns pre-aggregated OHLCV bars (vs tick-level).
    pub has_aggregated_bars: bool,
    // --- Auth & pricing ---
    /// Authentication scheme used by this connector (e.g. "API Key", "OAuth2", "None").
    pub auth_type: String,
    /// Whether an API key is required to access any functionality.
    pub requires_api_key: bool,
    /// Whether a free tier is available without payment.
    pub free_tier: bool,
    // --- Extended capabilities ---
    /// Whether the connector supports placing/cancelling orders.
    pub has_trading: bool,
    /// Whether the connector exposes account balance/info endpoints.
    pub has_account: bool,
    /// Whether the connector exposes positions (futures/margin).
    pub has_positions: bool,
    // --- Rate limits ---
    /// Max REST requests allowed per second (if known).
    pub rate_limit_per_second: Option<u32>,
    /// Max REST requests allowed per minute (if known).
    pub rate_limit_per_minute: Option<u32>,
    /// Weight-based limit per minute (Binance-style, if applicable).
    pub weight_per_minute: Option<u32>,
    // --- Endpoint URLs ---
    /// Base REST API URL.
    pub base_url: String,
    /// Primary WebSocket URL (empty string if WebSocket is not supported).
    pub ws_url: String,
    // --- Operational status strings ---
    /// REST health status: "unknown", "active", "inactive", or "error".
    pub rest_status: String,
    /// WebSocket status: "unknown", "available", "inactive", or "n/a".
    pub ws_status: String,
    // --- Live metrics ---
    /// Number of active WebSocket tasks for this exchange.
    pub ws_active_count: usize,
    /// Total HTTP requests made since the connector was created.
    pub http_requests_total: u64,
    /// Total HTTP errors encountered since the connector was created.
    pub http_errors_total: u64,
    /// Latency of the most recently completed HTTP request in milliseconds.
    pub last_latency_ms: u64,
    /// Current consumed rate-limiter weight for this window.
    pub rate_used: u32,
    /// Maximum rate-limiter weight allowed per window.
    pub rate_max: u32,
    /// Whether the extended metrics section is currently visible (user-toggled).
    pub show_metrics: bool,
    /// Copy of the last ≤60 metrics snapshots for this exchange, used by the
    /// sparkline renderer.  Populated from `SidebarState::metrics_history` just
    /// before render.
    pub metrics_history: Vec<MetricsSnapshot>,
    /// WebSocket ping round-trip time in milliseconds (0 = not measured yet).
    pub ws_ping_rtt_ms: u64,
    /// Per-group rate limit breakdown for connectors that use `GroupRateLimiter`.
    ///
    /// Each tuple is `(group_name, used, max)`.  Empty for single-limiter connectors.
    pub rate_groups: Vec<(String, u32, u32)>,
    /// Rate limiter window duration in seconds (from registry metadata).
    /// Used by the sparkline renderer to aggregate raw 10Hz samples into windows.
    pub rate_window_seconds: u32,
    /// Which UI group this connector belongs to (for panel section grouping).
    pub group: ConnectorGroup,
}

impl ConnectorStatusItem {
    /// Construct a minimal item with defaults (disabled, collapsed, unknown status).
    pub fn new(exchange_id: &str, display_name: &str) -> Self {
        Self {
            exchange_id: exchange_id.to_string(),
            display_name: display_name.to_string(),
            enabled: false,
            expanded: false,
            rest_healthy: false,
            ws_connected: false,
            has_klines: false,
            has_ws_klines: false,
            has_trades: false,
            has_ws_trades: false,
            has_orderbook: false,
            has_ws_orderbook: false,
            kline_batch_size: 0,
            supported_timeframes: Vec::new(),
            has_aggregated_bars: false,
            auth_type: String::new(),
            requires_api_key: false,
            free_tier: false,
            has_trading: false,
            has_account: false,
            has_positions: false,
            rate_limit_per_second: None,
            rate_limit_per_minute: None,
            weight_per_minute: None,
            base_url: String::new(),
            ws_url: String::new(),
            rest_status: "unknown".to_string(),
            ws_status: "unknown".to_string(),
            ws_active_count: 0,
            http_requests_total: 0,
            http_errors_total: 0,
            last_latency_ms: 0,
            rate_used: 0,
            rate_max: 0,
            show_metrics: false,
            metrics_history: Vec::new(),
            rate_window_seconds: 60,
            rate_groups: Vec::new(),
            ws_ping_rtt_ms: 0,
            group: ConnectorGroup::NoApiKey,
        }
    }
}
