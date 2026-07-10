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

/// Width of one letter cell in pixels at 1:1 (unscaled) legibility.
const LETTER_CELL_PX: f64 = 8.0;
/// Pixel gap between adjacent session blocks.
const SESSION_GAP_PX: f64 = 8.0;
/// Height reserved above the profile body for the date header.
const DATE_HEADER_PX: f64 = 16.0;
/// Below this row height (px), individual letter glyphs are no longer
/// legible — collapse the profile to solid row blocks instead (mirrors
/// footprint's `MIN_BAR_WIDTH_PX`/`MIN_FONT_SIZE_PX` legibility gate,
/// `chart_render/footprint.rs`).
const MIN_LEGIBLE_ROW_PX: f64 = 6.0;
/// Below this per-letter-cell width (px), a scaled-down letter grid is no
/// longer legible either — collapse instead of shrinking further.
const MIN_LEGIBLE_CELL_PX: f64 = 3.0;
/// Fixed strip width for the collapsed (no-letters) rendering regime.
const COLLAPSED_BLOCK_PX: f64 = LETTER_CELL_PX * 3.0;
/// First 2 chronological TPO periods (letters 'A' and 'B') define the
/// Initial Balance range — canon Market Profile convention.
const INITIAL_BALANCE_LETTERS: [char; 2] = ['A', 'B'];

/// One session's resolved width regime — either a legible letter grid
/// (possibly scaled down to fit the allotted slot) or a collapsed solid
/// block. Pure decision extracted out of `render_session` so the fit math
/// is unit-testable without a `RenderContext`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SessionWidthFit {
    /// Draw letters; `cell_px` is the (possibly scaled-down) per-letter
    /// column width, `block_w` is `max_letters * cell_px`.
    Legible { cell_px: f64, block_w: f64 },
    /// Draw solid row blocks only, fixed narrow strip width.
    Collapsed { block_w: f64 },
}

impl SessionWidthFit {
    fn block_w(&self) -> f64 {
        match self {
            SessionWidthFit::Legible { block_w, .. } => *block_w,
            SessionWidthFit::Collapsed { block_w } => *block_w,
        }
    }
}

/// Decide how a session's letter grid should fit into `allotted_w` pixels
/// of horizontal room (the viewport's per-slot spacing minus the inter-
/// session gap).
///
/// - `row_h` below [`MIN_LEGIBLE_ROW_PX`]: rows themselves are too thin for
///   glyphs regardless of width — always collapse (mirrors the pre-existing
///   vertical legibility gate).
/// - Otherwise, the natural (1:1) width `max_letters * LETTER_CELL_PX` is
///   used when it fits `allotted_w`. When it doesn't, the cell width is
///   scaled down to fit; if that scaled cell width would drop below
///   [`MIN_LEGIBLE_CELL_PX`], the session collapses instead of rendering
///   illegibly-narrow letters.
fn fit_session_width(max_letters: usize, row_h: f64, allotted_w: f64) -> SessionWidthFit {
    let allotted_w = allotted_w.max(0.0);
    if row_h < MIN_LEGIBLE_ROW_PX || max_letters == 0 {
        return SessionWidthFit::Collapsed { block_w: COLLAPSED_BLOCK_PX.min(allotted_w.max(COLLAPSED_BLOCK_PX)) };
    }
    let natural_w = max_letters as f64 * LETTER_CELL_PX;
    if natural_w <= allotted_w {
        return SessionWidthFit::Legible { cell_px: LETTER_CELL_PX, block_w: natural_w };
    }
    // Natural width overflows the slot — scale the cell down to fit.
    let scaled_cell = allotted_w / max_letters as f64;
    if scaled_cell >= MIN_LEGIBLE_CELL_PX {
        SessionWidthFit::Legible { cell_px: scaled_cell, block_w: allotted_w }
    } else {
        SessionWidthFit::Collapsed { block_w: COLLAPSED_BLOCK_PX.min(allotted_w.max(1.0)) }
    }
}

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
    let viewport = state.viewport;

    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    // Sessions are stored oldest → newest, one index slot per session
    // (mirrors renko/pnf/kagi/tlb — the viewport, not sequential
    // accumulation, decides X). `allotted_w` is the per-slot pixel budget:
    // the full `bar_spacing` minus the inter-session gap, floored so a
    // degenerate zoom-out never collapses to a negative width.
    let allotted_w = (viewport.bar_spacing - SESSION_GAP_PX).max(1.0);

    let (start, end) = viewport.visible_range();
    let end = end.min(sessions.len());
    if start >= end { return; }

    for (i, session) in sessions.iter().enumerate().take(end).skip(start) {
        let cx = rect.x + viewport.x_for_bar(i, session.ts_ms());
        let block_x = cx - allotted_w * 0.5;
        render_session(ctx, state, session, block_x, allotted_w);
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

/// Render one session block, centered on `slot_x_origin` within a
/// `allotted_w`-px slot (the viewport's per-session pixel budget). The
/// letter grid (or its collapsed fallback) is centered inside the
/// allotted slot so under-filled sessions don't visually crowd their
/// neighbors.
fn render_session(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    session: &TpoSession,
    slot_x_origin: f64,
    allotted_w: f64,
) {
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;

    if session.rows.is_empty() { return; }
    // Cull sessions whose slot falls entirely outside the visible chart
    // rect (mirrors `draw_candles`' bounds check).
    if slot_x_origin + allotted_w < rect.x || slot_x_origin > rect.x + rect.width {
        return;
    }
    let tick = session.tick_size.max(1e-9);
    let row_h = (rect.height * tick / (price_scale.price_max - price_scale.price_min)).max(7.0);
    let font_size = (row_h * 0.9).clamp(7.0, 14.0);

    let max_letters = session.rows.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
    let fit = fit_session_width(max_letters, row_h, allotted_w);
    let collapsed = matches!(fit, SessionWidthFit::Collapsed { .. });
    let cell_px = match fit {
        SessionWidthFit::Legible { cell_px, .. } => cell_px,
        SessionWidthFit::Collapsed { .. } => LETTER_CELL_PX,
    };
    let block_w = fit.block_w();
    // Center the (possibly narrower-than-slot) block inside the allotted
    // slot.
    let x_origin = slot_x_origin + (allotted_w - block_w) * 0.5;
    ctx.set_font(&format!("{:.0}px monospace", (font_size * (cell_px / LETTER_CELL_PX)).clamp(6.0, 14.0)));

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

    // Initial Balance overlay (canon-fix wave 2, item 6d) — the price range
    // spanned by the first 2 chronological TPO periods (letters 'A'/'B').
    // Drawn as a bordered band BEHIND the row content so letters/blocks
    // still read on top of it. Data already present on `TpoSession.rows`
    // (letters carry their chronological bucket identity verbatim from
    // `digdigdig3_station::derived`'s `alphabet[bi % 52]` assignment) — no
    // data-plane gap.
    if let Some((ib_lo, ib_hi)) = initial_balance_range(session) {
        let ib_top_y = rect.y + state.viewport.price_to_y(ib_hi, price_scale.price_min, price_scale.price_max);
        let ib_bot_y = rect.y + state.viewport.price_to_y(ib_lo, price_scale.price_min, price_scale.price_max);
        let ib_y = ib_top_y.min(ib_bot_y);
        let ib_h = (ib_bot_y - ib_top_y).abs().max(row_h);
        ctx.set_stroke_color("rgba(255, 255, 255, 0.35)");
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[2.0, 2.0]);
        ctx.stroke_rect(x_origin + 0.5, ib_y + 0.5, block_w - 1.0, ib_h - 1.0);
        ctx.set_line_dash(&[]);
    }

    // POC row tint (yellow band).
    let poc_y = rect.y + state.viewport.price_to_y(
        session.poc_price, price_scale.price_min, price_scale.price_max,
    );
    ctx.set_fill_color("rgba(255, 235, 59, 0.18)");
    ctx.fill_rect(x_origin, poc_y, block_w, row_h);

    // Single-print rows (canon-fix wave 2, item 6c) — exactly 1 letter
    // touched this price level. Highlighted with a dashed border, same
    // visual language as footprint's single-print marker
    // (`chart_render/footprint.rs`'s `is_single_print` dashed-rect).
    for (price, letters) in &session.rows {
        if letters.len() != 1 { continue; }
        if !price.is_finite() { continue; }
        let y_top = rect.y + state.viewport.price_to_y(*price, price_scale.price_min, price_scale.price_max);
        ctx.set_stroke_color("rgba(255, 255, 255, 0.45)");
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[3.0, 2.0]);
        ctx.stroke_rect(x_origin + 0.5, y_top + 0.5, block_w - 1.0, row_h - 1.0);
        ctx.set_line_dash(&[]);
    }

    if collapsed {
        // Solid row blocks, alpha scaled by relative letter count (denser
        // rows = more opaque) — same "collapse to a filled shape" doctrine
        // footprint/renko use below their own legibility floors.
        let max_count = max_letters.max(1) as f64;
        for (price, letters) in &session.rows {
            if letters.is_empty() || !price.is_finite() { continue; }
            let y_top = rect.y + state.viewport.price_to_y(*price, price_scale.price_min, price_scale.price_max);
            let ratio = (letters.len() as f64 / max_count).clamp(0.0, 1.0);
            let in_va = *price >= session.val_price && *price <= session.vah_price;
            let base = if in_va { "38, 166, 154" } else { "120, 144, 156" };
            ctx.set_fill_color(&format!("rgba({base}, {:.2})", 0.25 + ratio * 0.55));
            ctx.fill_rect(x_origin, y_top, block_w, row_h);
        }
    } else {
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
    }

    // POC horizontal line spanning the block.
    ctx.set_stroke_color("#ffd54f");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x_origin, poc_y);
    ctx.line_to(x_origin + block_w, poc_y);
    ctx.stroke();

    // Poor High / Poor Low markers (canon-fix wave 2, item 6b) — the
    // session-extreme row (high or low) was touched by MORE than one TPO
    // period, meaning price failed to reject cleanly at the extreme (an
    // "unfinished" auction edge, canon Market Profile "poor high/low").
    // Drawn as a small triangle at the extreme row's outer edge, distinct
    // from single-print's dashed border (a poor extreme is never a single
    // print by definition — >1 letter).
    ctx.set_text_align(uzor::render::TextAlign::Center);
    if let Some(price) = poor_high_price(session) {
        let y_top = rect.y + state.viewport.price_to_y(price, price_scale.price_min, price_scale.price_max);
        ctx.set_fill_color("#ff5252");
        ctx.set_text_baseline(uzor::render::TextBaseline::Bottom);
        ctx.fill_text("▲", x_origin + block_w * 0.5, y_top);
    }
    if let Some(price) = poor_low_price(session) {
        let y_bot = rect.y + state.viewport.price_to_y(price, price_scale.price_min, price_scale.price_max) + row_h;
        ctx.set_fill_color("#ff5252");
        ctx.set_text_baseline(uzor::render::TextBaseline::Top);
        ctx.fill_text("▼", x_origin + block_w * 0.5, y_bot);
    }

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
}

/// Initial Balance price range (canon-fix wave 2, item 6d) — `(low, high)`
/// spanned by every row containing letter 'A' or 'B' (the first 2
/// chronological TPO periods). `None` when the session has no A/B rows yet
/// (a brand-new session before its second period closes).
fn initial_balance_range(session: &TpoSession) -> Option<(f64, f64)> {
    let mut lo: Option<f64> = None;
    let mut hi: Option<f64> = None;
    for (price, letters) in &session.rows {
        if !price.is_finite() { continue; }
        if !letters.iter().any(|c| INITIAL_BALANCE_LETTERS.contains(c)) { continue; }
        lo = Some(lo.map_or(*price, |l: f64| l.min(*price)));
        hi = Some(hi.map_or(*price, |h: f64| h.max(*price)));
    }
    match (lo, hi) {
        (Some(lo), Some(hi)) => Some((lo, hi)),
        _ => None,
    }
}

/// Poor High marker price (canon-fix wave 2, item 6b) — the session's
/// highest-price row, when it was touched by MORE than one TPO period
/// (price failed to reject cleanly at the extreme). `None` for an empty
/// session or a clean (single-letter) high.
fn poor_high_price(session: &TpoSession) -> Option<f64> {
    session.rows.iter()
        .filter(|(p, _)| p.is_finite())
        .max_by(|(p1, _), (p2, _)| p1.partial_cmp(p2).unwrap_or(std::cmp::Ordering::Equal))
        .filter(|(_, l)| l.len() > 1)
        .map(|(p, _)| *p)
}

/// Poor Low marker price — symmetric counterpart of
/// [`poor_high_price`] for the session's lowest-price row.
fn poor_low_price(session: &TpoSession) -> Option<f64> {
    session.rows.iter()
        .filter(|(p, _)| p.is_finite())
        .min_by(|(p1, _), (p2, _)| p1.partial_cmp(p2).unwrap_or(std::cmp::Ordering::Equal))
        .filter(|(_, l)| l.len() > 1)
        .map(|(p, _)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_session(rows: Vec<(f64, &str)>) -> TpoSession {
        let rows: Vec<(f64, Vec<char>)> = rows.into_iter()
            .map(|(p, letters)| (p, letters.chars().collect()))
            .collect();
        let high = rows.iter().map(|(p, _)| *p).fold(f64::NEG_INFINITY, f64::max);
        let low = rows.iter().map(|(p, _)| *p).fold(f64::INFINITY, f64::min);
        TpoSession {
            open_time: 0,
            tick_size: 1.0,
            session_high: high,
            session_low: low,
            poc_price: rows.first().map(|(p, _)| *p).unwrap_or(0.0),
            vah_price: high,
            val_price: low,
            rows,
        }
    }

    #[test]
    fn initial_balance_range_spans_a_and_b_rows() {
        let session = mk_session(vec![
            (100.0, "A"),
            (101.0, "AB"),
            (102.0, "B"),
            (103.0, "C"),
            (104.0, "CD"),
        ]);
        assert_eq!(initial_balance_range(&session), Some((100.0, 102.0)));
    }

    #[test]
    fn initial_balance_range_none_when_no_ab_rows() {
        let session = mk_session(vec![(100.0, "C"), (101.0, "D")]);
        assert_eq!(initial_balance_range(&session), None);
    }

    #[test]
    fn poor_high_detected_when_extreme_row_has_multiple_letters() {
        let session = mk_session(vec![
            (100.0, "A"),
            (101.0, "AB"),
            (102.0, "BC"), // session high, multi-letter -> poor high
        ]);
        assert_eq!(poor_high_price(&session), Some(102.0));
        assert_eq!(poor_low_price(&session), None, "low row has a single letter -> clean low");
    }

    #[test]
    fn poor_low_detected_when_extreme_row_has_multiple_letters() {
        let session = mk_session(vec![
            (100.0, "AB"), // session low, multi-letter -> poor low
            (101.0, "B"),
            (102.0, "C"), // session high, single letter -> clean high
        ]);
        assert_eq!(poor_low_price(&session), Some(100.0));
        assert_eq!(poor_high_price(&session), None);
    }

    #[test]
    fn single_print_rows_are_exactly_one_letter() {
        let session = mk_session(vec![
            (100.0, "A"),
            (101.0, "AB"),
            (102.0, "C"),
        ]);
        let single_prints: Vec<f64> = session.rows.iter()
            .filter(|(_, l)| l.len() == 1)
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(single_prints, vec![100.0, 102.0]);
    }

    // ── fit_session_width ───────────────────────────────────────────────

    #[test]
    fn fit_natural_width_when_it_already_fits() {
        // 5 letters * 8px = 40px, well under a 120px slot -> unscaled.
        let fit = fit_session_width(5, 20.0, 120.0);
        assert_eq!(fit, SessionWidthFit::Legible { cell_px: LETTER_CELL_PX, block_w: 40.0 });
    }

    #[test]
    fn fit_scales_down_when_natural_width_overflows_slot() {
        // 20 letters * 8px = 160px natural, but only 60px allotted ->
        // scaled cell = 3.0px, right at the legibility floor -> still legible.
        let fit = fit_session_width(20, 20.0, 60.0);
        assert_eq!(fit, SessionWidthFit::Legible { cell_px: 3.0, block_w: 60.0 });
    }

    #[test]
    fn fit_collapses_when_scaled_cell_drops_below_legible_floor() {
        // 40 letters into a 60px slot -> scaled cell = 1.5px < MIN_LEGIBLE_CELL_PX.
        let fit = fit_session_width(40, 20.0, 60.0);
        assert_eq!(fit, SessionWidthFit::Collapsed { block_w: COLLAPSED_BLOCK_PX.min(60.0) });
    }

    #[test]
    fn fit_collapses_below_min_legible_row_height_regardless_of_width() {
        // Plenty of width, but row_h below the vertical legibility floor.
        let fit = fit_session_width(3, 4.0, 500.0);
        assert!(matches!(fit, SessionWidthFit::Collapsed { .. }));
    }

    #[test]
    fn fit_collapses_when_no_letters() {
        let fit = fit_session_width(0, 20.0, 120.0);
        assert!(matches!(fit, SessionWidthFit::Collapsed { .. }));
    }

    #[test]
    fn fit_block_w_never_exceeds_allotted_width_when_scaled() {
        for max_letters in [10usize, 25, 60] {
            let fit = fit_session_width(max_letters, 20.0, 80.0);
            assert!(fit.block_w() <= 80.0 + 1e-9, "block_w {:?} exceeds allotted 80px for {max_letters} letters", fit);
        }
    }
}
