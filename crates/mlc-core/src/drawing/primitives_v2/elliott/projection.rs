//! Fibonacci EXTENSION projections — where a motive (or motive-shaped)
//! wave that is STILL BEING PLACED is likely to end, measured as fibonacci
//! multiples of a NAMED prior leg (owner ask 2026-08-14: "по фибо проекциям
//! тоже нужно где-то удобный тугл сделать чтобы можно было считать фиба
//! удлинения по каждой движущей волне" — while a motive wave is still being
//! drawn, show where it can extend to).
//!
//! This is the FORWARD-LOOKING sibling of [`super::ratio`]: a ratio
//! annotation measures a leg that has ALREADY been drawn against one that
//! came before it; a projection measures a leg that has NOT been drawn yet
//! (or was JUST finished) against a named reference leg, so the analyst can
//! see the fibonacci target ticks BEFORE placing the wave's own endpoint.
//! Ratio values are the SAME extension table `mylittleindicators` already
//! owns for the same purpose (`mli::ewa::ratios::EXTENSION_TARGETS`) — read
//! from there, never copied chart-side, so a change to the canonical
//! extension catalogue is felt here automatically.
//!
//! # Geometry
//!
//! One relation per [`ProjectionLeg`]: a REFERENCE leg (`ref_start`→
//! `ref_end`) whose own price travel is the fibonacci unit, an `anchor`
//! pivot the projected wave STARTS from (the tick's own x), and the `end`
//! pivot the projected wave WOULD end at once placed (used only to decide
//! whether this leg is still live — see "Drawing rule" below; never used to
//! look up a point, because it may not be real yet). For each ratio `r` in
//! [`EXTENSION_TARGETS`]:
//!
//! ```text
//! dir           = sign of (price[ref_end] - price[ref_start])
//! target_price  = price[anchor] + dir * r * |price[ref_end] - price[ref_start]|
//! ```
//!
//! i.e. the target rides OFF the anchor, in the SAME direction the
//! reference leg itself traveled, magnitude `r` times the reference leg's
//! own travel — the ordinary Elliott extension convention (wave 3's target
//! rides off wave 2's end, in wave 1's own direction, `r` times wave 1's
//! length). Each target draws as a short horizontal tick starting at the
//! anchor's own x and running [`PROJECTION_TICK_WIDTH_PX`] to the right, at
//! the target's own y — never a diagonal line to a point that does not
//! exist yet, unlike a rib (a rib connects two REALIZED points; a
//! projection tick has nothing at its own far end to connect to).
//!
//! # Drawing rule (which legs are live right now)
//!
//! A leg draws only when:
//! - its reference leg is FULLY placed (`ref_start < realized && ref_end <
//!   realized`) — no reference unit, no projection;
//! - its `anchor` is placed (`anchor < realized`) — nothing to tick FROM
//!   yet otherwise;
//! - `end >= realized.saturating_sub(1)` — this leg is either the one
//!   CURRENTLY being placed (`end == realized`, the very next not-yet-real
//!   pivot) or the one JUST placed (`end == realized - 1`, still worth
//!   showing for one more step so the tick that guided the placement does
//!   not vanish the instant the click lands). A leg whose `end` is further
//!   in the past than that is SETTLED — the analyst has already moved on,
//!   so it stops projecting (this is also what keeps a FULLY realized
//!   structure from projecting its own already-placed waves — see each
//!   per-type table's own doc for which leg survives to the end);
//! - the reference leg's own price travel is non-zero (never divide by it).
//!
//! See [`projection_targets`] — the pure geometry helper implementing this
//! exactly, with no `RenderContext` dependency, so it is testable directly.
//!
//! # `ElliottTriangle` has no table and no toggle — DECISION, not a gap
//!
//! A triangle's five legs (A..E) CONTRACT toward its own apex — each leg is
//! definitionally shorter than the one two before it (`ratio::
//! TRIANGLE_RATIO_LEGS` already measures exactly that shrinking relation).
//! There is no "motive wave extending past its own reference leg" for a
//! triangle to project — inventing an extension table for a leg whose own
//! grammar requires it to CONTRACT would fabricate a relation the pattern
//! does not have. `ElliottTriangle` therefore gets no `PROJECTION_LEGS`
//! table here and no `show_projections` field on its own struct.
//!
//! # Style
//!
//! Gated per primitive by its own `show_projections` toggle. Colour/width/
//! dash resolution is IDENTICAL to a ratio rib's own
//! ([`super::ratio::default_rib_color`]/[`super::ratio::default_rib_width`]/
//! [`super::ratio::rib_dash_pattern`], driven by the SAME `ratio_rib_color`/
//! `ratio_rib_width`/`ratio_rib_style` fields every Elliott type already
//! carries) — a projection is the same annotation family as a ratio rib, not
//! a second palette with its own settings surface.

use super::super::config::LabelStyle;
use super::super::{crisp, draw_label_plate, LineStyle, RenderContext};
use super::ratio::{default_rib_color, default_rib_width, rib_dash_pattern};

use mli::ewa::ratios::EXTENSION_TARGETS;

/// Fixed pixel width of a projection tick, run to the right of its own
/// anchor point — a projection has no second REALIZED point to connect a
/// rib to (unlike [`super::ratio::Rib`]), so it draws a short fixed-length
/// marker instead of a line between two known pivots. ~64px reads clearly
/// at the ratio label's own font size without overrunning a tightly zoomed
/// chart.
const PROJECTION_TICK_WIDTH_PX: f64 = 64.0;

/// Horizontal push applied to a projection ratio label off the tick's own
/// far end, so the label plate does not sit centered ON TOP of the tick's
/// own stroke endpoint — same idea as `ratio::RATIO_LABEL_NUDGE_X`, smaller
/// here because this label anchors at a LINE END (nothing else competing
/// for that exact pixel), not beside a vertex a wave label already occupies.
const PROJECTION_LABEL_NUDGE_X: f64 = 6.0;

/// One reference→anchor relation to project, addressed by index into the
/// primitive's own `points()` array. `ref_start`/`ref_end` bound the leg
/// whose price travel is the fibonacci unit; `anchor` is the pivot the
/// projected wave starts from (the tick's own x); `end` is the pivot the
/// projected wave would end at once placed — used ONLY to decide whether
/// this leg is still live (see the module doc's "Drawing rule"), never to
/// index into `points` (it may not be real yet).
#[derive(Clone, Copy, Debug)]
pub(super) struct ProjectionLeg {
    pub(super) ref_start: usize,
    pub(super) ref_end: usize,
    pub(super) anchor: usize,
    pub(super) end: usize,
}

// ---- Per-type leg tables (owner spec, 2026-08-14) --------------------------

/// `ElliottImpulse` — points: start(0), wave 1..5 (1..=5). Wave 3 projects
/// off wave 1 (anchored at wave 2's own end); wave 5 projects off wave 1
/// (anchored at wave 4's own end) AND off the whole 1-3 span (same anchor)
/// — both wave-5 relations are standard confluence targets, so both stay
/// live at once once wave 4 is placed.
pub(super) const IMPULSE_PROJECTION_LEGS: [ProjectionLeg; 3] = [
    ProjectionLeg { ref_start: 0, ref_end: 1, anchor: 2, end: 3 }, // wave 3 from wave 1
    ProjectionLeg { ref_start: 0, ref_end: 1, anchor: 4, end: 5 }, // wave 5 from wave 1
    ProjectionLeg { ref_start: 0, ref_end: 3, anchor: 4, end: 5 }, // wave 5 from the 1-3 span
];

/// `ElliottCorrection` — points: start(0), A(1), B(2), C(3). C projects off
/// A (anchored at B's own end) — the standard zigzag C-extension target.
pub(super) const CORRECTION_PROJECTION_LEGS: [ProjectionLeg; 1] = [
    ProjectionLeg { ref_start: 0, ref_end: 1, anchor: 2, end: 3 }, // C from A
];

/// `ElliottDoubleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3),
/// Y subwaves(4,5,6) [Y: 3→6]. Y projects off W (anchored at X's own end).
pub(super) const DOUBLE_COMBO_PROJECTION_LEGS: [ProjectionLeg; 1] = [
    ProjectionLeg { ref_start: 0, ref_end: 2, anchor: 3, end: 6 }, // Y from W
];

/// `ElliottTripleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3),
/// Y subwaves(4,5) [Y: 3→5], XX(6), Z subwaves(7,8) [Z: 6→8]. Z projects off
/// Y (anchored at XX's own end).
pub(super) const TRIPLE_COMBO_PROJECTION_LEGS: [ProjectionLeg; 1] = [
    ProjectionLeg { ref_start: 3, ref_end: 5, anchor: 6, end: 8 }, // Z from Y
];

/// Pure geometry: the `(ratio, target_price)` pairs [`ProjectionLeg`] would
/// tick right now, given the primitive's own `points` and `realized` count
/// — see the module doc's "Drawing rule" for the gating and the geometry
/// formula. Empty when the leg is not live yet (a settled leg, an unplaced
/// anchor/reference), or when the reference leg's own price travel is zero
/// (never divides by it). No `RenderContext` dependency — this is the
/// helper [`draw_projection_annotations`] consumes, and what the unit tests
/// below assert on directly.
pub(super) fn projection_targets(points: &[(i64, f64)], realized: usize, leg: &ProjectionLeg) -> Vec<(f64, f64)> {
    if leg.ref_start >= realized || leg.ref_end >= realized || leg.anchor >= realized {
        return Vec::new();
    }
    // Only the leg currently under the hand (`end == realized`, not yet
    // real) or the one just finished (`end == realized - 1`) still
    // projects — see the module doc's own rationale for exactly this
    // boundary.
    if leg.end < realized.saturating_sub(1) {
        return Vec::new();
    }
    let (Some(&ref_start_pt), Some(&ref_end_pt), Some(&anchor_pt)) =
        (points.get(leg.ref_start), points.get(leg.ref_end), points.get(leg.anchor))
    else {
        return Vec::new();
    };

    let ref_travel = (ref_end_pt.1 - ref_start_pt.1).abs();
    if ref_travel <= 0.0 {
        return Vec::new();
    }
    let dir = (ref_end_pt.1 - ref_start_pt.1).signum();

    EXTENSION_TARGETS.iter().map(|&r| (r, anchor_pt.1 + dir * r * ref_travel)).collect()
}

/// A fixed extension ratio (`EXTENSION_TARGETS`'s own table values — `1.0`,
/// `1.272`, `1.618`, `2.0`, `2.618`, `3.618`, …) printed WITHOUT trailing
/// zeros — `1` not `1.000`, `1.618` not `1.618000`. Unlike
/// `ratio::draw_ratio_annotations`'s own `{ratio:.2}` (a LIVE division
/// result, arbitrary precision, always worth rounding), these are fixed
/// catalogue constants that already carry their own natural precision —
/// truncating `1.272`/`2.618` to two decimals would silently misreport the
/// exact target the table names.
fn format_projection_ratio(ratio: f64) -> String {
    let s = format!("{ratio:.3}");
    if let Some(trimmed) = s.strip_suffix("000") {
        trimmed.trim_end_matches('.').to_string()
    } else {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Draw every projection tick that is live right now — one call per
/// primitive's own `show_projections` gate (see each Elliott type's own
/// `render`). Read-only guide, never hit-tested (see the module doc) — a
/// projection has no clickable body of its own, unlike a ratio rib.
///
/// `rib_color`/`rib_width`/`rib_style` are the SAME fields every Elliott
/// type's own ratio ribs already read — see the module doc's own "Style"
/// section for why this draws from that exact resolved style rather than a
/// second one.
pub(super) fn draw_projection_annotations(
    ctx: &mut dyn RenderContext,
    points: &[(i64, f64)],
    realized: usize,
    legs: &[ProjectionLeg],
    stroke_color: &str,
    base_width: f64,
    label_style: &LabelStyle,
    rib_color: Option<&str>,
    rib_width: Option<f64>,
    rib_style: LineStyle,
) {
    let dpr = ctx.dpr();
    let ratio_font_size = (label_style.font_size - 1.0).max(9.0);
    let faded = rib_color.map(str::to_string).unwrap_or_else(|| default_rib_color(stroke_color));
    let line_width = rib_width.unwrap_or_else(|| default_rib_width(base_width));

    ctx.set_stroke_color(&faded);
    ctx.set_stroke_width(line_width);
    ctx.set_line_dash(rib_dash_pattern(rib_style));

    ctx.set_font(&format!("{}px sans-serif", ratio_font_size as i32));
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);

    for leg in legs {
        let targets = projection_targets(points, realized, leg);
        if targets.is_empty() {
            continue;
        }
        let Some(&anchor_pt) = points.get(leg.anchor) else { continue };
        let anchor_x = ctx.ts_to_x_ms(anchor_pt.0);
        let tick_end_x = anchor_x + PROJECTION_TICK_WIDTH_PX;

        for (ratio, price) in targets {
            let y = ctx.price_to_y(price);
            ctx.begin_path();
            ctx.move_to(crisp(anchor_x, dpr), crisp(y, dpr));
            ctx.line_to(crisp(tick_end_x, dpr), crisp(y, dpr));
            ctx.stroke();

            let label = format_projection_ratio(ratio);
            ctx.set_fill_color(&faded);
            draw_label_plate(ctx, tick_end_x + PROJECTION_LABEL_NUDGE_X, y, &label, ratio_font_size, label_style, &faded);
        }
    }

    ctx.set_line_dash(&[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 6-point impulse fixture, matching `ElliottImpulse`'s own point
    /// layout: start(0), wave1(1)=110 (up from 100), wave2(2)=105,
    /// wave3(3)=130, wave4(4)=120, wave5(5)=140.
    fn impulse_points() -> [(i64, f64); 6] {
        [(0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)]
    }

    #[test]
    fn three_realized_points_project_wave3_not_wave5() {
        let points = impulse_points();
        // realized = 3: start, wave1, wave2 are real — wave3 (index 3) is
        // the pivot currently under the hand.
        let wave3 = projection_targets(&points, 3, &IMPULSE_PROJECTION_LEGS[0]);
        assert_eq!(wave3.len(), EXTENSION_TARGETS.len(), "wave 3's own reference (wave 1) and anchor (wave 2) are both placed");

        let wave5_from_1 = projection_targets(&points, 3, &IMPULSE_PROJECTION_LEGS[1]);
        assert!(wave5_from_1.is_empty(), "wave 5's own anchor (wave 4, index 4) is not placed yet");

        let wave5_from_1_3 = projection_targets(&points, 3, &IMPULSE_PROJECTION_LEGS[2]);
        assert!(wave5_from_1_3.is_empty(), "wave 5's own anchor (wave 4, index 4) is not placed yet");
    }

    #[test]
    fn fully_realized_impulse_projects_wave5_only() {
        let points = impulse_points();
        // realized = 6: every point is real — wave 3's own leg (end = 3) is
        // long settled; only the two wave-5 legs (end = 5, the structure's
        // own last point) are still within the "just finished" window.
        let wave3 = projection_targets(&points, 6, &IMPULSE_PROJECTION_LEGS[0]);
        assert!(wave3.is_empty(), "wave 3 is settled — its own leg ended three pivots ago");

        let wave5_from_1 = projection_targets(&points, 6, &IMPULSE_PROJECTION_LEGS[1]);
        assert_eq!(wave5_from_1.len(), EXTENSION_TARGETS.len(), "wave 5's own leg ends on the structure's last point");

        let wave5_from_1_3 = projection_targets(&points, 6, &IMPULSE_PROJECTION_LEGS[2]);
        assert_eq!(wave5_from_1_3.len(), EXTENSION_TARGETS.len(), "wave 5's own leg ends on the structure's last point");
    }

    #[test]
    fn zero_travel_reference_leg_yields_nothing() {
        // Wave 1 travels from 100 to 100 — zero price travel, never divide
        // by it.
        let points: [(i64, f64); 6] = [(0, 100.0), (1_000, 100.0), (2_000, 95.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)];
        let wave3 = projection_targets(&points, 3, &IMPULSE_PROJECTION_LEGS[0]);
        assert!(wave3.is_empty(), "a zero-travel reference leg has no fibonacci unit to project with");
    }

    #[test]
    fn down_going_wave1_projects_wave3_below_the_anchor() {
        // Wave 1 travels from 100 DOWN to 90; wave 2 (the anchor) ends at 95.
        let points: [(i64, f64); 6] = [(0, 100.0), (1_000, 90.0), (2_000, 95.0), (3_000, 70.0), (4_000, 80.0), (5_000, 60.0)];
        let wave3 = projection_targets(&points, 3, &IMPULSE_PROJECTION_LEGS[0]);
        assert_eq!(wave3.len(), EXTENSION_TARGETS.len());
        let anchor_price = points[2].1;
        assert!(
            wave3.iter().all(|&(_, price)| price < anchor_price),
            "wave 1 traveled down, so every wave-3 target must sit below wave 2's own end"
        );
    }

    #[test]
    fn correction_projects_c_off_a_at_1_618_with_checked_price() {
        // Wave A travels from 100 down to 80 (travel = 20, direction down);
        // wave B (the anchor) ends at 90.
        let points: [(i64, f64); 4] = [(0, 100.0), (1_000, 80.0), (2_000, 90.0), (3_000, 0.0)];
        let targets = projection_targets(&points, 3, &CORRECTION_PROJECTION_LEGS[0]);
        assert_eq!(targets.len(), EXTENSION_TARGETS.len());

        let ratio_1618 = targets.iter().find(|&&(r, _)| (r - 1.618).abs() < 1e-9).expect("1.618 is in EXTENSION_TARGETS");
        // anchor (90) + dir (-1, A traveled down) * 1.618 * |travel| (20)
        let expected = 90.0 - 1.618 * 20.0;
        assert!((ratio_1618.1 - expected).abs() < 1e-9, "expected {}, got {}", expected, ratio_1618.1);
    }

    #[test]
    fn format_projection_ratio_trims_trailing_zeros() {
        assert_eq!(format_projection_ratio(1.0), "1");
        assert_eq!(format_projection_ratio(2.0), "2");
        assert_eq!(format_projection_ratio(1.272), "1.272");
        assert_eq!(format_projection_ratio(1.618), "1.618");
        assert_eq!(format_projection_ratio(3.618), "3.618");
    }
}
