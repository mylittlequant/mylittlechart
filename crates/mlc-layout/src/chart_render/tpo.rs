//! TPO (Time-Price Opportunity) Market Profile renderer.
//!
//! Reads `state.tpo_sessions` (typically a small ring — current
//! session + N closed sessions). Each session renders as a block of
//! letter glyphs stacked at each price level + a POC + VAH/VAL
//! horizontal overlay.
//!
//! Sourced from `nemo/docs/mlc/research/specs/tpo-market-profile-spec-2026-06-18.md`
//! (bfolkens / beinghorizontal / sivamgr consensus).

use mlc_core::render::RenderContext;
use mlc_core::types::TpoSession;
use super::ChartRenderState;

const LETTER_CELL_PX: f64 = 8.0;

pub fn draw_tpo(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let sessions = match state.tpo_sessions {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;
    let _ = (viewport, theme);

    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    // Each session is rendered as a horizontal block; place blocks
    // left-to-right starting at chart_rect.x.
    let mut block_x = rect.x;
    for session in sessions {
        let block_w = render_session(ctx, state, session, block_x);
        block_x += block_w + 8.0; // 8 px gap between sessions
        if block_x > rect.x + rect.width { break; }
    }
}

/// Render one session block. Returns the X width consumed.
fn render_session(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    session: &TpoSession,
    x_origin: f64,
) -> f64 {
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;

    if session.rows.is_empty() { return 0.0; }
    let tick = session.tick_size.max(1e-9);
    let row_h = (rect.height * tick / (price_scale.price_max - price_scale.price_min)).max(7.0);
    let font_size = (row_h * 0.9).clamp(7.0, 14.0);
    ctx.set_font(&format!("{:.0}px monospace", font_size));

    // VA fill — light green band from VAL to VAH.
    let va_top = rect.y + state.viewport.price_to_y(
        session.vah_price, price_scale.price_min, price_scale.price_max,
    );
    let va_bot = rect.y + state.viewport.price_to_y(
        session.val_price, price_scale.price_min, price_scale.price_max,
    );

    let max_letters = session.rows.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
    let block_w = max_letters as f64 * LETTER_CELL_PX;

    ctx.set_fill_color("rgba(38, 166, 154, 0.12)");
    let va_top_y = va_top.min(va_bot);
    let va_h = (va_bot - va_top).abs().max(row_h);
    ctx.fill_rect(x_origin, va_top_y, block_w, va_h);

    // POC row tint (yellow band).
    let poc_y = rect.y + state.viewport.price_to_y(
        session.poc_price, price_scale.price_min, price_scale.price_max,
    );
    ctx.set_fill_color("rgba(255, 235, 59, 0.18)");
    ctx.fill_rect(x_origin, poc_y, block_w, row_h);

    // Letters per row.
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
    for (price, letters) in &session.rows {
        if !price.is_finite() { continue; }
        let y_top = rect.y + state.viewport.price_to_y(
            *price, price_scale.price_min, price_scale.price_max,
        );
        let cy = y_top + row_h * 0.5;
        // Inside-VA row uses chart theme up-color, outside uses muted gray.
        let in_va = *price >= session.val_price && *price <= session.vah_price;
        let color = if in_va { "#cfd8dc" } else { "rgba(120, 144, 156, 0.85)" };
        ctx.set_fill_color(color);
        let s: String = letters.iter().collect();
        ctx.fill_text(&s, x_origin + 2.0, cy);
    }

    // POC horizontal line spanning the block.
    ctx.set_stroke_color("#ffd54f");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x_origin, poc_y);
    ctx.line_to(x_origin + block_w, poc_y);
    ctx.stroke();

    block_w
}
