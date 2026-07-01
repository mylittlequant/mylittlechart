//! Point-and-Figure renderer — consumes pre-computed columns from the
//! station.
//!
//! Columns arrive via `LiveUpdate::PnfBarUpdate` from
//! `digdigdig3_station::TradeToPnfBarDerived` and land in
//! `window.pnf_columns`. Multiple emits with the same `column_id` are
//! collapsed into one column entry by the tick handler — the renderer
//! always sees one row per closed/open column.
//!
//! Each column owns its own slot — the X axis counts column index.
//! X-columns render the letter "X" stacked from bottom to top; O-columns
//! render "O".

use mlc_core::render::RenderContext;
use super::ChartRenderState;

/// Draw P&F columns from `state.pnf_columns`.
pub fn draw_pnf(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let columns = match state.pnf_columns {
        Some(c) if !c.is_empty() => c,
        _ => return,
    };
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let bar_w = viewport.bar_width();
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    let (start, end) = viewport.visible_range();
    let end = end.min(columns.len());
    if start >= end { return; }

    // Box size is the fixed user-supplied increment stored on every column by
    // the bridge (from `Kind::PnfBar(box_size_units, _)` at subscription time).
    // Using the first visible column is correct for the full visible window —
    // all columns in a single P&F subscription share the same box_size.
    let box_size = columns[start].box_size.max(1e-12);

    let box_h_px = (rect.height * box_size / price_range).max(6.0);
    let font_size = (box_h_px * 0.8).clamp(7.0, 18.0);
    ctx.set_font(&format!("{:.0}px monospace", font_size));

    for i in start..end {
        let col = &columns[i];
        let cx = viewport.bar_to_x(i) + rect.x;
        let color = if col.is_x { &theme.candle_up } else { &theme.candle_down };
        ctx.set_fill_color(color);
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);

        let glyph = if col.is_x { "X" } else { "O" };
        // Render boxes top-to-bottom (top is highest price).
        let mut p = col.top;
        while p > col.bottom + 1e-9 {
            let y_top = rect.y + viewport.price_to_y(
                p, price_scale.price_min, price_scale.price_max,
            );
            let y_bot = rect.y + viewport.price_to_y(
                p - box_size, price_scale.price_min, price_scale.price_max,
            );
            let cy = (y_top + y_bot) * 0.5;
            ctx.fill_text(glyph, cx, cy);
            p -= box_size;
        }
        let _ = bar_w;
    }

    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
}

