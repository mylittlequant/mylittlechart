//! Elliott hard-rule guard — flags a drawn structure that breaks its own
//! family's geometric rules (owner report 2026-08-13: wave 4 drawn back
//! beyond wave 1's start, wave 2 drawn back beyond the origin, and nothing
//! flagged it — Elliott's hard rules are not advice).
//!
//! The rule VOCABULARY is not re-invented here: every boolean check below
//! calls straight into `mylittleindicators::ewa::rules` (`beyond`/
//! `beyond_origin`/`retracement_stays_above_origin`/`RatioBand`/`ratio`) —
//! the same OSS crate whose `primitives` catalogue (`EWA_PRIMITIVES`,
//! `mylittleindicators/src/ewa/primitives.rs`) names which rule ids apply to
//! which pattern shape (the impulse's own `rules` list is exactly
//! `["wave3_not_shortest", "wave2_not_beyond_wave1_origin",
//! "wave4_no_wave1_overlap"]`). This module only wires those catalogue rule
//! ids to THIS crate's own point layout and reports which [`WaveSlot`] broke
//! them — it adds no second copy of the predicates.
//!
//! Reads every primitive only through the [`Primitive`] trait (`points()`,
//! `type_id()`, `realized_point_count()`, `correction_subtype()`) — never
//! downcasts, matching `elliott::count`'s own contract for `child_seed`/
//! `successor_seed`/`promote_chain_shape`.
//!
//! A rule needing a point past the primitive's own REALIZED prefix (see
//! [`Primitive::realized_point_count`]'s own doc) is skipped, not reported —
//! a structure still being drawn is incomplete, not wrong. [`violations`]
//! never blocks a drawing and never moves a point; each Elliott type's own
//! `render` only uses the result to recolor the offending wave's segment and
//! mark its label plate (see [`VIOLATION_COLOR`]) — the analyst is allowed
//! to be wrong on purpose while working. Independent of `show_ratios` — a
//! broken count is broken whether or not the ratio ribs are showing.

use mli::ewa::rules as ewa_rules;
use mli::ewa::types::EwaSegmentDirection;

use crate::i18n::ConceptKey;

use super::super::Primitive;
use super::count::WaveSlot;

/// The theme's own warning colour (`UITheme::dark().colors.warning`,
/// `mlc-core/src/theme/preset.rs`) — reused literal, not invented. A
/// primitive's own `render` has no path to the live `ThemeManager` (see
/// `projection::bars_pattern`'s own doc for the same constraint on its
/// projected candle colours), so this mirrors that established pattern
/// rather than threading theme state through.
pub const VIOLATION_COLOR: &str = "#ff9800";

/// One broken hard rule on one wave of a drawn Elliott structure.
#[derive(Clone, Copy, Debug)]
pub struct Violation {
    /// The rule id exactly as `mylittleindicators::ewa::primitives`'s own
    /// catalogue names it (e.g. `"wave2_not_beyond_wave1_origin"`).
    pub rule_id: &'static str,
    /// Which wave broke the rule — the segment/label a caller marks.
    pub wave: WaveSlot,
    /// The user-facing explanation, via `mlc_core::i18n`.
    pub message: ConceptKey,
}

/// Every hard-rule violation `prim` currently exhibits, checked against its
/// own REALIZED points only (see the module doc). Empty for a compliant
/// structure, for a primitive still being drawn, and for every primitive
/// outside the three families this guard covers today (impulse, correction,
/// triangle — the combos have no hard-rule set of their own in the
/// catalogue yet).
pub fn violations(prim: &dyn Primitive) -> Vec<Violation> {
    let realized = prim.realized_point_count();
    let points = prim.points();
    match prim.type_id() {
        "elliott_impulse" => impulse_violations(&points, realized),
        "elliott_correction" => {
            correction_violations(&points, realized, prim.correction_subtype().unwrap_or("zigzag"))
        }
        "elliott_triangle" => triangle_violations(&points, realized),
        _ => Vec::new(),
    }
}

fn price(points: &[(i64, f64)], idx: usize) -> Option<f64> {
    points.get(idx).map(|p| p.1)
}

// =============================================================================
// Impulse — the three hard rules (owner spec, 2026-08-13)
// =============================================================================

fn impulse_violations(points: &[(i64, f64)], realized: usize) -> Vec<Violation> {
    let mut out = Vec::new();

    // wave2_not_beyond_wave1_origin — wave 2 must not retrace past wave 1's
    // own origin (point 0).
    if realized >= 3 {
        if let (Some(origin), Some(reference), Some(test)) =
            (price(points, 0), price(points, 1), price(points, 2))
        {
            if ewa_rules::beyond_origin(origin, reference, test) {
                out.push(Violation {
                    rule_id: "wave2_not_beyond_wave1_origin",
                    wave: WaveSlot::Impulse2,
                    message: ConceptKey::RuleWave2BeyondOrigin,
                });
            }
        }
    }

    // wave4_no_wave1_overlap — wave 4 must not enter wave 1's own price
    // territory, measured from wave 1's END (not its origin — a later wave's
    // overlap rule anchors on the leg it must not re-enter, not on the
    // structure's own start).
    if realized >= 5 {
        if let (Some(origin), Some(reference), Some(test)) =
            (price(points, 1), price(points, 3), price(points, 4))
        {
            if ewa_rules::beyond_origin(origin, reference, test) {
                out.push(Violation {
                    rule_id: "wave4_no_wave1_overlap",
                    wave: WaveSlot::Impulse4,
                    message: ConceptKey::RuleWave4OverlapsWave1,
                });
            }
        }
    }

    // wave3_not_shortest — wave 3 must never be the shortest of waves 1, 3,
    // 5. No predicate in `ewa::rules` covers a three-way magnitude
    // comparison (every exported predicate there is a two-point-vs-origin
    // geometry check) — this is real new logic, not a second copy of one of
    // them.
    if realized >= 6 {
        if let (Some(p0), Some(p1), Some(p2), Some(p3), Some(p4), Some(p5)) = (
            price(points, 0),
            price(points, 1),
            price(points, 2),
            price(points, 3),
            price(points, 4),
            price(points, 5),
        ) {
            let len1 = (p1 - p0).abs();
            let len3 = (p3 - p2).abs();
            let len5 = (p5 - p4).abs();
            if len3 <= len1 && len3 <= len5 {
                out.push(Violation {
                    rule_id: "wave3_not_shortest",
                    wave: WaveSlot::Impulse3,
                    message: ConceptKey::RuleWave3Shortest,
                });
            }
        }
    }

    out
}

// =============================================================================
// Correction — the catalogue's own per-subtype rule set (owner spec,
// 2026-08-13: "then the corrective ones the primitives catalogue lists")
// =============================================================================

/// `subtype` is `ElliottCorrection::correction_subtype()`'s own token —
/// `"zigzag"` (`EwaPatternKind::Zigzag` rules: `b_does_not_retrace_a_origin`,
/// `c_exceeds_a_end`), `"flat"` (`RegularFlat`: `b_terminates_near_a_origin`,
/// `c_slightly_exceeds_a_end`), `"expanded"` (`ExpandedFlat`:
/// `b_exceeds_a_origin`, `c_exceeds_a_end`), or `"running"` (`RunningFlat`:
/// `b_exceeds_a_origin`, `c_fails_to_exceed_a_end`) — see
/// `mylittleindicators/src/ewa/primitives.rs` for the catalogue entries this
/// mirrors.
fn correction_violations(points: &[(i64, f64)], realized: usize, subtype: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let (Some(start), Some(a_end)) = (price(points, 0), price(points, 1)) else {
        return out;
    };
    let direction = EwaSegmentDirection::from_delta(a_end - start);

    // B vs the origin — every subtype names a B-rule; which direction is
    // compliant flips by subtype (see each arm's own comment).
    if realized >= 3 {
        if let Some(b_end) = price(points, 2) {
            let stayed_within_origin = ewa_rules::retracement_stays_above_origin(direction, start, b_end);
            let (violated, rule_id) = match subtype {
                // Regular flat: B must terminate NEAR the origin — a ratio
                // band, not a hard beyond/within split (the SAME band the
                // catalogue's own `RegularFlat` entry is shaped by).
                "flat" => {
                    let violated = match ewa_rules::ratio(b_end - a_end, a_end - start) {
                        Some(r) => !ewa_rules::REGULAR_FLAT_B_A.contains(r),
                        None => false,
                    };
                    (violated, "b_terminates_near_a_origin")
                }
                // Expanded/running flat: B is REQUIRED to exceed the origin
                // — a compliant shape FAILS `retracement_stays_above_origin`,
                // so staying within bounds IS the violation.
                "expanded" | "running" => (stayed_within_origin, "b_exceeds_a_origin"),
                // Zigzag (default): B must NOT retrace past the origin.
                _ => (!stayed_within_origin, "b_does_not_retrace_a_origin"),
            };
            if violated {
                out.push(Violation { rule_id, wave: WaveSlot::CorrectionB, message: ConceptKey::RuleCorrectionBRule });
            }
        }
    }

    // C vs wave A's own end.
    if realized >= 4 {
        if let Some(c_end) = price(points, 3) {
            let progressed_past_a = ewa_rules::beyond(start, a_end, c_end);
            let (violated, rule_id) = match subtype {
                // Running flat: C is REQUIRED to fail to exceed A's end —
                // progressing past it breaks the "running" shape.
                "running" => (progressed_past_a, "c_fails_to_exceed_a_end"),
                "flat" => (!progressed_past_a, "c_slightly_exceeds_a_end"),
                _ => (!progressed_past_a, "c_exceeds_a_end"),
            };
            if violated {
                out.push(Violation { rule_id, wave: WaveSlot::CorrectionC, message: ConceptKey::RuleCorrectionCRule });
            }
        }
    }

    out
}

// =============================================================================
// Triangle — contracting_boundaries (ContractingTriangle, the classic shape
// `ElliottTriangle` draws — it carries no subtype selector of its own the
// way `ElliottCorrection` does)
// =============================================================================

/// `(ref_start, ref_end, measured_start, measured_end, wave)` — the same
/// "two waves back" comparison `ratio::TRIANGLE_RATIO_LEGS` already draws a
/// rib/label for; a contracting triangle requires each leg to be SHORTER
/// than the leg two waves before it.
const TRIANGLE_LEGS: [(usize, usize, usize, usize, WaveSlot); 3] = [
    (0, 1, 2, 3, WaveSlot::TriangleC),
    (1, 2, 3, 4, WaveSlot::TriangleD),
    (2, 3, 4, 5, WaveSlot::TriangleE),
];

fn triangle_violations(points: &[(i64, f64)], realized: usize) -> Vec<Violation> {
    let mut out = Vec::new();
    for (ref_start, ref_end, measured_start, measured_end, wave) in TRIANGLE_LEGS {
        if realized <= measured_end {
            continue;
        }
        let (Some(ref_start_p), Some(ref_end_p), Some(measured_start_p), Some(measured_end_p)) = (
            price(points, ref_start),
            price(points, ref_end),
            price(points, measured_start),
            price(points, measured_end),
        ) else {
            continue;
        };
        if let Some(ratio) = ewa_rules::ratio(measured_end_p - measured_start_p, ref_end_p - ref_start_p) {
            if ratio >= 1.0 {
                out.push(Violation {
                    rule_id: "contracting_boundaries",
                    wave,
                    message: ConceptKey::RuleTriangleNotContracting,
                });
            }
        }
    }
    out
}
