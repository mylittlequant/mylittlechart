//! Crosshair rendering
//!
//! Platform-agnostic crosshair (cursor tracking lines) rendering.
//!
//! # Functions
//!
//! - `draw_crosshair()` - Draw vertical and horizontal crosshair lines

use crate::render::{RenderContext, crisp};
use super::super::annotations::LineStyle;
use super::{ChartRenderState, draw_styled_line};

/// Configuration for crosshair rendering
#[derive(Clone, Debug)]
pub struct CrosshairConfig {
    /// Whether vertical line is visible
    pub vert_visible: bool,
    /// Vertical line width
    pub vert_width: f64,
    /// Vertical line style
    pub vert_style: LineStyle,
    /// Whether horizontal line is visible
    pub horz_visible: bool,
    /// Horizontal line width
    pub horz_width: f64,
    /// Horizontal line style
    pub horz_style: LineStyle,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            vert_visible: true,
            vert_width: 1.0,
            vert_style: LineStyle::Dashed,
            horz_visible: true,
            horz_width: 1.0,
            horz_style: LineStyle::Dashed,
        }
    }
}

/// Draw crosshair lines (vertical time line, horizontal price line)
///
/// The crosshair is drawn at the position stored in `state.crosshair`.
/// - **Vertical line**: extends from `total_chart_top` to `total_chart_bottom` (spans all panes)
/// - **Horizontal line**: only drawn if cursor is in main chart (pane_index = None)
///
/// When `is_dragging` is true (chart pan/zoom in progress), the crosshair
/// is always drawn with coordinates clamped to chart bounds, even if the
/// cursor is outside the chart area. This provides visual feedback during drag.
///
/// # Parameters
/// - `ctx` - Render context
/// - `state` - Chart render state (includes crosshair position)
/// - `config` - Crosshair configuration (visibility, width, style)
/// - `is_dragging` - Whether a chart drag is in progress (pan/zoom)
/// - `total_chart_top` - Y coordinate of top of main chart (for vertical line)
/// - `total_chart_bottom` - Y coordinate of bottom of all panes (for vertical line)
pub fn draw_crosshair(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    config: &CrosshairConfig,
    is_dragging: bool,
    total_chart_top: f64,
    total_chart_bottom: f64,
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let crosshair = state.crosshair;
    let theme = state.theme;
    let dpr = ctx.dpr();

    // Check both enabled (user toggle) and visible (mouse on chart)
    if !crosshair.enabled || !crosshair.visible {
        return;
    }

    // Calculate crosshair position
    // When dragging (primitive or chart), use crosshair.x directly since it's already clamped.
    // Otherwise, snap to bar center if bar_idx is available (within data bounds),
    // or use bar_f64 (extrapolated position beyond data bounds).
    let bar_x = if is_dragging {
        // Use pre-clamped x position directly during drag
        crosshair.x
    } else if let Some(bar_idx) = crosshair.bar_idx {
        viewport.bar_to_x(bar_idx)
    } else {
        // Out of data bounds: follow cursor pixel-perfect
        crosshair.x
    };

    // Get price Y coordinate (for horizontal line in main chart)
    // For the main chart (pane_index == None), re-derive Y from price using the corrected
    // viewport.chart_height. This fixes a stale chart_height bug on peer windows: when two
    // split windows have different numbers of sub-panes, set_crosshair_from_bar bakes
    // crosshair.y with the source window's chart_height, but the peer window may have a
    // different chart_height. Re-deriving here ensures the line appears at the correct
    // screen position in all windows.
    //
    // For sub-panes (pane_index == Some), keep using pre-baked Y since each pane has its
    // own scale and the baked value is already correct for that pane's coordinate space.
    let price_y = match crosshair.pane_index {
        None => {
            // Re-derive Y from price using the corrected viewport height
            let price = if crosshair.is_magnet() && !is_dragging {
                crosshair.snapped_price
            } else {
                crosshair.price
            };
            price_scale.price_to_y(price, viewport.chart_height)
        }
        Some(_) => {
            // Sub-pane: use pre-baked Y (set by sub-pane input handling)
            if crosshair.is_magnet() && !is_dragging {
                crosshair.snapped_y
            } else {
                crosshair.y
            }
        }
    };

    // Check bounds
    let x_in_bounds = bar_x >= 0.0 && bar_x <= viewport.chart_width;
    let y_in_bounds = price_y >= 0.0 && price_y <= viewport.chart_height;

    // During drag, always draw (with clamping). Otherwise, skip if completely outside.
    if !is_dragging && !x_in_bounds && !y_in_bounds {
        return;
    }

    // Clamp to visible area (always clamp, regardless of drag state)
    let bar_x = bar_x.clamp(1.0, (viewport.chart_width - 1.0).max(1.0));
    let price_y = price_y.clamp(1.0, (viewport.chart_height - 1.0).max(1.0));

    // Set crosshair color
    ctx.set_stroke_color(&theme.crosshair);

    // Draw vertical line (time) - spans ALL panes (main chart + sub-panes)
    // This line goes from top of main chart to bottom of last sub-pane (or main chart if no sub-panes)
    if config.vert_visible && (is_dragging || x_in_bounds) {
        ctx.set_stroke_width(config.vert_width);
        let cx = crisp(bar_x, dpr);
        let screen_x = rect.x + cx;

        draw_styled_line(
            ctx,
            screen_x, total_chart_top,
            screen_x, total_chart_bottom,
            &config.vert_style,
        );
    }

    // Draw horizontal line (price) - only for main chart (pane_index = None)
    // When crosshair is in a sub-pane, the horizontal line is drawn by draw_pane_crosshair
    if config.horz_visible && crosshair.pane_index.is_none() && (is_dragging || y_in_bounds) {
        ctx.set_stroke_width(config.horz_width);
        let cy = crisp(price_y, dpr);
        let screen_y = rect.y + cy;

        draw_styled_line(
            ctx,
            rect.x, screen_y,
            rect.right(), screen_y,
            &config.horz_style,
        );
    }

    // Draw virtual cursor dot when magnet-locked
    // This replaces the hidden system cursor at the snapped position
    if crosshair.is_snapped() && !is_dragging {
        let dot_x = rect.x + bar_x;
        let dot_y = rect.y + price_y; // price_y is already snapped_y in magnet mode
        let dot_radius = 4.0;

        // Outer circle (white border for visibility)
        ctx.set_fill_color("#ffffff");
        ctx.begin_path();
        ctx.arc(dot_x, dot_y, dot_radius + 1.0, 0.0, std::f64::consts::TAU);
        ctx.close_path();
        ctx.fill();

        // Inner circle (accent color)
        ctx.set_fill_color(&theme.crosshair);
        ctx.begin_path();
        ctx.arc(dot_x, dot_y, dot_radius, 0.0, std::f64::consts::TAU);
        ctx.close_path();
        ctx.fill();
    }
}

/// Draw horizontal crosshair line for a sub-pane
///
/// This only draws the HORIZONTAL line for sub-panes.
/// The vertical line is drawn by `draw_crosshair` which spans all panes.
///
/// When `is_dragging` is true, the crosshair is always drawn with
/// coordinates clamped to pane bounds.
///
/// # Parameters
/// - `ctx` - Render context
/// - `pane_rect` - Sub-pane rectangle
/// - `y_position` - Y position (in pane coordinates, already transformed for pane's scale)
/// - `config` - Crosshair configuration
/// - `crosshair_color` - Color for the crosshair
/// - `is_dragging` - Whether a chart drag is in progress
pub fn draw_pane_crosshair(
    ctx: &mut dyn RenderContext,
    pane_rect: &super::ChartRect,
    y_position: f64,
    config: &CrosshairConfig,
    crosshair_color: &str,
    is_dragging: bool,
) {
    let dpr = ctx.dpr();

    // Check bounds
    let y_in_bounds = y_position >= 0.0 && y_position <= pane_rect.height;

    // During drag, always draw (with clamping). Otherwise, skip if y is outside.
    if !is_dragging && !y_in_bounds {
        return;
    }

    // Clamp to visible area
    let y_position = y_position.clamp(1.0, (pane_rect.height - 1.0).max(1.0));

    ctx.set_stroke_color(crosshair_color);

    // Draw horizontal line (price/value) only
    // Vertical line is drawn by draw_crosshair which spans all panes
    if config.horz_visible && (is_dragging || y_in_bounds) {
        ctx.set_stroke_width(config.horz_width);
        let cy = crisp(y_position, dpr);
        let screen_y = pane_rect.y + cy;

        draw_styled_line(
            ctx,
            pane_rect.x, screen_y,
            pane_rect.right(), screen_y,
            &config.horz_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crosshair_config_default() {
        let config = CrosshairConfig::default();
        assert!(config.vert_visible);
        assert!(config.horz_visible);
        assert_eq!(config.vert_width, 1.0);
        assert_eq!(config.horz_width, 1.0);
    }
}
