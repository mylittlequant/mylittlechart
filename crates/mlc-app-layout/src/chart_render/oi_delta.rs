//! Open Interest Delta sub-pane renderer (C15).
//!
//! Draws a histogram strip below the main chart, one bar per OI event.
//! Green bars: Δ > 0 (positions added). Red bars: Δ < 0 (positions closed).
//! A 1 px zero line is drawn across the pane.
//! A right-aligned label shows the most-recent absolute OI value.
//!
//! Industry reference: TradingView "Open Interest" indicator,
//! Quantower "OI Delta" study.

use mlc_core::render::RenderContext;
use mlc_core::types::OiDeltaBar;
use super::ChartRenderState;

/// Height of the OI Delta sub-pane as a fraction of total chart height.
const PANE_HEIGHT_FRACTION: f64 = 0.15;
/// Minimum chart height required before the pane is drawn (avoid tiny slivers).
const MIN_CHART_HEIGHT_FOR_PANE_PX: f64 = 220.0;
/// Minimum pane height in absolute pixels.
const MIN_PANE_HEIGHT_PX: f64 = 40.0;

/// Color for positive Δ bars (positions added).
const COLOR_POSITIVE: &str = "#26a69a";
/// Color for negative Δ bars (positions closed).
const COLOR_NEGATIVE: &str = "#ef5350";
/// Zero-line color.
const COLOR_ZERO_LINE: &str = "rgba(120, 144, 156, 0.45)";
/// Background tint behind the sub-pane.
const COLOR_BG: &str = "rgba(0, 0, 0, 0.18)";

/// Draw the OI Delta sub-pane.
///
/// Allocates `PANE_HEIGHT_FRACTION` of `state.chart_rect.height` at the
/// bottom of the chart area (above the time scale). Returns the pane height
/// actually consumed (0.0 when skipped).
pub fn draw_oi_delta(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    bars: &[OiDeltaBar],
) -> f64 {
    if bars.is_empty() { return 0.0; }

    let rect = &state.chart_rect;
    if rect.height < MIN_CHART_HEIGHT_FOR_PANE_PX { return 0.0; }

    let pane_h = (rect.height * PANE_HEIGHT_FRACTION).max(MIN_PANE_HEIGHT_PX);
    let pane_y = rect.y + rect.height - pane_h;

    // Background tint.
    ctx.set_fill_color(COLOR_BG);
    ctx.fill_rect(rect.x, pane_y, rect.width, pane_h);

    // Determine visible bar range (same clipping as other renderers).
    let viewport = state.viewport;
    let all_bars = state.bars;
    let (start, end) = viewport.visible_range();
    let start_ts = all_bars.get(start).map(|b| b.ts_ms()).unwrap_or(i64::MIN);
    let end_ts = all_bars
        .get(end.saturating_sub(1))
        .map(|b| b.ts_ms())
        .unwrap_or(i64::MAX);

    // Collect visible OI bars.
    let visible: Vec<&OiDeltaBar> = bars
        .iter()
        .filter(|b| b.ts_ms >= start_ts && b.ts_ms <= end_ts)
        .collect();
    if visible.is_empty() { return pane_h; }

    // Scale: max(|Δ|) across visible bars maps to half the pane height.
    let max_abs_delta = visible
        .iter()
        .map(|b| b.delta.abs())
        .fold(0.0_f64, f64::max);
    let scale = if max_abs_delta > 0.0 {
        (pane_h * 0.5) / max_abs_delta
    } else {
        return pane_h; // All deltas are zero — nothing to draw.
    };

    // Zero line (horizontal centre of the pane).
    let zero_y = pane_y + pane_h * 0.5;
    ctx.set_stroke_color(COLOR_ZERO_LINE);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, zero_y);
    ctx.line_to(rect.x + rect.width, zero_y);
    ctx.stroke();

    // Bar width mirrors the main chart slot width.
    let bar_count = end.saturating_sub(start).max(1);
    let bar_w = (rect.width / bar_count as f64).max(1.0);

    // Time span for X mapping.
    let ts_span = (end_ts - start_ts) as f64;

    for b in &visible {
        let x = if ts_span > 0.0 {
            rect.x + rect.width * ((b.ts_ms - start_ts) as f64 / ts_span).clamp(0.0, 1.0)
        } else {
            rect.x
        };

        let bar_height = (b.delta.abs() * scale).max(1.0);
        let bar_y = if b.delta >= 0.0 {
            // Positive Δ grows upward from the zero line.
            zero_y - bar_height
        } else {
            // Negative Δ grows downward from the zero line.
            zero_y
        };

        let color = if b.delta >= 0.0 { COLOR_POSITIVE } else { COLOR_NEGATIVE };
        ctx.set_fill_color(color);
        ctx.fill_rect(x - bar_w * 0.5, bar_y, bar_w.max(1.0), bar_height);
    }

    // Right-side label: latest absolute OI value.
    if let Some(last) = bars.last() {
        let label = format_oi(last.oi);
        ctx.set_fill_color("#9598a1");
        ctx.set_font("9px sans-serif");
        ctx.fill_text(
            &label,
            rect.x + rect.width - 4.0,
            pane_y + 10.0,
        );
    }

    pane_h
}

/// Format an OI value using compact notation (K / M / B).
fn format_oi(oi: f64) -> String {
    let abs = oi.abs();
    if abs >= 1_000_000_000.0 {
        format!("{:.2}B", oi / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{:.2}M", oi / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}K", oi / 1_000.0)
    } else {
        format!("{:.0}", oi)
    }
}
