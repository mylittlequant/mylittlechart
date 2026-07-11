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
        _ => {
            super::draw_waiting_placeholder(ctx, state, "P&F: waiting for live data…");
            return;
        }
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

    // X/O are SHAPES stretched to the box cell (cell = bar_spacing wide ×
    // box_size-in-px tall), never fixed-size text glyphs — a 6-box column at
    // a 100px+ box height must read as a contiguous column, not confetti.
    let box_h_px = rect.height * box_size / price_range;
    let cell_w = (bar_w * 0.8).max(1.0);
    let half_w = cell_w * 0.5;

    // Degenerate zoom: cells too small to draw individual glyphs — render
    // each column as one solid bar spanning its full [bottom, top] extent.
    let tiny = box_h_px < 3.0 || cell_w < 3.0;

    for i in start..end {
        let col = &columns[i];
        let cx = viewport.bar_to_x(i) + rect.x;
        let color = if col.is_x { &theme.pnf_x } else { &theme.pnf_o };

        if tiny {
            let y_top = rect.y + viewport.price_to_y(
                col.top, price_scale.price_min, price_scale.price_max,
            );
            let y_bot = rect.y + viewport.price_to_y(
                col.bottom, price_scale.price_min, price_scale.price_max,
            );
            ctx.set_fill_color(color);
            let w = cell_w.max(1.0);
            ctx.fill_rect(cx - w * 0.5, y_top, w, (y_bot - y_top).max(1.0));
            continue;
        }

        ctx.set_stroke_color(color);
        // Render boxes top-to-bottom (top is highest price).
        let mut p = col.top;
        while p > col.bottom + 1e-9 {
            let y_top = rect.y + viewport.price_to_y(
                p, price_scale.price_min, price_scale.price_max,
            );
            let y_bot = rect.y + viewport.price_to_y(
                p - box_size, price_scale.price_min, price_scale.price_max,
            );
            // Inset the glyph slightly inside its cell so adjacent boxes
            // stay visually separate.
            let pad = ((y_bot - y_top) * 0.10).max(0.5);
            let top_i = y_top + pad;
            let bot_i = y_bot - pad;
            if col.is_x {
                ctx.begin_path();
                ctx.move_to(cx - half_w, bot_i);
                ctx.line_to(cx + half_w, top_i);
                ctx.move_to(cx - half_w, top_i);
                ctx.line_to(cx + half_w, bot_i);
                ctx.stroke();
            } else {
                let cy = (top_i + bot_i) * 0.5;
                let ry = ((bot_i - top_i) * 0.5).max(0.5);
                ctx.begin_path();
                ctx.ellipse(cx, cy, half_w, ry, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
            }
            p -= box_size;
        }
    }
}

