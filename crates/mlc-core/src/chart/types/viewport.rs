//! Viewport - chart coordinate system and transformations
//!
//! The viewport manages the mapping between data space (bar indices, prices)
//! and screen space (pixels). It handles panning, zooming, and coordinate
//! conversion for the chart.

use serde::{Deserialize, Serialize};

/// Time-domain X axis (additive, opt-in).
///
/// When a chart's data is non-uniform in time (raw trades, range/tick/volume
/// derived bars), the index-packed X axis mis-places everything because it
/// assumes one uniform slot per record. `TimeAxis` makes X a direct linear
/// function of the real timestamp: `x = (ts_ms - t_start_ms) / ms_per_px`.
///
/// This lives ALONGSIDE the legacy index fields. When `Viewport.time_axis` is
/// `Some`, the time-aware methods (`ts_to_x` / `x_to_ts`) drive rendering and
/// hit-testing for that chart; uniform time-bar charts leave it `None` and keep
/// the proven index-packed path (weekend gaps hidden, TradingView-style).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeAxis {
    /// Timestamp (ms) mapped to the left edge of the chart area (x = 0).
    pub t_start_ms: i64,
    /// Milliseconds per pixel (the zoom level in the time domain).
    /// Always > 0.
    pub ms_per_px: f64,
}

impl TimeAxis {
    /// Map a timestamp (ms) to an X pixel (left edge of chart area = 0).
    #[inline]
    pub fn ts_to_x(&self, ts_ms: i64) -> f64 {
        (ts_ms - self.t_start_ms) as f64 / self.ms_per_px
    }

    /// Map an X pixel back to a timestamp (ms).
    #[inline]
    pub fn x_to_ts(&self, x: f64) -> i64 {
        self.t_start_ms + (x * self.ms_per_px).round() as i64
    }
}

/// Viewport state for a chart
///
/// Manages the visible region of the chart and provides coordinate conversion
/// between data space (bar indices, prices) and screen space (pixels).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Viewport {
    /// Starting bar index (f64 to allow sub-bar panning)
    /// Can be negative (future space) or beyond data length (past space)
    pub view_start: f64,

    /// Pixels per bar (horizontal spacing)
    pub bar_spacing: f64,

    /// Ratio of bar body width to spacing (0.0 - 1.0)
    pub bar_width_ratio: f64,

    /// Width of the chart area in pixels (excluding price scale)
    pub chart_width: f64,

    /// Height of the chart area in pixels (excluding time scale)
    pub chart_height: f64,

    /// Total number of bars in the data
    pub bar_count: usize,

    /// Optional time-domain X axis for non-uniform-time data (raw trades,
    /// derived bars). `None` = legacy index-packed X (default, uniform bars).
    /// Serde-default so old snapshots deserialize as index-mode.
    #[serde(default)]
    pub time_axis: Option<TimeAxis>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            view_start: 0.0,
            bar_spacing: 8.0,
            bar_width_ratio: 0.8,
            chart_width: 0.0,
            chart_height: 0.0,
            bar_count: 0,
            time_axis: None,
        }
    }
}

impl Viewport {
    /// Minimum bar spacing in pixels (allows maximum zoom-out)
    pub const MIN_BAR_SPACING: f64 = 0.7;

    /// Maximum bar spacing as ratio of chart width (allows maximum zoom-in)
    pub const MAX_BAR_SPACING_RATIO: f64 = 0.5;

    /// Create a new viewport with the given dimensions
    pub fn new(chart_width: f64, chart_height: f64) -> Self {
        Self {
            chart_width,
            chart_height,
            ..Default::default()
        }
    }

    /// Get minimum bar spacing
    #[inline]
    pub fn min_bar_spacing(&self) -> f64 {
        Self::MIN_BAR_SPACING
    }

    /// Get maximum bar spacing (depends on chart width)
    #[inline]
    pub fn max_bar_spacing(&self) -> f64 {
        self.chart_width * Self::MAX_BAR_SPACING_RATIO
    }

    /// Calculate how many bars can fit in the visible area
    #[inline]
    pub fn visible_bars(&self) -> usize {
        ((self.chart_width / self.bar_spacing) as usize).max(1)
    }

    /// Convert f64 view_start to safe usize index (clamped to valid range)
    #[inline]
    pub fn view_start_idx(&self) -> usize {
        if self.view_start < 0.0 {
            0
        } else {
            (self.view_start as usize).min(self.bar_count.saturating_sub(1))
        }
    }

    /// Get visible range as (start_idx, end_idx) clamped to valid data
    ///
    /// Returns indices that can be used to iterate over visible bars.
    /// End index is exclusive (one past the last visible bar).
    /// Extends by 1 bar on each side so partially-visible bars (whose
    /// body/wick extends into the viewport) are still rendered.
    #[inline]
    pub fn visible_range(&self) -> (usize, usize) {
        let start = if self.view_start_idx() > 0 {
            self.view_start_idx() - 1
        } else {
            0
        };
        let visible_f = self.chart_width / self.bar_spacing;
        let end = ((self.view_start + visible_f).ceil() as usize + 1)
            .min(self.bar_count);
        (start, end)
    }

    /// Convert bar index to X pixel coordinate (center of bar)
    #[inline]
    pub fn bar_to_x(&self, bar_idx: usize) -> f64 {
        let relative_idx = bar_idx as f64 - self.view_start;
        relative_idx * self.bar_spacing + self.bar_spacing / 2.0
    }

    /// Convert fractional bar index to X pixel coordinate
    ///
    /// Supports sub-bar precision for smooth positioning of drawing primitives.
    #[inline]
    pub fn bar_to_x_f64(&self, bar_idx: f64) -> f64 {
        let relative_idx = bar_idx - self.view_start;
        relative_idx * self.bar_spacing + self.bar_spacing / 2.0
    }

    /// Convert X pixel coordinate to bar index
    ///
    /// Returns None if the coordinate is outside the chart area or
    /// doesn't correspond to a valid bar index.
    #[inline]
    pub fn x_to_bar(&self, x: f64) -> Option<usize> {
        if x < 0.0 || x > self.chart_width {
            return None;
        }
        let relative_idx = x / self.bar_spacing;
        let bar_idx = (self.view_start + relative_idx) as i64;
        if bar_idx >= 0 && (bar_idx as usize) < self.bar_count {
            Some(bar_idx as usize)
        } else {
            None
        }
    }

    /// Convert X pixel coordinate to bar index as f64 (for sub-bar precision)
    ///
    /// Returns the fractional bar index, useful for drawing primitives.
    #[inline]
    pub fn x_to_bar_f64(&self, x: f64) -> f64 {
        let relative_idx = x / self.bar_spacing;
        self.view_start + relative_idx
    }

    /// Convert price to Y pixel coordinate
    ///
    /// Uses inverted Y axis (price increases upward, Y increases downward).
    #[inline]
    pub fn price_to_y(&self, price: f64, price_min: f64, price_max: f64) -> f64 {
        let range = price_max - price_min;
        if range <= 0.0 {
            return self.chart_height / 2.0;
        }
        self.chart_height * (1.0 - (price - price_min) / range)
    }

    /// Convert Y pixel coordinate to price
    #[inline]
    pub fn y_to_price(&self, y: f64, price_min: f64, price_max: f64) -> f64 {
        let range = price_max - price_min;
        price_max - (y / self.chart_height) * range
    }

    /// Get the pixel width of a bar body
    #[inline]
    pub fn bar_width(&self) -> f64 {
        self.bar_spacing * self.bar_width_ratio
    }

    // ── Time-domain X axis (active only when `time_axis` is `Some`) ──────────

    /// `true` when this viewport positions X by real timestamp, not bar index.
    #[inline]
    pub fn is_time_axis(&self) -> bool {
        self.time_axis.is_some()
    }

    /// Enable time-axis mode, fitting `[t_start_ms, t_end_ms]` into the chart
    /// width with a small right margin (10% of the span) for incoming trades.
    /// Falls back to a 1-minute window if the span is degenerate.
    pub fn enable_time_axis_for_span(&mut self, t_start_ms: i64, t_end_ms: i64) {
        let span = (t_end_ms - t_start_ms).max(60_000); // >= 1 minute
        let margin = (span / 10).max(1);
        let total = span + margin;
        let width = self.chart_width.max(1.0);
        let ms_per_px = total as f64 / width;
        self.time_axis = Some(TimeAxis {
            t_start_ms,
            ms_per_px,
        });
    }

    /// Disable time-axis mode (back to index-packed X).
    #[inline]
    pub fn disable_time_axis(&mut self) {
        self.time_axis = None;
    }

    /// Map a timestamp (ms) to an X pixel. Returns `None` in index mode.
    #[inline]
    pub fn ts_to_x(&self, ts_ms: i64) -> Option<f64> {
        self.time_axis.map(|t| t.ts_to_x(ts_ms))
    }

    /// Map an X pixel back to a timestamp (ms). Returns `None` in index mode.
    #[inline]
    pub fn x_to_ts(&self, x: f64) -> Option<i64> {
        self.time_axis.map(|t| t.x_to_ts(x))
    }

    /// The (ts_start_ms, ts_end_ms) currently visible. `None` in index mode.
    #[inline]
    pub fn visible_time_range(&self) -> Option<(i64, i64)> {
        self.time_axis
            .map(|t| (t.t_start_ms, t.x_to_ts(self.chart_width)))
    }

    /// Pan the time axis by a pixel delta (drag). No-op in index mode.
    ///
    /// Dragging right (positive `pixel_delta`) moves the view into the past,
    /// mirroring `pan()`'s bar semantics (`view_start -= bar_delta`).
    pub fn pan_time(&mut self, pixel_delta: f64) {
        if let Some(t) = self.time_axis.as_mut() {
            t.t_start_ms -= (pixel_delta * t.ms_per_px).round() as i64;
        }
    }

    /// Zoom the time axis at a pixel, keeping the timestamp under the cursor
    /// fixed. `factor > 1.0` zooms in (fewer ms per pixel). No-op in index mode.
    pub fn zoom_time_at(&mut self, x: f64, factor: f64) {
        if let Some(t) = self.time_axis.as_mut() {
            // Timestamp under the cursor before the zoom.
            let ts_under = t.t_start_ms + (x * t.ms_per_px).round() as i64;
            // Zoom in => fewer ms per pixel.
            let new_ms_per_px = (t.ms_per_px / factor).max(f64::MIN_POSITIVE);
            t.ms_per_px = new_ms_per_px;
            // Re-anchor so the same timestamp stays under the cursor.
            t.t_start_ms = ts_under - (x * new_ms_per_px).round() as i64;
        }
    }

    /// Snap the time axis so the newest timestamp sits near the right edge,
    /// leaving `right_margin_px` of empty space. No-op in index mode.
    pub fn snap_time_to_end(&mut self, latest_ts_ms: i64, right_margin_px: f64) {
        if let Some(t) = self.time_axis.as_mut() {
            let viewport_ms = (self.chart_width * t.ms_per_px).round() as i64;
            let margin_ms = (right_margin_px * t.ms_per_px).round() as i64;
            t.t_start_ms = latest_ts_ms + margin_ms - viewport_ms;
        }
    }

    /// Pan the view by a number of bars (can be fractional)
    pub fn pan(&mut self, bar_delta: f64) {
        self.view_start -= bar_delta;
    }

    /// Zoom at a specific X coordinate, maintaining the bar under cursor
    ///
    /// Factor > 1.0 zooms in, factor < 1.0 zooms out.
    /// Uses viewport's built-in min/max spacing limits.
    pub fn zoom_at(&mut self, x: f64, factor: f64) {
        let old_spacing = self.bar_spacing;
        let min = self.min_bar_spacing();
        let max = self.max_bar_spacing();
        self.bar_spacing = (self.bar_spacing * factor).clamp(min, max);

        // Maintain bar index under cursor
        let bar_fraction = x / old_spacing;
        let bar_under_mouse = self.view_start + bar_fraction;
        let new_bar_fraction = x / self.bar_spacing;
        self.view_start = bar_under_mouse - new_bar_fraction;
    }

    /// Zoom at a specific X coordinate with custom min/max spacing
    ///
    /// Factor > 1.0 zooms in, factor < 1.0 zooms out.
    pub fn zoom_at_x(&mut self, x: f64, factor: f64, min_spacing: f64, max_spacing: f64) {
        let old_spacing = self.bar_spacing;
        self.bar_spacing = (self.bar_spacing * factor).clamp(min_spacing, max_spacing);

        // Maintain bar index under cursor
        let bar_fraction = x / old_spacing;
        let bar_under_mouse = self.view_start + bar_fraction;
        let new_bar_fraction = x / self.bar_spacing;
        self.view_start = bar_under_mouse - new_bar_fraction;
    }

    /// Scroll to show the most recent bars (right edge)
    pub fn scroll_to_end(&mut self) {
        self.view_start = (self.bar_count.saturating_sub(self.visible_bars())) as f64;
    }

    /// Clamp view_start to valid range (ensures we don't scroll beyond data)
    pub fn clamp_view_start(&mut self) {
        let max_start = (self.bar_count.saturating_sub(self.visible_bars())) as f64;
        if self.view_start > max_start {
            self.view_start = max_start;
        }
    }

    /// Fit all bars in the visible area (uses built-in limits)
    pub fn fit_all_bars(&mut self) {
        if self.bar_count > 0 {
            let min = self.min_bar_spacing();
            let max = self.max_bar_spacing();
            self.bar_spacing =
                (self.chart_width / self.bar_count as f64).clamp(min, max);
            self.view_start = 0.0;
        }
    }

    /// Fit all bars in the visible area with custom limits
    pub fn fit_all(&mut self, min_spacing: f64, max_spacing: f64) {
        if self.bar_count > 0 {
            self.bar_spacing =
                (self.chart_width / self.bar_count as f64).clamp(min_spacing, max_spacing);
            self.view_start = 0.0;
        }
    }

    /// Fit to default view: ~100 bars with right margin (uses built-in limits)
    ///
    /// Shows approximately 95 data bars + 5 empty bars on the right.
    pub fn reset_to_default(&mut self) {
        let min = self.min_bar_spacing();
        let max = self.max_bar_spacing();
        self.fit_default_view(min, max);
    }

    /// Fit to default view: ~100 bars with right margin for new data
    ///
    /// Shows approximately 95 data bars + 5 empty bars on the right.
    /// This provides a good default zoom level with space for incoming bars.
    ///
    /// # Arguments
    /// * `min_spacing` - Minimum bar spacing
    /// * `max_spacing` - Maximum bar spacing
    pub fn fit_default_view(&mut self, min_spacing: f64, max_spacing: f64) {
        const VISIBLE_DATA_BARS: usize = 95;
        const RIGHT_MARGIN_BARS: usize = 5;
        const TOTAL_VISIBLE_BARS: usize = VISIBLE_DATA_BARS + RIGHT_MARGIN_BARS;

        // Calculate bar_spacing to fit TOTAL_VISIBLE_BARS in chart width
        let new_spacing = self.chart_width / TOTAL_VISIBLE_BARS as f64;
        self.bar_spacing = new_spacing.clamp(min_spacing, max_spacing);

        // Position view so last bar has RIGHT_MARGIN_BARS of space on right
        let last_bar_idx = self.bar_count.saturating_sub(1);
        let right_edge_bar = last_bar_idx as f64 + RIGHT_MARGIN_BARS as f64;
        let visible_bars = self.chart_width / self.bar_spacing;
        self.view_start = right_edge_bar - visible_bars;
    }

    /// Compensate view_start when a right sidebar opens or closes.
    ///
    /// This keeps the right edge of the visible chart data in place while
    /// the chart area width changes. Call this BEFORE updating chart_width.
    ///
    /// - `opening`: true if sidebar is opening, false if closing
    /// - `sidebar_width`: width of the sidebar in pixels
    pub fn compensate_right_sidebar(&mut self, opening: bool, sidebar_width: f64) {
        if self.bar_spacing > 0.0 && sidebar_width > 0.0 {
            let bars_delta = sidebar_width / self.bar_spacing;
            if opening {
                // Opening sidebar: shift view right to keep right edge in place
                self.view_start += bars_delta;
            } else {
                // Closing sidebar: shift view left
                self.view_start -= bars_delta;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_bars() {
        let viewport = Viewport {
            chart_width: 800.0,
            bar_spacing: 10.0,
            ..Default::default()
        };
        assert_eq!(viewport.visible_bars(), 80);
    }

    #[test]
    fn test_bar_to_x() {
        let viewport = Viewport {
            view_start: 0.0,
            bar_spacing: 10.0,
            ..Default::default()
        };
        // First bar should be at center of first cell (5.0)
        assert!((viewport.bar_to_x(0) - 5.0).abs() < 0.001);
        // Second bar at 15.0
        assert!((viewport.bar_to_x(1) - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_x_to_bar() {
        let viewport = Viewport {
            view_start: 0.0,
            bar_spacing: 10.0,
            chart_width: 100.0,
            bar_count: 20,
            ..Default::default()
        };
        assert_eq!(viewport.x_to_bar(5.0), Some(0));
        assert_eq!(viewport.x_to_bar(15.0), Some(1));
        assert_eq!(viewport.x_to_bar(-5.0), None);
    }

    #[test]
    fn test_time_axis_maps_ts_to_x() {
        let mut vp = Viewport {
            chart_width: 1000.0,
            ..Default::default()
        };
        // 10 ms per pixel, t=0 at x=0.
        vp.time_axis = Some(TimeAxis { t_start_ms: 0, ms_per_px: 10.0 });
        assert!((vp.ts_to_x(0).unwrap() - 0.0).abs() < 1e-6);
        assert!((vp.ts_to_x(10_000).unwrap() - 1000.0).abs() < 1e-6); // right edge
        assert_eq!(vp.x_to_ts(500.0), Some(5_000));
        assert_eq!(vp.visible_time_range(), Some((0, 10_000)));
        // Index-mode viewport returns None.
        let idx_vp = Viewport::default();
        assert!(idx_vp.ts_to_x(123).is_none());
    }

    #[test]
    fn test_zoom_time_keeps_cursor_ts_fixed() {
        let mut vp = Viewport {
            chart_width: 1000.0,
            ..Default::default()
        };
        vp.time_axis = Some(TimeAxis { t_start_ms: 0, ms_per_px: 10.0 });
        // Cursor at x=400 → ts=4000 before zoom.
        let ts_before = vp.x_to_ts(400.0).unwrap();
        vp.zoom_time_at(400.0, 2.0); // zoom in 2x
        let ts_after = vp.x_to_ts(400.0).unwrap();
        assert_eq!(ts_before, ts_after, "ts under cursor must stay fixed on zoom");
        // ms_per_px halved on 2x zoom-in.
        assert!((vp.time_axis.unwrap().ms_per_px - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_snap_time_to_end() {
        let mut vp = Viewport {
            chart_width: 1000.0,
            ..Default::default()
        };
        vp.time_axis = Some(TimeAxis { t_start_ms: 0, ms_per_px: 10.0 });
        // latest ts = 50_000, margin 50px (=500ms). Viewport spans 10_000ms.
        vp.snap_time_to_end(50_000, 50.0);
        let (start, end) = vp.visible_time_range().unwrap();
        // end should be latest + margin = 50_500; start = end - 10_000 = 40_500.
        assert_eq!(start, 40_500);
        assert_eq!(end, 50_500);
    }

    #[test]
    fn test_price_to_y() {
        let viewport = Viewport {
            chart_height: 100.0,
            ..Default::default()
        };
        // At price_min, Y should be at bottom (chart_height)
        assert!((viewport.price_to_y(0.0, 0.0, 100.0) - 100.0).abs() < 0.001);
        // At price_max, Y should be at top (0)
        assert!((viewport.price_to_y(100.0, 0.0, 100.0) - 0.0).abs() < 0.001);
        // At midpoint
        assert!((viewport.price_to_y(50.0, 0.0, 100.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_y_to_price() {
        let viewport = Viewport {
            chart_height: 100.0,
            ..Default::default()
        };
        // At Y=0 (top), price should be max
        assert!((viewport.y_to_price(0.0, 0.0, 100.0) - 100.0).abs() < 0.001);
        // At Y=100 (bottom), price should be min
        assert!((viewport.y_to_price(100.0, 0.0, 100.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_visible_range() {
        let viewport = Viewport {
            view_start: 10.0,
            bar_spacing: 10.0,
            chart_width: 100.0,
            bar_count: 50,
            ..Default::default()
        };
        let (start, end) = viewport.visible_range();
        // visible_range backs up one bar (view_start_idx - 1) so the partially-
        // scrolled-in left bar is included → start = 9 for view_start 10.
        assert_eq!(start, 9);
        assert!(end <= 50);
        assert!(end > start);
    }
}
