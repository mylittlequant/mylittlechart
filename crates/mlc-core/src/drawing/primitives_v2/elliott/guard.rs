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

use mli::ewa::grammar;
use mli::ewa::rules as ewa_rules;
use mli::ewa::types::{EwaPatternKind, EwaSegmentDirection};

use crate::i18n::ConceptKey;

use super::super::Primitive;
use super::count::{self, WaveSlot};

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

    // Declared-subtype-vs-measured-ratio (owner ask 2026-08-13: "есть ли
    // проверка на плоскость? что плоскость >0.9 если меньше это не
    // плоскость") — the subtype `correction_type` CLAIMS must actually match
    // the measured B/A ratio, using the SAME ratio bands
    // `mli::ewa::rules` names for each shape (`FLAT_B_A`/`ZIGZAG_B_A`/
    // `EXPANDED_FLAT_B_A` — never a hardcoded 0.9 or 1.0 in this crate).
    // Independent of the direction check just above: a shape can point the
    // right way (stay within/beyond the origin) while still landing at the
    // wrong MAGNITUDE for the subtype it claims — a B that retraces 0.5 of A
    // never crosses the origin (so the direction check above is silent) but
    // is nowhere near "near the origin" either, so a declared flat is wrong
    // regardless. Skipped (nothing reported) when the ratio itself is
    // `None` (zero-length reference leg) — same "incomplete is not wrong"
    // contract the module doc states; the `realized >= 3` gate above already
    // covers "not yet realized to point 2".
    if realized >= 3 {
        if let Some(b_end) = price(points, 2) {
            if let Some(r) = ewa_rules::ratio(b_end - a_end, a_end - start) {
                if matches!(subtype, "flat" | "expanded" | "running") && !ewa_rules::FLAT_B_A.contains(r) {
                    out.push(Violation {
                        rule_id: "flat_requires_b_near_a_origin",
                        wave: WaveSlot::CorrectionB,
                        message: ConceptKey::RuleFlatRequiresBNearAOrigin,
                    });
                }
                if subtype == "zigzag" && !ewa_rules::ZIGZAG_B_A.contains(r) {
                    out.push(Violation {
                        rule_id: "zigzag_requires_shallow_b",
                        wave: WaveSlot::CorrectionB,
                        message: ConceptKey::RuleZigzagRequiresShallowB,
                    });
                }
                if subtype == "expanded" && !ewa_rules::EXPANDED_FLAT_B_A.contains(r) {
                    out.push(Violation {
                        rule_id: "expanded_flat_requires_b_beyond_a_origin",
                        wave: WaveSlot::CorrectionB,
                        message: ConceptKey::RuleExpandedFlatRequiresBBeyondAOrigin,
                    });
                }
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

// =============================================================================
// Context-aware guard — the checks a primitive cannot compute about ITSELF
// alone, because they depend on its own SUBDIVISIONS or its own PREDECESSOR
// (owner ask 2026-08-13, same report as the module doc's own opening line:
// a lone structure looks fine in isolation while the piece nested inside one
// of its own waves breaks the grammar, or overlaps territory the parent
// itself never touches). [`violations`] cannot see this — a primitive has no
// path to its own siblings (`elliott::count`'s own module doc) — so this is
// a SEPARATE entry point the collection's own owner calls once it has both
// the primitive and its siblings in hand.
// =============================================================================

/// The extra violations [`violations_in`] itself finds by resolving `prim`'s
/// own subdivisions (primitives in `others` whose [`Primitive::wave_link`]
/// names `prim` as parent) and predecessor (`others` primitive whose id
/// matches `prim`'s own [`Primitive::wave_follows`]). ONLY the extra
/// context-dependent violations — the caller merges this with
/// [`violations(prim)`] itself; nothing here duplicates a self-only check.
///
/// `others` need not be the WHOLE drawing collection — every rule below
/// only ever looks for `prim`'s own children or its own named predecessor,
/// so a caller that pre-filters `others` down to just those loses nothing.
/// Reads every primitive (including `prim` itself) only through the
/// [`Primitive`] trait — never downcasts, matching every other function in
/// this module and in `elliott::count`.
pub fn violations_in(prim: &dyn Primitive, others: &[&dyn Primitive]) -> Vec<Violation> {
    let mut out = Vec::new();
    let prim_id = prim.data().id;
    let children: Vec<(&dyn Primitive, WaveSlot)> = others
        .iter()
        .filter_map(|&p| p.wave_link().filter(|l| l.parent_id == prim_id).map(|l| (p, l.slot)))
        .collect();

    // a) subdivision_class — a child occupying a slot whose grammar its own
    // kind does not satisfy. Catches a stale file, a hand-edited preset, or
    // a promoted chain that ended up with the wrong shape in a slot.
    for &(child, slot) in &children {
        let child_kind = child_kind_id(child);
        if !count::child_kinds(prim, slot).iter().any(|&k| k == child_kind) {
            out.push(Violation { rule_id: "subdivision_class", wave: slot, message: ConceptKey::RuleSubdivisionClass });
        }
    }

    if prim.type_id() == "elliott_impulse" {
        let points = prim.points();
        let realized = prim.realized_point_count();

        // c) wave2_not_beyond_wave1_origin — DEEP, no exception: every
        // realized pivot of wave 2's own subdivision must stay on the
        // impulse's own side of point 0.
        if realized >= 2 {
            if let (Some(origin), Some(reference)) = (price(&points, 0), price(&points, 1)) {
                if let Some(child) = find_child(&children, WaveSlot::Impulse2) {
                    if any_child_pivot_beyond_origin(origin, reference, child) {
                        out.push(Violation {
                            rule_id: "wave2_not_beyond_wave1_origin",
                            wave: WaveSlot::Impulse2,
                            message: ConceptKey::RuleWave2BeyondOrigin,
                        });
                    }
                }
            }
        }

        // b) wave4_no_wave1_overlap — DEEP, WITH the triangle exception the
        // owner spelled out: wave 4 must not enter wave 1's own territory,
        // but a triangle in wave 4 may let intermediate vertices cross —
        // only its FINAL point (already the parent's own point 4, already
        // covered by the shallow self-only check in `impulse_violations`)
        // must not. Without a subdivision, the shallow end-point check is
        // the whole rule and is already correct for both cases; this deep
        // check only ADDS strictness for a non-triangle subdivision — it
        // never relaxes what the shallow one reports, and a triangle child
        // adds no violation of its own here.
        if realized >= 4 {
            if let (Some(origin), Some(reference)) = (price(&points, 1), price(&points, 3)) {
                if let Some(child) = find_child(&children, WaveSlot::Impulse4) {
                    if child.type_id() != "elliott_triangle"
                        && any_child_pivot_beyond_origin(origin, reference, child)
                    {
                        out.push(Violation {
                            rule_id: "wave4_no_wave1_overlap",
                            wave: WaveSlot::Impulse4,
                            message: ConceptKey::RuleWave4OverlapsWave1,
                        });
                    }
                }
            }
        }
    }

    // d) The combination grammar the catalogue names for `DoubleThree`/
    // `TripleThree` (`mli::ewa::primitives::EWA_PRIMITIVES`) but nothing
    // checked until now. Only the components that actually have a
    // subdivision are checked — a combo with no children reports nothing.
    if matches!(prim.type_id(), "elliott_double_combo" | "elliott_triple_combo") {
        let is_triple = prim.type_id() == "elliott_triple_combo";
        let component_slots: &[WaveSlot] = if is_triple {
            &[WaveSlot::ComboW, WaveSlot::ComboY, WaveSlot::ComboZ]
        } else {
            &[WaveSlot::ComboW, WaveSlot::ComboY]
        };
        let connector_slots: &[WaveSlot] =
            if is_triple { &[WaveSlot::ComboX, WaveSlot::ComboXX] } else { &[WaveSlot::ComboX] };
        let wyz_rule_id = if is_triple { "w_y_z_are_corrective" } else { "w_and_y_are_corrective" };

        // x_is_corrective — every realized connector's own subdivision must
        // be a corrective structure.
        for &slot in connector_slots {
            if let Some(child) = find_child(&children, slot) {
                if !child_pattern(child).map(grammar::is_corrective).unwrap_or(true) {
                    out.push(Violation { rule_id: "x_is_corrective", wave: slot, message: ConceptKey::RuleComboXNotCorrective });
                }
            }
        }

        // w_and_y_are_corrective / w_y_z_are_corrective — every realized
        // component's own subdivision must be a corrective structure.
        for &slot in component_slots {
            if let Some(child) = find_child(&children, slot) {
                if !child_pattern(child).map(grammar::is_corrective).unwrap_or(true) {
                    out.push(Violation { rule_id: wyz_rule_id, wave: slot, message: ConceptKey::RuleComboWyzNotCorrective });
                }
            }
        }

        // at_most_one_zigzag — across the W/Y/Z components: flag every
        // zigzag component once a second one appears.
        let zigzag_slots: Vec<WaveSlot> = component_slots
            .iter()
            .copied()
            .filter(|&slot| {
                find_child(&children, slot)
                    .and_then(child_pattern)
                    .map(grammar::is_zigzag)
                    .unwrap_or(false)
            })
            .collect();
        if zigzag_slots.len() > 1 {
            for slot in zigzag_slots {
                out.push(Violation { rule_id: "at_most_one_zigzag", wave: slot, message: ConceptKey::RuleComboAtMostOneZigzag });
            }
        }

        // triangle_only_final_component — a triangle may occupy only the
        // LAST component slot (Y in a double, Z in a triple).
        if let Some(&final_slot) = component_slots.last() {
            for &slot in component_slots {
                if slot == final_slot {
                    continue;
                }
                if let Some(child) = find_child(&children, slot) {
                    if child.type_id() == "elliott_triangle" {
                        out.push(Violation {
                            rule_id: "triangle_only_final_component",
                            wave: slot,
                            message: ConceptKey::RuleComboTriangleOnlyFinal,
                        });
                    }
                }
            }
        }
    }

    // e) chain_continuity — for a primitive with `wave_follows() ==
    // Some(prev_id)`, its own first point must coincide with the
    // predecessor's own last point. Tolerance is derived from the
    // predecessor's own price travel (0.1% of it) rather than an absolute
    // epsilon — two structures at wildly different price scales need
    // different absolute slack. Skipped when the predecessor is not present
    // in `others`.
    if let Some(prev_id) = prim.wave_follows() {
        if let Some(prev) = others.iter().find(|p| p.data().id == prev_id) {
            let prev_points = prev.points();
            if let (Some(&prev_first), Some(&prev_last)) = (prev_points.first(), prev_points.last()) {
                let own_points = prim.points();
                if let Some(&own_first) = own_points.first() {
                    let tolerance = (prev_last.1 - prev_first.1).abs() * 0.001;
                    if (own_first.1 - prev_last.1).abs() > tolerance {
                        if let Some(&first_slot) = prim.wave_slots().first() {
                            out.push(Violation {
                                rule_id: "chain_continuity",
                                wave: first_slot,
                                message: ConceptKey::RuleChainContinuityGap,
                            });
                        }
                    }
                }
            }
        }
    }

    out
}

/// This primitive's own "kind id" — see `elliott::count`'s own doc for the
/// `type_id[:subtype]` convention — computed from [`Primitive::type_id`]
/// and [`Primitive::correction_subtype`], never a second copy of either.
fn child_kind_id(prim: &dyn Primitive) -> String {
    match prim.correction_subtype() {
        Some(subtype) => format!("{}:{}", prim.type_id(), subtype),
        None => prim.type_id().to_string(),
    }
}

/// This primitive's own catalogue pattern kind — [`count::kind_pattern`]
/// applied to [`child_kind_id`], so `d)`'s combination-grammar checks read
/// the SAME mapping [`count::child_kinds`] itself is built from.
fn child_pattern(prim: &dyn Primitive) -> Option<EwaPatternKind> {
    count::kind_pattern(&child_kind_id(prim))
}

/// The child occupying `slot` among `children` (see [`violations_in`]'s own
/// `children` resolution), if any.
fn find_child<'a>(children: &[(&'a dyn Primitive, WaveSlot)], slot: WaveSlot) -> Option<&'a dyn Primitive> {
    for &(child, s) in children {
        if s == slot {
            return Some(child);
        }
    }
    None
}

/// Whether ANY of `child`'s own realized pivots crosses `origin` relative to
/// `reference` (`mli::ewa::rules::beyond_origin`) — the DEEP form of the
/// same predicate `impulse_violations`'s own shallow wave-2/wave-4 checks
/// already apply to a single end point. Shared by both deep checks in
/// [`violations_in`] (`b` and `c`) so the anchor logic is written once.
fn any_child_pivot_beyond_origin(origin: f64, reference: f64, child: &dyn Primitive) -> bool {
    let points = child.points();
    let realized = child.realized_point_count();
    points.iter().take(realized).any(|&(_, p)| ewa_rules::beyond_origin(origin, reference, p))
}
