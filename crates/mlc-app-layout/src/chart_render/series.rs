//! Line and area series rendering
//!
//! Platform-agnostic line chart, area chart, and histogram rendering.
//!
//! # Series Types
//!
//! - `draw_line_series()` - Simple line connecting close prices
//! - `draw_area_series()` - Filled area under line

//! - `draw_baseline_series()` - Split fill above/below baseline
//! - `draw_step_line()` - Staircase pattern line
//! - `draw_line_with_markers()` - Line with dot markers

use mlc_core::render::RenderContext;
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

    // Own per-type color + width (owner-approved settings matrix wave 3,
    // 2026-07-11) — was `theme.candle_up` + hardcoded 2.0.
    let line_color = &theme.line;
    let line_width = state.line_family_display.line_width;

    // Collect points
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y));
    }

    if points.len() < 2 {
        return;
    }

    // Draw line
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(line_width);
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

    // Own per-type colors + width (owner-approved settings matrix wave 3,
    // 2026-07-11) — was `theme.candle_up` with a single hardcoded-alpha
    // fill and hardcoded 2.0 width. Fill = vertical linear gradient
    // `area_top` → `area_bottom` anchored to the pane (TV canon: gradient
    // spans pane height, not the fill polygon).
    let line_color = &theme.area_line;
    let fill_top = theme.area_top.as_str();
    let fill_bottom = theme.area_bottom.as_str();
    let line_width = state.line_family_display.area_line_width;
    let bottom_y = rect.bottom();

    // Collect top points
    let mut top_points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        top_points.push((x, y));
    }

    if top_points.len() < 2 {
        return;
    }

    // Draw filled area as ONE continuous path (top forward + bottom
    // reverse), single fill(). Owner 2026-07-06 (HLC-artifact audit,
    // docs/mlc/audits/hlc-area-render-artifacts-2026-07-06.md): the
    // per-bar strip pattern with alpha=0.3 doubled the effective alpha at
    // every bar boundary — regular dark vertical bands, most visible on
    // narrow zooms. HLC Area's version was already fixed there; this Area
    // path has the same bug, just masked by a taller fill. `area_top` /
    // `area_bottom` are pre-blended rgba strings (existing convention), so
    // no separate alpha call is needed here (unlike the old `candle_up` +
    // `set_fill_color_alpha(_, 0.3)` pairing).
    ctx.begin_path();
    ctx.move_to(top_points[0].0, top_points[0].1);
    for &(x, y) in &top_points[1..] {
        ctx.line_to(x, y);
    }
    if let Some(&(last_x, _)) = top_points.last() {
        ctx.line_to(last_x, bottom_y);
        ctx.line_to(top_points[0].0, bottom_y);
    }
    ctx.close_path();
    ctx.fill_linear_gradient(
        &[(0.0, fill_top), (1.0, fill_bottom)],
        rect.x,
        rect.y,
        rect.x,
        bottom_y,
    );

    // Draw line on top
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(line_width);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(top_points[0].0, top_points[0].1);
    for &(x, y) in &top_points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

// NOTE: `draw_histogram` (the "histogram" CHART TYPE — a full-pane VOLUME
// render with no price series) was removed 2026-07-05. It was a
// lightweight-charts SERIES type mistakenly ported as a chart style; the
// VOLUME indicator (main-pane bottom overlay via `draw_volume_overlay`)
// owns that visual now.

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

    // Own per-type colors + width (owner-approved settings matrix wave 3,
    // 2026-07-11) — was `theme.candle_up`/`theme.candle_down` for both line
    // and fill (fill alpha-blended from the line color); now 4 independent
    // fields (top line / top fill / bottom line / bottom fill) plus its own
    // line width, was hardcoded 2.0.
    let top_line_color = &theme.baseline_top_line;
    let top_fill_color = &theme.baseline_top_fill;
    let bottom_line_color = &theme.baseline_bottom_line;
    let bottom_fill_color = &theme.baseline_bottom_fill;
    let line_width = state.line_family_display.baseline_line_width;

    // Collect points with their values
    let mut points: Vec<(f64, f64, f64)> = Vec::with_capacity(end - start); // (x, y, close)
    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y, bar.close));
    }

    if points.len() < 2 {
        return;
    }

    // Two accumulated multi-subpath paths (one per baseline side) instead of
    // N-per-run fills/strokes. Owner 2026-07-06 (post hlc-area-render-artifacts
    // fix): at bar_spacing near MIN_BAR_SPACING with dense baseline crossings,
    // each same-sign run collapses to a ~1px-wide strip; N separate
    // begin_path()/fill() pairs each get their own independent AA falloff on
    // both edges with no opaque interior, reading as a field of hairlines. The
    // fix used for Area/HLC Area (one accumulated path, one fill) does not
    // apply directly here because runs alternate between two colors — so we
    // accumulate ALL above-baseline islands into one multi-subpath path (fill
    // once) and ALL below-baseline islands into a second (fill once). Same
    // split for the stroke polylines. Backends fill all subpaths of a single
    // path as one NonZero-winded region composited with a single alpha pass
    // (see `emit_svg_path_body` in uzor/uzor/src/core/render/path.rs for the
    // same accumulated-subpath principle already used for SVG icon fills).
    //
    // Crossing points are computed exactly once per flip and shared
    // bit-identically between: the fill island's baseline-return start/end,
    // the next fill island's first vertex, and the corresponding stroke
    // polyline endpoints — no seam risk from divergent interpolation.
    let mut above_islands: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut below_islands: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut above_polylines: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut below_polylines: Vec<Vec<(f64, f64)>> = Vec::new();

    let (x0, y0, v0) = points[0];
    let mut above = v0 >= baseline_value;
    let mut top: Vec<(f64, f64)> = vec![(x0, y0)];

    for w in points.windows(2) {
        let (x1, _y1, v1) = w[0];
        let (x2, y2, v2) = w[1];
        let above1 = v1 >= baseline_value;
        let above2 = v2 >= baseline_value;

        if above1 == above2 {
            top.push((x2, y2));
            continue;
        }

        // Crossing between this pair — compute once, close the current run
        // with it, then open the next run with the same tuple.
        let denom = v2 - v1;
        let t = if denom.abs() > f64::EPSILON {
            ((baseline_value - v1) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cross = (x1 + (x2 - x1) * t, baseline_y);

        top.push(cross);
        push_run(&mut above_islands, &mut below_islands, &mut above_polylines, &mut below_polylines, &top, above, baseline_y);

        above = above2;
        top = vec![cross, (x2, y2)];
    }

    push_run(&mut above_islands, &mut below_islands, &mut above_polylines, &mut below_polylines, &top, above, baseline_y);

    // Fill pass — exactly 2 fill() calls total regardless of crossing count.
    // `baseline_top_fill`/`baseline_bottom_fill` are already pre-blended
    // rgba strings (existing convention, same alpha the old
    // `set_fill_color_alpha(_, 0.3)` call used), so no separate alpha call
    // is needed here.
    ctx.set_fill_color(top_fill_color);
    ctx.begin_path();
    for island in &above_islands {
        ctx.move_to(island[0].0, island[0].1);
        for &(x, y) in &island[1..] {
            ctx.line_to(x, y);
        }
        ctx.close_path();
    }
    ctx.fill();

    ctx.set_fill_color(bottom_fill_color);
    ctx.begin_path();
    for island in &below_islands {
        ctx.move_to(island[0].0, island[0].1);
        for &(x, y) in &island[1..] {
            ctx.line_to(x, y);
        }
        ctx.close_path();
    }
    ctx.fill();

    // Stroke pass — exactly 2 stroke() calls total regardless of crossing count.
    ctx.set_stroke_width(line_width);
    ctx.set_line_dash(&[]);

    ctx.set_stroke_color(top_line_color);
    ctx.begin_path();
    for polyline in &above_polylines {
        ctx.move_to(polyline[0].0, polyline[0].1);
        for &(x, y) in &polyline[1..] {
            ctx.line_to(x, y);
        }
    }
    ctx.stroke();

    ctx.set_stroke_color(bottom_line_color);
    ctx.begin_path();
    for polyline in &below_polylines {
        ctx.move_to(polyline[0].0, polyline[0].1);
        for &(x, y) in &polyline[1..] {
            ctx.line_to(x, y);
        }
    }
    ctx.stroke();

    // Draw baseline reference line
    ctx.set_stroke_color("#96969680"); // Gray with 50% alpha
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, baseline_y);
    ctx.line_to(rect.right(), baseline_y);
    ctx.stroke();
}

/// Finalize one same-side run for `draw_baseline_series`: pushes the run's
/// stroke polyline (`top`, unmodified) and — if the run has at least 2
/// vertices — its fill island (top forward + baseline-return) into the
/// appropriate above/below accumulator so the caller can later issue exactly
/// one `fill()` and one `stroke()` per side.
fn push_run(
    above_islands: &mut Vec<Vec<(f64, f64)>>,
    below_islands: &mut Vec<Vec<(f64, f64)>>,
    above_polylines: &mut Vec<Vec<(f64, f64)>>,
    below_polylines: &mut Vec<Vec<(f64, f64)>>,
    top: &[(f64, f64)],
    above: bool,
    baseline_y: f64,
) {
    if top.len() < 2 {
        return;
    }

    let (islands, polylines) = if above {
        (&mut *above_islands, &mut *above_polylines)
    } else {
        (&mut *below_islands, &mut *below_polylines)
    };

    polylines.push(top.to_vec());

    let mut island = top.to_vec();
    let last_x = island.last().map(|p| p.0).unwrap_or(top[0].0);
    let first_x = top[0].0;
    island.push((last_x, baseline_y));
    island.push((first_x, baseline_y));
    islands.push(island);
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

    // Own per-type color + width (owner-approved settings matrix wave 3,
    // 2026-07-11) — was `theme.candle_up` + hardcoded 2.0.
    let line_color = &theme.step_line;
    let line_width = state.line_family_display.step_line_width;

    // Collect points with step pattern
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut prev_y: Option<f64> = None;

    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
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
    ctx.set_stroke_width(line_width);
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

    // Line reuses the shared "Line" family color/width (owner-approved
    // settings matrix wave 3, 2026-07-11); marker color/shape/size are
    // their own fields — was `theme.candle_up` + hardcoded 2.0/circle/3.0
    // for both.
    let line_color = &theme.line;
    let marker_color = &theme.line_markers;
    let marker_style = state.line_family_display.marker_style;
    let marker_radius = state.line_family_display.marker_size;

    // Collect points
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(end - start);
    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
        let y = rect.y + viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max);
        points.push((x, y));
    }

    if points.len() < 2 {
        return;
    }

    // Draw line first
    ctx.set_stroke_color(line_color);
    ctx.set_stroke_width(state.line_family_display.line_width);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();

    // Draw markers on top
    ctx.set_fill_color(marker_color);
    for &(x, y) in &points {
        draw_line_marker(ctx, x, y, marker_radius, marker_style);
    }
}

/// Draw one marker glyph for `draw_line_with_markers`, dispatching on
/// [`mlc_core::chart::LineMarkerStyle`] (owner-approved settings matrix
/// wave 3, 2026-07-11). Fill color/`begin_path`/`fill` bracket every arm —
/// this only builds path geometry.
fn draw_line_marker(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    radius: f64,
    style: mlc_core::chart::LineMarkerStyle,
) {
    use mlc_core::chart::LineMarkerStyle;
    ctx.begin_path();
    match style {
        LineMarkerStyle::Circle => {
            ctx.arc(x, y, radius, 0.0, std::f64::consts::TAU);
        }
        LineMarkerStyle::Square => {
            ctx.rect(x - radius, y - radius, radius * 2.0, radius * 2.0);
        }
        LineMarkerStyle::Triangle => {
            ctx.move_to(x, y - radius);
            ctx.line_to(x + radius, y + radius);
            ctx.line_to(x - radius, y + radius);
            ctx.close_path();
        }
        LineMarkerStyle::Diamond => {
            ctx.move_to(x, y - radius);
            ctx.line_to(x + radius, y);
            ctx.line_to(x, y + radius);
            ctx.line_to(x - radius, y);
            ctx.close_path();
        }
    }
    ctx.fill();
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

    for (i, &v) in data.iter().enumerate().take(end).skip(start) {
        if v.is_nan() {
            // Break the line at NaN values
            if in_path {
                ctx.stroke();
                in_path = false;
            }
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = rect.x + viewport.x_for_bar(i, bar_ts_ms);
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

    for (i, bar) in bars.iter().enumerate().take(end).skip(start) {
        let x = rect.x + viewport.x_for_bar(i, bar.ts_ms());
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

    // Draw filled area between high and low as ONE continuous path (top
    // forward + bottom reverse). Owner 2026-07-06: the previous per-bar
    // strip pattern (N fill()s with alpha=0.3) produced regular dark
    // vertical bands — each strip's antialiased edges composited with the
    // next strip's edge on the shared vertical, doubling the effective
    // alpha at every bar boundary. One path = one fill = one AA pass, no
    // per-edge composition, no bands.
    ctx.set_fill_color_alpha(area_color, 0.3);
    ctx.begin_path();
    ctx.move_to(high_points[0].0, high_points[0].1);
    for &(x, y) in &high_points[1..] {
        ctx.line_to(x, y);
    }
    for &(x, y) in low_points.iter().rev() {
        ctx.line_to(x, y);
    }
    ctx.close_path();
    ctx.fill();
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

/// Draw compare overlay (symbol comparison lines)
///
/// Renders comparison symbol lines on top of the main chart.
/// Each compare series is drawn as a line chart using its own color.
/// When in percent mode, values are converted to percentage change from base
/// and rendered using a computed percent Y range.
pub fn draw_compare_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    compare_overlay: &mlc_core::CompareOverlay,
) {
    use mlc_core::PriceScaleMode;
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
        let compare_map: HashMap<i64, &mlc_core::Bar> =
            series.bars.iter().map(|b| (b.timestamp, b)).collect();

        let mut in_path = false;

        // Draw line by iterating through main chart's visible bars
        for (i, main_bar) in bars.iter().enumerate().take(end).skip(start) {

            // Find matching compare bar by timestamp
            if let Some(compare_bar) = compare_map.get(&main_bar.timestamp) {
                let x = rect.x + viewport.x_for_bar(i, main_bar.ts_ms());

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
