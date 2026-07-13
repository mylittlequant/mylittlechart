//! Grid rendering
//!
//! Platform-agnostic grid drawing using RenderContext.

use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::chart::annotations::LineStyle;
use super::{ChartRenderState, LineRenderStyle};

/// Cap on gap markers drawn per frame (data-driven, but bounded — a
/// pathological series with thousands of tiny gaps must not blow up frame
/// time or text allocation).
const MAX_GAP_MARKERS: usize = 64;

/// Multiplier applied to the visible-range median inter-bar delta to decide
/// whether a delta counts as a "hole" worth marking.
const GAP_MEDIAN_MULTIPLIER: i64 = 10;

/// Absolute floor for the gap threshold (ms) — guards against a
/// pathologically small median (e.g. a burst of near-duplicate ticks)
/// flagging normal spacing as a gap.
const GAP_MIN_THRESHOLD_MS: i64 = 30 * 60 * 1000;

/// Minimum bar count in the visible range before trusting its own median;
/// below this, fall back to the whole series so a mostly-empty viewport
/// doesn't derive a meaningless threshold from 1-2 samples.
const GAP_MIN_SAMPLE_BARS: usize = 8;

/// Draw chart grid (horizontal price lines, vertical time lines)
pub fn draw_grid(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let grid = state.grid;
    let theme = state.theme;
    let viewport = state.viewport;
    let price_scale = state.price_scale;

    // Clip grid to chart rect
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();

    // Draw horizontal grid lines (price levels)
    if grid.horz_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);

        // Calculate price ticks
        let step = price_scale.calc_step(rect.height);
        if step > 0.0 {
            // Extend range by 50px worth of price in each direction
            let range = price_scale.price_max - price_scale.price_min;
            let margin = if rect.height > 0.0 { 50.0 * range / rect.height } else { 0.0 };
            let gen_min = price_scale.price_min - margin;
            let gen_max = price_scale.price_max + margin;

            // Use index-based iteration to avoid floating point accumulation errors
            // At extreme zoom, `price += step` accumulates errors causing uneven grid spacing
            let start_price = (gen_min / step).floor() * step;
            let num_ticks = ((gen_max - start_price) / step).ceil() as i32 + 1;

            for i in 0..num_ticks {
                // Calculate price fresh each iteration from index to avoid accumulation
                let price = start_price + (i as f64) * step;

                if price > gen_max {
                    break;
                }

                let y = viewport.price_to_y(price, price_scale.price_min, price_scale.price_max);
                let screen_y = rect.y + y;
                draw_styled_line(
                    ctx,
                    rect.x, screen_y,
                    rect.right(), screen_y,
                    &grid.horz_lines.style,
                );
            }
        }
    }

    // Draw vertical grid lines (time intervals). `hide_time_grid` (counted-bar
    // family: range/volume/dollar/tick/imbalance/run bars) and the derived
    // slot check that used to live in `ChartRenderState::non_temporal()`
    // (renko/pnf/kagi/3lb) used to blanket-suppress the grid because the old
    // per-bar tick path had no calendar cadence to draw a meaningful grid
    // from. `generate_ticks` now routes both families through the auto
    // calendar grid (round boundaries snapped to the nearest real bar/
    // element) whenever it can find a usable step, so neither flag gates the
    // grid anymore — `ticks` is empty only when there is genuinely nothing
    // to draw, and the loop below is a no-op.
    if grid.vert_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);

        // Use pre-computed ticks if available, otherwise generate on-demand
        let owned_ticks;
        let ticks: &[mlc_core::chart::types::TimeTick] = if let Some(t) = state.time_ticks {
            t
        } else {
            owned_ticks = state.time_scale.generate_ticks(
                viewport,
                state.bars,
                |text| ctx.measure_text(text),
                state.time_format_settings,
                state.non_temporal_axis,
            );
            &owned_ticks
        };

        for tick in ticks {
            // TimeTick already has pre-computed x coordinate
            let screen_x = rect.x + tick.x;
            draw_styled_line(
                ctx,
                screen_x, rect.y,
                screen_x, rect.bottom(),
                &grid.vert_lines.style,
            );
        }
    }

    ctx.restore();
}

/// Draw chart grid extended to custom bounds (for Glass styles)
/// Uses chart_rect for coordinate calculations but draws to extended_bounds
pub fn draw_grid_extended(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    extended_bounds: &mlc_core::state::geometry::LayoutRect,
) {
    let rect = &state.chart_rect;
    let grid = state.grid;
    let theme = state.theme;
    let viewport = state.viewport;
    let price_scale = state.price_scale;

    // Draw horizontal grid lines (price levels) - extended to full bounds
    if grid.horz_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);

        let step = price_scale.calc_step(rect.height);
        if step > 0.0 {
            // Extend price range to cover full window
            let extra_top = (rect.y - extended_bounds.y) / rect.height * (price_scale.price_max - price_scale.price_min);
            let extra_bottom = (extended_bounds.bottom() - rect.bottom()) / rect.height * (price_scale.price_max - price_scale.price_min);

            let extended_price_min = price_scale.price_min - extra_bottom;
            let extended_price_max = price_scale.price_max + extra_top;

            let start_price = (extended_price_min / step).floor() * step;
            let num_ticks = ((extended_price_max - start_price) / step).ceil() as i32 + 1;

            for i in 0..num_ticks {
                let price = start_price + (i as f64) * step;

                if price > extended_price_max {
                    break;
                }

                let y = viewport.price_to_y(price, price_scale.price_min, price_scale.price_max);
                let screen_y = rect.y + y;
                // No clipping - draw across full horizontal bounds
                draw_styled_line(
                    ctx,
                    extended_bounds.x, screen_y,
                    extended_bounds.right(), screen_y,
                    &grid.horz_lines.style,
                );
            }
        }
    }

    // Draw vertical grid lines (time intervals) - extended to full bounds.
    // Same rule as `draw_grid` — neither `hide_time_grid` nor
    // `non_temporal()` gates the grid anymore now that `generate_ticks`
    // routes both families through the auto calendar grid.
    if grid.vert_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);

        let owned_ticks;
        let ticks: &[mlc_core::chart::types::TimeTick] = if let Some(t) = state.time_ticks {
            t
        } else {
            owned_ticks = state.time_scale.generate_ticks(
                viewport,
                state.bars,
                |text| ctx.measure_text(text),
                state.time_format_settings,
                state.non_temporal_axis,
            );
            &owned_ticks
        };

        for tick in ticks {
            let screen_x = rect.x + tick.x;
            // No clipping - draw across full vertical bounds
            draw_styled_line(
                ctx,
                screen_x, extended_bounds.y,
                screen_x, extended_bounds.bottom(),
                &grid.vert_lines.style,
            );
        }
    }
}

/// Draw visible DATA-GAP markers: a floor-to-ceiling band ~5 bar slots
/// wide at the seam, dashed edges, labeled "no data · <dur>" (+ the
/// pending archive day when the hole spans a UTC date) — wherever two
/// consecutive visible bars hide a time discontinuity much larger than the
/// series' own typical spacing (owner requirement 2026-07-13: index-axis
/// charts glue bars side-by-side, so an archive-day lag looked contiguous;
/// a thin line was not readable — he asked for a band with text).
///
/// Threshold is derived purely from the data (median consecutive-delta
/// across the visible range, widened for irregular/counted-bar families by
/// falling back to the whole series when the visible range is too short to
/// give a stable median) — no chart-type string matching, per doctrine.
pub fn draw_data_gaps(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let theme = state.theme;
    let bars = state.bars;

    if bars.len() < 2 {
        return;
    }

    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());
    if end <= start + 1 {
        return;
    }

    let median_delta_ms = match median_consecutive_delta_ms(bars, start, end) {
        Some(m) if m > 0 => m,
        _ => return,
    };
    let threshold_ms = (GAP_MEDIAN_MULTIPLIER * median_delta_ms).max(GAP_MIN_THRESHOLD_MS);

    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();

    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);

    let mut markers_drawn = 0usize;
    for i in start..(end - 1) {
        if markers_drawn >= MAX_GAP_MARKERS {
            break;
        }
        let ts_a = bars[i].ts_ms();
        let ts_b = bars[i + 1].ts_ms();
        let delta_ms = ts_b - ts_a;
        if delta_ms <= threshold_ms {
            continue;
        }

        // Midpoint between bar i and bar i+1 slots (same index->x mapping
        // the candle/grid renderers use).
        let x_a = viewport.x_for_bar(i, ts_a);
        let x_b = viewport.x_for_bar(i + 1, ts_b);
        let screen_x = rect.x + (x_a + x_b) * 0.5;

        // Floor-to-ceiling band ~5 bar slots wide, clamped so it stays a
        // readable block at extreme zooms and never narrower than its label.
        let label = format!("no data \u{b7} {}", humanize_gap_duration(delta_ms));
        let days_label = missing_utc_days_label(ts_a, ts_b);
        let text_w = ctx
            .measure_text(&label)
            .max(days_label.as_deref().map(|d| ctx.measure_text(d)).unwrap_or(0.0));
        let band_w = (5.0 * viewport.bar_spacing)
            .clamp(28.0, 180.0)
            .max(text_w + 14.0);
        let band_x = screen_x - band_w * 0.5;
        ctx.set_fill_color("rgba(150, 152, 161, 0.10)");
        ctx.fill_rect(band_x, rect.y, band_w, rect.height);

        // draw_dashed_line strokes with whatever color is current — pin it,
        // or the marker inherits the last grid/scale stroke.
        ctx.set_stroke_color(&theme.crosshair);
        ctx.set_stroke_width(1.0);
        draw_dashed_line(ctx, band_x, rect.y, band_x, rect.bottom(), 4.0, 4.0);
        draw_dashed_line(ctx, band_x + band_w, rect.y, band_x + band_w, rect.bottom(), 4.0, 4.0);

        ctx.set_fill_color(&theme.crosshair);
        ctx.fill_text(&label, screen_x, rect.y + rect.height * 0.5 - 14.0);
        // Archive holes are day-file lag: name the missing UTC day(s) so
        // the user knows what the chart is waiting for.
        if let Some(days) = days_label.as_deref() {
            ctx.fill_text(days, screen_x, rect.y + rect.height * 0.5 + 2.0);
        }

        markers_drawn += 1;
    }

    ctx.restore();
}

/// Second label line for an archive-class hole: the UTC day(s) the gap
/// spans, phrased as the zip(s) the chart is waiting for. `None` when the
/// gap stays inside one UTC day (not an archive-day hole — nothing to
/// await, e.g. a venue outage).
fn missing_utc_days_label(ts_a: i64, ts_b: i64) -> Option<String> {
    let day_a = ts_a.div_euclid(86_400_000);
    let day_b = ts_b.div_euclid(86_400_000);
    if day_b <= day_a {
        return None;
    }
    let fmt = |days: i64| -> String {
        // Howard Hinnant's civil_from_days — no chrono dep in this crate.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        format!("{m:02}-{d:02}")
    };
    // The hole starts inside day_a (its zip is the first one still missing)
    // and ends inside day_b (whose zip lands only after that day closes).
    if day_b - day_a == 1 && ts_b % 86_400_000 == 0 {
        Some(format!("zip {} pending", fmt(day_a)))
    } else {
        Some(format!("zip {}..{} pending", fmt(day_a), fmt(day_b)))
    }
}

/// Median millisecond delta between consecutive bars over `[start, end)`.
/// Falls back to the whole series when the visible slice is shorter than
/// [`GAP_MIN_SAMPLE_BARS`], so a near-empty viewport doesn't derive a
/// threshold from 1-2 samples. Returns `None` when fewer than 2 deltas are
/// available anywhere.
fn median_consecutive_delta_ms(bars: &[mlc_core::Bar], start: usize, end: usize) -> Option<i64> {
    let (range_start, range_end) = if end.saturating_sub(start) >= GAP_MIN_SAMPLE_BARS {
        (start, end)
    } else {
        (0, bars.len())
    };

    if range_end <= range_start + 1 {
        return None;
    }

    let mut deltas: Vec<i64> = Vec::with_capacity(range_end - range_start - 1);
    for i in range_start..(range_end - 1) {
        let d = bars[i + 1].ts_ms() - bars[i].ts_ms();
        if d > 0 {
            deltas.push(d);
        }
    }
    if deltas.is_empty() {
        return None;
    }
    deltas.sort_unstable();
    Some(deltas[deltas.len() / 2])
}

/// Humanize a millisecond gap duration for the marker label:
/// `>=48h -> "Nd Hh"`, `>=1h -> "Nh"` (rounded), else `"Nm"`.
fn humanize_gap_duration(delta_ms: i64) -> String {
    let total_minutes = (delta_ms as f64 / 60_000.0).round() as i64;
    let total_hours = delta_ms as f64 / 3_600_000.0;

    if total_hours >= 48.0 {
        let days = (delta_ms / (24 * 3_600_000)).max(1);
        let remaining_hours = (delta_ms - days * 24 * 3_600_000) / 3_600_000;
        format!("{}d {}h", days, remaining_hours)
    } else if total_hours >= 1.0 {
        format!("{}h", total_hours.round() as i64)
    } else {
        format!("{}m", total_minutes.max(1))
    }
}

/// Draw a styled line (solid, dashed, dotted)
pub fn draw_styled_line(
    ctx: &mut dyn RenderContext,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    style: &LineStyle,
) {
    let line_style = LineRenderStyle::from_line_style(*style);

    if line_style.is_solid() {
        // Solid line
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(x1, y1);
        ctx.line_to(x2, y2);
        ctx.stroke();
    } else {
        // Dashed/dotted line
        draw_dashed_line(ctx, x1, y1, x2, y2, line_style.dash, line_style.gap);
    }
}

/// Draw a dashed line with custom dash/gap lengths
///
/// This function manually draws dashes for precise control over dash appearance.
pub fn draw_dashed_line(
    ctx: &mut dyn RenderContext,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    dash: f64,
    gap: f64,
) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 0.1 {
        return;
    }

    // Normalize direction
    let nx = dx / len;
    let ny = dy / len;

    let mut pos = 0.0;
    let mut drawing = true;

    ctx.begin_path();

    while pos < len {
        let segment_len = if drawing { dash } else { gap };
        let end_pos = (pos + segment_len).min(len);

        if drawing {
            let start_x = x1 + nx * pos;
            let start_y = y1 + ny * pos;
            let end_x = x1 + nx * end_pos;
            let end_y = y1 + ny * end_pos;

            ctx.move_to(start_x, start_y);
            ctx.line_to(end_x, end_y);
        }

        pos = end_pos;
        drawing = !drawing;
    }

    ctx.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_render_style() {
        let solid = LineRenderStyle::solid();
        assert!(solid.is_solid());

        let dashed = LineRenderStyle::dashed();
        assert!(!dashed.is_solid());
        assert_eq!(dashed.dash, 8.0);
        assert_eq!(dashed.gap, 4.0);
    }
}
