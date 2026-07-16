//! Scale rendering (price and time axes)
//!
//! Platform-agnostic price scale (Y-axis) and time scale (X-axis) rendering.
//!
//! # Functions
//!
//! - `draw_price_scale()` - Y-axis with price labels and crosshair indicator
//! - `draw_time_scale()` - X-axis with time labels and crosshair indicator

use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::chart::format_time_full_with_settings;
use mlc_core::TimeFormatSettings;
use mlc_core::chart::render_data::{ChartRenderState, ScaleConfig, ScaleTheme};

/// Draw the price scale (Y-axis) on the right side of the chart
///
/// # Parameters
/// - `ctx` - Render context
/// - `state` - Chart render state
/// - `config` - Scale configuration
/// - `scale_theme` - Scale-specific theme colors
/// - `origin_x` - X position where chart area ends (left edge of scale)
/// - `origin_y` - Y position of chart top
pub fn draw_price_scale(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    config: &ScaleConfig,
    scale_theme: &ScaleTheme,
    origin_x: f64,
    origin_y: f64,
) {
    let viewport = state.viewport;
    let price_scale = state.price_scale;

    // Blur background (FrostedGlass/LiquidGlass) - draws before solid background
    ctx.draw_blur_background(origin_x, origin_y, config.price_scale_width, viewport.chart_height);

    // Draw scale background (semi-transparent when blur style is active)
    ctx.set_fill_color(&scale_theme.scale_bg);
    ctx.fill_rect(origin_x, origin_y, config.price_scale_width, viewport.chart_height);

    // Note: Scale borders are now drawn by draw_scale_borders() in layout/render.rs
    // to ensure consistent full-rectangle borders around all scale areas.

    // Center text horizontally in price scale
    let text_x = origin_x + config.price_scale_width / 2.0;

    // Generate price ticks
    let ticks = price_scale.generate_ticks_for_mode(viewport.chart_height);

    // Dynamic font size: shrinks for long labels so they fit in 70px scale
    let dynamic_font_size = price_scale.calc_font_size(viewport.chart_height);

    // Set text styles before save so they persist after restore for boxes
    ctx.set_font(&format!("{}px sans-serif", dynamic_font_size));
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Clip tick labels to price scale bounds
    ctx.save();
    ctx.begin_path();
    ctx.rect(origin_x, origin_y, config.price_scale_width, viewport.chart_height);
    ctx.clip();

    ctx.set_fill_color(&scale_theme.scale_text);

    for price in &ticks {
        let y = price_scale.price_to_y(*price, viewport.chart_height);
        let label = price_scale.format_label(*price, viewport.chart_height);
        ctx.fill_text(&label, text_x, origin_y + y);
    }

    ctx.restore();

    // Draw last price label (always visible, not just on hover).
    // Use state.chart_rect.height as the height basis so the chip hides and
    // shows together with the last-price line in overlays.rs (which also uses
    // rect.height rather than viewport.chart_height to avoid escaping the chart
    // after a viewport pan where chart_height may not match the rendered area).
    if let Some(last_bar) = state.bars.last() {
        let last_price = last_bar.close;
        let chart_h = state.chart_rect.height;
        let display_y = price_scale.price_to_y(last_price, chart_h);

        if display_y > 0.0 && display_y < chart_h {
            // Determine if bullish or bearish
            let is_bullish = last_bar.close >= last_bar.open;

            // Choose background color based on direction
            let bg_color = if is_bullish {
                "#26a69a" // Green for bullish
            } else {
                "#ef5350" // Red for bearish
            };

            let label = price_scale.format_label(last_price, chart_h);

            // Compute countdown text when enabled
            let countdown_opt: Option<String> =
                if let (Some(scale_settings), Some(tf_interval_ms)) = (state.scale_settings, state.timeframe_interval_ms) {
                    if scale_settings.show_bar_countdown && tf_interval_ms > 0 {
                        let now = web_time::SystemTime::now()
                            .duration_since(web_time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        let bar_interval_secs = (tf_interval_ms / 1000).max(1);
                        // Bar-grid start from the LOCAL wall-clock ONLY, never
                        // from `last_bar.timestamp`. Anchoring to the data
                        // is wrong in BOTH directions: a stale bar (sparse-
                        // tick instrument, e.g. HyperLiquid for a
                        // low-liquidity symbol — no candle for a zero-trade
                        // interval) sits BELOW the grid cell and used to
                        // freeze the timer at 00:00; an exchange-stamped bar
                        // can sit ABOVE the grid cell when the local clock
                        // trails the venue (owner incident 2026-07-16: ~15s
                        // epoch skew vs bybit made a 15s TF countdown read
                        // up to 00:30, looking like a 1m timer). Anchored
                        // purely to the local grid, `remaining` is always
                        // within [0, interval] regardless of data staleness
                        // or clock skew. No mutation of `window.bars` —
                        // empty intervals stay empty (a gap, not a
                        // synthetic flat candle).
                        let grid_bar_start = (now / bar_interval_secs) * bar_interval_secs;
                        let bar_start = grid_bar_start;
                        let bar_close_time = bar_start + bar_interval_secs;
                        let remaining_secs = (bar_close_time - now).max(0);
                        let formatted = if remaining_secs >= 3600 {
                            format!("{:02}:{:02}:{:02}",
                                remaining_secs / 3600,
                                (remaining_secs % 3600) / 60,
                                remaining_secs % 60)
                        } else {
                            format!("{:02}:{:02}",
                                remaining_secs / 60,
                                remaining_secs % 60)
                        };
                        Some(formatted)
                    } else {
                        None
                    }
                } else {
                    None
                };

            // Label height grows when countdown is shown (2 lines of same font size)
            let height = if countdown_opt.is_some() { (dynamic_font_size * 2.6).max(34.0) } else { 20.0 };
            let width = config.price_scale_width - 2.0;
            let label_x = origin_x + 1.0;
            // Center the taller box on the price line
            let label_y = origin_y + display_y - height / 2.0;

            // Clip to price scale column so the label never overflows above/below the chart area.
            // Use chart_h (state.chart_rect.height) to match the hide-guard basis above.
            ctx.save();
            ctx.begin_path();
            ctx.rect(origin_x, origin_y, config.price_scale_width, chart_h);
            ctx.clip();

            // Draw blur background (for FrostedGlass/LiquidGlass styles)
            ctx.draw_blur_background(label_x, label_y, width, height);

            // Draw label background with color based on direction
            ctx.set_fill_color(bg_color);
            ctx.fill_rect(label_x, label_y, width, height);

            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);

            if let Some(ref countdown_text) = countdown_opt {
                // Two-line layout: price on upper line, countdown on lower line (same font size)
                let line_offset = dynamic_font_size * 0.55;
                let price_line_y = origin_y + display_y - line_offset;
                let countdown_line_y = origin_y + display_y + line_offset;

                // Draw price text (white)
                ctx.set_font(&format!("{}px sans-serif", dynamic_font_size));
                ctx.set_fill_color("#ffffff");
                ctx.fill_text(&label, text_x, price_line_y);

                // Draw countdown text (same size, muted white)
                ctx.set_font(&format!("{}px sans-serif", dynamic_font_size));
                ctx.set_fill_color("rgba(255,255,255,0.7)");
                ctx.fill_text(countdown_text, text_x, countdown_line_y);
            } else {
                // Single-line: price centered in the 20px box
                ctx.set_font(&format!("{}px sans-serif", dynamic_font_size));
                ctx.set_fill_color("#ffffff");
                ctx.fill_text(&label, text_x, origin_y + display_y);
            }

            ctx.restore();
        }
    }

}

/// Draw only the crosshair price-indicator label on the price scale.
///
/// This is the cursor-dependent part of price-scale rendering, split out so
/// it can be called from the overlay pass without re-drawing the static ticks.
///
/// # Parameters
/// - `ctx`           – Render context
/// - `price_scale`   – Price scale for coordinate conversion and label formatting
/// - `viewport`      – Viewport (supplies `chart_height`)
/// - `config`        – Scale configuration (widths, font sizes)
/// - `scale_theme`   – Scale-specific theme colors (crosshair label colors)
/// - `crosshair`     – Crosshair state (price, visible)
/// - `origin_x`      – X position where chart area ends (left edge of price scale)
/// - `origin_y`      – Y position of chart top
/// - `text_x`        – Horizontal centre of the price scale (for text centering)
/// - `dynamic_font_size` – Font size computed in the static pass
pub fn draw_price_scale_cursor_label(
    ctx: &mut dyn RenderContext,
    price_scale: &mlc_core::chart::types::PriceScale,
    viewport: &mlc_core::chart::types::Viewport,
    config: &ScaleConfig,
    scale_theme: &ScaleTheme,
    crosshair: &mlc_core::chart::types::Crosshair,
    origin_x: f64,
    origin_y: f64,
    text_x: f64,
    dynamic_font_size: f64,
) {
    if !crosshair.visible {
        return;
    }

    let display_y = price_scale.price_to_y(crosshair.price, viewport.chart_height);

    if display_y > 0.0 && display_y < viewport.chart_height {
        let label = price_scale.format_label(crosshair.price, viewport.chart_height);
        let width = config.price_scale_width - 2.0;
        let height = 20.0;
        let label_x = origin_x + 1.0;
        let label_y = origin_y + display_y - 10.0;

        // Clip to price scale column so the label never overflows above/below the chart area
        ctx.save();
        ctx.begin_path();
        ctx.rect(origin_x, origin_y, config.price_scale_width, viewport.chart_height);
        ctx.clip();

        // Draw blur background (for FrostedGlass/LiquidGlass styles)
        ctx.draw_blur_background(label_x, label_y, width, height);

        // Draw label background with style opacity
        ctx.set_fill_color(&scale_theme.crosshair_label_bg_styled);
        ctx.fill_rect(label_x, label_y, width, height);

        // Draw label text
        ctx.set_font(&format!("{}px sans-serif", dynamic_font_size));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&scale_theme.crosshair_label_text);
        ctx.fill_text(&label, text_x, origin_y + display_y);

        ctx.restore();
    }
}

/// Draw the time scale (X-axis) at the bottom of the chart
///
/// # Parameters
/// - `ctx` - Render context
/// - `state` - Chart render state
/// - `config` - Scale configuration
/// - `scale_theme` - Scale-specific theme colors
/// - `origin_x` - X position of chart left edge
/// - `origin_y` - Y position where time scale starts (below chart)
pub fn draw_time_scale(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    config: &ScaleConfig,
    scale_theme: &ScaleTheme,
    origin_x: f64,
    origin_y: f64,
) {
    let viewport = state.viewport;
    let time_scale = state.time_scale;
    let bars = state.bars;

    // Blur background (FrostedGlass/LiquidGlass) - draws before solid background
    ctx.draw_blur_background(origin_x, origin_y, viewport.chart_width, config.time_scale_height);

    // Draw scale background (semi-transparent when blur style is active)
    ctx.set_fill_color(&scale_theme.scale_bg);
    ctx.fill_rect(origin_x, origin_y, viewport.chart_width, config.time_scale_height);

    // Note: Scale borders are now drawn by draw_scale_borders() in layout/render.rs
    // to ensure consistent full-rectangle borders around all scale areas.

    // Non-temporal chart types (renko/pnf/kagi/3lb) render on a derived
    // series whose ts-shadow carries real per-element timestamps
    // (`active_series::to_bar_shadow`) — `generate_ticks` routes them
    // through the auto calendar grid (round boundaries snapped to the
    // nearest real element), same as the counted-bar family. When that
    // yields no usable step (degenerate shadow, e.g. 2 elements) it falls
    // back to the per-bar picker internally; only a genuinely empty `bars`
    // slice produces zero ticks, which the loop below is a no-op for — no
    // separate blank-strip branch needed here.

    // Use pre-computed ticks if available, otherwise generate on-demand
    let owned_ticks;
    let ticks: &[mlc_core::chart::types::TimeTick] = if let Some(t) = state.time_ticks {
        t
    } else {
        owned_ticks = time_scale.generate_ticks(
            viewport,
            bars,
            |text| ctx.measure_text(text),
            state.time_format_settings,
            state.non_temporal_axis,
        );
        &owned_ticks
    };

    let label_y = origin_y + config.time_scale_height / 2.0;

    // Set text styles before save so they persist after restore for crosshair box
    ctx.set_font(&format!("{}px sans-serif", config.font_size));
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Clip tick labels to time scale bounds
    ctx.save();
    ctx.begin_path();
    ctx.rect(origin_x, origin_y, viewport.chart_width, config.time_scale_height);
    ctx.clip();

    for tick in ticks {
        // Choose color based on tick weight
        let color = if tick.weight.is_major() {
            &scale_theme.scale_text
        } else if tick.weight.is_medium() {
            &scale_theme.scale_text_medium
        } else {
            &scale_theme.scale_text_muted
        };
        ctx.set_fill_color(color);
        ctx.fill_text(&tick.label, origin_x + tick.x, label_y);
    }

    ctx.restore();

}

/// Draw only the crosshair time-indicator label on the time scale.
///
/// This is the cursor-dependent part of time-scale rendering, split out so
/// it can be called from the overlay pass without re-drawing the static ticks.
///
/// # Parameters
/// - `ctx`            – Render context
/// - `viewport`       – Viewport (supplies `chart_width`, `bar_to_x`, etc.)
/// - `bars`           – Bar data (used to resolve bar index to timestamp)
/// - `config`         – Scale configuration (font sizes, scale height)
/// - `scale_theme`    – Scale-specific theme colors (crosshair label colors)
/// - `crosshair`      – Crosshair state (bar_idx, bar_f64, visible)
/// - `time_format_settings` – Optional time format settings
/// - `origin_x`       – X position of chart left edge
/// - `origin_y`       – Y position where time scale starts (below chart)
/// - `label_y`        – Vertical center of the time scale (for text placement)
pub fn draw_time_scale_cursor_label(
    ctx: &mut dyn RenderContext,
    viewport: &mlc_core::chart::types::Viewport,
    bars: &[mlc_core::Bar],
    config: &ScaleConfig,
    scale_theme: &ScaleTheme,
    crosshair: &mlc_core::chart::types::Crosshair,
    time_format_settings: Option<&TimeFormatSettings>,
    origin_x: f64,
    origin_y: f64,
    label_y: f64,
) {
    // Draw crosshair time indicator.
    // Works for both in-data and future (extrapolated) positions.
    if !crosshair.visible {
        return;
    }

    // Resolve timestamp: use bar data when available, extrapolate for future bars.
    let ts_opt: Option<i64> = if let Some(bar_idx) = crosshair.bar_idx {
        bars.get(bar_idx).map(|b| b.timestamp)
    } else if bars.len() >= 2 {
        // Cursor is outside the data range (future or past).  Extrapolate from the
        // last two bars: derive the bar interval and apply it to bar_f64.
        let last = bars[bars.len() - 1].timestamp;
        let prev = bars[bars.len() - 2].timestamp;
        let interval_secs = last - prev; // seconds per bar (may be 0 for bad data)
        if interval_secs > 0 {
            let bars_past_end = crosshair.bar_f64 - (bars.len() - 1) as f64;
            let extra_secs = (bars_past_end * interval_secs as f64).round() as i64;
            Some(last + extra_secs)
        } else {
            None
        }
    } else if bars.len() == 1 {
        // Only one bar — can't derive interval; show that bar's time.
        Some(bars[0].timestamp)
    } else {
        None
    };

    if let Some(ts) = ts_opt {
        // ts is in seconds; convert to ms for the axis-aware helpers.
        let ts_ms = ts.saturating_mul(1000);
        let x = if let Some(bar_idx) = crosshair.bar_idx {
            let bar_ts_ms = bars.get(bar_idx).map(|b| b.ts_ms()).unwrap_or(ts_ms);
            viewport.x_for_bar(bar_idx, bar_ts_ms)
        } else {
            // Beyond data — resolve via fractional bar index from timestamp.
            viewport.x_for_ts(ts_ms, bars)
        };

        // Get format settings (use default if not provided)
        let default_settings = TimeFormatSettings::default();
        let format_settings = time_format_settings.unwrap_or(&default_settings);

        // Use new formatting function
        let label = format_time_full_with_settings(ts, format_settings);

        // Measure label width for centering
        let tw = ctx.measure_text(&label) + 10.0;
        let min_x = tw / 2.0;
        let max_x = (viewport.chart_width - tw / 2.0).max(min_x);
        let tx = if max_x >= min_x { x.clamp(min_x, max_x) } else { viewport.chart_width / 2.0 };
        let tx = tx.round();
        let box_x = origin_x + tx - tw / 2.0;
        let box_y = origin_y + 2.0;
        let box_height = config.time_scale_height - 4.0;

        ctx.save();
        ctx.begin_path();
        ctx.rect(origin_x, origin_y, viewport.chart_width, config.time_scale_height);
        ctx.clip();

        // Draw blur background (for FrostedGlass/LiquidGlass styles)
        ctx.draw_blur_background(box_x, box_y, tw, box_height);

        // Draw label background with style opacity
        ctx.set_fill_color(&scale_theme.crosshair_label_bg_styled);
        ctx.fill_rect(box_x, box_y, tw, box_height);

        // Draw label text
        ctx.set_font(&format!("{}px sans-serif", config.crosshair_font_size));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&scale_theme.crosshair_label_text);
        ctx.fill_text(&label, origin_x + tx, label_y);

        ctx.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_config_default() {
        let config = ScaleConfig::default();
        assert_eq!(config.price_scale_width, mlc_core::PRICE_SCALE_WIDTH);
        assert_eq!(config.time_scale_height, mlc_core::TIME_SCALE_HEIGHT);
    }

    #[test]
    fn test_scale_theme_default() {
        let theme = ScaleTheme::default();
        assert!(!theme.scale_bg.is_empty());
        assert!(!theme.crosshair_label_bg.is_empty());
    }
}
