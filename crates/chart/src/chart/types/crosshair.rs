//! Crosshair - cursor tracking and magnet snap functionality
//!
//! The crosshair follows the mouse cursor and optionally snaps to
//! OHLC values (magnet mode) for precise price reading.

use crate::Bar;
use serde::{Deserialize, Serialize};

// Re-export LineStyle from price_line for convenience
pub use super::super::annotations::price_line::LineStyle;

// =============================================================================
// Crosshair Mode
// =============================================================================

/// Crosshair behavior mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum CrosshairMode {
    /// Follows mouse exactly (no magnet)
    #[default]
    Normal = 0,
    /// Strong magnet: snaps to candle body (open + close)
    Magnet = 1,
    /// Hidden crosshair
    Hidden = 2,
    /// Light magnet: snaps to nearest OHLC value (body + pivots)
    MagnetOHLC = 3,
}

// =============================================================================
// Crosshair Line Options
// =============================================================================

/// Options for a single crosshair line (vertical or horizontal)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrosshairLineOptions {
    /// Line color
    pub color: String,

    /// Line width (1-4)
    #[serde(default = "default_line_width")]
    pub width: f64,

    /// Line style
    #[serde(default)]
    pub style: LineStyle,

    /// Visibility of line
    #[serde(default = "default_true")]
    pub visible: bool,

    /// Visibility of scale label
    #[serde(default = "default_true")]
    pub label_visible: bool,

    /// Label background color
    pub label_background_color: String,
}

fn default_line_width() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

impl Default for CrosshairLineOptions {
    fn default() -> Self {
        Self {
            color: "#758696".to_string(),
            width: 1.0,
            style: LineStyle::LargeDashed,
            visible: true,
            label_visible: true,
            label_background_color: "#363a45".to_string(),
        }
    }
}

// =============================================================================
// Crosshair Options
// =============================================================================

/// Complete crosshair configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrosshairOptions {
    /// Crosshair mode
    #[serde(default)]
    pub mode: CrosshairMode,

    /// Vertical line options
    #[serde(default)]
    pub vert_line: CrosshairLineOptions,

    /// Horizontal line options
    #[serde(default)]
    pub horz_line: CrosshairLineOptions,
}

impl Default for CrosshairOptions {
    fn default() -> Self {
        Self {
            mode: CrosshairMode::MagnetOHLC,
            vert_line: CrosshairLineOptions::default(),
            horz_line: CrosshairLineOptions::default(),
        }
    }
}

// =============================================================================
// Crosshair
// =============================================================================

/// Crosshair state for cursor tracking
#[derive(Clone, Copy, Debug)]
pub struct Crosshair {
    /// Whether crosshair is enabled (user toggle)
    pub enabled: bool,
    /// Whether the crosshair is currently visible (mouse on chart)
    pub visible: bool,
    /// Current X pixel coordinate (mouse position)
    pub x: f64,
    /// Current Y pixel coordinate (mouse position, local to pane)
    pub y: f64,
    /// Bar index under cursor (if any - None when outside data bounds)
    pub bar_idx: Option<usize>,
    /// Extrapolated bar index as f64 (works beyond data bounds)
    /// Used for rendering crosshair when scrolled past data edges
    pub bar_f64: f64,
    /// Price at cursor Y position (from pane's price scale)
    pub price: f64,
    /// Current crosshair mode
    pub mode: CrosshairMode,
    /// Magnet-snapped Y position (may differ from mouse Y in Magnet mode)
    pub snapped_y: f64,
    /// Magnet-snapped price (OHLC value in Magnet mode)
    pub snapped_price: f64,
    /// Which pane the crosshair is in: None = main chart, Some(n) = sub-pane n
    pub pane_index: Option<usize>,
    /// True when crosshair position was set via sync (not local mouse).
    /// Renderer should skip horizontal line since Y is not meaningful.
    pub synced: bool,
}

impl Default for Crosshair {
    fn default() -> Self {
        Self {
            enabled: true,  // Enabled by default
            visible: false,
            x: 0.0,
            y: 0.0,
            bar_idx: None,
            bar_f64: 0.0,
            price: 0.0,
            mode: CrosshairMode::default(),
            snapped_y: 0.0,
            snapped_price: 0.0,
            pane_index: None,
            synced: false,
        }
    }
}

impl Crosshair {
    /// Create a new crosshair with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle magnet ON/OFF (single-click behavior).
    ///
    /// - If magnet is OFF (Normal): turn ON to MagnetOHLC (default weak magnet).
    /// - If magnet is ON (MagnetOHLC or Magnet): turn OFF (Normal).
    ///
    /// Use `set_magnet_mode` to switch between MagnetOHLC and Magnet (via dropdown).
    pub fn toggle_magnet(&mut self) {
        self.mode = match self.mode {
            CrosshairMode::Normal | CrosshairMode::Hidden => CrosshairMode::MagnetOHLC,
            CrosshairMode::MagnetOHLC | CrosshairMode::Magnet => CrosshairMode::Normal,
        };
    }

    /// Set magnet mode directly (used by double-click dropdown selections).
    ///
    /// Calling this with `Normal` or `Hidden` turns magnet off.
    pub fn set_magnet_mode(&mut self, mode: CrosshairMode) {
        self.mode = mode;
    }

    /// Check if magnet mode is active
    #[inline]
    pub fn is_magnet(&self) -> bool {
        matches!(self.mode, CrosshairMode::Magnet | CrosshairMode::MagnetOHLC)
    }

    /// Check if magnet snap is currently active (cursor is locked to OHLC)
    /// This is true when magnet mode is on AND snapped_price differs from raw price
    #[inline]
    pub fn is_snapped(&self) -> bool {
        self.is_magnet() && (self.snapped_y - self.y).abs() > 2.0
    }

    /// Update crosshair position
    ///
    /// Call this on mouse move to update the crosshair state.
    /// - `bar_idx`: discrete bar index (None when outside data bounds)
    /// - `bar_f64`: extrapolated fractional bar index (works beyond data bounds)
    pub fn update(&mut self, x: f64, y: f64, bar_idx: Option<usize>, bar_f64: f64, price: f64, visible: bool) {
        self.x = x;
        self.y = y;
        self.bar_idx = bar_idx;
        self.bar_f64 = bar_f64;
        self.price = price;
        self.visible = visible;

        // Reset snapped values to current values (may be updated by find_nearest_ohlc)
        self.snapped_y = y;
        self.snapped_price = price;
    }

    /// Update crosshair position with auto_scale awareness
    ///
    /// In auto mode (A): Y is clamped to visible chart area - the crosshair
    /// stays within screen bounds because the price scale auto-adjusts.
    ///
    /// In manual mode (M): Y tracks the price coordinate and can go off-screen
    /// when dragging, since the price scale is fixed.
    ///
    /// - `auto_scale`: true for A mode (auto), false for M mode (manual)
    /// - `chart_height`: height of the chart area for clamping
    pub fn update_with_clamp(
        &mut self,
        x: f64,
        y: f64,
        bar_idx: Option<usize>,
        bar_f64: f64,
        price: f64,
        visible: bool,
        auto_scale: bool,
        chart_height: f64,
    ) {
        self.x = x;
        self.bar_idx = bar_idx;
        self.bar_f64 = bar_f64;
        self.price = price;
        self.visible = visible;

        // In auto mode: clamp Y to visible area (crosshair can't leave screen)
        // In manual mode: Y follows the price coordinate (can go off-screen during drag)
        self.y = if auto_scale {
            y.clamp(0.0, chart_height.max(0.0))
        } else {
            y
        };

        // Reset snapped values to current values (may be updated by find_nearest_ohlc)
        self.snapped_y = self.y;
        self.snapped_price = price;
    }

    /// Find nearest OHLC value to the given price (for magnet mode)
    ///
    /// Returns (snapped_price, snapped_y). The `price_to_y` callback converts
    /// price to Y pixel coordinate.
    pub fn find_nearest_ohlc<F>(
        &self,
        bar: &Bar,
        mouse_price: f64,
        price_to_y: F,
        bar_spacing: f64,
    ) -> (f64, f64)
    where
        F: Fn(f64) -> f64,
    {
        match self.mode {
            CrosshairMode::Normal | CrosshairMode::Hidden => {
                (mouse_price, price_to_y(mouse_price))
            }
            CrosshairMode::Magnet => {
                // Strong magnet: snap to candle body (open + close)
                // Tolerance: 40% of bar width, clamped to [20, 80]px
                let tolerance = (bar_spacing * 0.4).clamp(20.0, 80.0);
                let mouse_y = price_to_y(mouse_price);
                let prices = [bar.open, bar.close];
                let mut nearest_price = mouse_price;
                let mut min_pixel_dist = f64::INFINITY;

                for &p in &prices {
                    let p_y = price_to_y(p);
                    let pixel_dist = (p_y - mouse_y).abs();
                    if pixel_dist < min_pixel_dist {
                        min_pixel_dist = pixel_dist;
                        nearest_price = p;
                    }
                }

                if min_pixel_dist <= tolerance {
                    (nearest_price, price_to_y(nearest_price))
                } else {
                    (mouse_price, mouse_y)
                }
            }
            CrosshairMode::MagnetOHLC => {
                // Light magnet: snap to nearest OHLC value (body + pivots)
                // Tolerance: 25% of bar width, clamped to [10, 50]px
                let tolerance = (bar_spacing * 0.25).clamp(10.0, 50.0);
                let mouse_y = price_to_y(mouse_price);
                let prices = [bar.open, bar.high, bar.low, bar.close];
                let mut nearest_price = mouse_price;
                let mut min_pixel_dist = f64::INFINITY;

                for &p in &prices {
                    let p_y = price_to_y(p);
                    let pixel_dist = (p_y - mouse_y).abs();
                    if pixel_dist < min_pixel_dist {
                        min_pixel_dist = pixel_dist;
                        nearest_price = p;
                    }
                }

                if min_pixel_dist <= tolerance {
                    (nearest_price, price_to_y(nearest_price))
                } else {
                    (mouse_price, mouse_y)
                }
            }
        }
    }

    /// Update snapped values for magnet mode
    ///
    /// Call this after find_nearest_ohlc to update the crosshair state.
    pub fn set_snapped(&mut self, price: f64, y: f64) {
        self.snapped_price = price;
        self.snapped_y = y;
    }

    /// Get the effective Y position (snapped if in Magnet mode during non-drag)
    #[inline]
    pub fn effective_y(&self, is_dragging: bool) -> f64 {
        if is_dragging || !self.is_magnet() {
            self.y
        } else {
            self.snapped_y
        }
    }

    /// Get the effective price (snapped if in Magnet mode during non-drag)
    #[inline]
    pub fn effective_price(&self, is_dragging: bool) -> f64 {
        if is_dragging || !self.is_magnet() {
            self.price
        } else {
            self.snapped_price
        }
    }

    /// Hide the crosshair (e.g., when mouse leaves chart area)
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Show the crosshair
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Calculate tooltip position based on crosshair coordinates
    ///
    /// This is THE single source of truth for tooltip positioning.
    ///
    /// Parameters:
    /// - `is_dragging`: whether chart is being dragged (affects which Y to use)
    /// - `chart_width`, `chart_height`: chart dimensions
    /// - `tooltip_width`, `tooltip_height`: tooltip dimensions
    /// - `offset_x`, `offset_y`: offset from cursor
    ///
    /// Returns (x, y) position for tooltip with boundary handling.
    pub fn get_tooltip_position(
        &self,
        is_dragging: bool,
        chart_width: f64,
        chart_height: f64,
        tooltip_width: f64,
        tooltip_height: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> (f64, f64) {
        self.get_tooltip_position_at(
            self.x,
            is_dragging,
            chart_width,
            chart_height,
            tooltip_width,
            tooltip_height,
            offset_x,
            offset_y,
        )
    }

    /// Calculate tooltip position at a specific X coordinate
    ///
    /// Use this when you need to anchor the tooltip to a specific X position
    /// (e.g., bar position during drag) rather than the crosshair X.
    ///
    /// Parameters:
    /// - `anchor_x`: X coordinate to anchor tooltip to
    /// - `is_dragging`: whether chart is being dragged (affects which Y to use)
    /// - `chart_width`, `chart_height`: chart dimensions
    /// - `tooltip_width`, `tooltip_height`: tooltip dimensions
    /// - `offset_x`, `offset_y`: offset from anchor
    ///
    /// Returns (x, y) position for tooltip with boundary handling.
    pub fn get_tooltip_position_at(
        &self,
        anchor_x: f64,
        is_dragging: bool,
        chart_width: f64,
        chart_height: f64,
        tooltip_width: f64,
        tooltip_height: f64,
        offset_x: f64,
        offset_y: f64,
    ) -> (f64, f64) {
        // Use effective Y (snapped or raw depending on mode and drag state)
        let anchor_y = self.effective_y(is_dragging);

        let mut x = anchor_x + offset_x;
        let mut y = anchor_y + offset_y;

        // Check right boundary - flip to left
        if x + tooltip_width > chart_width {
            x = anchor_x - tooltip_width - offset_x;
        }

        // Check bottom boundary - flip to top
        if y + tooltip_height > chart_height {
            y = anchor_y - tooltip_height - offset_y;
        }

        // Clamp to visible area (ensure max bounds are non-negative)
        x = x.clamp(0.0, (chart_width - tooltip_width).max(0.0));
        y = y.clamp(0.0, (chart_height - tooltip_height).max(0.0));

        (x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_style_dash_pattern() {
        assert_eq!(LineStyle::Solid.dash_pattern(1.0), Vec::<f64>::new());
        assert_eq!(LineStyle::LargeDashed.dash_pattern(1.0), vec![6.0, 6.0]);
        assert_eq!(LineStyle::Dotted.dash_pattern(1.0), vec![1.0, 1.0]);
    }

    #[test]
    fn test_crosshair_default() {
        let crosshair = Crosshair::default();
        assert!(!crosshair.visible);
        assert_eq!(crosshair.mode, CrosshairMode::Normal);
        assert!(!crosshair.is_magnet());
    }

    #[test]
    fn test_crosshair_magnet_mode() {
        let mut crosshair = Crosshair::default();
        crosshair.mode = CrosshairMode::Magnet;
        assert!(crosshair.is_magnet());
    }

    #[test]
    fn test_toggle_magnet() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::Normal;

        // OFF -> ON: should become MagnetOHLC (default weak magnet)
        crosshair.toggle_magnet();
        assert_eq!(crosshair.mode, CrosshairMode::MagnetOHLC);
        assert!(crosshair.is_magnet());

        // ON (MagnetOHLC) -> OFF
        crosshair.toggle_magnet();
        assert_eq!(crosshair.mode, CrosshairMode::Normal);
        assert!(!crosshair.is_magnet());

        // When strong magnet is on, toggle OFF should also return to Normal
        crosshair.mode = CrosshairMode::Magnet;
        crosshair.toggle_magnet();
        assert_eq!(crosshair.mode, CrosshairMode::Normal);
        assert!(!crosshair.is_magnet());
    }

    #[test]
    fn test_find_nearest_ohlc_magnet_mode() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::Magnet;
        let bar = Bar::new(0, 100.0, 110.0, 90.0, 105.0);

        let price_to_y = |p: f64| 200.0 - p;

        // In Magnet mode, should snap to nearest of open/close (body)
        // Mouse at 103.0 -> closest body value is close (105.0), dist=2px < 20px
        let (price, y) = crosshair.find_nearest_ohlc(&bar, 103.0, price_to_y, 8.0);
        assert!((price - 105.0).abs() < 0.001); // close is nearest body value
        assert!((y - 95.0).abs() < 0.001); // 200 - 105 = 95

        // Mouse at 101.0 -> closest body value is open (100.0), dist=1px < 20px
        let (price2, _y2) = crosshair.find_nearest_ohlc(&bar, 101.0, price_to_y, 8.0);
        assert!((price2 - 100.0).abs() < 0.001); // open is nearest body value
    }

    #[test]
    fn test_find_nearest_ohlc_magnet_ohlc_mode() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::MagnetOHLC;
        let bar = Bar::new(0, 100.0, 110.0, 90.0, 105.0);
        let price_to_y = |p: f64| 200.0 - p;

        // Mouse at 109.0 should snap to high (110.0)
        let (price, _y) = crosshair.find_nearest_ohlc(&bar, 109.0, price_to_y, 8.0);
        assert!((price - 110.0).abs() < 0.001);

        // Mouse at 91.0 should snap to low (90.0)
        let (price, _y) = crosshair.find_nearest_ohlc(&bar, 91.0, price_to_y, 8.0);
        assert!((price - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_find_nearest_ohlc_no_ma_snap() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::MagnetOHLC;
        let bar = Bar::new(0, 100.0, 110.0, 90.0, 105.0);
        let price_to_y = |p: f64| 200.0 - p;

        // Mouse at 97.0 should snap to nearest OHLC
        // Nearest OHLC is low=90.0 (dist=7px) or open=100.0 (dist=3px)
        // open is nearest at 3px < 10px tolerance
        let (price, _y) = crosshair.find_nearest_ohlc(&bar, 97.0, price_to_y, 8.0);
        assert!((price - 100.0).abs() < 0.001); // open
    }

    #[test]
    fn test_effective_values() {
        let mut crosshair = Crosshair::new();
        crosshair.y = 100.0;
        crosshair.price = 50.0;
        crosshair.snapped_y = 95.0;
        crosshair.snapped_price = 55.0;
        crosshair.mode = CrosshairMode::Magnet;

        // Not dragging - should use snapped values
        assert!((crosshair.effective_y(false) - 95.0).abs() < 0.001);
        assert!((crosshair.effective_price(false) - 55.0).abs() < 0.001);

        // Dragging - should use raw values
        assert!((crosshair.effective_y(true) - 100.0).abs() < 0.001);
        assert!((crosshair.effective_price(true) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_crosshair_options_default() {
        let opts = CrosshairOptions::default();
        assert_eq!(opts.mode, CrosshairMode::MagnetOHLC);
        assert!(opts.vert_line.visible);
        assert!(opts.horz_line.visible);
        assert_eq!(opts.vert_line.width, 1.0);
    }

    #[test]
    fn test_get_tooltip_position_normal() {
        let mut crosshair = Crosshair::new();
        crosshair.x = 100.0;
        crosshair.y = 100.0;
        crosshair.snapped_y = 100.0;

        // Tooltip at cursor + offset, no flip needed
        let (x, y) = crosshair.get_tooltip_position(
            false, // not dragging
            800.0, // chart_width
            600.0, // chart_height
            150.0, // tooltip_width
            100.0, // tooltip_height
            10.0,  // offset_x
            10.0,  // offset_y
        );

        assert_eq!(x, 110.0); // 100 + 10
        assert_eq!(y, 110.0); // 100 + 10
    }

    #[test]
    fn test_get_tooltip_position_flip_right() {
        let mut crosshair = Crosshair::new();
        crosshair.x = 750.0;
        crosshair.y = 100.0;
        crosshair.snapped_y = 100.0;

        // Cursor near right edge - should flip to left
        let (x, _y) = crosshair.get_tooltip_position(
            false,
            800.0, // chart_width
            600.0, 150.0, // tooltip_width
            100.0, 10.0, 10.0,
        );

        // At x=750 + 10 + 150 = 910 > 800, flip to: 750 - 150 - 10 = 590
        assert!((x - 590.0).abs() < 0.1);
    }

    #[test]
    fn test_get_tooltip_position_flip_bottom() {
        let mut crosshair = Crosshair::new();
        crosshair.x = 100.0;
        crosshair.y = 550.0;
        crosshair.snapped_y = 550.0;

        // Cursor near bottom edge - should flip to top
        let (_x, y) = crosshair.get_tooltip_position(
            false,
            800.0,
            600.0, // chart_height
            150.0,
            100.0, // tooltip_height
            10.0, 10.0,
        );

        // At y=550 + 10 + 100 = 660 > 600, flip to: 550 - 100 - 10 = 440
        assert!((y - 440.0).abs() < 0.1);
    }

    #[test]
    fn test_get_tooltip_position_uses_snapped_y_in_magnet_mode() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::Magnet;
        crosshair.x = 100.0;
        crosshair.y = 200.0; // raw cursor Y
        crosshair.snapped_y = 150.0; // snapped Y (closer to OHLC)

        // Not dragging - should use snapped_y
        let (_x, y) = crosshair.get_tooltip_position(false, 800.0, 600.0, 150.0, 100.0, 10.0, 10.0);

        // Should be snapped_y + offset = 150 + 10 = 160
        assert_eq!(y, 160.0);
    }

    #[test]
    fn test_get_tooltip_position_uses_raw_y_when_dragging() {
        let mut crosshair = Crosshair::new();
        crosshair.mode = CrosshairMode::Magnet;
        crosshair.x = 100.0;
        crosshair.y = 200.0; // raw cursor Y
        crosshair.snapped_y = 150.0; // snapped Y

        // Dragging - should use raw y, not snapped
        let (_x, y) = crosshair.get_tooltip_position(
            true, // is_dragging
            800.0, 600.0, 150.0, 100.0, 10.0, 10.0,
        );

        // Should be raw y + offset = 200 + 10 = 210
        assert_eq!(y, 210.0);
    }

    #[test]
    fn test_get_tooltip_position_clamp_to_bounds() {
        let mut crosshair = Crosshair::new();
        crosshair.x = 0.0;
        crosshair.y = 0.0;
        crosshair.snapped_y = 0.0;

        // Cursor at top-left corner with negative offset result
        let (x, y) = crosshair.get_tooltip_position(false, 800.0, 600.0, 150.0, 100.0, 10.0, 10.0);

        // Should be clamped to 0, not negative
        assert!(x >= 0.0);
        assert!(y >= 0.0);
    }
}
