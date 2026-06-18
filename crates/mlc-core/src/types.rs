//! Core types and constants for zengeld-chart
//!
//! This module contains all fundamental data structures, color constants,
//! layout constants, and helper functions used throughout the chart library.

// =============================================================================
// Re-exports for backwards compatibility
// =============================================================================

use serde::{Serialize, Deserialize};

/// Re-export DragMode from engine/input module for backwards compatibility.
///
/// New code should use `mlc_core::engine::input::DragMode` directly.
pub use crate::engine::input::DragMode;

// =============================================================================
// Chart Color Palette (Theme)
// =============================================================================

/// Legacy chart color theme - use UITheme for new code
///
/// This struct provides a simplified view of chart colors.
/// For full theme control, use `UITheme` which includes:
/// - UI colors (toolbars, buttons)
/// - Chart colors (grid, scales, crosshair)
/// - Series colors (candles, line, area, etc.)
/// - Full font configuration
///
/// To create from UITheme: `Theme::from_ui_theme(&ui_theme)`
#[derive(Clone, Debug)]
pub struct Theme {
    pub candle_up: &'static str,
    pub candle_down: &'static str,
    pub candle_up_wick: &'static str,
    pub candle_down_wick: &'static str,
    pub grid_color: &'static str,
    pub bg_color: &'static str,
    pub scale_bg: &'static str,
    pub scale_border: &'static str,
    pub text_color: &'static str,
    pub text_muted: &'static str,
    pub crosshair_color: &'static str,
    pub crosshair_label_bg: &'static str,
    pub ma_fast: &'static str,
    pub ma_slow: &'static str,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            candle_up: "#26a69a",
            candle_down: "#ef5350",
            candle_up_wick: "#26a69a",
            candle_down_wick: "#ef5350",
            grid_color: "#2a2e3999",
            bg_color: "#131722",
            scale_bg: "#1e222d",
            scale_border: "#2a2e39",
            text_color: "#b2b5be",
            text_muted: "#787b86",
            crosshair_color: "#758696",
            crosshair_label_bg: "#363a45",
            ma_fast: "#2962ff",
            ma_slow: "#ff6d00",
        }
    }
}

impl Theme {
    /// Create Theme from UITheme by extracting relevant chart/series colors
    ///
    /// This allows using the centralized UITheme while maintaining
    /// backwards compatibility with code using the legacy Theme struct.
    pub fn from_ui_theme(ui_theme: &crate::theme::UITheme) -> Self {
        Self {
            candle_up: ui_theme.series.candle_up_body,
            candle_down: ui_theme.series.candle_down_body,
            candle_up_wick: ui_theme.series.candle_up_wick,
            candle_down_wick: ui_theme.series.candle_down_wick,
            grid_color: ui_theme.chart.grid_line,
            bg_color: ui_theme.chart.background,
            scale_bg: ui_theme.chart.scale_bg,
            scale_border: ui_theme.chart.scale_border,
            text_color: ui_theme.chart.scale_text,
            text_muted: ui_theme.chart.scale_text_muted,
            crosshair_color: ui_theme.chart.crosshair_line,
            crosshair_label_bg: ui_theme.chart.crosshair_label_bg,
            ma_fast: ui_theme.series.ma_fast,
            ma_slow: ui_theme.series.ma_slow,
        }
    }

    /// Create dark theme (default)
    pub fn dark() -> Self {
        Self::default()
    }

    /// Create light theme
    pub fn light() -> Self {
        Self {
            candle_up: "#26a69a",
            candle_down: "#ef5350",
            candle_up_wick: "#26a69a",
            candle_down_wick: "#ef5350",
            grid_color: "#0000000f",
            bg_color: "#ffffff",
            scale_bg: "#f8f9fa",
            scale_border: "#dee2e6",
            text_color: "#434651",
            text_muted: "#787b86",
            crosshair_color: "#9598a1",
            crosshair_label_bg: "#131722",
            ma_fast: "#2962ff",
            ma_slow: "#ff6d00",
        }
    }
}

// =============================================================================
// Layout Constants
// =============================================================================

/// Height of the time scale area in pixels (CONSTANT)
pub const TIME_SCALE_HEIGHT: f64 = 30.0;

/// Font size for time scale labels
pub const TIME_SCALE_FONT_SIZE: f64 = 12.0;

/// Fixed width for price scale (CONSTANT)
pub const PRICE_SCALE_WIDTH: f64 = 70.0;

/// Max font size for price scale labels (when few digits)
pub const PRICE_SCALE_FONT_SIZE_MAX: f64 = 13.0;

/// Min font size for price scale labels (when many digits)
pub const PRICE_SCALE_FONT_SIZE_MIN: f64 = 9.0;

/// Default font size for price scale labels
pub const PRICE_SCALE_FONT_SIZE: f64 = 12.0;

/// Font specification for price scale (default)
pub const PRICE_SCALE_FONT: &str =
    "12px 'Trebuchet MS', Arial, sans-serif";

/// Border width for price scale
pub const PRICE_SCALE_BORDER_SIZE: f64 = 1.0;

/// Small tick mark length
pub const PRICE_SCALE_TICK_LENGTH: f64 = 3.0;

/// Padding between tick and text
pub const PRICE_SCALE_PADDING_INNER: f64 = 5.0;

/// Right edge padding
pub const PRICE_SCALE_PADDING_OUTER: f64 = 5.0;

/// Constant for label positioning
pub const PRICE_SCALE_LABEL_OFFSET: f64 = 5.0;

/// Minimum width for price scale (legacy, use PRICE_SCALE_WIDTH)
pub const PRICE_SCALE_MIN_WIDTH: f64 = 50.0;

// =============================================================================
// Sidebar & Toolbar Constants
// =============================================================================

/// Width of the left sidebar panel in pixels (main menu, account, settings)
pub const LEFT_SIDEBAR_WIDTH: f64 = 280.0;

/// Width of the right sidebar panel in pixels
pub const RIGHT_SIDEBAR_WIDTH: f64 = 340.0;

/// Height of the bottom sidebar panel in pixels
pub const BOTTOM_SIDEBAR_HEIGHT: f64 = 200.0;

/// Width of the right toolbar in pixels
pub const RIGHT_TOOLBAR_WIDTH: f64 = 50.0;

/// Width of the left toolbar in pixels
pub const LEFT_TOOLBAR_WIDTH: f64 = 50.0;

/// Height of the bottom toolbar in pixels
pub const BOTTOM_TOOLBAR_HEIGHT: f64 = 40.0;

/// Height of the top toolbar in pixels
pub const TOP_TOOLBAR_HEIGHT: f64 = 40.0;

/// Height of the status bar in pixels (0 = hidden/removed)
pub const STATUS_BAR_HEIGHT: f64 = 0.0;

// =============================================================================
// Data Structures
// =============================================================================

/// Per-price-level breakdown inside a footprint bar.
///
/// `buy_vol` = aggressive buy volume (market buy, lifted offer).
/// `sell_vol` = aggressive sell volume (market sell, hit bid).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct FootprintLevel {
    pub price: f64,
    pub buy_vol: f64,
    pub sell_vol: f64,
    /// `total_vol <= bar.max_total_vol * 0.05`. Renderer marks the cell
    /// with a dashed border (MQL5 Part 11 SinglePrint).
    #[serde(default)]
    pub is_single_print: bool,
    /// Level at bar high or low AND one side is zero. Renderer marks
    /// the cell edge with a dot (MQL5 Part 11 UnfinishedBusiness).
    #[serde(default)]
    pub is_unfinished: bool,
}

impl FootprintLevel {
    #[inline]
    pub fn total_vol(&self) -> f64 {
        self.buy_vol + self.sell_vol
    }
    #[inline]
    pub fn delta(&self) -> f64 {
        self.buy_vol - self.sell_vol
    }
}

/// One footprint bar — OHLCV plus per-price-level (buy_vol, sell_vol).
///
/// `open_time` is the bucket-start in milliseconds. Levels are sorted
/// ascending by price after construction.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FootprintBar {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub levels: Vec<FootprintLevel>,
    /// Running session-level CVD seeded from prior bar's close.
    /// Renderer plots a line below the cluster grid (F5).
    #[serde(default)]
    pub cumulative_delta: f64,
    /// Start index of a stacked-ask-imbalance run (3+ consecutive
    /// diagonal-imbalanced asks). Renderer draws tinted row fill +
    /// `▲` marker (F1).
    #[serde(default)]
    pub stacked_ask_start: Option<usize>,
    /// Length of the stacked-ask-imbalance run (= the actual streak
    /// length recorded by `compute_extras`). Renderer uses this to
    /// size the highlight band instead of assuming the default 3.
    #[serde(default)]
    pub stacked_ask_len: Option<usize>,
    /// Start index of a stacked-bid-imbalance run. Renderer draws
    /// tinted row fill + `▼` marker (F1).
    #[serde(default)]
    pub stacked_bid_start: Option<usize>,
    /// Length of the stacked-bid-imbalance run.
    #[serde(default)]
    pub stacked_bid_len: Option<usize>,
    /// `|delta| / total_vol <= 0.15` → absorption. Renderer paints a
    /// background tint + "ABS" label (F2).
    #[serde(default)]
    pub is_absorption: bool,
}

impl FootprintBar {
    /// Bar timestamp in milliseconds (axis-aware units).
    #[inline]
    pub fn ts_ms(&self) -> i64 {
        self.open_time
    }

    #[inline]
    pub fn is_bullish(&self) -> bool {
        self.close >= self.open
    }

    /// Total delta (sum of buy_vol - sell_vol across all levels).
    #[inline]
    pub fn total_delta(&self) -> f64 {
        self.levels.iter().map(|l| l.delta()).sum()
    }

    /// Index of the Point of Control — the level with maximum total volume.
    pub fn poc_idx(&self) -> Option<usize> {
        if self.levels.is_empty() {
            return None;
        }
        let mut best = 0usize;
        let mut best_vol = self.levels[0].total_vol();
        for (i, l) in self.levels.iter().enumerate().skip(1) {
            let v = l.total_vol();
            if v > best_vol {
                best_vol = v;
                best = i;
            }
        }
        Some(best)
    }

    /// Compute and store F1/F2/F4/F5 extras in-place after `levels` are
    /// finalised. `prev_cvd` is the carrying CVD value from the previous
    /// bar's `cumulative_delta` (0.0 for the first bar of a session).
    /// Stays defensive on tiny / empty bars.
    /// Set the running cumulative delta from the previous bar's CVD.
    /// Window-scoped — must be called per-window because each window
    /// has its own store and prior-bar reference.
    #[inline]
    pub fn set_cumulative_delta(&mut self, prev_cvd: f64) {
        self.cumulative_delta = prev_cvd + self.total_delta();
    }

    /// Compute F1-F4 extras (stacked imbalance, absorption, single
    /// print, unfinished auction). All inputs are bar-local; the
    /// resulting flags can be shared across every window that observes
    /// the same event. Caller must still call
    /// [`set_cumulative_delta`] per-window after this, because CVD
    /// carries across bars and the prior CVD differs per window store.
    pub fn compute_static_extras(
        &mut self,
        imbalance_ratio: f64,
        stack_count: usize,
        absorption_threshold: f64,
        single_print_threshold: f64,
        tick_size: f64,
    ) {
        self._compute_static_extras_inner(
            imbalance_ratio, stack_count, absorption_threshold,
            single_print_threshold, tick_size,
        );
    }

    /// Legacy combined entry point — sets CVD AND computes static
    /// extras in one shot. Kept for callers that still want the bundled
    /// form (e.g. one-window scenarios where the per-event split has
    /// no benefit).
    pub fn compute_extras(
        &mut self,
        prev_cvd: f64,
        imbalance_ratio: f64,
        stack_count: usize,
        absorption_threshold: f64,
        single_print_threshold: f64,
        tick_size: f64,
    ) {
        // CVD carry (F5).
        self.cumulative_delta = prev_cvd + self.total_delta();
        self._compute_static_extras_inner(
            imbalance_ratio, stack_count, absorption_threshold,
            single_print_threshold, tick_size,
        );
    }

    fn _compute_static_extras_inner(
        &mut self,
        imbalance_ratio: f64,
        stack_count: usize,
        absorption_threshold: f64,
        single_print_threshold: f64,
        tick_size: f64,
    ) {

        // Absorption (F2).
        self.is_absorption = if self.volume > 0.0 {
            (self.total_delta().abs() / self.volume) <= absorption_threshold
        } else { false };

        // Max total volume on this bar — used by F4 single print + F3 gate.
        let max_total: f64 = self
            .levels
            .iter()
            .map(|l| l.total_vol())
            .fold(0.0_f64, f64::max);

        // F4 single print + unfinished auction per level.
        let n = self.levels.len();
        let high = self.high;
        let low = self.low;
        let edge = tick_size * 0.5;
        for (i, lv) in self.levels.iter_mut().enumerate() {
            lv.is_single_print = max_total > 0.0
                && lv.total_vol() > 0.0
                && lv.total_vol() <= max_total * single_print_threshold;
            lv.is_unfinished = (((lv.price - high).abs() <= edge) || ((lv.price - low).abs() <= edge))
                && (lv.buy_vol == 0.0 || lv.sell_vol == 0.0);
            let _ = i;
            let _ = n;
        }

        // F1 stacked imbalance — ascending price order assumed.
        // We track the LONGEST run of each side and record both the
        // run's start index and its actual length. Renderer reads
        // (start, len) and sizes the highlight band exactly — fixes
        // the predecessor's hardcoded +2 (= 3-level) assumption that
        // broke when stack_count != 3.
        self.stacked_ask_start = None;
        self.stacked_ask_len = None;
        self.stacked_bid_start = None;
        self.stacked_bid_len = None;
        if n >= stack_count.max(2) {
            let max_buy = self.levels.iter().map(|l| l.buy_vol).fold(0.0_f64, f64::max);
            let max_sell = self.levels.iter().map(|l| l.sell_vol).fold(0.0_f64, f64::max);
            let mut run_ask: usize = 0;
            let mut best_ask: Option<(usize, usize)> = None;
            for i in 1..n {
                let ask = self.levels[i].buy_vol;
                let bid_below = self.levels[i - 1].sell_vol;
                if bid_below > 0.0
                    && ask / bid_below >= imbalance_ratio
                    && (max_buy <= 0.0 || ask >= 0.30 * max_buy)
                {
                    run_ask += 1;
                    if run_ask >= stack_count {
                        let cand = (i + 1 - run_ask, run_ask);
                        if best_ask.map(|(_, l)| run_ask > l).unwrap_or(true) {
                            best_ask = Some(cand);
                        }
                    }
                } else {
                    run_ask = 0;
                }
            }
            if let Some((s, l)) = best_ask {
                self.stacked_ask_start = Some(s);
                self.stacked_ask_len = Some(l);
            }
            let mut run_bid: usize = 0;
            let mut best_bid: Option<(usize, usize)> = None;
            for i in 0..(n - 1) {
                let bid = self.levels[i].sell_vol;
                let ask_above = self.levels[i + 1].buy_vol;
                if ask_above > 0.0
                    && bid / ask_above >= imbalance_ratio
                    && (max_sell <= 0.0 || bid >= 0.30 * max_sell)
                {
                    run_bid += 1;
                    if run_bid >= stack_count {
                        let cand = (i + 1 - run_bid, run_bid);
                        if best_bid.map(|(_, l)| run_bid > l).unwrap_or(true) {
                            best_bid = Some(cand);
                        }
                    }
                } else {
                    run_bid = 0;
                }
            }
            if let Some((s, l)) = best_bid {
                self.stacked_bid_start = Some(s);
                self.stacked_bid_len = Some(l);
            }
        }
    }

    /// Value Area: (low_idx, high_idx) — greedy bidirectional expansion
    /// from POC until cumulative volume >= 70% of total. Returns the
    /// pair `(low, high)` such that `levels[low..=high]` is the VA.
    /// Always expands toward the higher-volume neighbour.
    pub fn value_area(&self, pct: f64) -> Option<(usize, usize)> {
        let poc = self.poc_idx()?;
        let target = self.volume * pct.clamp(0.01, 1.0);
        if target <= 0.0 {
            return Some((poc, poc));
        }
        let mut lo = poc;
        let mut hi = poc;
        let mut acc = self.levels[poc].total_vol();
        let n = self.levels.len();
        while acc < target {
            let down_vol = if lo > 0 { self.levels[lo - 1].total_vol() } else { -1.0 };
            let up_vol   = if hi + 1 < n { self.levels[hi + 1].total_vol() } else { -1.0 };
            if down_vol < 0.0 && up_vol < 0.0 {
                break;
            }
            if up_vol >= down_vol {
                hi += 1;
                acc += up_vol.max(0.0);
            } else {
                lo -= 1;
                acc += down_vol.max(0.0);
            }
        }
        Some((lo, hi))
    }
}

// =============================================================================
// Renko / Point-and-Figure / Kagi / CVD / TPO renderer-facing types
// =============================================================================
//
// These mirror the dig3-station Point types but use mlc-native names
// (and only the fields the renderer needs). The bridge converts from
// station Point → these on event arrival, so the renderer never holds
// a station-typed reference.

/// One Renko brick mirroring `digdigdig3_station::data::RenkoBrickPoint`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenkoBrick {
    pub open_time: i64,
    pub bottom: f64,
    pub top: f64,
    pub up: bool,
    pub volume: f64,
    pub trades_count: u64,
}

impl RenkoBrick {
    #[inline] pub fn ts_ms(&self) -> i64 { self.open_time }
}

/// One Point-and-Figure column mirroring
/// `digdigdig3_station::data::PnfColumnPoint`. Renderer groups consecutive
/// emits with the same `column_id` into one column block (the same id
/// emits multiple times as the column extends).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PnfColumn {
    pub open_time: i64,
    pub column_id: u64,
    pub is_x: bool,
    pub bottom: f64,
    pub top: f64,
    pub volume: f64,
    pub trades_count: u64,
}

impl PnfColumn {
    #[inline] pub fn ts_ms(&self) -> i64 { self.open_time }
}

/// One Kagi segment / connector mirroring
/// `digdigdig3_station::data::KagiSegmentPoint`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KagiSegment {
    pub open_time: i64,
    pub start_price: f64,
    pub end_price: f64,
    pub up: bool,
    pub yang: bool,
    pub is_connector: bool,
}

impl KagiSegment {
    #[inline] pub fn ts_ms(&self) -> i64 { self.open_time }
}

/// One Three Line Break line mirroring
/// `digdigdig3_station::data::ThreeLineBreakLinePoint`.
///
/// `direction`: 0 = up, 1 = down.
/// `high = max(open, close)`, `low = min(open, close)` (no wicks by
/// construction — the algorithm only uses closes for boundaries).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreeLineBreakLine {
    pub ts_open: i64,
    pub ts_close: i64,
    pub price_open: f64,
    pub price_close: f64,
    pub volume: f64,
    pub direction: u8,
}

impl ThreeLineBreakLine {
    /// Returns the timestamp used for X-axis positioning (line open).
    #[inline]
    pub fn ts_ms(&self) -> i64 {
        self.ts_open
    }

    /// High of the line: `max(price_open, price_close)`.
    #[inline]
    pub fn high(&self) -> f64 {
        self.price_open.max(self.price_close)
    }

    /// Low of the line: `min(price_open, price_close)`.
    #[inline]
    pub fn low(&self) -> f64 {
        self.price_open.min(self.price_close)
    }
}

/// One Cumulative Volume Delta sample mirroring
/// `digdigdig3_station::data::ScalarBarPoint` but scoped to CVD usage.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CvdSample {
    pub ts_ms: i64,
    pub value: f64,
}

/// One TPO Market Profile session snapshot mirroring
/// `digdigdig3_station::data::TpoSessionPoint`. The renderer reads `rows`
/// to draw letter glyphs and `poc_price` / `vah_price` / `val_price` to
/// overlay the POC + Value Area horizontal lines.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TpoSession {
    pub open_time: i64,
    pub tick_size: f64,
    pub session_high: f64,
    pub session_low: f64,
    pub poc_price: f64,
    pub vah_price: f64,
    pub val_price: f64,
    /// `(price, letters)` rows sorted ascending by price.
    pub rows: Vec<(f64, Vec<char>)>,
}

impl TpoSession {
    #[inline] pub fn ts_ms(&self) -> i64 { self.open_time }
}

/// Pre-computed Heikin Ashi bar; cache element used by the renderer
/// memo on `ChartContainer` so HA values aren't recomputed from index 0
/// every frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct HaBar {
    pub ts_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// OHLCV bar data with timestamp
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Bar {
    /// Unix timestamp in seconds
    pub timestamp: i64,
    /// Opening price
    pub open: f64,
    /// Highest price
    pub high: f64,
    /// Lowest price
    pub low: f64,
    /// Closing price
    pub close: f64,
    /// Trading volume
    pub volume: f64,
}

impl Bar {
    /// Create a new bar with the given values (without volume, defaults to 0)
    pub fn new(timestamp: i64, open: f64, high: f64, low: f64, close: f64) -> Self {
        Self {
            timestamp,
            open,
            high,
            low,
            close,
            volume: 0.0,
        }
    }

    /// Create a new bar with volume (OHLCV)
    pub fn with_volume(timestamp: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self {
            timestamp,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    /// Bar timestamp in milliseconds (the unit used by the time axis
    /// and by every drawing primitive's `ts_ms` field).
    ///
    /// Storage stays in seconds (existing snapshots load 1:1, candle-
    /// boundary math in the live aggregation path stays in seconds).
    /// Callers that need the time-axis unit go through this one method
    /// instead of repeating `* 1000` everywhere.
    #[inline]
    pub fn ts_ms(&self) -> i64 {
        self.timestamp.saturating_mul(1000)
    }

    /// Returns true if this bar closed higher than it opened
    #[inline]
    pub fn is_bullish(&self) -> bool {
        self.close >= self.open
    }

    /// Returns the body size (absolute difference between open and close)
    #[inline]
    pub fn body_size(&self) -> f64 {
        (self.close - self.open).abs()
    }

    /// Returns the full range (high - low)
    #[inline]
    pub fn range(&self) -> f64 {
        self.high - self.low
    }
}

// =============================================================================
// Pixel-Perfect Helpers
// =============================================================================

/// Snap a coordinate to pixel boundaries for crisp line rendering
///
/// This ensures 1px lines render sharply by aligning to half-pixel offsets,
/// accounting for the device pixel ratio (DPR).
#[inline]
pub fn crisp(coord: f64, dpr: f64) -> f64 {
    (coord * dpr).floor() / dpr + 0.5 / dpr
}

/// Snap a rectangle to pixel boundaries for crisp rendering
///
/// Returns (x, y, width, height) with coordinates aligned to device pixels.
/// Ensures minimum 1px dimensions even at high DPR.
#[inline]
pub fn crisp_rect(x: f64, y: f64, w: f64, h: f64, dpr: f64) -> (f64, f64, f64, f64) {
    let bx = (x * dpr).floor() / dpr;
    let by = (y * dpr).floor() / dpr;
    let bw = ((x + w) * dpr).floor() / dpr - bx;
    let bh = ((y + h) * dpr).floor() / dpr - by;
    (bx, by, bw.max(1.0 / dpr), bh.max(1.0 / dpr))
}

// =============================================================================
// Timestamp-Bar Conversion
// =============================================================================

/// Find the bar index for a given timestamp.
///
/// Uses binary search to find the bar whose time interval contains the timestamp.
/// If timestamp is before the first bar, returns 0.
/// If timestamp is after the last bar, returns the last bar index.
///
/// # Arguments
/// * `bars` - Slice of bars sorted by timestamp (ascending)
/// * `timestamp` - Unix timestamp in seconds to find
///
/// # Returns
/// * `Some(index)` - Index of the bar containing or nearest to the timestamp
///   (can be >= bars.len() for future timestamps via extrapolation)
/// * `None` - If bars slice is empty
#[inline]
pub fn find_bar_for_timestamp(bars: &[Bar], timestamp: i64) -> Option<usize> {
    if bars.is_empty() {
        return None;
    }

    let last_bar = &bars[bars.len() - 1];

    // If timestamp is after last bar, extrapolate
    if timestamp > last_bar.timestamp {
        // Calculate bar interval
        let interval = if bars.len() >= 2 {
            last_bar.timestamp - bars[bars.len() - 2].timestamp
        } else {
            3600 // Default 1 hour
        };

        if interval > 0 {
            let bars_beyond = (timestamp - last_bar.timestamp) / interval;
            return Some(bars.len() - 1 + bars_beyond as usize);
        } else {
            return Some(bars.len() - 1);
        }
    }

    // Binary search: find first bar with timestamp > target
    let idx = bars.partition_point(|b| b.timestamp <= timestamp);

    if idx == 0 {
        // Timestamp is before or at first bar
        Some(0)
    } else {
        // Return the bar that contains this timestamp (previous one)
        Some(idx - 1)
    }
}

/// Compute the bar interval in seconds from a bars slice.
///
/// Returns the difference between the last two bar timestamps,
/// or 3600 (1 hour) as a default when fewer than 2 bars are available.
#[inline]
pub fn bar_interval_seconds(bars: &[Bar]) -> i64 {
    if bars.len() >= 2 {
        bars[bars.len() - 1].timestamp - bars[bars.len() - 2].timestamp
    } else {
        3600
    }
}

/// Find the bar index for a given timestamp in **milliseconds**.
///
/// Converts `ts_ms` to seconds and delegates to `find_bar_for_timestamp`.
#[inline]
pub fn find_bar_for_timestamp_ms(bars: &[Bar], ts_ms: i64) -> Option<usize> {
    find_bar_for_timestamp(bars, ts_ms / 1000)
}

/// Convert a timestamp in **milliseconds** to a fractional bar index.
///
/// Returns `0.0` when `bars` is empty.
/// Extrapolates RIGHT beyond the last bar and LEFT before the first bar using
/// the bar interval, so rays and extended lines anchored outside the loaded
/// window project correctly instead of collapsing to bar 0.
/// Negative return values are valid: `bar_to_x_f64` renders them off-screen left.
#[inline]
pub fn timestamp_ms_to_bar_f64(bars: &[Bar], ts_ms: i64) -> f64 {
    if bars.is_empty() {
        return 0.0;
    }
    let first_ts_ms = bars[0].timestamp * 1000;
    if ts_ms < first_ts_ms {
        // Pre-history: extrapolate left using the bar interval.
        let interval_ms = bar_interval_seconds(bars) * 1000;
        if interval_ms > 0 {
            return (ts_ms - first_ts_ms) as f64 / interval_ms as f64;
        }
        return 0.0;
    }
    match find_bar_for_timestamp_ms(bars, ts_ms) {
        Some(idx) => idx as f64,
        None => 0.0,
    }
}

/// Convert a fractional bar index to a timestamp in **milliseconds**.
///
/// Used to round-trip from bar position back to timestamp for drag math.
#[inline]
pub fn bar_f64_to_timestamp_ms(bars: &[Bar], bar_f64: f64) -> i64 {
    let interval_ms = bar_interval_seconds(bars) * 1000;
    let idx = bar_f64.floor() as i64;
    if bars.is_empty() {
        return 0;
    }
    if idx < 0 {
        let before = (-idx) as i64;
        return bars[0].timestamp * 1000 - before * interval_ms;
    }
    let idx_u = idx as usize;
    if idx_u < bars.len() {
        bars[idx_u].timestamp * 1000
    } else {
        let beyond = (idx_u - (bars.len() - 1)) as i64;
        bars[bars.len() - 1].timestamp * 1000 + beyond * interval_ms
    }
}

/// Convert bar index to timestamp using the bars array.
///
/// # Arguments
/// * `bars` - Slice of bars
/// * `bar_idx` - Bar index to convert
///
/// # Returns
/// * `Some(timestamp)` - Timestamp of the bar at given index
/// * `None` - If index is out of bounds or bars is empty
#[inline]
pub fn bar_to_timestamp(bars: &[Bar], bar_idx: usize) -> Option<i64> {
    bars.get(bar_idx).map(|b| b.timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_is_bullish() {
        let bullish = Bar::new(0, 100.0, 110.0, 95.0, 105.0);
        let bearish = Bar::new(0, 105.0, 110.0, 95.0, 100.0);
        let doji = Bar::new(0, 100.0, 105.0, 95.0, 100.0);

        assert!(bullish.is_bullish());
        assert!(!bearish.is_bullish());
        assert!(doji.is_bullish()); // Equal close/open is considered bullish
    }

    #[test]
    fn test_crisp() {
        // At DPR 1.0, should add 0.5 offset
        let result = crisp(10.0, 1.0);
        assert!((result - 10.5).abs() < 0.001);

        // At DPR 2.0, should align to half-pixels
        let result = crisp(10.3, 2.0);
        assert!((result - 10.25).abs() < 0.001);
    }

    #[test]
    fn test_crisp_rect() {
        let (x, y, w, h) = crisp_rect(10.3, 20.7, 50.5, 30.2, 1.0);
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
        // Width and height should be whole pixels at DPR 1.0
        assert!(w >= 1.0);
        assert!(h >= 1.0);
    }

    #[test]
    fn test_drag_mode_reexport() {
        // Verify that DragMode is accessible and works correctly
        let mode = DragMode::Chart;
        assert!(mode.is_dragging());
        assert!(mode.affects_view());

        let none = DragMode::None;
        assert!(!none.is_dragging());
    }

    #[test]
    fn test_find_bar_for_timestamp() {
        let bars = vec![
            Bar::new(1000, 100.0, 110.0, 95.0, 105.0),
            Bar::new(2000, 105.0, 115.0, 100.0, 110.0),
            Bar::new(3000, 110.0, 120.0, 105.0, 115.0),
            Bar::new(4000, 115.0, 125.0, 110.0, 120.0),
        ];

        // Empty bars
        assert_eq!(find_bar_for_timestamp(&[], 1500), None);

        // Exact match
        assert_eq!(find_bar_for_timestamp(&bars, 2000), Some(1));

        // Between bars - should return the bar containing the timestamp
        assert_eq!(find_bar_for_timestamp(&bars, 2500), Some(1));

        // Before first bar
        assert_eq!(find_bar_for_timestamp(&bars, 500), Some(0));

        // After last bar - extrapolates to future index
        // bars[3].timestamp = 4000, interval = 1000, so timestamp 5000 = bar 4
        assert_eq!(find_bar_for_timestamp(&bars, 5000), Some(4));
        // timestamp 6000 = bar 5
        assert_eq!(find_bar_for_timestamp(&bars, 6000), Some(5));
    }

    #[test]
    fn test_bar_to_timestamp() {
        let bars = vec![
            Bar::new(1000, 100.0, 110.0, 95.0, 105.0),
            Bar::new(2000, 105.0, 115.0, 100.0, 110.0),
        ];

        assert_eq!(bar_to_timestamp(&bars, 0), Some(1000));
        assert_eq!(bar_to_timestamp(&bars, 1), Some(2000));
        assert_eq!(bar_to_timestamp(&bars, 2), None);
        assert_eq!(bar_to_timestamp(&[], 0), None);
    }
}
