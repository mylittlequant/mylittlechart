//! TPO (Time-Price Opportunity) Market Profile renderer.
//!
//! Reads `state.tpo_sessions` — a ring of up to 11 sessions (10 closed +
//! 1 current, oldest left → newest right). Each session renders as a
//! side-by-side block of letter glyphs stacked at each price level, with
//! a VA fill band, POC tint + line, and a date header above the block.
//!
//! Industry-standard CBOT Market Profile layout per Steidlmayer:
//! sessions are displayed side-by-side so the trader can compare the
//! developing profile to prior sessions at a glance.
//!
//! Sourced from `nemo/docs/mlc/research/specs/tpo-market-profile-spec-2026-06-18.md`
//! (bfolkens / beinghorizontal / sivamgr consensus).

use mlc_core::render::RenderContext;
use mlc_core::types::TpoSession;
use super::ChartRenderState;

/// Width of one letter cell in pixels.
const LETTER_CELL_PX: f64 = 8.0;
/// Pixel gap between adjacent session blocks.
const SESSION_GAP_PX: f64 = 8.0;
/// Height reserved above the profile body for the date header.
const DATE_HEADER_PX: f64 = 16.0;

pub fn draw_tpo(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let sessions = match state.tpo_sessions {
        Some(s) if !s.is_empty() => s,
        _ => {
            super::draw_waiting_placeholder(ctx, state, "TPO: waiting for live data…");
            return;
        }
    };
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;

    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    // Sessions are stored oldest → newest; render left → right so the
    // current (newest) session appears on the right edge — matching the
    // standard Market Profile convention.
    let mut block_x = rect.x;
    for session in sessions {
        let block_w = render_session(ctx, state, session, block_x);
        if block_w == 0.0 { continue; }
        block_x += block_w + SESSION_GAP_PX;
        if block_x > rect.x + rect.width { break; }
    }
}

/// Format `open_time` (UTC midnight ms) as `"MMM DD"` for the date header.
fn format_session_date(open_time_ms: i64) -> String {
    const MS_PER_DAY: i64 = 86_400_000;
    // Days since Unix epoch.
    let days = (open_time_ms / MS_PER_DAY).max(0) as u32;
    // Gregorian calendar decomposition (civil calendar algorithm).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    const MONTHS: [&str; 12] = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let month_str = MONTHS.get((month as usize).saturating_sub(1)).unwrap_or(&"?");
    format!("{} {:02}", month_str, day)
}

/// Render one session block. Returns the X width consumed (0 if empty).
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

    let max_letters = session.rows.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
    let block_w = (max_letters as f64 * LETTER_CELL_PX).max(LETTER_CELL_PX);

    // VA fill — light teal band from VAL to VAH.
    let va_top_y = rect.y + state.viewport.price_to_y(
        session.vah_price, price_scale.price_min, price_scale.price_max,
    );
    let va_bot_y = rect.y + state.viewport.price_to_y(
        session.val_price, price_scale.price_min, price_scale.price_max,
    );
    let va_y = va_top_y.min(va_bot_y);
    let va_h = (va_bot_y - va_top_y).abs().max(row_h);
    ctx.set_fill_color("rgba(38, 166, 154, 0.12)");
    ctx.fill_rect(x_origin, va_y, block_w, va_h);

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
        // Inside-VA rows: slightly brighter than the muted outside rows.
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

    // Date header above the block (e.g. "Jun 17").
    let header_y = rect.y + DATE_HEADER_PX * 0.5;
    if header_y < rect.y + rect.height {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("rgba(176, 190, 197, 0.90)");
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);
        let label = format_session_date(session.open_time);
        ctx.fill_text(&label, x_origin + block_w * 0.5, header_y);
    }

    block_w
}
