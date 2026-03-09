//! Line and area series rendering
//!
//! Platform-agnostic line chart, area chart, and histogram rendering.
//!
//! # Series Types
//!
//! - `draw_line_series()` - Simple line connecting close prices
//! - `draw_area_series()` - Filled area under line
//! - `draw_histogram()` - Volume/histogram bars
//! - `draw_baseline_series()` - Split fill above/below baseline
//! - `draw_step_line()` - Staircase pattern line
//! - `draw_line_with_markers()` - Line with dot markers

use crate::render::RenderContext;
use super::ChartRenderState;

/// Draw simple line series using bar close prices
pub fn draw_line_series(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    // Use candle_up color for line (or could add line_color to theme)
    let line_color = &theme.candle_up;

    // Collect points
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y));
    }

    if points.len() < 2 {
        return;
    }

    // Draw line
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

/// Draw area series (filled area under line) using bar close prices
pub fn draw_area_series(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    let line_color = &theme.candle_up;
    let bottom_y = rect.bottom();

    // Collect top points
    let mut top_points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        top_points.push((x, y));
    }

    if top_points.len() < 2 {
        return;
    }

    // Draw filled area using vertical strips for non-convex polygon
    ctx.set_fill_color_alpha(line_color, 0.3);
    for i in 0..top_points.len() - 1 {
        let (x1, y1) = top_points[i];
        let (x2, y2) = top_points[i + 1];

        ctx.begin_path();
        ctx.move_to(x1, y1);
        ctx.line_to(x2, y2);
        ctx.line_to(x2, bottom_y);
        ctx.line_to(x1, bottom_y);
        ctx.close_path();
        ctx.fill();
    }
    ctx.reset_alpha();

    // Draw line on top
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(top_points[0].0, top_points[0].1);
    for &(x, y) in &top_points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

/// Draw histogram bars (volume bars) from bar data
///
/// Bars are drawn from bottom, scaled to 30% of chart height.
/// Color based on bullish/bearish direction.
pub fn draw_histogram(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    let bar_w = (viewport.bar_width() * 0.6).max(2.0);

    // Find volume range for scaling
    let mut vol_min = f64::MAX;
    let mut vol_max = f64::MIN;
    for i in start..end {
        let v = bars[i].volume;
        if v < vol_min { vol_min = v; }
        if v > vol_max { vol_max = v; }
    }
    if vol_max <= vol_min {
        vol_max = vol_min + 1.0;
    }

    let bottom_y = rect.bottom();
    let max_height = rect.height * 0.3; // 30% of chart height

    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);

        // Scale volume to height
        let vol_ratio = (bar.volume - vol_min) / (vol_max - vol_min);
        let h = (vol_ratio * max_height).max(1.0);

        let color = if bar.is_bullish() {
            &theme.candle_up
        } else {
            &theme.candle_down
        };

        ctx.set_fill_color(color);
        ctx.fill_rect(x - bar_w / 2.0, bottom_y - h, bar_w, h);
    }
}

/// Draw baseline series (area fill above/below baseline with color coding)
///
/// Uses first bar's open price as baseline.
/// Above baseline: green fill
/// Below baseline: red fill
pub fn draw_baseline_series(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end || bars.is_empty() {
        return;
    }

    // Use first bar's open as baseline
    let baseline_value = bars[0].open;
    let baseline_y = rect.y + viewport.price_to_y(baseline_value, price_scale.price_min, price_scale.price_max);

    let top_color = &theme.candle_up;
    let bottom_color = &theme.candle_down;

    // Collect points with their values
    let mut points: Vec<(f64, f64, f64)> = Vec::with_capacity(end - start); // (x, y, close)
    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y, bar.close));
    }

    if points.len() < 2 {
        return;
    }

    // Draw filled areas
    for i in 0..points.len() - 1 {
        let (x1, y1, v1) = points[i];
        let (x2, y2, v2) = points[i + 1];

        let above1 = v1 >= baseline_value;
        let above2 = v2 >= baseline_value;

        if above1 && above2 {
            // Both above baseline - green fill
            ctx.set_fill_color_alpha(top_color, 0.3);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x2, baseline_y);
            ctx.line_to(x1, baseline_y);
            ctx.close_path();
            ctx.fill();
        } else if !above1 && !above2 {
            // Both below baseline - red fill
            ctx.set_fill_color_alpha(bottom_color, 0.3);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x2, baseline_y);
            ctx.line_to(x1, baseline_y);
            ctx.close_path();
            ctx.fill();
        } else {
            // Crossing - need to split
            let t = (baseline_value - v1) / (v2 - v1);
            let cross_x = x1 + (x2 - x1) * t;

            if above1 {
                // Goes from above to below
                ctx.set_fill_color_alpha(top_color, 0.3);
                ctx.begin_path();
                ctx.move_to(x1, y1);
                ctx.line_to(cross_x, baseline_y);
                ctx.line_to(x1, baseline_y);
                ctx.close_path();
                ctx.fill();

                ctx.set_fill_color_alpha(bottom_color, 0.3);
                ctx.begin_path();
                ctx.move_to(cross_x, baseline_y);
                ctx.line_to(x2, y2);
                ctx.line_to(x2, baseline_y);
                ctx.close_path();
                ctx.fill();
            } else {
                // Goes from below to above
                ctx.set_fill_color_alpha(bottom_color, 0.3);
                ctx.begin_path();
                ctx.move_to(x1, y1);
                ctx.line_to(cross_x, baseline_y);
                ctx.line_to(x1, baseline_y);
                ctx.close_path();
                ctx.fill();

                ctx.set_fill_color_alpha(top_color, 0.3);
                ctx.begin_path();
                ctx.move_to(cross_x, baseline_y);
                ctx.line_to(x2, y2);
                ctx.line_to(x2, baseline_y);
                ctx.close_path();
                ctx.fill();
            }
        }
    }
    ctx.reset_alpha();

    // Draw line with color based on position
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    for i in 0..points.len() - 1 {
        let (x1, y1, v1) = points[i];
        let (x2, y2, v2) = points[i + 1];

        let above1 = v1 >= baseline_value;
        let above2 = v2 >= baseline_value;

        if above1 && above2 {
            ctx.set_stroke_color(top_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.stroke();
        } else if !above1 && !above2 {
            ctx.set_stroke_color(bottom_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.stroke();
        } else {
            // Crossing - draw in two parts
            let t = (baseline_value - v1) / (v2 - v1);
            let cross_x = x1 + (x2 - x1) * t;

            if above1 {
                ctx.set_stroke_color(top_color);
                ctx.begin_path();
                ctx.move_to(x1, y1);
                ctx.line_to(cross_x, baseline_y);
                ctx.stroke();

                ctx.set_stroke_color(bottom_color);
                ctx.begin_path();
                ctx.move_to(cross_x, baseline_y);
                ctx.line_to(x2, y2);
                ctx.stroke();
            } else {
                ctx.set_stroke_color(bottom_color);
                ctx.begin_path();
                ctx.move_to(x1, y1);
                ctx.line_to(cross_x, baseline_y);
                ctx.stroke();

                ctx.set_stroke_color(top_color);
                ctx.begin_path();
                ctx.move_to(cross_x, baseline_y);
                ctx.line_to(x2, y2);
                ctx.stroke();
            }
        }
    }

    // Draw baseline reference line
    ctx.set_stroke_color("#96969680"); // Gray with 50% alpha
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, baseline_y);
    ctx.line_to(rect.right(), baseline_y);
    ctx.stroke();
}

/// Draw step line (staircase pattern)
pub fn draw_step_line(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    let line_color = &theme.candle_up;

    // Collect points with step pattern
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut prev_y: Option<f64> = None;

    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);

        if let Some(py) = prev_y {
            // Step: horizontal then vertical
            points.push((x, py));
        }
        points.push((x, y));
        prev_y = Some(y);
    }

    if points.len() < 2 {
        return;
    }

    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

/// Draw line with dot markers at each point
pub fn draw_line_with_markers(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    let line_color = &theme.candle_up;
    let marker_radius = 3.0;

    // Collect points
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y));
    }

    if points.len() < 2 {
        return;
    }

    // Draw line first
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();

    // Draw markers on top
    ctx.set_fill_color(line_color);
    for &(x, y) in &points {
        ctx.begin_path();
        ctx.arc(x, y, marker_radius, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
}

/// Draw a simple line from data array (used for MAs and indicators)
///
/// This is a generic helper for drawing any f64 data as a line.
pub fn draw_line_from_data(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    data: &[f64],
    color: &str,
    width: f64,
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;

    let (start, end) = viewport.visible_range();
    let end = end.min(data.len());

    if start >= end {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(width);
    ctx.set_line_dash(&[]);

    let mut in_path = false;

    for i in start..end {
        let v = data[i];
        if v.is_nan() {
            // Break the line at NaN values
            if in_path {
                ctx.stroke();
                in_path = false;
            }
            continue;
        }

        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(v, price_scale.price_min, price_scale.price_max);

        if !in_path {
            ctx.begin_path();
            ctx.move_to(x, y);
            in_path = true;
        } else {
            ctx.line_to(x, y);
        }
    }

    if in_path {
        ctx.stroke();
    }
}

/// Draw HLC Area (High-Low-Close with filled area between high and low)
///
/// Displays the high-low range as a filled area with the close line on top.
pub fn draw_hlc_area(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end {
        return;
    }

    let area_color = &theme.candle_up;
    let line_color = &theme.candle_up;

    // Collect high, low, and close points
    let mut high_points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    let mut low_points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    let mut close_points: Vec<(f64, f64)> = Vec::with_capacity(end - start);

    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let high_y = rect.y + viewport.price_to_y(bar.high, price_scale.price_min, price_scale.price_max);
        let low_y = rect.y + viewport.price_to_y(bar.low, price_scale.price_min, price_scale.price_max);
        let close_y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);

        high_points.push((x, high_y));
        low_points.push((x, low_y));
        close_points.push((x, close_y));
    }

    if high_points.len() < 2 {
        return;
    }

    // Draw filled area between high and low using vertical strips
    ctx.set_fill_color_alpha(area_color, 0.3);
    for i in 0..high_points.len() - 1 {
        let (hx1, hy1) = high_points[i];
        let (hx2, hy2) = high_points[i + 1];
        let (_, ly1) = low_points[i];
        let (_, ly2) = low_points[i + 1];

        ctx.begin_path();
        ctx.move_to(hx1, hy1);
        ctx.line_to(hx2, hy2);
        ctx.line_to(hx2, ly2);
        ctx.line_to(hx1, ly1);
        ctx.close_path();
        ctx.fill();
    }
    ctx.reset_alpha();

    // Draw close line on top
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(close_points[0].0, close_points[0].1);
    for &(x, y) in &close_points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

/// Draw columns (vertical bars from baseline)
///
/// Similar to histogram but uses first bar's open as baseline,
/// coloring above/below differently.
pub fn draw_columns(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end || bars.is_empty() {
        return;
    }

    let bar_w = (viewport.bar_width() * 0.8).max(2.0);

    // Use first bar's open as baseline
    let baseline = bars[0].open;
    let baseline_y = rect.y + viewport.price_to_y(baseline, price_scale.price_min, price_scale.price_max);

    for i in start..end {
        let bar = &bars[i];
        let x = rect.x + viewport.bar_to_x(i);
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);

        let is_up = bar.close >= baseline;
        let color = if is_up {
            &theme.candle_up
        } else {
            &theme.candle_down
        };

        let top = y.min(baseline_y);
        let height = (y - baseline_y).abs().max(1.0);

        ctx.set_fill_color(color);
        ctx.fill_rect(x - bar_w / 2.0, top, bar_w, height);
    }
}

/// Draw compare overlay (symbol comparison lines)
///
/// Renders comparison symbol lines on top of the main chart.
/// Each compare series is drawn as a line chart using its own color.
/// When in percent mode, values are converted to percentage change from base
/// and rendered using a computed percent Y range.
pub fn draw_compare_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    compare_overlay: &crate::CompareOverlay,
) {
    use crate::PriceScaleMode;
    use std::collections::HashMap;

    if !compare_overlay.active || compare_overlay.series.is_empty() {
        return;
    }

    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let bars = state.bars;

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());

    if start >= end || bars.is_empty() {
        return;
    }

    // Check if in percent mode
    let is_percent_mode = compare_overlay.scale_mode == PriceScaleMode::Percent;

    // Get percent range for Y-axis scaling (if in percent mode)
    let (pct_min, pct_max) = if is_percent_mode {
        let range = compare_overlay.get_percent_range(bars, (start, end))
            .unwrap_or((-10.0, 10.0));
        // Add padding
        let pct_range = range.1 - range.0;
        let pct_padding = pct_range * 0.1;
        (range.0 - pct_padding, range.1 + pct_padding)
    } else {
        (state.price_scale.price_min, state.price_scale.price_max)
    };

    // Helper to convert percent to Y coordinate
    let percent_to_y = |pct: f64| -> f64 {
        let range = pct_max - pct_min;
        if range <= 0.0 {
            return viewport.chart_height / 2.0;
        }
        viewport.chart_height * (1.0 - (pct - pct_min) / range)
    };

    // Draw each visible compare series
    for series in &compare_overlay.series {
        if !series.visible {
            continue;
        }

        // Check per-timeframe visibility if configured
        if let Some(ref tf_config) = series.timeframe_visibility {
            if let Some(tf_label) = state.current_timeframe {
                if !tf_config.is_visible_on_label(tf_label) {
                    continue;
                }
            }
        }

        ctx.set_stroke_color(&series.color);
        ctx.set_stroke_width(series.line_width as f64);
        // Apply line style
        match series.line_style.as_str() {
            "dashed" => ctx.set_line_dash(&[8.0, 4.0]),
            "dotted" => ctx.set_line_dash(&[2.0, 4.0]),
            _ => ctx.set_line_dash(&[]), // "solid" or default
        }

        // Build a timestamp -> bar map for the compare series
        let compare_map: HashMap<i64, &crate::Bar> =
            series.bars.iter().map(|b| (b.timestamp, b)).collect();

        let mut in_path = false;

        // Draw line by iterating through main chart's visible bars
        for i in start..end {
            let main_bar = &bars[i];

            // Find matching compare bar by timestamp
            if let Some(compare_bar) = compare_map.get(&main_bar.timestamp) {
                let x = rect.x + viewport.bar_to_x(i);

                let y = if is_percent_mode {
                    let pct = series.price_to_percent(compare_bar.close);
                    rect.y + percent_to_y(pct)
                } else {
                    rect.y + viewport.price_to_y(
                        compare_bar.close,
                        state.price_scale.price_min,
                        state.price_scale.price_max
                    )
                };

                if !in_path {
                    ctx.begin_path();
                    ctx.move_to(x, y);
                    in_path = true;
                } else {
                    ctx.line_to(x, y);
                }
            } else {
                // No matching timestamp - break the line
                if in_path {
                    ctx.stroke();
                    in_path = false;
                }
            }
        }

        if in_path {
            ctx.stroke();
        }
        // Reset line dash so later rendering is not affected
        ctx.set_line_dash(&[]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_baseline_crossing() {
        // Test crossing calculation
        let v1: f64 = 100.0;
        let v2: f64 = 90.0;
        let baseline: f64 = 95.0;
        let t = (baseline - v1) / (v2 - v1);
        assert!((t - 0.5).abs() < 0.001);
    }
}
