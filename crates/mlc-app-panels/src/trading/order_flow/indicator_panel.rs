//! Standalone indicator-visualization panel.
//!
//! `IndicatorPanel` renders a single indicator (by `instance_id`) outside of
//! any chart container.  Carries its own symbol / exchange / account_type +
//! viewport + Y-axis price scale.  Subscribes to bars for its symbol/exchange/
//! account_type via the shared bar-service ring; calculates the indicator
//! values on those bars; renders the value series with line / histogram /
//! reference-lines per `RenderingMetadata` from mli.
//!
//! Distinct from chart-internal `SubPane`: a SubPane inherits X-axis / bars /
//! crosshair from its parent ChartWindow, while an IndicatorPanel is fully
//! standalone — same level as DOM, TradeTape, BigTrades.

use serde::{Deserialize, Serialize};

use mlc_core::Bar;

use crate::panel_theme::PanelTheme;
use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// IndicatorPanel id.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndicatorPanelId(pub u64);

/// Heavy state for a single standalone IndicatorPanel.
#[derive(Clone)]
pub struct IndicatorPanelState {
    // === Sync-group fields (mirror DOM/TradeTape) ===
    /// Symbol being monitored (e.g. "BTCUSDT").
    pub symbol: String,
    /// Exchange identifier string (e.g. "binance").
    pub exchange: String,
    /// Account type short label (e.g. "S", "F", "FI").
    pub account_type: String,

    // === Indicator identity ===
    /// Which mli indicator to compute and render (matches a `BarIndicatorId`
    /// instance from the indicator manager).
    pub instance_id: u64,
    /// Snapshot of indicator parameters (config-id from tag_manager's
    /// `IndicatorGroupConfig`).  `0` until bound to a config.
    pub indicator_config_id: u64,

    // === Data feed ===
    /// Bars for this panel's symbol/exchange/account_type.  Owned by the
    /// panel — the data-tick path in chart-app populates this from the
    /// shared bar pool (same source the chart pulls from), so the
    /// indicator always renders against the same bar series as a matching
    /// chart would.  Vec rather than Arc<RwLock<>> because no cross-thread
    /// sharing happens here (single-threaded render loop).
    pub bars: Vec<Bar>,
    /// Last seen `BarSeries::version` we synced from.
    pub last_seen_bar_version: u64,

    // === Viewport (own pan/zoom, can sync via group) ===
    /// Bars-per-screen-pixel.  Same convention as `ChartWindow.viewport.bar_spacing`.
    pub bar_spacing: f64,
    /// View position — bar index of the leftmost visible bar.
    pub view_position: f64,

    // === Y-axis state ===
    /// Current Y-axis minimum (price-units, indicator-units, or 0..N depending on indicator).
    pub price_min: f64,
    /// Current Y-axis maximum.
    pub price_max: f64,
    /// Auto-rescale Y-axis on each frame from the visible indicator values.
    pub auto_scale: bool,

    // === Crosshair (synced via group) ===
    /// Crosshair X bar index (None = no crosshair shown).
    pub crosshair_bar: Option<f64>,
    /// Crosshair Y value (price/indicator-units).
    pub crosshair_value: Option<f64>,

    // === Layout ===
    /// Fraction of containing leaf height used by this panel (1.0 = fill).
    /// Reserved for future TabbedIndicatorPanels grouping; ignored for solo
    /// leaves.
    pub height_ratio: f32,
}

impl std::fmt::Debug for IndicatorPanelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndicatorPanelState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("instance_id", &self.instance_id)
            .field("indicator_config_id", &self.indicator_config_id)
            .field("bars_len", &self.bars.len())
            .field("last_seen_bar_version", &self.last_seen_bar_version)
            .field("price_min", &self.price_min)
            .field("price_max", &self.price_max)
            .field("auto_scale", &self.auto_scale)
            .finish()
    }
}

impl IndicatorPanelState {
    /// Create a fresh, unbound IndicatorPanel state.  Symbol/exchange/
    /// account_type default to empty (filled in by the spawn-handler from
    /// the active chart's sync group); `instance_id` is `0` until the user
    /// picks an indicator.
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
            instance_id: 0,
            indicator_config_id: 0,
            bars: Vec::new(),
            last_seen_bar_version: 0,
            bar_spacing: 8.0,
            view_position: 0.0,
            price_min: 0.0,
            price_max: 100.0,
            auto_scale: true,
            crosshair_bar: None,
            crosshair_value: None,
            height_ratio: 1.0,
        }
    }

    /// Create with a pre-bound indicator instance.
    pub fn with_instance(symbol: String, instance_id: u64) -> Self {
        Self {
            symbol,
            instance_id,
            ..Self::new()
        }
    }
}

impl Default for IndicatorPanelState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TradingPanel impl — minimal stub
//
// Stage 4 wires the storage + dispatcher.  Full indicator render (axis,
// line/histogram, reference lines from RenderingMetadata) lives in
// `chart::layout::render_chart::render_indicator_panel_leaf` (Stage 4
// follow-up).  This impl provides only the trait floor required by
// `TradingPanelsStore::get_panel` dispatch.
// =============================================================================

impl TradingPanel for IndicatorPanelState {
    fn kind(&self) -> &'static str {
        "indicator_panel"
    }

    fn label(&self) -> &'static str {
        "Indicator"
    }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &PanelTheme,
        _coordinator: &mut uzor::InputCoordinator,
        _slot_prefix: &str,
    ) {
        // Background fill.
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // Placeholder label until the real indicator render path lands.
        let label = if self.instance_id == 0 {
            "Indicator: (unbound — pick one)".to_string()
        } else if self.symbol.is_empty() {
            format!("Indicator #{} (no symbol)", self.instance_id)
        } else {
            format!("Indicator #{} · {}", self.instance_id, self.symbol)
        };
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, (x + w / 2.0) as f64, (y + h / 2.0) as f64);
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool {
        false
    }
}
