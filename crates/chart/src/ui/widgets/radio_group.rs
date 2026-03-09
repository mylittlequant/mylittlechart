//! Radio group widget for selecting one option from a vertical list.
//!
//! Each option renders a circle indicator, a label, and a description line.

use crate::render::{TextAlign, TextBaseline};
use crate::engine::render::RenderContext;
use super::types::WidgetTheme;

/// Result from rendering a radio group — contains per-option rects for hit testing.
pub struct RadioGroupResult {
    /// (x, y, w, h) for each option row, in the same order as the input `options` slice.
    pub option_rects: Vec<(f64, f64, f64, f64)>,
}

/// A single radio option with a short label and a longer description.
pub struct RadioOption<'a> {
    /// Unique key used as the hit-zone suffix (e.g. `"PerFrame"`).
    pub key: &'a str,
    /// Short label rendered at normal weight (e.g. `"Per Frame"`).
    pub label: &'a str,
    /// One-line description rendered at muted/small size below the label.
    pub description: &'a str,
}

/// Draw a vertical radio group and return per-option rects for hit-zone registration.
///
/// # Parameters
/// - `ctx`            – Render context
/// - `options`        – Ordered list of radio options
/// - `selected_index` – Index of the currently selected option
/// - `hovered_key`    – Key of the option currently under the cursor (for hover highlight)
/// - `x`, `y`        – Top-left origin of the group
/// - `width`          – Width of each option row
/// - `theme`          – Widget theme colours
pub fn draw_radio_group(
    ctx: &mut dyn RenderContext,
    options: &[RadioOption<'_>],
    selected_index: usize,
    hovered_key: Option<&str>,
    x: f64,
    y: f64,
    width: f64,
    theme: &WidgetTheme,
) -> RadioGroupResult {
    let item_height = 52.0;
    let gap = 8.0;
    let circle_radius = 7.0;
    let inner_dot_radius = 4.0;
    let label_font_size = 13.0;
    let desc_font_size = 11.0;
    // Circle centre x: a small indent from the left edge
    let circle_x = x + circle_radius + 4.0;
    // Text starts after the circle + gap
    let text_x = x + circle_radius * 2.0 + 16.0;

    let mut result = RadioGroupResult {
        option_rects: Vec::with_capacity(options.len()),
    };
    let mut current_y = y;

    for (i, option) in options.iter().enumerate() {
        let is_selected = i == selected_index;
        let is_hovered = hovered_key == Some(option.key);

        // ---- Hover background ----
        if is_hovered {
            ctx.set_fill_color(&theme.bg_hover);
            ctx.fill_rounded_rect(x, current_y, width, item_height, 6.0);
        }

        // ---- Outer ring of the radio circle ----
        // Vertically align with the label baseline (top of item + 14px ≈ middle of label).
        let circle_cy = current_y + 14.0;
        ctx.begin_path();
        ctx.arc(
            circle_x,
            circle_cy,
            circle_radius,
            0.0,
            std::f64::consts::TAU,
        );
        if is_selected {
            ctx.set_stroke_color(&theme.accent);
        } else {
            ctx.set_stroke_color(&theme.border_normal);
        }
        ctx.set_stroke_width(1.5);
        ctx.stroke();

        // ---- Inner filled dot when selected ----
        if is_selected {
            ctx.begin_path();
            ctx.arc(
                circle_x,
                circle_cy,
                inner_dot_radius,
                0.0,
                std::f64::consts::TAU,
            );
            ctx.set_fill_color(&theme.accent);
            ctx.fill();
        }

        // ---- Label text ----
        ctx.set_font(&format!("{}px sans-serif", label_font_size));
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_fill_color(if is_selected {
            &theme.text_hover
        } else {
            &theme.text_normal
        });
        ctx.fill_text(option.label, text_x, current_y + 4.0);

        // ---- Description text (muted, smaller) ----
        ctx.set_font(&format!("{}px sans-serif", desc_font_size));
        ctx.set_fill_color(&theme.text_disabled);
        ctx.fill_text(option.description, text_x, current_y + 24.0);

        // ---- Store rect for caller's hit-zone registration ----
        result.option_rects.push((x, current_y, width, item_height));

        current_y += item_height + gap;
    }

    result
}
