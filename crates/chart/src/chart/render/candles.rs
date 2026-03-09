//! Candlestick and bar rendering
//!
//! Platform-agnostic OHLC visualization using RenderContext.
//!
//! # Series Types
//!
//! - `draw_candles()` - Standard candlestick bars
//! - `draw_bars()` - OHLC bars with horizontal ticks
//! - `draw_hollow_candles()` - Hollow bullish, filled bearish
//! - `draw_heikin_ashi()` - Smoothed Heikin Ashi candles

use crate::render::{RenderContext, crisp, crisp_rect};
use super::ChartRenderState;

/// Draw standard candlestick bars (filled bodies, wicks)
///
/// Colors from theme:
/// - `candle_up` / `candle_down` - body colors
/// - `wick_up` / `wick_down` - wick colors
pub fn draw_candles(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;
    let dpr = ctx.dpr();
    let disable_clip = state.disable_clip;

    let bar_w = viewport.bar_width();
    let (start, end) = viewport.visible_range();
    let bounds = rect.bounds();

    // For Glass styles, extend range to draw bars under toolbars
    // Extra bars needed = toolbar_width / bar_spacing
    let (start, end) = if disable_clip {
        let extra_bars = (100.0 / viewport.bar_spacing).ceil() as usize; // ~100px for toolbars
        let start = start.saturating_sub(extra_bars);
        let end = (end + extra_bars).min(bars.len());
        (start, end)
    } else {
        (start, end)
    };

    for i in start..end {
        if i >= bars.len() {
            break;
        }
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i);
        let is_up = if state.use_prev_close && i > 0 {
            // Compare current close to previous bar's close
            bar.close >= bars[i - 1].close
        } else {
            bar.is_bullish()
        };

        // Select colors based on direction
        let body_color = if is_up { &theme.candle_up } else { &theme.candle_down };
        let wick_color = if is_up { &theme.wick_up } else { &theme.wick_down };
        let border_color = if is_up { &theme.candle_up_border } else { &theme.candle_down_border };

        // Calculate screen coordinates
        let wick_x = crisp(cx, dpr);
        let screen_x = rect.x + wick_x;

        // Skip if X is outside chart bounds (unless clipping disabled for Glass styles)
        if !disable_clip && !bounds.x_in_bounds(screen_x) {
            continue;
        }

        // Calculate Y coordinates
        let high_y_raw = rect.y + viewport.price_to_y(bar.high, price_scale.price_min, price_scale.price_max);
        let low_y_raw = rect.y + viewport.price_to_y(bar.low, price_scale.price_min, price_scale.price_max);

        // Clamp Y to chart bounds only if clipping enabled
        let (high_y, low_y) = if disable_clip {
            (high_y_raw, low_y_raw)
        } else {
            (bounds.clamp_y(high_y_raw), bounds.clamp_y(low_y_raw))
        };

        // Skip if completely outside Y bounds (only when clipping enabled)
        if !disable_clip && bounds.is_y_range_outside(high_y, low_y) {
            continue;
        }

        // Draw wick (vertical line from high to low)
        if state.wick_enabled {
            ctx.set_stroke_color(wick_color);
            ctx.set_stroke_width(1.0);
            ctx.set_line_dash(&[]);
            ctx.begin_path();
            ctx.move_to(screen_x, high_y);
            ctx.line_to(screen_x, low_y);
            ctx.stroke();
        }

        // Calculate body coordinates
        let top_raw = viewport.price_to_y(bar.open.max(bar.close), price_scale.price_min, price_scale.price_max);
        let bot_raw = viewport.price_to_y(bar.open.min(bar.close), price_scale.price_min, price_scale.price_max);
        let (top, bot) = if disable_clip {
            (top_raw, bot_raw)
        } else {
            (top_raw.clamp(0.0, rect.height), bot_raw.clamp(0.0, rect.height))
        };
        let h = (bot - top).max(1.0);

        // Crisp body rectangle
        let (rx, ry, rw, rh) = crisp_rect(cx - bar_w / 2.0, top, bar_w, h, dpr);

        if disable_clip {
            // No clipping - draw directly
            if state.body_enabled {
                ctx.set_fill_color(body_color);
                ctx.fill_rect(rect.x + rx, rect.y + ry, rw, rh);
            }
            if state.border_enabled {
                if let Some(border) = border_color {
                    ctx.set_stroke_color(border);
                    ctx.set_stroke_width(1.0);
                    ctx.set_line_dash(&[]);
                    // Add 0.5 offset for crisp stroke (like crisp() does for lines)
                    ctx.stroke_rect(rect.x + rx + 0.5, rect.y + ry + 0.5, rw - 1.0, rh - 1.0);
                }
            }
        } else {
            // Clamp to bounds
            if let (Some((body_x, body_w)), Some((body_y, body_h))) = (
                bounds.clamp_rect_x(rect.x + rx, rw),
                bounds.clamp_rect_y(rect.y + ry, rh),
            ) {
                if state.body_enabled {
                    ctx.set_fill_color(body_color);
                    ctx.fill_rect(body_x, body_y, body_w, body_h);
                }
                if state.border_enabled {
                    if let Some(border) = border_color {
                        ctx.set_stroke_color(border);
                        ctx.set_stroke_width(1.0);
                        ctx.set_line_dash(&[]);
                        // Add 0.5 offset for crisp stroke (like crisp() does for lines)
                        ctx.stroke_rect(body_x + 0.5, body_y + 0.5, body_w - 1.0, body_h - 1.0);
                    }
                }
            }
        }
    }
}

/// Draw OHLC bars with horizontal ticks for open/close
///
/// Each bar consists of:
/// - Vertical line from high to low
/// - Left tick at open price
/// - Right tick at close price
pub fn draw_bars(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;
    let dpr = ctx.dpr();

    let bar_w = viewport.bar_width();
    let tick_len = (bar_w * 0.4).clamp(2.0, 6.0);
    let (start, end) = viewport.visible_range();
    let bounds = rect.bounds();

    for i in start..end {
        if i >= bars.len() {
            break;
        }
        let bar = &bars[i];
        let cx = crisp(viewport.bar_to_x(i), dpr);
        let is_up = bar.is_bullish();

        let color = if is_up { &theme.candle_up } else { &theme.candle_down };
        let screen_x = rect.x + cx;

        // Skip if X is outside chart bounds
        if !bounds.x_in_bounds(screen_x) {
            continue;
        }

        // Calculate Y coordinates - clamp to chart bounds
        let high_y = bounds.clamp_y(rect.y + viewport.price_to_y(bar.high, price_scale.price_min, price_scale.price_max));
        let low_y = bounds.clamp_y(rect.y + viewport.price_to_y(bar.low, price_scale.price_min, price_scale.price_max));
        let open_y = bounds.clamp_y(rect.y + crisp(viewport.price_to_y(bar.open, price_scale.price_min, price_scale.price_max), dpr));
        let close_y = bounds.clamp_y(rect.y + crisp(viewport.price_to_y(bar.close, price_scale.price_min, price_scale.price_max), dpr));

        // Skip if completely outside Y bounds
        if bounds.is_y_range_outside(high_y, low_y) {
            continue;
        }

        ctx.set_stroke_color(color);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);

        // Vertical line (high to low)
        ctx.begin_path();
        ctx.move_to(screen_x, high_y);
        ctx.line_to(screen_x, low_y);
        ctx.stroke();

        // Open tick (left) - clamp to chart bounds
        if bounds.y_in_bounds(open_y) {
            let tick_start = bounds.clamp_x(screen_x - tick_len);
            ctx.begin_path();
            ctx.move_to(tick_start, open_y);
            ctx.line_to(screen_x, open_y);
            ctx.stroke();
        }

        // Close tick (right) - clamp to chart bounds
        if bounds.y_in_bounds(close_y) {
            let tick_end = bounds.clamp_x(screen_x + tick_len);
            ctx.begin_path();
            ctx.move_to(screen_x, close_y);
            ctx.line_to(tick_end, close_y);
            ctx.stroke();
        }
    }
}

/// Draw hollow candles - bullish are hollow (outline), bearish are filled
pub fn draw_hollow_candles(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;
    let dpr = ctx.dpr();

    let bar_w = viewport.bar_width();
    let (start, end) = viewport.visible_range();
    let bounds = rect.bounds();

    for i in start..end {
        if i >= bars.len() {
            break;
        }
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i);
        let is_up = bar.is_bullish();

        let body_color = if is_up { &theme.candle_up } else { &theme.candle_down };
        let wick_color = if is_up { &theme.wick_up } else { &theme.wick_down };

        // Calculate screen coordinates
        let wick_x = crisp(cx, dpr);
        let screen_x = rect.x + wick_x;

        // Skip if X is outside chart bounds
        if !bounds.x_in_bounds(screen_x) {
            continue;
        }

        // Clamp Y to chart bounds
        let high_y = bounds.clamp_y(rect.y + viewport.price_to_y(bar.high, price_scale.price_min, price_scale.price_max));
        let low_y = bounds.clamp_y(rect.y + viewport.price_to_y(bar.low, price_scale.price_min, price_scale.price_max));

        // Skip if completely outside Y bounds
        if bounds.is_y_range_outside(high_y, low_y) {
            continue;
        }

        // Draw wick
        ctx.set_stroke_color(wick_color);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(screen_x, high_y);
        ctx.line_to(screen_x, low_y);
        ctx.stroke();

        // Body coordinates - clamp to chart bounds
        let top_raw = viewport.price_to_y(bar.open.max(bar.close), price_scale.price_min, price_scale.price_max);
        let bot_raw = viewport.price_to_y(bar.open.min(bar.close), price_scale.price_min, price_scale.price_max);
        let top = top_raw.clamp(0.0, rect.height);
        let bot = bot_raw.clamp(0.0, rect.height);
        let h = (bot - top).max(1.0);

        let (rx, ry, rw, rh) = crisp_rect(cx - bar_w / 2.0, top, bar_w, h, dpr);
        if let (Some((body_x, body_w)), Some((body_y, body_h))) = (
            bounds.clamp_rect_x(rect.x + rx, rw),
            bounds.clamp_rect_y(rect.y + ry, rh),
        ) {
            if is_up {
                // Hollow (outline only) for bullish
                ctx.set_stroke_color(body_color);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rect(body_x, body_y, body_w, body_h);
            } else {
                // Filled for bearish
                ctx.set_fill_color(body_color);
                ctx.fill_rect(body_x, body_y, body_w, body_h);
            }
        }
    }
}

/// Draw Heikin Ashi candles (smoothed OHLC)
///
/// Heikin Ashi calculation:
/// - HA Close = (Open + High + Low + Close) / 4
/// - HA Open = (prev HA Open + prev HA Close) / 2
/// - HA High = max(High, HA Open, HA Close)
/// - HA Low = min(Low, HA Open, HA Close)
pub fn draw_heikin_ashi(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;
    let dpr = ctx.dpr();

    if bars.is_empty() {
        return;
    }

    let bar_w = viewport.bar_width();
    let (start, end) = viewport.visible_range();
    let bounds = rect.bounds();

    // Initialize Heikin Ashi values
    let mut prev_ha_open = (bars[0].open + bars[0].close) / 2.0;
    let mut prev_ha_close = (bars[0].open + bars[0].high + bars[0].low + bars[0].close) / 4.0;

    // Pre-calculate HA values up to visible range
    for i in 1..start.min(bars.len()) {
        let bar = &bars[i];
        let ha_close = (bar.open + bar.high + bar.low + bar.close) / 4.0;
        let ha_open = (prev_ha_open + prev_ha_close) / 2.0;
        prev_ha_open = ha_open;
        prev_ha_close = ha_close;
    }

    for i in start..end {
        if i >= bars.len() {
            break;
        }
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i);

        // Calculate HA OHLC
        let ha_close = (bar.open + bar.high + bar.low + bar.close) / 4.0;
        let ha_open = if i == 0 {
            (bar.open + bar.close) / 2.0
        } else {
            (prev_ha_open + prev_ha_close) / 2.0
        };
        let ha_high = bar.high.max(ha_open).max(ha_close);
        let ha_low = bar.low.min(ha_open).min(ha_close);

        let is_up = ha_close >= ha_open;

        let body_color = if is_up { &theme.candle_up } else { &theme.candle_down };
        let wick_color = if is_up { &theme.wick_up } else { &theme.wick_down };

        // Screen coordinates
        let wick_x = crisp(cx, dpr);
        let screen_x = rect.x + wick_x;

        // Skip if X is outside chart bounds
        if !bounds.x_in_bounds(screen_x) {
            // Still need to update HA values for next iteration
            prev_ha_open = ha_open;
            prev_ha_close = ha_close;
            continue;
        }

        // Clamp Y to chart bounds
        let high_y = bounds.clamp_y(rect.y + viewport.price_to_y(ha_high, price_scale.price_min, price_scale.price_max));
        let low_y = bounds.clamp_y(rect.y + viewport.price_to_y(ha_low, price_scale.price_min, price_scale.price_max));

        // Skip if completely outside Y bounds
        if bounds.is_y_range_outside(high_y, low_y) {
            prev_ha_open = ha_open;
            prev_ha_close = ha_close;
            continue;
        }

        // Wick
        ctx.set_stroke_color(wick_color);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(screen_x, high_y);
        ctx.line_to(screen_x, low_y);
        ctx.stroke();

        // Body - clamp to chart bounds
        let top_raw = viewport.price_to_y(ha_open.max(ha_close), price_scale.price_min, price_scale.price_max);
        let bot_raw = viewport.price_to_y(ha_open.min(ha_close), price_scale.price_min, price_scale.price_max);
        let top = top_raw.clamp(0.0, rect.height);
        let bot = bot_raw.clamp(0.0, rect.height);
        let h = (bot - top).max(1.0);

        let (rx, ry, rw, rh) = crisp_rect(cx - bar_w / 2.0, top, bar_w, h, dpr);
        if let (Some((body_x, body_w)), Some((body_y, body_h))) = (
            bounds.clamp_rect_x(rect.x + rx, rw),
            bounds.clamp_rect_y(rect.y + ry, rh),
        ) {
            ctx.set_fill_color(body_color);
            ctx.fill_rect(body_x, body_y, body_w, body_h);
        }

        // Update for next iteration
        prev_ha_open = ha_open;
        prev_ha_close = ha_close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heikin_ashi_formula() {
        // Test HA calculation
        let o = 100.0;
        let h = 110.0;
        let l = 95.0;
        let c = 105.0;

        let ha_close = (o + h + l + c) / 4.0;
        assert_eq!(ha_close, 102.5);

        let ha_open = (o + c) / 2.0; // For first bar
        assert_eq!(ha_open, 102.5);
    }

    #[test]
    fn test_chart_bounds() {
        use super::super::ChartRect;

        let rect = ChartRect::new(10.0, 20.0, 100.0, 80.0);
        let bounds = rect.bounds();

        assert_eq!(bounds.left, 10.0);
        assert_eq!(bounds.right, 110.0);
        assert_eq!(bounds.top, 20.0);
        assert_eq!(bounds.bottom, 100.0);

        assert!(bounds.x_in_bounds(50.0));
        assert!(!bounds.x_in_bounds(5.0));
        assert!(!bounds.x_in_bounds(115.0));

        assert_eq!(bounds.clamp_x(5.0), 10.0);
        assert_eq!(bounds.clamp_x(115.0), 110.0);
        assert_eq!(bounds.clamp_x(50.0), 50.0);
    }
}
