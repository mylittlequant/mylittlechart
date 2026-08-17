//! Measurement module - price and time measurement tools

pub mod price_range;
pub mod date_range;
pub mod price_date_range;

pub use price_range::PriceRange;
pub use date_range::DateRange;
pub use price_date_range::PriceDateRange;

use super::{RenderContext, LabelStyle};

/// Absolute price distance between two levels — the ONE production point
/// `PriceRange`/`PriceDateRange` use for both their own on-chart stat plate
/// (`render`) and their settings-modal stat block (`style_properties`), so
/// the two can never disagree about the number.
pub(super) fn price_diff(price1: f64, price2: f64) -> f64 {
    (price2 - price1).abs()
}

/// Percentage move of [`price_diff`] relative to `price1` — same shared-
/// production-point reasoning.
pub(super) fn percentage(price1: f64, price2: f64) -> f64 {
    if price1 != 0.0 { (price_diff(price1, price2) / price1.abs()) * 100.0 } else { 0.0 }
}

/// Human-readable elapsed-time label (`"3.2h"`, `"45m"`, ...) — the ONE
/// production point `DateRange`/`PriceDateRange` use for both their own
/// on-chart stat plate and their settings-modal stat block.
pub(super) fn duration_label(ts1: i64, ts2: i64) -> String {
    let duration_ms = (ts2 - ts1).abs();
    let duration_secs = duration_ms / 1000;
    if duration_secs >= 86400 {
        format!("{:.1}d", duration_secs as f64 / 86400.0)
    } else if duration_secs >= 3600 {
        format!("{:.1}h", duration_secs as f64 / 3600.0)
    } else if duration_secs >= 60 {
        format!("{:.0}m", duration_secs as f64 / 60.0)
    } else {
        format!("{duration_secs}s")
    }
}

/// Draw a multi-line stat plate (background + border via `LabelStyle`, lines
/// centered and stacked vertically) at `(cx, cy)`.
///
/// Shared by all three rulers (price/date/price-date range) so "light band +
/// readable stat plate on top" looks identical everywhere instead of each
/// tool inventing its own tiny low-contrast label.
pub(super) fn draw_stat_plate(
    ctx: &mut dyn RenderContext,
    cx: f64,
    cy: f64,
    lines: &[String],
    label_style: &LabelStyle,
    fallback_color: &str,
) {
    if lines.is_empty() {
        return;
    }
    ctx.set_font(&label_style.font_string());
    let line_height = label_style.font_size * 1.3;
    let total_h = line_height * lines.len() as f64;
    let max_w = lines.iter().map(|l| ctx.measure_text(l)).fold(0.0f64, f64::max);
    let text_color = label_style.color.as_deref().unwrap_or(fallback_color);

    if let Some(ref bg) = label_style.background_color {
        let padding = label_style.background_padding;
        let bg_x = cx - max_w / 2.0 - padding;
        let bg_y = cy - total_h / 2.0 - padding;
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(bg_x, bg_y, max_w + padding * 2.0, total_h + padding * 2.0, label_style.background_radius);
        ctx.fill();

        if let Some(ref border) = label_style.border_color {
            ctx.set_stroke_color(border);
            ctx.set_stroke_width(label_style.border_width);
            ctx.stroke();
        }
    }

    ctx.set_fill_color(text_color);
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
    let start_y = cy - total_h / 2.0 + line_height / 2.0;
    for (i, line) in lines.iter().enumerate() {
        ctx.fill_text(line, cx, start_y + line_height * i as f64);
    }
}
