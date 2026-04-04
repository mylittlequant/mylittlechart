//! Overlay rendering (watermark, legend, tooltip, price lines, markers)
//!
//! Platform-agnostic overlay rendering using RenderContext.
//!
//! # Functions
//!
//! - `draw_watermark()` - Symbol/exchange text behind chart
//! - `draw_legend()` - OHLC values in corner
//! - `draw_tooltip()` - Floating OHLC box near cursor
//! - `draw_price_lines()` - Horizontal price level lines
//! - `draw_markers()` - Chart markers (arrows, circles, etc.)

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::chart::{Watermark, Legend, Tooltip, HorzAlign, VertAlign, LegendPosition};
use super::super::annotations::LineStyle;
use crate::Bar;
use super::{ChartRenderState, ChartRect, draw_styled_line};

/// Draw watermark text (symbol, exchange info) behind chart
///
/// The watermark is drawn with configurable position and alignment.
pub fn draw_watermark(
    ctx: &mut dyn RenderContext,
    rect: &ChartRect,
    watermark: &Watermark,
) {
    if !watermark.visible || watermark.lines.is_empty() {
        return;
    }

    let padding = watermark.padding;

    // Calculate total text block height
    let total_height: f64 = watermark.lines.iter().map(|l| l.font_size * 1.2).sum();
    let first_font_size = watermark.lines[0].font_size;

    // Vertical position
    let start_y = match watermark.vert_align {
        VertAlign::Top => padding + first_font_size / 2.0,
        VertAlign::Center => (rect.height - total_height) / 2.0 + first_font_size / 2.0,
        VertAlign::Bottom => rect.height - padding - total_height + first_font_size / 2.0,
    };

    // Horizontal position
    let text_x = match watermark.horz_align {
        HorzAlign::Left => padding,
        HorzAlign::Center => rect.width / 2.0,
        HorzAlign::Right => rect.width - padding,
    };

    let text_align = match watermark.horz_align {
        HorzAlign::Left => TextAlign::Left,
        HorzAlign::Center => TextAlign::Center,
        HorzAlign::Right => TextAlign::Right,
    };

    ctx.set_text_align(text_align);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Draw all lines
    let mut current_y = start_y;
    for line in &watermark.lines {
        ctx.set_font(&format!("{}px sans-serif", line.font_size));
        ctx.set_fill_color(&line.color);
        ctx.fill_text(&line.text, rect.x + text_x, rect.y + current_y);
        current_y += line.font_size * 1.2;
    }
}

/// Legend data for rendering
#[derive(Clone, Debug)]
pub struct LegendData {
    /// Symbol name
    pub symbol: String,
    /// Timeframe label
    pub timeframe: String,
    /// Current bar data
    pub bar: Bar,
    /// Change from open
    pub change: f64,
    /// Percent change
    pub percent: f64,
    /// Is bullish
    pub is_bullish: bool,
}

/// Draw OHLC legend in corner of chart
pub fn draw_legend(
    ctx: &mut dyn RenderContext,
    rect: &ChartRect,
    legend: &Legend,
    data: &LegendData,
    up_color: &str,
    down_color: &str,
) {
    if !legend.visible {
        return;
    }

    let padding = legend.padding;
    let font_size = legend.font_size;

    // Calculate position based on legend.position
    let (base_x_raw, base_y_raw, text_align) = match legend.position {
        LegendPosition::TopLeft => (
            rect.x + padding,
            rect.y + padding + font_size,
            TextAlign::Left,
        ),
        LegendPosition::TopRight => (
            rect.right() - padding,
            rect.y + padding + font_size,
            TextAlign::Right,
        ),
        LegendPosition::BottomLeft => (
            rect.x + padding,
            rect.bottom() - padding - font_size,
            TextAlign::Left,
        ),
        LegendPosition::BottomRight => (
            rect.right() - padding,
            rect.bottom() - padding - font_size,
            TextAlign::Right,
        ),
    };
    let base_x = base_x_raw.round();
    let base_y = base_y_raw.round();

    // Build legend text
    let mut parts = Vec::new();

    // Symbol and timeframe
    parts.push(format!("{} . {}", data.symbol, data.timeframe));

    // OHLC values (if enabled)
    if legend.show_ohlc {
        parts.push(format!("O: {}", TooltipLines::fmt_price(data.bar.open)));
        parts.push(format!("H: {}", TooltipLines::fmt_price(data.bar.high)));
        parts.push(format!("L: {}", TooltipLines::fmt_price(data.bar.low)));
        parts.push(format!("C: {}", TooltipLines::fmt_price(data.bar.close)));
    }

    // Change (if enabled)
    if legend.show_change && legend.show_percent {
        parts.push(format!("{:+.2} ({:+.2}%)", data.change, data.percent));
    } else if legend.show_change {
        parts.push(format!("{:+.2}", data.change));
    } else if legend.show_percent {
        parts.push(format!("({:+.2}%)", data.percent));
    }

    let legend_text = parts.join("  ");

    // Choose color based on direction
    let value_color = if data.is_bullish { up_color } else { down_color };

    // Draw legend
    ctx.set_font(&format!("{}px sans-serif", font_size));
    ctx.set_text_align(text_align);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_color(value_color);
    ctx.fill_text(&legend_text, base_x, base_y);
}

/// Tooltip content lines
#[derive(Clone, Debug)]
pub struct TooltipLines {
    pub lines: Vec<String>,
}

impl TooltipLines {
    pub fn from_bar(bar: &Bar) -> Self {
        Self {
            lines: vec![
                format!("O: {}", Self::fmt_price(bar.open)),
                format!("H: {}", Self::fmt_price(bar.high)),
                format!("L: {}", Self::fmt_price(bar.low)),
                format!("C: {}", Self::fmt_price(bar.close)),
                format!("V: {:.0}", bar.volume),
            ],
        }
    }

    /// Format price with appropriate precision for tooltip display
    fn fmt_price(price: f64) -> String {
        if price == 0.0 {
            "0".to_string()
        } else if price.abs() < 1e-8 {
            // Scientific notation for extremely small prices
            let exp = price.abs().log10().floor() as i32;
            let mantissa = price / 10f64.powi(exp);
            format!("{:.2}e{}", mantissa, exp)
        } else if price.abs() < 0.01 {
            format!("{:.8}", price)
        } else if price.abs() < 1.0 {
            format!("{:.6}", price)
        } else if price.abs() < 100.0 {
            format!("{:.4}", price)
        } else if price.abs() < 10000.0 {
            format!("{:.2}", price)
        } else {
            format!("{:.2}", price)
        }
    }
}

/// Draw tooltip with OHLC info near cursor
pub fn draw_tooltip(
    ctx: &mut dyn RenderContext,
    rect: &ChartRect,
    tooltip: &Tooltip,
    content: &TooltipLines,
    x: f64,
    y: f64,
) {
    let padding = tooltip.padding;
    let font_size = tooltip.font_size;
    let line_height = font_size * 1.4;

    // Calculate tooltip size
    ctx.set_font(&format!("{}px sans-serif", font_size));
    let mut max_width = 80.0f64;
    for line in &content.lines {
        let w = ctx.measure_text(line);
        if w > max_width {
            max_width = w;
        }
    }
    let tooltip_width = max_width + padding * 2.0;
    let tooltip_height = content.lines.len() as f64 * line_height + padding * 2.0;

    // Clamp position to chart bounds
    let tooltip_x = x.clamp(0.0, (rect.width - tooltip_width).max(0.0));
    let tooltip_y = y.clamp(0.0, (rect.height - tooltip_height).max(0.0));

    // Draw background
    ctx.set_fill_color(&tooltip.background_color);
    ctx.fill_rounded_rect(
        rect.x + tooltip_x,
        rect.y + tooltip_y,
        tooltip_width,
        tooltip_height,
        4.0,
    );

    // Draw border
    ctx.set_stroke_color(&tooltip.border_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(
        rect.x + tooltip_x,
        rect.y + tooltip_y,
        tooltip_width,
        tooltip_height,
        4.0,
    );

    // Draw lines
    ctx.set_fill_color(&tooltip.text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let mut text_y = rect.y + tooltip_y + padding + line_height * 0.5;
    for line in &content.lines {
        ctx.fill_text(line, rect.x + tooltip_x + padding, text_y);
        text_y += line_height;
    }
}

/// Price line configuration
#[derive(Clone, Debug)]
pub struct PriceLine {
    pub price: f64,
    pub color: String,
    pub line_width: f64,
    pub line_style: LineStyle,
    pub title: String,
    pub visible: bool,
}

/// Draw horizontal price lines with labels
pub fn draw_price_lines(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    price_lines: &[PriceLine],
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;

    for line in price_lines {
        if !line.visible {
            continue;
        }

        let y = viewport.price_to_y(line.price, price_scale.price_min, price_scale.price_max);
        if y < 0.0 || y > viewport.chart_height {
            continue;
        }

        let screen_y = rect.y + y;

        ctx.set_stroke_color(&line.color);
        ctx.set_stroke_width(line.line_width);

        draw_styled_line(
            ctx,
            rect.x, screen_y,
            rect.right(), screen_y,
            &line.line_style,
        );

        // Draw title
        if !line.title.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&line.color);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Bottom);
            ctx.fill_text(&line.title, rect.x + 5.0, screen_y - 2.0);
        }
    }
}

/// Marker shape
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerShape {
    Circle,
    Square,
    ArrowUp,
    ArrowDown,
}

/// Chart marker
#[derive(Clone, Debug)]
pub struct Marker {
    pub bar_idx: usize,
    pub price: f64,
    pub shape: MarkerShape,
    pub color: String,
    pub size: f64,
    pub text: Option<String>,
}

/// Draw chart markers (arrows, circles, squares)
pub fn draw_markers(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    markers: &[Marker],
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let (start_idx, end_idx) = viewport.visible_range();

    for marker in markers {
        // Skip markers outside visible range
        if marker.bar_idx < start_idx || marker.bar_idx >= end_idx {
            continue;
        }

        let x = viewport.bar_to_x(marker.bar_idx);
        let y = viewport.price_to_y(marker.price, price_scale.price_min, price_scale.price_max);

        // Skip if off-screen
        if y < -marker.size || y > viewport.chart_height + marker.size {
            continue;
        }
        if x < -marker.size || x > viewport.chart_width + marker.size {
            continue;
        }

        let screen_x = rect.x + x;
        let screen_y = rect.y + y;
        let size = marker.size;

        ctx.set_fill_color(&marker.color);

        match marker.shape {
            MarkerShape::Circle => {
                ctx.begin_path();
                ctx.arc(screen_x, screen_y, size / 2.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            MarkerShape::Square => {
                ctx.fill_rect(
                    screen_x - size / 2.0,
                    screen_y - size / 2.0,
                    size,
                    size,
                );
            }
            MarkerShape::ArrowUp => {
                ctx.begin_path();
                ctx.move_to(screen_x, screen_y - size * 0.5);
                ctx.line_to(screen_x - size * 0.5, screen_y + size * 0.5);
                ctx.line_to(screen_x + size * 0.5, screen_y + size * 0.5);
                ctx.close_path();
                ctx.fill();
            }
            MarkerShape::ArrowDown => {
                ctx.begin_path();
                ctx.move_to(screen_x, screen_y + size * 0.5);
                ctx.line_to(screen_x - size * 0.5, screen_y - size * 0.5);
                ctx.line_to(screen_x + size * 0.5, screen_y - size * 0.5);
                ctx.close_path();
                ctx.fill();
            }
        }

        // Draw text label
        if let Some(ref text) = marker.text {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(text, screen_x + size * 0.5 + 4.0, screen_y);
        }
    }
}

/// Draw last price projection line (horizontal line from last bar to right edge)
///
/// Shows the current/live price extending from the last candle's close.
/// - Green if close >= open (bullish)
/// - Red if close < open (bearish)
/// - Dashed line with ~0.7 alpha
pub fn draw_last_price_line(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let bars = state.bars;

    // Get last bar
    let last_bar = match bars.last() {
        Some(bar) => bar,
        None => return, // No data
    };

    // Determine if bullish or bearish
    let is_bullish = last_bar.close >= last_bar.open;

    // Choose color based on direction (with alpha for transparency)
    let base_color = if is_bullish { &theme.candle_up } else { &theme.candle_down };

    // Add alpha to color (assuming colors are in #RRGGBB format)
    let color_with_alpha = if base_color.starts_with('#') && base_color.len() == 7 {
        format!("{}b3", base_color) // Add ~0.7 alpha (b3 in hex = 179/255 ≈ 0.7)
    } else {
        base_color.clone()
    };

    // Calculate Y position using same function as candles (viewport.price_to_y)
    let close_price = last_bar.close;
    let y = viewport.price_to_y(close_price, price_scale.price_min, price_scale.price_max);

    // Clamp to chart area bounds (guard against collapsed/negative-height rects)
    let screen_y = (rect.y + y).clamp(rect.y, (rect.y + rect.height).max(rect.y));

    // Calculate X start position (from last bar's center)
    let last_bar_idx = bars.len() - 1;
    let start_x = viewport.bar_to_x(last_bar_idx);
    let screen_start_x = rect.x + start_x;

    // End at right edge of chart
    let screen_end_x = rect.right();

    // Only draw if there's visible space to the right
    if screen_start_x >= screen_end_x {
        return;
    }

    // Draw dashed line
    ctx.set_stroke_color(&color_with_alpha);
    ctx.set_stroke_width(1.0);
    ctx.set_line_dash(&[4.0, 3.0]); // 4px dash, 3px gap

    ctx.begin_path();
    ctx.move_to(screen_start_x, screen_y);
    ctx.line_to(screen_end_x, screen_y);
    ctx.stroke();

    // Reset line dash for other drawing operations
    ctx.set_line_dash(&[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tooltip_lines_from_bar() {
        let bar = Bar::new(0, 100.0, 110.0, 95.0, 105.0);
        let lines = TooltipLines::from_bar(&bar);
        assert_eq!(lines.lines.len(), 5);
    }

    #[test]
    fn test_marker_shape() {
        assert_eq!(MarkerShape::Circle, MarkerShape::Circle);
        assert_ne!(MarkerShape::Circle, MarkerShape::Square);
    }
}
