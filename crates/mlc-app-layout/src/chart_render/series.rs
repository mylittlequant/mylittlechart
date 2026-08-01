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

/// Kline-renderable chart types a compare series may pick in its Style tab
/// dropdown (`CompareSettingsState::chart_type_dropdown_open`). Anything
/// else (unknown/legacy value, or explicitly "line") keeps the hand-rolled
/// line path below unchanged.
const COMPARE_BAR_FAMILY: &[&str] = &[
    "candles", "bars", "hollow_candles", "heikin_ashi", "step_line", "area",
];

/// Dim a `#rrggbb`/`#rgb` hex color to a 50%-alpha rgba string for a
/// compare series' "down" candle/bar color. Simplest honest v1 (owner
/// spec 2026-07-16): compare overlays have no separate up/down color pair,
/// only one `series.color` — up reuses it verbatim, down is the same hue
/// dimmed, cheaper than deriving a real complementary color and good
/// enough to visually distinguish direction without inventing meaning the
/// data doesn't carry (a compare series has no real bid/ask semantics).
/// Falls back to the input string unchanged if it isn't valid 3/6-digit hex
/// (e.g. already an rgba() string from a future color-picker mode).
fn dim_compare_color(hex: &str) -> String {
    let stripped = hex.trim_start_matches('#');
    let (r, g, b) = match stripped.len() {
        6 => (
            u8::from_str_radix(&stripped[0..2], 16),
            u8::from_str_radix(&stripped[2..4], 16),
            u8::from_str_radix(&stripped[4..6], 16),
        ),
        3 => (
            u8::from_str_radix(&stripped[0..1], 16).map(|v| v * 17),
            u8::from_str_radix(&stripped[1..2], 16).map(|v| v * 17),
            u8::from_str_radix(&stripped[2..3], 16).map(|v| v * 17),
        ),
        _ => return hex.to_string(),
    };
    match (r, g, b) {
        (Ok(r), Ok(g), Ok(b)) => format!("rgba({}, {}, {}, 0.5)", r, g, b),
        _ => hex.to_string(),
    }
}

/// Draw compare overlay (symbol comparison lines)
///
/// Renders comparison symbol overlays on top of the main chart. Series
/// whose `chart_type` is "line" (or any unrecognized value) keep the
/// original hand-rolled percent/absolute line path. Series with a
/// kline-renderable `chart_type` (candles/bars/hollow_candles/
/// heikin_ashi/step_line/area — see `COMPARE_BAR_FAMILY`) are rendered by
/// building a throwaway `ChartRenderState` (main viewport/chart_rect/theme
/// reused, `bars` + `price_scale` scoped to the series) and dispatching
/// through the SAME `draw_ops_by_id` table the main series uses (see
/// `mlc-app-layout::chart_type::draw_ops`).
///
/// When in percent mode, values are converted to percentage change from
/// base and rendered using a computed percent Y range (shared by both
/// paths). In Normal (absolute) mode the bar-family path computes each
/// series' OWN min/max over the visible range (+10% pad) instead of
/// reusing the main window's price scale — the line path's Normal-mode
/// behavior is left as-is (pre-existing, documented latent bug: absolute
/// compare lines still plot against the main scale) to avoid visual churn
/// on existing presets.
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

    // Clip to chart_rect, same as the main series dispatch (`draw_series`)
    // — compare overlays must not bleed onto the price scale/chrome.
    ctx.save();
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(rect.x, rect.y, rect.width, rect.height);
        ctx.clip();
    }

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

        if COMPARE_BAR_FAMILY.contains(&series.chart_type.as_str()) {
            draw_compare_series_bar_family(
                ctx, state, series, bars, start, end,
                is_percent_mode, pct_min, pct_max,
            );
            continue;
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

    ctx.restore();
}

/// Render one compare series through the kline-family draw dispatch
/// (`draw_ops_by_id`) instead of the hand-rolled line path.
///
/// X-axis alignment: the kline draw fns (`draw_candles`, `draw_bars`, ...)
/// index `state.bars[i]` directly for `i` in `viewport.visible_range()` —
/// `Viewport::x_for_bar` positions purely by INDEX, the `ts_ms` argument is
/// unused in index mode (see `mlc-core::chart::types::viewport::x_for_bar`
/// doc comment). So the transformed compare-bars vec must be a FULL-LENGTH
/// array parallel to the main window's `bars` slice — one entry per main-bar
/// slot, not a sparse/compacted vec — or the draw fn would paint the
/// series' Nth matched bar at the main chart's Nth visible INDEX instead of
/// its own matched slot, desyncing the X axis. Timestamps present in the
/// main window but missing from the compare series' own bar history become
/// gap placeholders: zero-range OHLC pinned to the previous matched close
/// (volume 0) — chosen over skipping the slot because every draw fn in the
/// family indexes unconditionally and has no "skip this slot" contract;
/// a flat zero-height placeholder reads visually as "no data here" without
/// crashing or silently reusing a stale close as a fabricated OHLC range.
#[allow(clippy::too_many_arguments)]
fn draw_compare_series_bar_family(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    series: &mlc_core::CompareSeries,
    main_bars: &[mlc_core::Bar],
    start: usize,
    end: usize,
    is_percent_mode: bool,
    pct_min: f64,
    pct_max: f64,
) {
    use std::collections::HashMap;

    let Some(ops) = crate::chart_type::draw_ops_by_id(series.chart_type.as_str()) else {
        return;
    };

    // Timestamp -> compare bar map (same join key the line path uses).
    let compare_map: HashMap<i64, &mlc_core::Bar> =
        series.bars.iter().map(|b| (b.timestamp, b)).collect();

    // Full-length parallel array: index i mirrors main_bars[i]. Percent
    // mode pre-transforms O/H/L/C into percent-space via the SAME formula
    // the line path uses (`series.price_to_percent`), so the throwaway
    // PriceScale below can just be the overlay's already-computed percent
    // range — identical Y placement to the line path at matched points.
    let mut transformed: Vec<mlc_core::Bar> = Vec::with_capacity(main_bars.len());
    let mut last_close = series.bars.first().map(|b| b.close).unwrap_or(0.0);
    for main_bar in main_bars {
        if let Some(cb) = compare_map.get(&main_bar.timestamp) {
            let bar = if is_percent_mode {
                mlc_core::Bar {
                    timestamp: cb.timestamp,
                    open: series.price_to_percent(cb.open),
                    high: series.price_to_percent(cb.high),
                    low: series.price_to_percent(cb.low),
                    close: series.price_to_percent(cb.close),
                    volume: cb.volume,
                    derived_ts_ms: cb.derived_ts_ms,
                }
            } else {
                **cb
            };
            last_close = bar.close;
            transformed.push(bar);
        } else {
            // Gap placeholder — flat zero-range bar pinned to the last
            // matched close, volume 0. Renders as a flat line/zero-height
            // body, an honest "no data here" signal instead of a
            // fabricated OHLC range or a desynced skip.
            transformed.push(mlc_core::Bar {
                timestamp: main_bar.timestamp,
                open: last_close,
                high: last_close,
                low: last_close,
                close: last_close,
                volume: 0.0,
                derived_ts_ms: 0,
            });
        }
    }

    // Y-scale: Percent mode reuses the overlay's already-computed percent
    // range (transformed values above are already in percent-space).
    // Normal mode computes THIS series' own min/max over the visible
    // range (+10% pad) — fixes the documented latent absolute-scale bug
    // (reusing the main window's price scale is meaningless for a
    // differently-priced instrument) for the new bar-family types; the
    // legacy line path's Normal-mode behavior is untouched on purpose.
    let (scale_min, scale_max) = if is_percent_mode {
        (pct_min, pct_max)
    } else {
        let visible = &transformed[start..end.min(transformed.len())];
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for b in visible {
            if b.volume == 0.0 && b.high == b.low && b.open == b.close {
                continue; // gap placeholder — excluded from range calc
            }
            lo = lo.min(b.low);
            hi = hi.max(b.high);
        }
        if !lo.is_finite() || !hi.is_finite() {
            (0.0, 100.0)
        } else if (hi - lo).abs() < f64::EPSILON {
            (lo - 1.0, hi + 1.0)
        } else {
            let pad = (hi - lo) * 0.1;
            (lo - pad, hi + pad)
        }
    };
    let price_scale = mlc_core::PriceScale::new(scale_min, scale_max);

    // Theme tint: series has one `color`, no up/down pair. Up reuses it
    // verbatim; down is the same hue dimmed to 50% alpha (see
    // `dim_compare_color` doc comment) — simplest honest v1, not a real
    // bid/ask-derived color since compare series carry no such semantics.
    let mut theme = state.theme.clone();
    theme.candle_up = series.color.clone();
    theme.candle_down = dim_compare_color(&series.color);
    theme.wick_up = series.color.clone();
    theme.wick_down = dim_compare_color(&series.color);
    theme.candle_up_border = Some(series.color.clone());
    theme.candle_down_border = Some(dim_compare_color(&series.color));
    theme.hollow_up = series.color.clone();
    theme.hollow_down = dim_compare_color(&series.color);
    theme.bar_up = series.color.clone();
    theme.bar_down = dim_compare_color(&series.color);
    theme.step_line = series.color.clone();
    theme.area_line = series.color.clone();
    theme.area_top = dim_compare_color(&series.color);
    theme.area_bottom = dim_compare_color(&series.color);

    let throwaway = ChartRenderState {
        viewport: state.viewport,
        price_scale: &price_scale,
        time_scale: state.time_scale,
        bars: &transformed,
        grid: state.grid,
        crosshair: state.crosshair,
        legend: state.legend,
        chart_rect: state.chart_rect,
        theme: &theme,
        time_ticks: state.time_ticks,
        current_timeframe: state.current_timeframe,
        disable_clip: state.disable_clip,
        time_format_settings: state.time_format_settings,
        timeframe_interval_ms: state.timeframe_interval_ms,
        scale_settings: state.scale_settings,
        body_enabled: state.body_enabled,
        border_enabled: state.border_enabled,
        wick_enabled: state.wick_enabled,
        use_prev_close: false,
        footprint_bars: None,
        footprint_display: state.footprint_display,
        tick_intensity: state.tick_intensity,
        renko_bars: None,
        three_line_break: None,
        pnf_columns: None,
        kagi_segments: None,
        tpo_sessions: None,
        tpo_display: state.tpo_display,
        line_family_display: state.line_family_display,
        ha_cache: None,
        vp_overlay: None,
        tpo_profile_overlay: None,
        oi_delta_ref: None,
        funding_rate_ref: None,
        dom_heatmap_ref: None,
        dom_trade_dots_ref: None,
        liq_heatmap_ref: None,
        liq_dots_ref: None,
        liq_sensitivity: state.liq_sensitivity,
        non_temporal_axis: state.non_temporal_axis,
        hide_time_grid: state.hide_time_grid,
        history_depth: state.history_depth,
        cvd_paint_deltas: None,
        trade_archive_available: state.trade_archive_available,
        bars_trade_built: state.bars_trade_built,
    };

    (ops.draw)(ctx, &throwaway);
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
