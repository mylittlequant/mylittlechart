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
//!
//! # Geometry (owner correction 2026-08-13)
//!
//! The ratio annotation is NOT a set of short connectors, one per leg —
//! that draws a dotted copy of the zigzag itself, which is not how the
//! owner's reference chart reads it. Instead every Elliott type threads its
//! SAME-PARITY wave-ends onto shared dotted "ribs" — e.g. an impulse's odd
//! waves (1, 3, 5) end-to-end on one rib, its even waves (2, 4) end-to-end
//! on the other, running down opposite sides of the structure — and each
//! wave's own ratio prints near the vertex where THAT wave ends (nudged off
//! it so it never lands on top of the wave label already drawn there). See
//! [`Rib`] for the per-type tables and [`draw_ratio_annotations`] for how a
//! rib and a leg's label are drawn from the same point-index vocabulary.

use super::super::config::LabelStyle;
use super::super::{crisp, draw_label_plate, RenderContext, HIT_TOLERANCE};

/// Alpha byte appended to the primitive's own stroke colour for a ratio
/// annotation's dotted connector and label — REDUCED EMPHASIS on the SAME
/// colour the primitive already draws its wave line with, never a second
/// palette (owner instruction: reuse the primitive's own colour).
const RATIO_ANNOTATION_ALPHA: u8 = 0x8a; // ~54% opacity

/// Horizontal push applied to a ratio label off its own anchor vertex. The
/// wave label already sits AT that same vertex via a VERTICAL `offset_y`
/// push (see each primitive's own `render`, above/below depending on
/// direction) — pushing the ratio label horizontally instead keeps the two
/// labels apart regardless of which side the wave label's own offset picked,
/// without needing to know that direction here.
const RATIO_LABEL_NUDGE_X: f64 = 14.0;

/// One reference→measured pair to annotate, addressed by index into the
/// primitive's own `points()` array. `ref_start`/`ref_end` bound the leg the
/// ratio is taken AGAINST; `measured_start`/`measured_end` bound the leg
/// being measured. The ratio label anchors at `measured_end` — the vertex
/// where the MEASURED wave itself ends (see [`draw_ratio_annotations`]) —
/// owner correction 2026-08-13: an extension reads from the reference
/// wave's OWN peak, not from the structure's start (`ref_start`), so a
/// wave-3-vs-wave-1 extension's ratio is still computed from wave 1's END
/// to wave 3's END, never from point 0. All four indices are still needed
/// because the ratio itself is computed from BOTH legs' own price travel,
/// not from a single connector's endpoints (the connector geometry itself
/// is now a shared [`Rib`], not per-leg — see the module doc).
#[derive(Clone, Copy, Debug)]
pub(super) struct RatioLeg {
    pub(super) ref_start: usize,
    pub(super) ref_end: usize,
    pub(super) measured_start: usize,
    pub(super) measured_end: usize,
}

/// A same-parity dotted rib: a fixed sequence of point indices, in draw
/// order, threading through one side of the structure (e.g. an impulse's
/// odd wave-ends `1, 3, 5`). Two consecutive indices are one drawn/hit-test
/// segment, exactly like the primitive's own wave line — see the module
/// doc for the parity rule and [`draw_ratio_annotations`] /
/// [`hit_test_ratio_ribs`] for how it is drawn and clicked.
pub(super) type Rib = &'static [usize];

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

/// `ElliottImpulse` ribs — odd wave-ends (1, 3, 5) on one rib, even
/// wave-ends (2, 4) on the other, opposite sides of the zigzag.
pub(super) const IMPULSE_RATIO_RIBS: [Rib; 2] = [&[1, 3, 5], &[2, 4]];

/// `ElliottCorrection` — points: start(0), A(1), B(2), C(3). B vs A is the
/// retracement, C vs A is the extension.
pub(super) const CORRECTION_RATIO_LEGS: [RatioLeg; 2] = [
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 1, measured_end: 2 }, // B vs A
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 2, measured_end: 3 }, // C vs A
];

/// `ElliottCorrection` ribs — A and C are the two same-direction waves, so
/// one rib threads `A → C`; B has no same-parity partner in a 3-wave
/// correction, so it carries no rib of its own (see the module doc: "where
/// a family genuinely has one comparison, one rib is correct").
pub(super) const CORRECTION_RATIO_RIBS: [Rib; 1] = [&[1, 3]];

/// `ElliottTriangle` — points: start(0), A..E (1..=5). Each wave measured
/// against the one TWO before it.
pub(super) const TRIANGLE_RATIO_LEGS: [RatioLeg; 3] = [
    RatioLeg { ref_start: 0, ref_end: 1, measured_start: 2, measured_end: 3 }, // C vs A
    RatioLeg { ref_start: 1, ref_end: 2, measured_start: 3, measured_end: 4 }, // D vs B
    RatioLeg { ref_start: 2, ref_end: 3, measured_start: 4, measured_end: 5 }, // E vs C
];

/// `ElliottTriangle` ribs — A-C-E on one rib, B-D on the other.
pub(super) const TRIANGLE_RATIO_RIBS: [Rib; 2] = [&[1, 3, 5], &[2, 4]];

/// `ElliottDoubleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3)
/// [connector, ends at 3], Y subwaves(4,5,6) [Y: 3→6].
pub(super) const DOUBLE_COMBO_RATIO_LEGS: [RatioLeg; 1] = [
    RatioLeg { ref_start: 0, ref_end: 2, measured_start: 3, measured_end: 6 }, // Y vs W
];

/// `ElliottDoubleCombo` ribs — a single rib, W's own end to Y's own end.
pub(super) const DOUBLE_COMBO_RATIO_RIBS: [Rib; 1] = [&[2, 6]];

/// `ElliottTripleCombo` — points: start(0), W subwaves(1,2) [W: 0→2], X(3),
/// Y subwaves(4,5) [Y: 3→5], XX(6), Z subwaves(7,8) [Z: 6→8].
pub(super) const TRIPLE_COMBO_RATIO_LEGS: [RatioLeg; 2] = [
    RatioLeg { ref_start: 0, ref_end: 2, measured_start: 3, measured_end: 5 }, // Y vs W
    RatioLeg { ref_start: 3, ref_end: 5, measured_start: 6, measured_end: 8 }, // Z vs Y
];

/// `ElliottTripleCombo` ribs — W's end to Y's end, then Y's end to Z's end.
pub(super) const TRIPLE_COMBO_RATIO_RIBS: [Rib; 2] = [&[2, 5], &[5, 8]];

/// Draw every ratio rib and label that is fully within the REALIZED point
/// prefix (`realized` — see each primitive's own `realized_points` field
/// doc; an index touching a not-yet-real, padded point is dropped rather
/// than drawing a rib or label against a phantom pivot).
///
/// Two passes, sharing the same `points`/`realized` vocabulary:
/// - **Ribs** (`ribs`): each is one dotted polyline through its own fixed
///   point-index sequence — see [`Rib`] and the module doc's parity rule.
///   Realized-gated per index, so a rib partially drawn while a chain is
///   still being placed only runs to the last realized index, same as the
///   primitive's own wave line and (for the triangle) its trendlines.
/// - **Labels** (`legs`): one per [`RatioLeg`], anchored at
///   `points[leg.measured_end]` — the vertex where the MEASURED wave itself
///   ends — nudged off it via [`RATIO_LABEL_NUDGE_X`] so it does not
///   collide with the wave label already drawn there. The ratio value
///   itself is unchanged: the measured leg's price travel divided by the
///   reference leg's price travel — plain magnitudes (see this module's
///   own doc); a reference leg with zero price travel is skipped rather
///   than dividing by it.
pub(super) fn draw_ratio_annotations(
    ctx: &mut dyn RenderContext,
    points: &[(i64, f64)],
    realized: usize,
    legs: &[RatioLeg],
    ribs: &[Rib],
    stroke_color: &str,
    base_width: f64,
    label_style: &LabelStyle,
) {
    let dpr = ctx.dpr();
    let ratio_font_size = (label_style.font_size - 1.0).max(9.0);
    let faded = format!("{}{:02x}", &stroke_color[..stroke_color.len().min(7)], RATIO_ANNOTATION_ALPHA);
    let line_width = (base_width * 0.6).max(1.0);

    // ---- Ribs: one dotted polyline per same-parity thread ----
    ctx.set_stroke_color(&faded);
    ctx.set_stroke_width(line_width);
    ctx.set_line_dash(&[2.0, 2.0]);
    for rib in ribs {
        let screen_pts: Vec<(f64, f64)> = rib
            .iter()
            .filter(|&&i| i < realized)
            .filter_map(|&i| points.get(i))
            .map(|&(ts, price)| (ctx.ts_to_x_ms(ts), ctx.price_to_y(price)))
            .collect();
        if screen_pts.len() < 2 {
            continue;
        }
        ctx.begin_path();
        ctx.move_to(crisp(screen_pts[0].0, dpr), crisp(screen_pts[0].1, dpr));
        for &(x, y) in &screen_pts[1..] {
            ctx.line_to(crisp(x, dpr), crisp(y, dpr));
        }
        ctx.stroke();
    }
    ctx.set_line_dash(&[]);

    // ---- Labels: one per leg, anchored at the vertex the measured wave ends on ----
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

        let anchor_x = ctx.ts_to_x_ms(measured_end_pt.0);
        let anchor_y = ctx.price_to_y(measured_end_pt.1);
        let label_x = anchor_x + RATIO_LABEL_NUDGE_X;
        let label_y = anchor_y;

        let label = format!("{ratio:.2}");
        ctx.set_fill_color(&faded);
        draw_label_plate(ctx, label_x, label_y, &label, ratio_font_size, label_style, &faded);
    }
}

/// Hit-test the ratio ribs' own dotted segments in screen space, using the
/// SAME per-segment distance test and tolerance every primitive's own wave
/// line already uses (`point_to_line_dist` + [`HIT_TOLERANCE`] — reused
/// here, not duplicated with a looser tolerance of its own). `screen` is
/// the primitive's own points already projected to screen space, indexed
/// exactly like `points` in [`draw_ratio_annotations`]; a rib index past
/// `screen`'s own length is skipped rather than panicking (the same
/// bounds-checked pattern the draw side uses via `points.get`).
///
/// Callers gate this on their own `show_ratios` flag before invoking it —
/// this function does not re-check it, matching how `draw_ratio_annotations`
/// is itself only ever called from behind that same gate.
pub(super) fn hit_test_ratio_ribs(sx: f64, sy: f64, screen: &[(f64, f64)], ribs: &[Rib]) -> bool {
    for rib in ribs {
        for pair in rib.windows(2) {
            let (Some(&(x1, y1)), Some(&(x2, y2))) = (screen.get(pair[0]), screen.get(pair[1])) else {
                continue;
            };
            if point_to_line_dist(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE {
                return true;
            }
        }
    }
    false
}

/// Point-to-segment distance — the same test every Elliott primitive's own
/// `hit_test` already runs for its wave line; centralized here (rather than
/// the five identical private copies that used to live one per
/// `elliott_*.rs` file) so the rib hit test in [`hit_test_ratio_ribs`]
/// shares the exact same test, not a second one.
pub(super) fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}
