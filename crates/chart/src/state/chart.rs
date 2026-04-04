//! Base Chart - core chart data and state
//!
//! This is the fundamental chart structure that can be used standalone
//! or extended by terminal with additional features (indicators, alerts, etc.)

use std::collections::HashMap;
use crate::{Bar, Viewport, PriceScale, TimeScale};
use crate::{Crosshair, CrosshairOptions, KineticState, DragMode};
use crate::{GridOptions, Legend, Watermark, Tooltip, PriceLine, MarkerManager};
use crate::drawing::DrawingManager;
use super::SubPane;

/// Base chart structure - contains all core chart functionality
///
/// Terminal extends this with ChartWindow which adds:
/// - Window management (id, title, is_active)
/// - Symbol/timeframe handling
/// - Indicators, alerts, signals, trades
/// - Multi-window sync
/// - Undo/redo history
pub struct Chart {
    // =========================================================================
    // Data
    // =========================================================================

    /// OHLCV bar data
    pub bars: Vec<Bar>,

    // =========================================================================
    // Viewport & Scales
    // =========================================================================

    /// Viewport state (pan/zoom)
    pub viewport: Viewport,
    /// Price scale (Y-axis)
    pub price_scale: PriceScale,
    /// Time scale (X-axis)
    pub time_scale: TimeScale,

    // =========================================================================
    // Layout
    // =========================================================================

    /// Sub-panes for indicators (layout only, no indicator logic)
    pub sub_panes: Vec<SubPane>,
    /// Total chart height including sub-panes
    pub total_chart_height: f64,

    // =========================================================================
    // Interaction State
    // =========================================================================

    /// Crosshair state
    pub crosshair: Crosshair,
    /// Kinetic scrolling state
    pub kinetic: KineticState,
    /// Current drag mode
    pub drag_mode: DragMode,
    /// Last mouse position
    pub last_mouse_pos: Option<(f32, f32)>,
    /// Drag start X position
    pub drag_start_x: f64,
    /// Drag start Y position
    pub drag_start_y: f64,
    /// Drag start view position
    pub drag_start_view: f64,
    /// Drag start bar spacing
    pub drag_start_spacing: f64,
    /// Drag start price min
    pub drag_start_price_min: f64,
    /// Drag start price max
    pub drag_start_price_max: f64,

    // =========================================================================
    // Display Options
    // =========================================================================

    /// Grid options
    pub grid_options: GridOptions,
    /// Crosshair options
    pub crosshair_options: CrosshairOptions,
    /// Legend
    pub legend: Legend,
    /// Watermark
    pub watermark: Option<Watermark>,
    /// Tooltip
    pub tooltip: Tooltip,

    // =========================================================================
    // Drawing Primitives
    // =========================================================================

    /// Drawing primitives manager (trend lines, shapes, etc.)
    pub drawing_manager: DrawingManager,
    /// Marker manager
    pub marker_manager: MarkerManager,
    /// Price lines (horizontal levels)
    pub price_lines: HashMap<String, PriceLine>,

    // =========================================================================
    // Series Display Flags
    // =========================================================================

    /// Show candlestick series
    pub show_candles: bool,
    /// Show bar series (OHLC bars)
    pub show_bars: bool,
    /// Show hollow candles
    pub show_hollow_candles: bool,
    /// Show Heikin-Ashi candles
    pub show_heikin_ashi: bool,
    /// Show line series
    pub show_line: bool,
    /// Show step line series
    pub show_step_line: bool,
    /// Show line markers
    pub show_line_markers: bool,
    /// Show area series
    pub show_area: bool,
    /// Show HLC area
    pub show_hlc_area: bool,
    /// Show histogram
    pub show_histogram: bool,
    /// Show columns
    pub show_columns: bool,
    /// Show baseline
    pub show_baseline: bool,
}

impl Default for Chart {
    fn default() -> Self {
        Self::new()
    }
}

impl Chart {
    /// Create a new empty chart
    pub fn new() -> Self {
        Self {
            // Data
            bars: Vec::new(),

            // Viewport & Scales
            viewport: Viewport::default(),
            price_scale: PriceScale::default(),
            time_scale: TimeScale::default(),

            // Layout
            sub_panes: Vec::new(),
            total_chart_height: 0.0,

            // Interaction
            crosshair: Crosshair::default(),
            kinetic: KineticState::default(),
            drag_mode: DragMode::None,
            last_mouse_pos: None,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_view: 0.0,
            drag_start_spacing: 0.0,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,

            // Display options
            grid_options: GridOptions::default(),
            crosshair_options: CrosshairOptions::default(),
            legend: Legend::default(),
            watermark: None,
            tooltip: Tooltip::default(),

            // Primitives
            drawing_manager: DrawingManager::new(),
            marker_manager: MarkerManager::new(),
            price_lines: HashMap::new(),

            // Series flags - default to candles only
            show_candles: true,
            show_bars: false,
            show_hollow_candles: false,
            show_heikin_ashi: false,
            show_line: false,
            show_step_line: false,
            show_line_markers: false,
            show_area: false,
            show_hlc_area: false,
            show_histogram: false,
            show_columns: false,
            show_baseline: false,
        }
    }

    /// Create chart with initial bars
    pub fn with_bars(bars: Vec<Bar>) -> Self {
        let mut chart = Self::new();
        chart.set_bars(bars);
        chart
    }

    /// Set bar data and recalculate scales
    pub fn set_bars(&mut self, bars: Vec<Bar>) {
        self.bars = bars;
        self.calc_auto_scale();
    }

    /// Get bar count
    pub fn bar_count(&self) -> usize {
        self.bars.len()
    }

    /// Check if chart has data
    pub fn has_data(&self) -> bool {
        !self.bars.is_empty()
    }

    /// Calculate auto-scale based on visible bars
    pub fn calc_auto_scale(&mut self) {
        if self.bars.is_empty() {
            return;
        }

        let (start, end) = self.viewport.visible_range();
        let start = start.max(0);
        let end = end.min(self.bars.len());

        if start >= end {
            return;
        }

        let mut min = f64::MAX;
        let mut max = f64::MIN;

        for bar in &self.bars[start..end] {
            min = min.min(bar.low);
            max = max.max(bar.high);
        }

        if min < max {
            let padding = (max - min) * 0.1;
            self.price_scale.price_min = min - padding;
            self.price_scale.price_max = max + padding;
        }
    }

    /// Get sub-pane by index
    pub fn get_sub_pane(&self, index: usize) -> Option<&SubPane> {
        self.sub_panes.get(index)
    }

    /// Get mutable sub-pane by index
    pub fn get_sub_pane_mut(&mut self, index: usize) -> Option<&mut SubPane> {
        self.sub_panes.get_mut(index)
    }

    /// Add a sub-pane
    pub fn add_sub_pane(&mut self, pane: SubPane) {
        self.sub_panes.push(pane);
    }

    /// Remove sub-pane by index
    pub fn remove_sub_pane(&mut self, index: usize) -> Option<SubPane> {
        if index < self.sub_panes.len() {
            Some(self.sub_panes.remove(index))
        } else {
            None
        }
    }

    /// Hide crosshair
    pub fn hide_crosshair(&mut self) {
        self.crosshair.visible = false;
    }

    /// Show crosshair at position
    pub fn show_crosshair(&mut self, x: f64, y: f64) {
        self.crosshair.visible = true;
        self.crosshair.x = x;
        self.crosshair.y = y;
    }

    /// Update crosshair position
    pub fn update_crosshair(&mut self, x: f64, y: f64) {
        if self.crosshair.visible {
            self.crosshair.x = x;
            self.crosshair.y = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_chart() {
        let chart = Chart::new();
        assert!(chart.bars.is_empty());
        assert!(chart.show_candles);
        assert!(!chart.show_line);
    }

    #[test]
    fn test_chart_with_bars() {
        let bars = vec![
            Bar { timestamp: 0, open: 100.0, high: 110.0, low: 90.0, close: 105.0, volume: 1000.0 },
            Bar { timestamp: 1, open: 105.0, high: 115.0, low: 95.0, close: 110.0, volume: 1200.0 },
        ];
        let chart = Chart::with_bars(bars);
        assert_eq!(chart.bar_count(), 2);
        assert!(chart.has_data());
    }

    #[test]
    fn test_sub_panes() {
        let mut chart = Chart::new();
        assert!(chart.sub_panes.is_empty());

        chart.add_sub_pane(SubPane::new(1)); // instance_id = 1
        assert_eq!(chart.sub_panes.len(), 1);

        chart.remove_sub_pane(0);
        assert!(chart.sub_panes.is_empty());
    }
}
