//! Layout computation - platform-agnostic layout for charts
//!
//! This module provides layout computation for chart areas.
//! The chart library only cares about the available chart panel rect -
//! toolbars, sidebars, and other UI chrome are handled by the terminal.
//!
//! # Design
//!
//! The chart accepts a `Margins` struct that specifies how much space
//! is consumed by external UI elements. The chart then computes its
//! internal layout (chart area, price scale, time scale) within the
//! remaining space.
//!
//! # Terminal Integration
//!
//! In the terminal:
//! ```ignore
//! // Terminal computes margins based on its UI state
//! let margins = Margins {
//!     top: TOP_TOOLBAR_HEIGHT,
//!     left: LEFT_TOOLBAR_WIDTH,
//!     right: RIGHT_TOOLBAR_WIDTH + if sidebar_open { SIDEBAR_WIDTH } else { 0.0 },
//!     bottom: BOTTOM_TOOLBAR_HEIGHT + if bottom_panel_open { PANEL_HEIGHT } else { 0.0 },
//! };
//!
//! // Chart computes layout from window size and margins
//! let layout = FrameLayout::compute(window_width, window_height, &margins);
//! ```

use crate::types::{PRICE_SCALE_WIDTH, TIME_SCALE_HEIGHT};
use super::rects::{LayoutRect, ChartAreaLayout, FrameLayout, ExtendedFrameLayout, SubPaneLayout};

/// Margins around the chart area
///
/// These represent space consumed by external UI elements (toolbars, sidebars, etc.).
/// The chart library doesn't know or care what those elements are - it just knows
/// how much space they take.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    /// Space consumed at top (e.g., toolbar)
    pub top: f64,
    /// Space consumed at left (e.g., drawing toolbar)
    pub left: f64,
    /// Space consumed at right (e.g., sidebar)
    pub right: f64,
    /// Space consumed at bottom (e.g., status bar, trading panel)
    pub bottom: f64,
}

impl Margins {
    /// Create new margins
    pub const fn new(top: f64, left: f64, right: f64, bottom: f64) -> Self {
        Self { top, left, right, bottom }
    }

    /// Create zero margins (chart takes full window)
    pub const fn zero() -> Self {
        Self { top: 0.0, left: 0.0, right: 0.0, bottom: 0.0 }
    }

    /// Create uniform margins
    pub const fn uniform(margin: f64) -> Self {
        Self { top: margin, left: margin, right: margin, bottom: margin }
    }

    /// Total horizontal margin (left + right)
    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }

    /// Total vertical margin (top + bottom)
    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }
}

/// Configuration for layout computation
#[derive(Clone, Debug)]
pub struct LayoutConfig {
    /// Width of price scale
    pub price_scale_width: f64,
    /// Height of time scale
    pub time_scale_height: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            price_scale_width: PRICE_SCALE_WIDTH,
            time_scale_height: TIME_SCALE_HEIGHT,
        }
    }
}

impl FrameLayout {
    /// Compute frame layout from total window size and margins
    ///
    /// The margins represent space consumed by external UI (toolbars, sidebars).
    /// The chart panel is the remaining space after margins are applied.
    ///
    /// # Arguments
    /// * `total_width` - Total window/frame width
    /// * `total_height` - Total window/frame height
    /// * `margins` - Space consumed by external UI elements
    pub fn compute(total_width: f64, total_height: f64, margins: &Margins) -> Self {
        Self::compute_with_config(total_width, total_height, margins, &LayoutConfig::default())
    }

    /// Compute layout with custom configuration
    pub fn compute_with_config(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        config: &LayoutConfig,
    ) -> Self {
        let total = LayoutRect::new(0.0, 0.0, total_width, total_height);

        // Chart panel is the space after margins are applied
        let chart_panel = LayoutRect::new(
            margins.left,
            margins.top,
            (total_width - margins.horizontal()).max(0.0),
            (total_height - margins.vertical()).max(0.0),
        );

        // Subdivide chart panel into chart + scales
        let chart_area = ChartAreaLayout::compute(
            chart_panel,
            config.price_scale_width,
            config.time_scale_height,
        );

        Self {
            total,
            chart_area,
            chart_panel,
        }
    }

    /// Compute from just the chart panel rect (no margins)
    ///
    /// Use this when you already know the available chart panel rect.
    pub fn from_chart_panel(chart_panel: LayoutRect) -> Self {
        Self::from_chart_panel_with_config(chart_panel, &LayoutConfig::default())
    }

    /// Compute from chart panel with custom config
    pub fn from_chart_panel_with_config(chart_panel: LayoutRect, config: &LayoutConfig) -> Self {
        let chart_area = ChartAreaLayout::compute(
            chart_panel,
            config.price_scale_width,
            config.time_scale_height,
        );

        Self {
            total: chart_panel, // Total equals chart_panel when no margins
            chart_area,
            chart_panel,
        }
    }

    /// Get the chart rect (main drawing area for candles)
    #[inline]
    pub fn chart_rect(&self) -> LayoutRect {
        self.chart_area.chart
    }

    /// Get price scale rect
    #[inline]
    pub fn price_scale_rect(&self) -> LayoutRect {
        self.chart_area.price_scale
    }

    /// Get time scale rect
    #[inline]
    pub fn time_scale_rect(&self) -> LayoutRect {
        self.chart_area.time_scale
    }

    /// Get scale corner rect
    #[inline]
    pub fn scale_corner_rect(&self) -> LayoutRect {
        self.chart_area.scale_corner
    }

    /// Check if a point is in the chart area (excluding scales)
    pub fn point_in_chart(&self, x: f64, y: f64) -> bool {
        self.chart_area.chart.contains(x, y)
    }

    /// Check if a point is in the price scale
    pub fn point_in_price_scale(&self, x: f64, y: f64) -> bool {
        self.chart_area.price_scale.contains(x, y)
    }

    /// Check if a point is in the time scale
    pub fn point_in_time_scale(&self, x: f64, y: f64) -> bool {
        self.chart_area.time_scale.contains(x, y)
    }

    /// Check if a point is in the scale corner
    pub fn point_in_scale_corner(&self, x: f64, y: f64) -> bool {
        self.chart_area.scale_corner.contains(x, y)
    }
}

impl ExtendedFrameLayout {
    /// Compute extended layout with sub-panes for indicators
    ///
    /// # Arguments
    /// * `total_width` - Total window width
    /// * `total_height` - Total window height
    /// * `margins` - Space consumed by external UI elements
    /// * `sub_pane_instance_ids` - Instance IDs of indicators that need sub-panes
    /// * `sub_pane_height` - Height of each sub-pane (typically 100.0)
    /// * `separator_height` - Height of separator between panes (typically 4.0)
    pub fn compute(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
    ) -> Self {
        Self::compute_with_config(
            total_width,
            total_height,
            margins,
            sub_pane_instance_ids,
            sub_pane_height,
            separator_height,
            &LayoutConfig::default(),
        )
    }

    /// Compute with custom configuration
    pub fn compute_with_config(
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
        config: &LayoutConfig,
    ) -> Self {
        // First compute base frame layout
        let frame = FrameLayout::compute_with_config(total_width, total_height, margins, config);

        let sub_pane_count = sub_pane_instance_ids.len();

        if sub_pane_count == 0 {
            // No sub-panes - main chart gets entire chart_area
            return Self {
                main_chart: frame.chart_area,
                sub_panes: Vec::new(),
                total_chart_height: frame.chart_area.chart.height,
                frame,
            };
        }

        // Calculate total height needed for sub-panes
        // Each sub-pane has: separator + content
        let total_sub_panes_height = (sub_pane_height + separator_height) * sub_pane_count as f64;

        // Main chart height is reduced by sub-panes total
        let min_main_chart = if sub_pane_count > 0 {
            let reserved = (separator_height + 20.0) * sub_pane_count as f64;
            (frame.chart_area.chart.height - reserved).clamp(50.0, 200.0)
        } else {
            200.0_f64.min(frame.chart_area.chart.height)
        };
        let main_chart_height = (frame.chart_area.chart.height - total_sub_panes_height).max(min_main_chart);

        // Recalculate actual sub-pane height if main chart was clamped
        let actual_available_for_sub = frame.chart_area.chart.height - main_chart_height;
        let actual_pane_height = if sub_pane_count > 0 {
            (actual_available_for_sub - separator_height * sub_pane_count as f64) / sub_pane_count as f64
        } else {
            0.0
        };

        // Compute main chart area (reduced height)
        let main_chart = ChartAreaLayout {
            chart: LayoutRect::new(
                frame.chart_area.chart.x,
                frame.chart_area.chart.y,
                frame.chart_area.chart.width,
                main_chart_height,
            ),
            price_scale: LayoutRect::new(
                frame.chart_area.price_scale.x,
                frame.chart_area.price_scale.y,
                frame.chart_area.price_scale.width,
                main_chart_height,
            ),
            // Time scale and scale_corner remain at their original positions
            // (at the bottom of the full chart area)
            time_scale: frame.chart_area.time_scale,
            scale_corner: frame.chart_area.scale_corner,
        };

        // Total height for positioning time scale
        let total_chart_height = main_chart_height + actual_available_for_sub;

        // Compute sub-pane layouts
        let mut sub_panes = Vec::with_capacity(sub_pane_count);
        let mut current_y = frame.chart_area.chart.y + main_chart_height;

        for &instance_id in sub_pane_instance_ids {
            // Separator
            let separator = LayoutRect::new(
                frame.chart_area.chart.x,
                current_y,
                frame.chart_panel.width, // Full width including price scale
                separator_height,
            );
            current_y += separator_height;

            // Content area
            let content = LayoutRect::new(
                frame.chart_area.chart.x,
                current_y,
                frame.chart_area.chart.width, // Same width as main chart
                actual_pane_height,
            );

            // Price scale for this pane
            let price_scale = LayoutRect::new(
                frame.chart_area.price_scale.x,
                current_y,
                frame.chart_area.price_scale.width,
                actual_pane_height,
            );

            sub_panes.push(SubPaneLayout {
                instance_id,
                separator,
                content,
                price_scale,
            });

            current_y += actual_pane_height;
        }

        Self {
            frame,
            main_chart,
            sub_panes,
            total_chart_height,
        }
    }

    /// Find which sub-pane (if any) contains the given Y coordinate
    /// Returns (pane_index, local_y within pane)
    pub fn find_sub_pane_at_y(&self, y: f64) -> Option<(usize, f64)> {
        for (idx, pane) in self.sub_panes.iter().enumerate() {
            if pane.content.contains(pane.content.x, y) {
                let local_y = y - pane.content.y;
                return Some((idx, local_y));
            }
        }
        None
    }

    /// Check if Y coordinate is on a separator between panes.
    /// Returns the instance ID of the sub-pane whose separator was hit.  The
    /// hit zone is expanded to ±6 px around the separator centre so a 1-px
    /// separator line is reliably clickable.  Zero-height separators
    /// (maximized overlay) are skipped.
    pub fn find_separator_at_y(&self, y: f64) -> Option<u64> {
        const HIT_TOLERANCE: f64 = 6.0;
        for pane in &self.sub_panes {
            // Maximized panes have separator.height == 0 — no interactive separator.
            if pane.separator.height == 0.0 {
                continue;
            }
            let sep_center = pane.separator.y + pane.separator.height * 0.5;
            if (y - sep_center).abs() <= HIT_TOLERANCE {
                return Some(pane.instance_id);
            }
        }
        None
    }

    /// Get sub-pane layout by instance ID
    pub fn get_sub_pane_by_id(&self, instance_id: u64) -> Option<&SubPaneLayout> {
        self.sub_panes.iter().find(|p| p.instance_id == instance_id)
    }
}

/// Layout manager - convenience wrapper for computing layouts
///
/// This struct holds configuration and provides methods for computing layouts.
/// Use this when you need to compute layouts multiple times with the same config.
#[derive(Clone, Debug, Default)]
pub struct LayoutManager {
    config: LayoutConfig,
}

impl LayoutManager {
    /// Create a new layout manager with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom configuration
    pub fn with_config(config: LayoutConfig) -> Self {
        Self { config }
    }

    /// Get current configuration
    pub fn config(&self) -> &LayoutConfig {
        &self.config
    }

    /// Set configuration
    pub fn set_config(&mut self, config: LayoutConfig) {
        self.config = config;
    }

    /// Compute frame layout for given dimensions and margins
    pub fn compute_frame(
        &self,
        total_width: f64,
        total_height: f64,
        margins: &Margins,
    ) -> FrameLayout {
        FrameLayout::compute_with_config(total_width, total_height, margins, &self.config)
    }

    /// Compute extended frame layout with sub-panes
    pub fn compute_extended(
        &self,
        total_width: f64,
        total_height: f64,
        margins: &Margins,
        sub_pane_instance_ids: &[u64],
        sub_pane_height: f64,
        separator_height: f64,
    ) -> ExtendedFrameLayout {
        ExtendedFrameLayout::compute_with_config(
            total_width,
            total_height,
            margins,
            sub_pane_instance_ids,
            sub_pane_height,
            separator_height,
            &self.config,
        )
    }

    /// Compute only chart area from panel dimensions
    pub fn compute_chart_area(&self, panel_width: f64, panel_height: f64) -> ChartAreaLayout {
        ChartAreaLayout::compute(
            LayoutRect::new(0.0, 0.0, panel_width, panel_height),
            self.config.price_scale_width,
            self.config.time_scale_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_layout_no_margins() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1920.0, 1080.0, &margins);

        // With zero margins, chart panel should be full window
        assert_eq!(layout.chart_panel.x, 0.0);
        assert_eq!(layout.chart_panel.y, 0.0);
        assert_eq!(layout.chart_panel.width, 1920.0);
        assert_eq!(layout.chart_panel.height, 1080.0);
    }

    #[test]
    fn test_frame_layout_with_margins() {
        let margins = Margins::new(40.0, 50.0, 60.0, 30.0); // top, left, right, bottom
        let layout = FrameLayout::compute(1920.0, 1080.0, &margins);

        // Chart panel should be offset by margins
        assert_eq!(layout.chart_panel.x, 50.0); // left margin
        assert_eq!(layout.chart_panel.y, 40.0); // top margin
        assert_eq!(layout.chart_panel.width, 1920.0 - 50.0 - 60.0); // width - left - right
        assert_eq!(layout.chart_panel.height, 1080.0 - 40.0 - 30.0); // height - top - bottom
    }

    #[test]
    fn test_chart_area_subdivision() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1000.0, 800.0, &margins);

        // Chart + price_scale should equal panel width
        let chart_width = layout.chart_area.chart.width;
        let price_scale_width = layout.chart_area.price_scale.width;
        assert!((chart_width + price_scale_width - layout.chart_panel.width).abs() < 0.001);

        // Chart + time_scale should equal panel height
        let chart_height = layout.chart_area.chart.height;
        let time_scale_height = layout.chart_area.time_scale.height;
        assert!((chart_height + time_scale_height - layout.chart_panel.height).abs() < 0.001);

        // Scale corner should be at intersection
        assert_eq!(layout.chart_area.scale_corner.x, layout.chart_area.price_scale.x);
        assert_eq!(layout.chart_area.scale_corner.y, layout.chart_area.time_scale.y);
    }

    #[test]
    fn test_extended_layout_with_sub_panes() {
        let margins = Margins::zero();
        let sub_pane_ids = vec![1, 2];
        let layout = ExtendedFrameLayout::compute(
            1920.0, 1080.0, &margins, &sub_pane_ids, 100.0, 4.0,
        );

        // Should have 2 sub-panes
        assert_eq!(layout.sub_panes.len(), 2);

        // Main chart height should be reduced
        let main_chart_height = layout.main_chart.chart.height;
        assert!(main_chart_height < layout.frame.chart_area.chart.height);

        // First sub-pane separator should start at main chart bottom
        let main_chart_bottom = layout.main_chart.chart.bottom();
        assert!((layout.sub_panes[0].separator.y - main_chart_bottom).abs() < 0.001);

        // Second sub-pane separator should be after first pane content (or equal due to rounding)
        assert!(layout.sub_panes[1].separator.y >= layout.sub_panes[0].content.bottom() - 0.001);
    }

    #[test]
    fn test_hit_testing() {
        let margins = Margins::zero();
        let layout = FrameLayout::compute(1000.0, 800.0, &margins);

        // Chart center should be in chart
        let chart_center_x = layout.chart_area.chart.center_x();
        let chart_center_y = layout.chart_area.chart.center_y();
        assert!(layout.point_in_chart(chart_center_x, chart_center_y));

        // Price scale should be detected
        let price_scale_center_x = layout.chart_area.price_scale.center_x();
        let price_scale_center_y = layout.chart_area.price_scale.center_y();
        assert!(layout.point_in_price_scale(price_scale_center_x, price_scale_center_y));
        assert!(!layout.point_in_chart(price_scale_center_x, price_scale_center_y));
    }

    #[test]
    fn test_margins_helpers() {
        let margins = Margins::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(margins.horizontal(), 50.0); // left + right
        assert_eq!(margins.vertical(), 50.0); // top + bottom

        let uniform = Margins::uniform(15.0);
        assert_eq!(uniform.top, 15.0);
        assert_eq!(uniform.left, 15.0);
        assert_eq!(uniform.right, 15.0);
        assert_eq!(uniform.bottom, 15.0);
    }

    #[test]
    fn test_from_chart_panel() {
        let panel = LayoutRect::new(100.0, 50.0, 800.0, 600.0);
        let layout = FrameLayout::from_chart_panel(panel);

        assert_eq!(layout.chart_panel.x, 100.0);
        assert_eq!(layout.chart_panel.y, 50.0);
        assert_eq!(layout.chart_panel.width, 800.0);
        assert_eq!(layout.chart_panel.height, 600.0);
    }
}
