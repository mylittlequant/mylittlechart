//! Funding Rate strip renderer (C15).
//!
//! Draws a thin horizontal strip of colored cells at the bottom edge of the
//! chart area (directly above the time scale). One cell per funding-rate event
//! (usually every 8 h on perp swaps).
//!
//! Cell color: green-to-red gradient by sign and magnitude
//! (`+` = longs pay shorts = bullish funding bias,
//!  `-` = shorts pay longs = bearish funding bias).
//!
//! Numeric label inside each cell: rate in basis-points
//! (e.g. "+12bps" / "-3bps"). Font 9 px; white text always (cells are
//! dark enough at any magnitude).
//!
//! Industry reference: TradingView "Funding Rate" panel, Coinglass funding
//! heat-map.

use mlc_core::render::RenderContext;
use mlc_core::types::FundingRateCell;
use super::ChartRenderState;

/// Minimum cell width in pixels (cells never shrink below this).
const MIN_CELL_WIDTH_PX: f64 = 40.0;

/// Neutral (zero-rate) cell color.
const COLOR_NEUTRAL: (u8, u8, u8) = (60, 63, 72);
/// Full positive-rate cell color (RGB).
const COLOR_POSITIVE: (u8, u8, u8) = (38, 166, 154); // #26a69a (teal / green)
/// Full negative-rate cell color (RGB).
const COLOR_NEGATIVE: (u8, u8, u8) = (239, 83, 80); // #ef5350 (red)

/// Draw the Funding Rate strip.
///
/// Cells are equal-width and NOT time-proportional — funding events are sparse
/// (≈ 3 per day) so time-proportional layout would leave the strip almost
/// empty during normal trading hours.
///
/// Returns the strip height consumed (0.0 when skipped).
pub fn draw_funding_rate_strip(
    ctx: &mut dyn RenderContext,
    // Kept for signature parity with the other pane renderers; the strip
    // needs no viewport or theme lookup of its own.
    _state: &ChartRenderState,
    cells: &[FundingRateCell],
    area: &mlc_core::chart::render_data::ChartRect,
) -> f64 {
    if cells.is_empty() { return 0.0; }

    // The caller owns the geometry. This used to take a 4% slice off the
    // bottom of `state.chart_rect` — of the PRICE PLOT itself — so the strip
    // landed on top of the volume histogram instead of in a pane.
    let rect = area;
    let strip_h = rect.height;
    let strip_y = rect.y;

    // Determine how many cells fit given min width and total rect width.
    let max_cells = ((rect.width / MIN_CELL_WIDTH_PX) as usize).max(1);
    // Show only the most-recent N cells that fit.
    let display_cells: &[FundingRateCell] = if cells.len() > max_cells {
        &cells[cells.len() - max_cells..]
    } else {
        cells
    };
    let n = display_cells.len();
    if n == 0 { return strip_h; }

    let cell_w = rect.width / n as f64;

    // Max |rate| across visible cells for color gradient normalisation.
    let max_abs_rate = display_cells
        .iter()
        .map(|c| c.rate.abs())
        .fold(0.0001_f64, f64::max); // floor avoids division by zero

    for (i, cell) in display_cells.iter().enumerate() {
        let cx = rect.x + i as f64 * cell_w;

        // Color: lerp between neutral and the positive/negative anchor.
        let intensity = (cell.rate.abs() / max_abs_rate).clamp(0.0, 1.0);
        let (tr, tg, tb) = if cell.rate >= 0.0 {
            COLOR_POSITIVE
        } else {
            COLOR_NEGATIVE
        };
        let (nr, ng, nb) = COLOR_NEUTRAL;
        let r = lerp_u8(nr, tr, intensity);
        let g = lerp_u8(ng, tg, intensity);
        let b = lerp_u8(nb, tb, intensity);
        let bg = format!("rgb({},{},{})", r, g, b);

        ctx.set_fill_color(&bg);
        ctx.fill_rect(cx, strip_y, cell_w, strip_h);

        // Label: rate in basis points.
        let bps = cell.rate * 10_000.0;
        let label = if bps >= 0.0 {
            format!("+{:.1}bps", bps)
        } else {
            format!("{:.1}bps", bps)
        };
        // White text is readable on both dark and light saturated backgrounds.
        ctx.set_fill_color("#ffffff");
        ctx.set_font("9px sans-serif");
        ctx.fill_text(
            &label,
            cx + cell_w * 0.5,
            strip_y + strip_h * 0.5 + 3.0,
        );
    }

    strip_h
}

/// Linear interpolation between two u8 channel values.
#[inline]
fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let a = a as f64;
    let b = b as f64;
    (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
}
