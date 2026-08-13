//! Wave-ratio annotations — the retracement/extension percentages an
//! Elliott analyst prints between pivots (owner reference chart: wave ii
//! read 0.62 against wave i's own price travel, wave iii read 1.50 against
//! wave i, wave iv read 0.33 against wave iii, wave v read 0.44 against
//! wave iii — each measured against a NAMED prior leg, never a fixed
//! "against the wave right before it" rule).
//!
//! One relation, computed the same way for every Elliott type: given a
//! REFERENCE leg (two pivots) and a MEASURED leg (two pivots), the ratio is
//! `|measured leg price travel| / |reference leg price travel|` — a plain
//! magnitude, no sign. `mylittleindicators` already owns this exact
//! vocabulary, but it is not a dependency of `mlc-core` (see the crate
//! contract in `mlc-core/../CLAUDE.md`), so the ratio is computed locally
//! here instead of pulling that dependency in for one division.
//!
//! Each of the five Elliott primitives owns its own point-layout knowledge
//! (which index is which pivot) and declares its own fixed leg table below
//! — the same centralization `count.rs`'s own `slot_span` already uses for
//! the OTHER point-layout lookup this module (WaveSlot child seeding).
//!
//! Gated per primitive by its own `show_ratios` toggle (default OFF).

use super::super::config::LabelStyle;
use super::super::{crisp, draw_label_plate, RenderContext};

/// Alpha byte appended to the primitive's own stroke colour for a ratio
/// annotation's dotted connector and label — REDUCED EMPHASIS on the SAME
/// colour the primitive already draws its wave line with, never a second
/// palette (owner instruction: reuse the primitive's own colour).
const RATIO_ANNOTATION_ALPHA: u8 = 0x8a; // ~54% opacity

/// One reference→measured pair to annotate, addressed by index into the
/// primitive's own `points()` array. `ref_start`/`ref_end` bound the leg the
/// ratio is taken AGAINST; `measured_start`/`measured_end` bound the leg
/// being measured. The drawn connector spans `ref_start` → `measured_end`
/// only (see [`draw_ratio_annotations`]'s own doc) — all four indices are
/// still needed because the ratio itself is computed from BOTH legs' own
/// price travel, not from the connector's own endpoints.
#[derive(Clone, Copy, Debug)]
pub(super) struct RatioLeg {
    pub(super) ref_start: usize,
    pub(super) ref_end: usize,
    pub(super) measured_start: usize,
    pub(super) measured_end: usize,
}

// ---- Per-type leg tables (owner spec, 2026-08-13) --------------------------

/// `ElliottImpulse` — points: start(0), wave 1..5 (1..=5). Wave 2 vs wave 1
/// and wave 4 vs wave 3 are retracements; wave 3 vs wave 1 and wave 5 vs
/// wave 3 are extensions.
pub(super) const IMPULSE_RATIO_LEGS: [RatioLeg; 4] = [
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 1, measured_end: 2 }, // wave 2 vs wave 1
    RatioLeg { ref_start: 2, ref_end: 3, measured_start: 3, measured_end: 4 }, // wave 4 vs wave 3
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 2, measured_end: 3 }, // wave 3 vs wave 1
    RatioLeg { ref_start: 2, ref_end: 3, measured_start: 4, measured_end: 5 }, // wave 5 vs wave 3
];

/// `ElliottCorrection` — points: start(0), A(1), B(2), C(3). B vs A is the
/// retracement, C vs A is the extension.
pub(super) const CORRECTION_RATIO_LEGS: [RatioLeg; 2] = [
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 1, measured_end: 2 }, // B vs A
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 2, measured_end: 3 }, // C vs A
];

/// `ElliottTriangle` — points: start(0), A..E (1..=5). Each wave measured
/// against the one TWO before it.
pub(super) const TRIANGLE_RATIO_LEGS: [RatioLeg; 3] = [
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 2, measured_end: 3 }, // C vs A
    RatioLeg { ref_start: 1, ref_end: 2, measured_start: 3, measured_end: 4 }, // D vs B
    RatioLeg { ref_start: 2, ref_end: 3, measured_start: 4, measured_end: 5 }, // E vs C
];

/// `ElliottDoubleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3)
/// [connector, ends at 3], Y subwaves(4,5,6) [Y: 3→6].
pub(super) const DOUBLE_COMBO_RATIO_LEGS: [RatioLeg; 1] = [
    RatioLeg { ref_start: 0, ref_end: 2, measured_start: 3, measured_end: 6 }, // Y vs W
];

/// `ElliottTripleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3),
/// Y subwaves(4,5) [Y: 3→5], XX(6), Z subwaves(7,8) [Z: 6→8].
pub(super) const TRIPLE_COMBO_RATIO_LEGS: [RatioLeg; 2] = [
    RatioLeg { ref_start: 0, ref_end: 2, measured_start: 3, measured_end: 5 }, // Y vs W
    RatioLeg { ref_start: 3, ref_end: 5, measured_start: 6, measured_end: 8 }, // Z vs Y
];

/// Draw every ratio annotation in `legs` that is fully within the REALIZED
/// point prefix (`realized` — see each primitive's own `realized_points`
/// field doc; a leg touching a not-yet-real, padded point is skipped rather
/// than reporting a ratio against a phantom pivot).
///
/// Each annotation is a DOTTED connector from its own reference leg's START
/// pivot to its own measured leg's END pivot, with the ratio printed at the
/// connector's own midpoint to two decimals. The ratio is the measured
/// leg's price travel divided by the reference leg's price travel — plain
/// magnitudes (see this module's own doc); a reference leg with zero price
/// travel is skipped rather than dividing by it.
pub(super) fn draw_ratio_annotations(
    ctx: &mut dyn RenderContext,
    points: &[(i64, f64)],
    realized: usize,
    legs: &[RatioLeg],
    stroke_color: &str,
    base_width: f64,
    label_style: &LabelStyle,
) {
    let dpr = ctx.dpr();
    let ratio_font_size = (label_style.font_size - 1.0).max(9.0);
    let faded = format!("{}{:02x}", &stroke_color[..stroke_color.len().min(7)], RATIO_ANNOTATION_ALPHA);
    let line_width = (base_width * 0.6).max(1.0);

    ctx.set_font(&format!("{}px sans-serif", ratio_font_size as i32));
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);

    for leg in legs {
        let max_idx = leg.ref_start.max(leg.ref_end).max(leg.measured_start).max(leg.measured_end);
        if max_idx >= realized {
            continue;
        }
        let (Some(&ref_start_pt), Some(&ref_end_pt), Some(&measured_start_pt), Some(&measured_end_pt)) = (
            points.get(leg.ref_start),
            points.get(leg.ref_end),
            points.get(leg.measured_start),
            points.get(leg.measured_end),
        ) else {
            continue;
        };

        let ref_travel = (ref_end_pt.1 - ref_start_pt.1).abs();
        if ref_travel <= 0.0 {
            continue;
        }
        let measured_travel = (measured_end_pt.1 - measured_start_pt.1).abs();
        let ratio = measured_travel / ref_travel;

        let line_start = (ctx.ts_to_x_ms(ref_start_pt.0), ctx.price_to_y(ref_start_pt.1));
        let line_end = (ctx.ts_to_x_ms(measured_end_pt.0), ctx.price_to_y(measured_end_pt.1));

        ctx.set_stroke_color(&faded);
        ctx.set_stroke_width(line_width);
        ctx.set_line_dash(&[2.0, 2.0]);
        ctx.begin_path();
        ctx.move_to(crisp(line_start.0, dpr), crisp(line_start.1, dpr));
        ctx.line_to(crisp(line_end.0, dpr), crisp(line_end.1, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        let mid_x = (line_start.0 + line_end.0) / 2.0;
        let mid_y = (line_start.1 + line_end.1) / 2.0;
        let label = format!("{ratio:.2}");
        ctx.set_fill_color(&faded);
        draw_label_plate(ctx, mid_x, mid_y, &label, ratio_font_size, label_style, &faded);
    }
}
