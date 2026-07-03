//! Cumulative Volume Delta renderer.
//!
//! Two visual modes share the same data source (`state.cvd_line`):
//!
//! 1. [`draw_cvd_chart`] — when `chart_type == "cvd"`, this is the
//!    primary visual on the chart. The whole chart vertical extent is
//!    used; min/max auto-scaled to the visible CVD range.
//! 2. [`draw_cvd_overlay`] — a reserved strip at the bottom of the
//!    chart (similar to the footprint F5 strip), drawn on top of any
//!    other chart type. Caller decides whether to enable.
//!
//! Color flips per segment: green when CVD rising, red when falling.
//! Baseline (zero or visible-range-min) drawn as a thin axis line.

use mlc_core::render::RenderContext;
use mlc_core::types::CvdSample;
use super::ChartRenderState;

const STRIP_HEIGHT_PX: f64 = 60.0;
const MIN_CHART_HEIGHT_FOR_OVERLAY_PX: f64 = 220.0;

/// Primary CVD chart (chart_type = "cvd"). Draws the line filling the
/// entire chart_rect height, with auto-scale to visible CVD range.
pub fn draw_cvd_chart(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let samples = match state.cvd_line {
        Some(s) if !s.is_empty() => s,
        _ => {
            super::draw_waiting_placeholder(ctx, state, "CVD: waiting for live data…");
            return;
        }
    };
    let rect = &state.chart_rect;
    draw_cvd_line_into_rect(ctx, state, samples, rect.x, rect.y, rect.width, rect.height);
}

/// CVD overlay strip at the bottom of the chart. Returns the strip
/// height (px) so the caller knows to leave that vertical space free
/// when drawing other content; returns 0.0 if the strip was skipped
/// (no data, or chart too short).
pub fn draw_cvd_overlay(ctx: &mut dyn RenderContext, state: &ChartRenderState) -> f64 {
    let samples = match state.cvd_line {
        Some(s) if !s.is_empty() => s,
        _ => return 0.0,
    };
    let rect = &state.chart_rect;
    if rect.height < MIN_CHART_HEIGHT_FOR_OVERLAY_PX { return 0.0; }
    let strip_y = rect.y + rect.height - STRIP_HEIGHT_PX;
    // Tinted background to mark the strip region.
    ctx.set_fill_color("rgba(0, 0, 0, 0.18)");
    ctx.fill_rect(rect.x, strip_y, rect.width, STRIP_HEIGHT_PX);
    draw_cvd_line_into_rect(ctx, state, samples, rect.x, strip_y, rect.width, STRIP_HEIGHT_PX);
    STRIP_HEIGHT_PX
}

fn draw_cvd_line_into_rect(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    samples: &[CvdSample],
    rx: f64, ry: f64, rw: f64, rh: f64,
) {
    let viewport = state.viewport;
    let bars = state.bars;
    let rect = &state.chart_rect;
    let _ = rect;

    // Min/max CVD across the visible-bar window. CVD samples are
    // trade-ticks (typically far denser than bars); use the visible bar
    // range to clip both axes consistently.
    let (start, end) = viewport.visible_range();
    let start_ts = bars.get(start).map(|b| b.ts_ms()).unwrap_or(i64::MIN);
    let end_ts = bars.get(end.saturating_sub(1)).map(|b| b.ts_ms()).unwrap_or(i64::MAX);

    let (mut cvd_min, mut cvd_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in samples {
        if s.ts_ms < start_ts || s.ts_ms > end_ts { continue; }
        if s.value < cvd_min { cvd_min = s.value; }
        if s.value > cvd_max { cvd_max = s.value; }
    }
    if !cvd_min.is_finite() || !cvd_max.is_finite() { return; }
    let span = (cvd_max - cvd_min).max(1e-9);

    // Baseline (zero) within the visible range.
    if cvd_min < 0.0 && cvd_max > 0.0 {
        let zero_y = ry + rh * (1.0 - (-cvd_min / span)).clamp(0.0, 1.0);
        ctx.set_stroke_color("rgba(120, 144, 156, 0.45)");
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(rx, zero_y);
        ctx.line_to(rx + rw, zero_y);
        ctx.stroke();
    }

    ctx.set_stroke_width(1.5);
    let mut last_xy: Option<(f64, f64, f64)> = None;
    for s in samples {
        if s.ts_ms < start_ts || s.ts_ms > end_ts { continue; }
        let x_within = if end_ts > start_ts {
            rx + rw * ((s.ts_ms - start_ts) as f64 / (end_ts - start_ts) as f64).clamp(0.0, 1.0)
        } else { rx };
        let y = ry + rh * (1.0 - (s.value - cvd_min) / span);
        if let Some((px, py, prev_v)) = last_xy {
            let rising = s.value >= prev_v;
            let c = if rising { "#26a69a" } else { "#ef5350" };
            ctx.set_stroke_color(c);
            ctx.begin_path();
            ctx.move_to(px, py);
            ctx.line_to(x_within, y);
            ctx.stroke();
        }
        last_xy = Some((x_within, y, s.value));
    }
}
