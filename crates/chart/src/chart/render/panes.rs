//! Sub-pane rendering for indicators
//!
//! Platform-agnostic sub-pane (indicator pane) rendering using RenderContext.
//!
//! # Functions
//!
//! - `draw_pane_grid()` - Grid lines in sub-pane
//! - `draw_pane_line()` - Line indicator output
//! - `draw_pane_histogram()` - Histogram indicator output
//! - `draw_pane_separator()` - Separator between panes
//! - `draw_pane_background()` - Pane background and title
//! - `draw_pane_price_scale()` - Y-axis labels for sub-pane

use crate::render::{RenderContext, TextAlign, TextBaseline, crisp};
use crate::chart::format_price;
use super::ChartRect;

/// Sub-pane geometry and state
#[derive(Clone, Debug)]
pub struct PaneGeom {
    /// Pane rectangle (in screen coordinates)
    pub rect: ChartRect,
    /// Minimum value for Y-axis
    pub value_min: f64,
    /// Maximum value for Y-axis
    pub value_max: f64,
    /// Pane title
    pub title: String,
}

/// Histogram rendering style
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistogramStyle {
    /// Draw bars from bottom of pane
    #[default]
    FromBottom,
    /// Draw bars from zero line (for MACD-style)
    Centered,
}

/// Theme for pane rendering
#[derive(Clone, Debug)]
pub struct PaneTheme {
    pub background: String,
    pub separator: String,
    pub grid_line: String,
    pub text: String,
    pub scale_bg: String,
    pub scale_border: String,
    pub up_color: String,
    pub down_color: String,
}

impl Default for PaneTheme {
    fn default() -> Self {
        Self {
            background: "#131722".to_string(),
            separator: "#2a2e39".to_string(),
            grid_line: "#2a2e3980".to_string(),
            text: "#d1d4dc".to_string(),
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            up_color: "#26a69a".to_string(),
            down_color: "#ef5350".to_string(),
        }
    }
}

/// Draw separator between panes
pub fn draw_pane_separator(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    color: &str,
) {
    ctx.set_fill_color(color);
    ctx.fill_rect(x, y, width, height);
}

/// Draw pane background and title
pub fn draw_pane_background(
    ctx: &mut dyn RenderContext,
    pane: &PaneGeom,
    theme: &PaneTheme,
) {
    let rect = &pane.rect;

    // Background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    // Title
    if !pane.title.is_empty() {
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&theme.text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&pane.title, rect.x + 8.0, rect.y + 4.0);
    }
}

/// Draw grid lines in sub-pane
pub fn draw_pane_grid(
    ctx: &mut dyn RenderContext,
    pane: &PaneGeom,
    theme: &PaneTheme,
    num_lines: usize,
) {
    let rect = &pane.rect;

    ctx.set_stroke_color(&theme.grid_line);
    ctx.set_stroke_width(0.5);
    ctx.set_line_dash(&[]);

    for i in 1..num_lines {
        let y = rect.y + rect.height * i as f64 / num_lines as f64;
        ctx.begin_path();
        ctx.move_to(rect.x, y);
        ctx.line_to(rect.right(), y);
        ctx.stroke();
    }
}

/// Draw line indicator output in sub-pane
///
/// # Parameters
/// - `ctx` - Render context
/// - `pane` - Pane geometry
/// - `values` - Indicator values (one per bar)
/// - `visible_range` - (start_idx, end_idx) of visible bars
/// - `bar_to_x` - Function to convert bar index to X coordinate
/// - `color` - Line color
/// - `width` - Line width
pub fn draw_pane_line(
    ctx: &mut dyn RenderContext,
    pane: &PaneGeom,
    values: &[f64],
    visible_range: (usize, usize),
    bar_to_x: impl Fn(usize) -> f64,
    color: &str,
    width: f64,
) {
    let rect = &pane.rect;
    let (start, end) = visible_range;
    let end = end.min(values.len());
    let pane_height = rect.height;
    let value_range = pane.value_max - pane.value_min;

    if value_range <= 0.0 || start >= end {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(width);
    ctx.set_line_dash(&[]);

    let mut in_path = false;

    for i in start..end {
        let v = values[i];
        if v.is_nan() || v.is_infinite() {
            if in_path {
                ctx.stroke();
                in_path = false;
            }
            continue;
        }

        let x = rect.x + bar_to_x(i);
        let ratio = (v - pane.value_min) / value_range;
        let y = rect.bottom() - ratio * pane_height;

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

/// Draw histogram indicator output in sub-pane
///
/// # Parameters
/// - `ctx` - Render context
/// - `pane` - Pane geometry
/// - `values` - Indicator values (one per bar)
/// - `visible_range` - (start_idx, end_idx) of visible bars
/// - `bar_to_x` - Function to convert bar index to X coordinate
/// - `bar_width` - Width of each bar
/// - `theme` - Pane theme for colors
/// - `style` - Histogram style (FromBottom or Centered)
pub fn draw_pane_histogram(
    ctx: &mut dyn RenderContext,
    pane: &PaneGeom,
    values: &[f64],
    visible_range: (usize, usize),
    bar_to_x: impl Fn(usize) -> f64,
    bar_width: f64,
    theme: &PaneTheme,
    style: HistogramStyle,
) {
    let rect = &pane.rect;
    let (start, end) = visible_range;
    let end = end.min(values.len());
    let pane_height = rect.height;
    let value_range = pane.value_max - pane.value_min;

    if value_range <= 0.0 || start >= end {
        return;
    }

    match style {
        HistogramStyle::Centered => {
            // Centered histogram: draw from zero line
            let max_bar_height = pane_height * 0.15;

            // Find max absolute value for scaling
            let mut max_abs = 0.0f64;
            for i in start..end {
                let v = values[i];
                if !v.is_nan() && !v.is_infinite() {
                    max_abs = max_abs.max(v.abs());
                }
            }
            if max_abs == 0.0 {
                max_abs = 1.0;
            }

            // Zero line Y position
            let zero_ratio = (0.0 - pane.value_min) / value_range;
            let zero_y = rect.bottom() - zero_ratio * pane_height;

            for i in start..end {
                let v = values[i];
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let x = rect.x + bar_to_x(i) - bar_width / 2.0;
                let h = (v.abs() / max_abs) * max_bar_height;

                let color = if v >= 0.0 { &theme.up_color } else { &theme.down_color };
                ctx.set_fill_color(color);

                if v >= 0.0 {
                    ctx.fill_rect(x, zero_y - h, bar_width, h);
                } else {
                    ctx.fill_rect(x, zero_y, bar_width, h);
                }
            }
        }
        HistogramStyle::FromBottom => {
            // FromBottom histogram: draw bars from bottom
            let base_y = rect.bottom();

            for i in start..end {
                let v = values[i];
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let x = rect.x + bar_to_x(i) - bar_width / 2.0;
                let ratio = (v - pane.value_min) / value_range;
                let h = ratio * pane_height;

                ctx.set_fill_color(&theme.up_color);
                ctx.fill_rect(x, base_y - h, bar_width, h);
            }
        }
    }
}

/// Draw price scale for sub-pane
pub fn draw_pane_price_scale(
    ctx: &mut dyn RenderContext,
    pane: &PaneGeom,
    scale_width: f64,
    theme: &PaneTheme,
) {
    let rect = &pane.rect;
    let pane_height = rect.height;
    let value_range = pane.value_max - pane.value_min;

    if value_range <= 0.0 {
        return;
    }

    // Scale rect (to the right of pane)
    let scale_x = rect.right();

    // Background
    ctx.set_fill_color(&theme.scale_bg);
    ctx.fill_rect(scale_x, rect.y, scale_width, pane_height);

    // Left border
    let border_x = crisp(scale_x, ctx.dpr());
    ctx.set_stroke_color(&theme.scale_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(border_x, rect.y);
    ctx.line_to(border_x, rect.bottom());
    ctx.stroke();

    // Calculate step
    let target_ticks = (pane_height / 30.0).clamp(2.0, 10.0);
    let raw_step = value_range / target_ticks;
    let step = nice_step(raw_step);

    if step <= 0.0 {
        return;
    }

    // Draw labels
    let text_x = scale_x + scale_width / 2.0;
    let font_size = 10.0;

    ctx.set_font(&format!("{}px sans-serif", font_size));
    ctx.set_fill_color(&theme.text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Use index-based iteration to avoid floating point accumulation errors
    let first = (pane.value_min / step).ceil() * step;
    let num_ticks = ((pane.value_max - first) / step).ceil() as i32 + 1;

    for i in 0..num_ticks {
        let value = first + (i as f64) * step;
        if value >= pane.value_max {
            break;
        }

        let ratio = (pane.value_max - value) / value_range;
        let y = rect.y + ratio * pane_height;

        if y > rect.y + 8.0 && y < rect.bottom() - 8.0 {
            let label = format_price(value, step);
            ctx.fill_text(&label, text_x, y);
        }
    }
}

/// Calculate a "nice" step value for labels
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let exp = raw.log10().floor();
    let frac = raw / 10.0_f64.powf(exp);

    let nice_frac = if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };

    nice_frac * 10.0_f64.powf(exp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nice_step() {
        assert_eq!(nice_step(0.3), 0.5);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(15.0), 20.0);
    }

    #[test]
    fn test_histogram_style() {
        assert_eq!(HistogramStyle::default(), HistogramStyle::FromBottom);
    }
}
