//! Grid rendering
//!
//! Platform-agnostic grid drawing using RenderContext.

use crate::render::RenderContext;
use super::super::annotations::LineStyle;
use super::{ChartRenderState, LineRenderStyle};

/// Draw chart grid (horizontal price lines, vertical time lines)
pub fn draw_grid(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let rect = &state.chart_rect;
    let grid = state.grid;
    let theme = state.theme;
    let viewport = state.viewport;
    let price_scale = state.price_scale;

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

    // Draw vertical grid lines (time intervals)
    if grid.vert_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);

        // Use pre-computed ticks if available, otherwise generate on-demand
        let owned_ticks;
        let ticks: &[crate::chart::types::TimeTick] = if let Some(t) = state.time_ticks {
            t
        } else {
            owned_ticks = state.time_scale.generate_ticks(
                viewport,
                state.bars,
                |text| ctx.measure_text(text),
                state.time_format_settings,
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
}

/// Draw chart grid extended to custom bounds (for Glass styles)
/// Uses chart_rect for coordinate calculations but draws to extended_bounds
pub fn draw_grid_extended(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    extended_bounds: &crate::layout::LayoutRect,
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

    // Draw vertical grid lines (time intervals) - extended to full bounds
    if grid.vert_lines.visible {
        ctx.set_stroke_color(&theme.grid_line);
        ctx.set_stroke_width(1.0);

        let owned_ticks;
        let ticks: &[crate::chart::types::TimeTick] = if let Some(t) = state.time_ticks {
            t
        } else {
            owned_ticks = state.time_scale.generate_ticks(
                viewport,
                state.bars,
                |text| ctx.measure_text(text),
                state.time_format_settings,
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
