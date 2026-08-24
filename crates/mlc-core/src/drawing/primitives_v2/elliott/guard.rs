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
//!
//! [`chain_violations`] (owner ask 2026-08-13: "ну и правила внутри
//! цепочек") extends this same guard to a `follows` chain's own joints — see
//! that function's own doc, and the "Chain guard" section below, for why a
//! chain's hard rules are the SAME family rules run against the chain's own
//! promoted pivot sequence rather than a third rule set.
//!
//! Owner report 2026-08-14: a plain downward impulse with wave 4 drawn to
//! END BELOW wave 3 (continuing in the trend direction instead of retracing
//! against it) rendered with nothing flagged — none of the three rules above
//! cover DIRECTION at all. Two more checks close that gap, plus one guideline
//! deliberately left unimplemented:
//! - [`impulse_violations`] gained `"wave3_exceeds_wave1"` — a motive wave
//!   must progress (`mli::ewa::rules::beyond`), unconditionally, so it lives
//!   with the other self-only impulse rules.
//! - [`violations_in`] gained `"wave2_retraces_wave1"`/
//!   `"wave4_retraces_wave3"` — a corrective wave must retrace AGAINST the
//!   direction its own preceding motive wave established
//!   (`mli::ewa::rules::progresses`), the exact rule the owner's report
//!   names. This one is deliberately NOT in [`impulse_violations`]: a wave 2
//!   or wave 4 whose own subdivision is a RUNNING variant (running flat/
//!   triangle/zigzag) legitimately fails to retrace, and only the
//!   context-aware pass can see that subdivision at all — see
//!   [`violations_in`]'s own doc, further down, for why the rule lives ONLY
//!   there rather than being reported self-only and retracted by context.
//! - Wave 5 exceeding wave 3 (the OTHER motive-progression rule the owner's
//!   report describes) gained its own check on 2026-08-15:
//!   `"wave5_beyond_wave3"` in [`impulse_violations`], mirroring
//!   `"wave3_exceeds_wave1"` exactly, one wave later
//!   (`ewa_rules::beyond(wave1_origin, wave3_end, wave5_end)` — the same
//!   "does test progress past reference, relative to origin" predicate, no
//!   new comparison). A failing wave 5 is a real Elliott shape
//!   (`EwaPatternKind::TruncatedImpulse`, whose own catalogue rule is
//!   literally `"wave5_fails_to_exceed_wave3"`), so `ElliottImpulse` carries
//!   `truncated_fifth: bool` (`#[serde(default)]`, `false`) — the analyst's
//!   own PER-STRUCTURE claim that THIS fifth is deliberately short (see
//!   that field's own doc for the full rationale).
//!
//!   Owner correction, same day: the flag is a CLAIM about the shape, not
//!   permission to stop checking it — "если поставил укороченную, то она
//!   уже должна давать ошибку если она выходит за пределы 3, раз уж её
//!   выбрал как укороченную". So `truncated_fifth` does not silence a rule,
//!   it SWITCHES which of two opposite rules applies to the same wave-5
//!   end point, both reusing the identical `beyond(wave1_origin, wave3_end,
//!   wave5_end)` call, one negated:
//!   - `false` (default): unchanged — `!beyond(...)` reports
//!     `"wave5_beyond_wave3"`, GEOMETRY family, exactly as described above.
//!   - `true`: `beyond(...)` reports `"wave5_not_truncated"` instead — the
//!     analyst declared this fifth truncated and the measured shape
//!     contradicts him. This half lives in the GRAMMAR family (see
//!     [`rule_family`]), not geometry, even though it is computed right
//!     next to its sibling in [`impulse_violations`]: it is not a raw
//!     point-position check like every other rule in that function, it is a
//!     DECLARED property (`truncated_fifth`) checked against the measured
//!     shape — the same kind of check `correction_violations`'s own
//!     declared-subtype-vs-measured-ratio block already runs (that block's
//!     rule ids — `"flat_requires_b_near_a_origin"` and siblings — are
//!     GRAMMAR for exactly this reason, see [`rule_family`]'s own doc), so
//!     this rule follows that precedent rather than the family of the
//!     function it happens to sit in.
//!
//!   Neither half adds a tolerance or a third "exactly equal" state:
//!   `beyond`'s own boundary-case decision (`test == reference` reads as
//!   NOT beyond, since its branches are strict `>`/`<`) is inherited
//!   unchanged by both rules — there is exactly one predicate, negated.
//!
//! Every new predicate call here reads `mli::ewa::rules::{beyond,
//! progresses}` exactly as this module's own opening paragraph promises —
//! no second copy of either comparison.
//!
//! Diagonals (`ElliottImpulse::impulse_kind`, leading/ending): the ONE rule
//! that changes is `"wave4_no_wave1_overlap"` — `mylittleindicators::ewa::
//! primitives`'s own catalogue entry for `LeadingDiagonalContracting`/
//! `EndingDiagonalContracting` names the rule id `"wave4_overlaps_wave1"`
//! instead, i.e. the overlap this module otherwise flags is part of the
//! shape, not a defect in it — a diagonal's own wedge legitimately narrows
//! wave 4 back into wave 1's own territory. `mli::ewa::rules` exports no
//! predicate function AT ALL for `"diagonal_core"`/`"contracting_boundaries"`/
//! `"expanding_boundaries"` (the catalogue's OTHER diagonal rule ids) —
//! unlike the zigzag/flat ratio bands (`ZIGZAG_B_A`/`FLAT_B_A`/…), which are
//! real exported constants this module already reads, those three are bare
//! catalogue labels with no runnable counterpart anywhere in `mli`, so
//! implementing a check under one of those ids here would be inventing new
//! Elliott logic never sourced from `mli::ewa` — the one thing this module's
//! own opening paragraph forbids. Every OTHER impulse rule
//! ([`impulse_violations`]'s own `"wave2_not_beyond_wave1_origin"`,
//! `"wave3_exceeds_wave1"`, `"wave5_beyond_wave3"`/`"wave5_not_truncated"`,
//! `"wave3_not_shortest"`, and [`violations_in`]'s own
//! `"wave2_retraces_wave1"`/`"wave4_retraces_wave3"`) keeps checking a
//! diagonal exactly as it checks a plain impulse — `mli::ewa::grammar`'s own
//! `LEADING_DIAGONAL_SLOTS`/`ENDING_DIAGONAL_SLOTS` class waves 2 and 4
//! Corrective, same as `IMPULSE_SLOTS`, and gives no other reason to differ.

use mli::ewa::grammar;
use mli::ewa::rules as ewa_rules;
use mli::ewa::types::{EwaPatternKind, EwaSegmentDirection};

use crate::i18n::ConceptKey;
use crate::state::ewa_engine::ewa_engine_config;

use super::super::Primitive;
use super::count::{self, WaveSlot};

// =============================================================================
// Rule families — which class of rule id [`EwaEngineConfig::guard_families`]
// (owner ask 2026-08-14) gates. Classification only — no rule id below is
// new; every one already exists in the functions further down this file.
// =============================================================================

/// The three rule classes [`crate::state::ewa_engine::GuardFamilies`] toggles
/// independently — see [`rule_family`] for which rule id belongs to which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardRuleFamily {
    /// Point-position hard rules.
    Geometry,
    /// Subdivision/typing and declared-shape-vs-measured-ratio rules.
    Grammar,
    /// `follows`-chain-level rules.
    Chain,
}

/// Which [`GuardRuleFamily`] a rule id belongs to. The catch-all arm covers
/// every point-geometry rule id this module produces
/// (`wave2_not_beyond_wave1_origin`, `wave4_no_wave1_overlap`,
/// `wave3_exceeds_wave1`, `wave5_beyond_wave3`, `wave3_not_shortest`,
/// `b_terminates_near_a_origin`,
/// `b_exceeds_a_origin`, `b_does_not_retrace_a_origin`,
/// `c_fails_to_exceed_a_end`, `c_slightly_exceeds_a_end`, `c_exceeds_a_end`,
/// `contracting_boundaries`, `wave2_retraces_wave1`, `wave4_retraces_wave3`)
/// — named here in this doc rather than in the match arm itself, since
/// [`GuardRuleFamily::Geometry`] is the class every OTHER rule id in this
/// module is not.
pub fn rule_family(rule_id: &str) -> GuardRuleFamily {
    match rule_id {
        "chain_continuity" | "chain_alternation" | "chain_degree" => GuardRuleFamily::Chain,
        "subdivision_class"
        | "subdivision_degree"
        | "flat_requires_b_near_a_origin"
        | "zigzag_requires_shallow_b"
        | "expanded_flat_requires_b_beyond_a_origin"
        | "x_is_corrective"
        | "w_and_y_are_corrective"
        | "w_y_z_are_corrective"
        | "at_most_one_zigzag"
        | "triangle_only_final_component"
        // `"wave5_not_truncated"` (2026-08-15): a DECLARED property
        // (`ElliottImpulse::truncated_fifth`) checked against the measured
        // shape — the same class as `"flat_requires_b_near_a_origin"` and
        // its siblings just above, not a raw point-geometry check, even
        // though it is computed inside `impulse_violations` right next to
        // its GEOMETRY sibling `"wave5_beyond_wave3"` (see the module doc's
        // own "Owner correction" paragraph for why the two halves of one
        // toggle sit in two different families).
        | "wave5_not_truncated" => GuardRuleFamily::Grammar,
        _ => GuardRuleFamily::Geometry,
    }
}

/// Whether the ACTIVE `EwaEngineConfig::guard_families` currently reports
/// `rule_id` — read fresh (never cached), the same "read fresh every use"
/// contract `control_point_style` already uses. [`violations`],
/// [`violations_in`] and [`chain_violations`] each filter their own output
/// through this at the very end, so gating a family is one small `retain`
/// per entry point rather than a check threaded through every individual
/// predicate above.
fn family_enabled(rule_id: &str) -> bool {
    let families = ewa_engine_config().guard_families;
    match rule_family(rule_id) {
        GuardRuleFamily::Geometry => families.geometry,
        GuardRuleFamily::Grammar => families.grammar,
        GuardRuleFamily::Chain => families.chain,
    }
}

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
    let mut out = match prim.type_id() {
        "elliott_impulse" => impulse_violations(&points, realized, prim.truncated_fifth(), prim.impulse_kind()),
        "elliott_correction" => {
            correction_violations(&points, realized, prim.correction_subtype().unwrap_or("zigzag"))
        }
        "elliott_triangle" => triangle_violations(&points, realized),
        _ => Vec::new(),
    };
    out.retain(|v| family_enabled(v.rule_id));
    out
}

fn price(points: &[(i64, f64)], idx: usize) -> Option<f64> {
    points.get(idx).map(|p| p.1)
}

// =============================================================================
// Impulse — the three hard rules (owner spec, 2026-08-13)
// =============================================================================

/// `impulse_kind` is [`Primitive::impulse_kind`]'s own token — `Some("leading_diagonal")`/
/// `Some("ending_diagonal")` for a diagonal, `None` for a plain impulse
/// (including a chain-promoted, synthesized coarser impulse — see this
/// function's own call site inside [`chain_violations`], further down, for
/// why that one always passes `None`). Read ONLY to gate
/// `"wave4_no_wave1_overlap"` — see the module doc's own "Diagonals"
/// paragraph for why that is the ONE rule this parameter changes.
fn impulse_violations(
    points: &[(i64, f64)],
    realized: usize,
    truncated_fifth: bool,
    impulse_kind: Option<&str>,
) -> Vec<Violation> {
    let mut out = Vec::new();
    let is_diagonal = matches!(impulse_kind, Some("leading_diagonal") | Some("ending_diagonal"));

    // wave2_not_beyond_wave1_origin — wave 2 must not retrace past wave 1's
    // own origin (point 0). Unchanged for a diagonal — `mli::ewa::grammar`'s
    // own `LEADING_DIAGONAL_SLOTS`/`ENDING_DIAGONAL_SLOTS` class wave 2
    // Corrective, same as a plain impulse's own `IMPULSE_SLOTS`.
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
    // structure's own start). SKIPPED for a diagonal: `mylittleindicators::
    // ewa::primitives`'s own catalogue names this rule `"wave4_overlaps_wave1"`
    // for `LeadingDiagonalContracting`/`EndingDiagonalContracting` — the
    // overlap is part of the shape, not a defect in it (see the module doc's
    // own "Diagonals" paragraph).
    if !is_diagonal && realized >= 5 {
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

    // wave3_exceeds_wave1 — a motive wave must progress: wave 3 must end
    // beyond wave 1's own end, continuing in the direction wave 1 itself
    // established (owner report 2026-08-14, "A motive wave must progress").
    // `beyond(origin, reference, test)` with origin/reference = wave 1's own
    // start/end and test = wave 3's own end is exactly the same "does test
    // progress past reference, relative to origin" predicate
    // `correction_violations` already reuses for its own C-vs-A-end check —
    // no new comparison. Unconditional (unlike the retracement rule in
    // `violations_in`, further down) — no Elliott shape lets wave 3 fail to
    // make new trend-direction ground past wave 1, so this is a plain
    // self-only check, live every frame like every other rule in this
    // function.
    if realized >= 4 {
        if let (Some(p0), Some(p1), Some(p3)) = (price(points, 0), price(points, 1), price(points, 3)) {
            if !ewa_rules::beyond(p0, p1, p3) {
                out.push(Violation {
                    rule_id: "wave3_exceeds_wave1",
                    wave: WaveSlot::Impulse3,
                    message: ConceptKey::RuleWave3NotExceedingWave1,
                });
            }
        }
    }

    // wave5_beyond_wave3 / wave5_not_truncated — the OTHER motive-
    // progression rule (owner report 2026-08-14 named it, implemented
    // 2026-08-15, corrected same day): wave 5's own end is judged against
    // wave 3's own end with the SAME `beyond(origin, reference, test)` call
    // — origin/reference = wave 1's own start (point 0) / wave 3's own end
    // (point 3), test = wave 5's own end (point 5) — one negated, never a
    // second comparison and never a tolerance or "exactly equal" third
    // state; whatever `beyond` decides at the boundary is what both halves
    // inherit (see the module doc's own "Owner correction" paragraph).
    //
    // `truncated_fifth == false` (default): unchanged — `!beyond(...)`
    // reports `"wave5_beyond_wave3"`, GEOMETRY family (`rule_family`'s own
    // catch-all), a plain progression check like `wave3_exceeds_wave1`
    // above.
    //
    // `truncated_fifth == true`: the OPPOSITE reading — `beyond(...)`
    // reports `"wave5_not_truncated"` instead. The analyst declared this
    // fifth truncated (`ElliottImpulse::truncated_fifth`'s own field doc);
    // if the measured shape actually exceeds wave 3, that declaration is
    // wrong, and this is a GRAMMAR-family rule (`rule_family`'s own doc),
    // not geometry: it checks a DECLARED property against the measured
    // shape, the same class `correction_violations`'s own declared-
    // subtype-vs-measured-ratio block already reports as grammar. The two
    // halves of one toggle sitting in two different families looks wrong
    // until this is read — see the module doc for the full rationale.
    if realized >= 6 {
        if let (Some(p0), Some(p3), Some(p5)) = (price(points, 0), price(points, 3), price(points, 5)) {
            let wave5_beyond_wave3 = ewa_rules::beyond(p0, p3, p5);
            if truncated_fifth {
                if wave5_beyond_wave3 {
                    out.push(Violation {
                        rule_id: "wave5_not_truncated",
                        wave: WaveSlot::Impulse5,
                        message: ConceptKey::RuleWave5NotTruncated,
                    });
                }
            } else if !wave5_beyond_wave3 {
                out.push(Violation {
                    rule_id: "wave5_beyond_wave3",
                    wave: WaveSlot::Impulse5,
                    message: ConceptKey::RuleWave5BeyondWave3,
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
///
/// Contract note (owner report 2026-08-14, the `"wave2_retraces_wave1"`/
/// `"wave4_retraces_wave3"` checks below): this function's own contract
/// stated just above — ONLY extra violations, nothing here duplicates OR
/// REVOKES a self-only one — is why the retracement rule is reported ONLY
/// here rather than unconditionally in [`impulse_violations`] and retracted
/// by this pass once a running subdivision is seen. Retraction would need
/// every one of the five Elliott types' own `render` to change its
/// `guard::violations(self)` + `self.context_violations()` merge from a
/// plain concatenation into something that reconciles the two lists —
/// this module chose to keep that merge untouched and this function
/// strictly additive, and pays for it with one real cost: unlike a
/// self-only rule, `"wave2_retraces_wave1"`/`"wave4_retraces_wave3"` do not
/// update live while the analyst drags a wave 2 or wave 4 pivot around —
/// this pass only recomputes once per gesture
/// (`DrawingManager::refresh_wave_guards`'s own doc), not once per frame
/// the way [`impulse_violations`] does. Accepted because mid-drag, before
/// any subdivision has even been chosen, this rule cannot yet know whether
/// a running variant is coming either — the exception it needs to honor
/// literally does not exist until a child primitive does.
pub fn violations_in(prim: &dyn Primitive, others: &[&dyn Primitive]) -> Vec<Violation> {
    let mut out = Vec::new();
    let prim_id = prim.data().id;
    let children: Vec<(&dyn Primitive, WaveSlot)> = others
        .iter()
        .filter_map(|&p| p.wave_link().filter(|l| l.parent_id == prim_id).map(|l| (p, l.slot)))
        .collect();

    // a) subdivision_class — a child occupying a slot whose grammar its own
    // kind does not satisfy. Catches a stale file, a hand-edited preset, or
    // a promoted chain that ended up with the wrong shape in a slot. A
    // child `count::kind_id_of` cannot even NAME (outside the five Elliott
    // types) is treated as a violation too, not skipped — it certainly
    // does not satisfy whatever `count::child_kinds` allows.
    for &(child, slot) in &children {
        let matches = count::kind_id_of(child)
            .is_some_and(|kind| count::child_kinds(prim, slot).iter().any(|&k| k == kind));
        if !matches {
            out.push(Violation { rule_id: "subdivision_class", wave: slot, message: ConceptKey::RuleSubdivisionClass });
        }
    }

    // a.1) subdivision_degree — a subdivision must carry a degree exactly
    // one step FINER than the parent it is nested inside
    // (`elliott::count::child_seed`'s own contract:
    // `parent.wave_degree()?.finer()`). Checked from the CHILD side (`prim`
    // itself, when it carries a [`count::WaveLink`]) rather than from the
    // parent's own pass above (`a`): the parent's own `children` list can
    // only see THAT a child is nested in a slot, never report on it by
    // name from there without duplicating this same lookup per child, so
    // this reads more naturally as the child's own violation, matching
    // `wave: own_first_slot` — a stand-in "the whole structure" marker,
    // same shape `chain_continuity` (below) already uses for the same
    // reason: there is no single INTERNAL wave this is about.
    //
    // Closes a real, previously-silent gap: higher-degree auto-adoption
    // (`count::adopted_parent_degree`) derives the new parent's own degree
    // as the adopted children's shared degree, one step
    // [`crate::drawing::primitives_v2::config::WaveDegree::coarser`]. That
    // step SATURATES at `WaveDegree::Supermillennium` rather than wrapping
    // or panicking — adopting children already at that ceiling leaves the
    // new parent SHARING its own children's degree instead of sitting one
    // step above them, and until this check existed nothing reported it.
    if let Some(link) = prim.wave_link() {
        if let Some(parent) = others.iter().find(|p| p.data().id == link.parent_id) {
            if let (Some(parent_degree), Some(own_degree)) = (parent.wave_degree(), prim.wave_degree()) {
                if own_degree != parent_degree.finer() {
                    if let Some(&own_first_slot) = prim.wave_slots().first() {
                        out.push(Violation {
                            rule_id: "subdivision_degree",
                            wave: own_first_slot,
                            message: ConceptKey::RuleSubdivisionDegree,
                        });
                    }
                }
            }
        }
    }

    if prim.type_id() == "elliott_impulse" {
        let points = prim.points();
        let realized = prim.realized_point_count();
        let is_diagonal = matches!(prim.impulse_kind(), Some("leading_diagonal") | Some("ending_diagonal"));

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
        // adds no violation of its own here. SKIPPED entirely for a diagonal
        // — same reason `impulse_violations`'s own shallow check skips it
        // (see the module doc's own "Diagonals" paragraph): the overlap is
        // legal, so there is nothing here to add strictness TO.
        if !is_diagonal && realized >= 4 {
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

        // wave2_retraces_wave1 / wave4_retraces_wave3 — a corrective wave
        // must move AGAINST the trend direction the impulse's own wave 1
        // established (owner report 2026-08-14: "видно, что 4 волна ниже,
        // чем 3, но ведь так не может быть по правилам"). Direction is
        // derived once from wave 1's own leg (points 0->1), exactly as
        // `correction_violations` derives its own B/C direction from wave
        // A's leg; `ewa_rules::progresses` is the SAME predicate that
        // function already reads there, not a new comparison. See this
        // function's own doc, above, for why this rule is reported ONLY
        // here — gated on [`running_subdivision`] — rather than
        // unconditionally in [`impulse_violations`].
        if realized >= 3 {
            if let (Some(p0), Some(p1)) = (price(&points, 0), price(&points, 1)) {
                let direction = EwaSegmentDirection::from_delta(p1 - p0);
                if let Some(p2) = price(&points, 2) {
                    if ewa_rules::progresses(direction, p1, p2) && !running_subdivision(&children, WaveSlot::Impulse2) {
                        out.push(Violation {
                            rule_id: "wave2_retraces_wave1",
                            wave: WaveSlot::Impulse2,
                            message: ConceptKey::RuleWave2NotRetracingWave1,
                        });
                    }
                }
                if realized >= 5 {
                    if let (Some(p3), Some(p4)) = (price(&points, 3), price(&points, 4)) {
                        if ewa_rules::progresses(direction, p3, p4) && !running_subdivision(&children, WaveSlot::Impulse4) {
                            out.push(Violation {
                                rule_id: "wave4_retraces_wave3",
                                wave: WaveSlot::Impulse4,
                                message: ConceptKey::RuleWave4NotRetracingWave3,
                            });
                        }
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

    out.retain(|v| family_enabled(v.rule_id));
    out
}

// =============================================================================
// Chain guard — the rules that live INSIDE a `follows` chain rather than
// within one structure (owner ask 2026-08-13, "ну и правила внутри
// цепочек" — filed the same day as this module's own opening report: the
// owner tore a chain apart by dragging one member, and nothing flagged it).
//
// A `follows` chain at one degree IS the subdivision of a coarser structure
// — `count::promote_chain_shape`'s own doc — so the chain's own hard rules
// are the SAME family rules [`violations`] already runs above
// (`impulse_violations`/`correction_violations`, both already written over
// a plain `&[(i64, f64)]` slice, so nothing here re-implements either), just
// applied to the CHAIN's own pivot sequence instead of one structure's own
// `points()`. [`chain_violations`] never re-derives that sequence itself —
// it reads it back off `count::promote_chain_shape(chain)`'s own result
// (`PromotedShape::points`, which already exposed the pivot list this
// needed — see `count::chain_pivots`'s own doc for the shared derivation),
// so the pivot list a promoted parent would actually carry and the pivot
// list these rules run against can never drift apart.
// =============================================================================

/// One broken hard rule found along a `follows` chain rather than within a
/// single structure — see this section's own doc.
#[derive(Clone, Copy, Debug)]
pub struct ChainViolation {
    /// The rule id — the SAME family ids [`Violation::rule_id`] already uses
    /// for the two family rule sets re-run here against the chain's own
    /// joints (e.g. `"wave2_not_beyond_wave1_origin"`), plus the two ids
    /// that exist ONLY at chain level: `"chain_alternation"` and
    /// `"chain_degree"`.
    pub rule_id: &'static str,
    /// Which member of the `chain` slice passed to [`chain_violations`]
    /// (0-indexed) owns the offending wave — a caller marks THAT member's
    /// own drawing, never the whole chain.
    pub member_index: usize,
    /// The promoted slot `chain[member_index]` occupies — the same
    /// [`WaveSlot`] vocabulary a single structure's own [`Violation::wave`]
    /// uses, via [`count::chain_slot_for_index`].
    pub wave: WaveSlot,
    /// The user-facing explanation, via `mlc_core::i18n`.
    pub message: ConceptKey,
}

/// Every hard-rule violation a `follows` chain currently exhibits, checked
/// at the chain's own joints — see this section's own doc.
///
/// `chain` must already be in succession order (`chain[0]` is the chain's
/// own head) — same contract [`count::promote_chain_shape`] itself has;
/// this function does not sort or search for it.
///
/// Two rules exist ONLY at chain level:
/// - `"chain_alternation"` — a member must not share its own
///   motive/corrective class with the member right before it (an impulse
///   directly following another impulse, or a corrective structure
///   directly following another corrective one). The creation path
///   ([`count::successor_kinds`]) already enforces this when a chain is
///   built one link at a time — this re-check catches a promoted, pasted,
///   or hand-edited chain that ends up violating it anyway.
/// - `"chain_degree"` — every member must carry the SAME
///   [`Primitive::wave_degree`] as the chain's own head. A chain is by
///   definition one degree.
///
/// Both chain-level rules are evaluated regardless of whether the chain
/// otherwise promotes — they exist PRECISELY to catch a chain
/// [`count::promote_chain_shape`] itself would refuse.
///
/// The family rules (the SAME ids [`violations`] runs against a single
/// structure) additionally run whenever the chain has at least 3 members
/// AND [`count::promote_chain_shape`] resolves it to a well-formed coarser
/// shape — never a half-applied family rule against a chain too short,
/// mixed-degree, non-contiguous, already nested elsewhere, or alternation-
/// broken, because there is no well-formed promoted shape to run them
/// against in any of those cases (the two chain-level rules above already
/// cover the alternation/degree cases on their own).
pub fn chain_violations(chain: &[&dyn Primitive]) -> Vec<ChainViolation> {
    let mut out = Vec::new();
    let Some(head) = chain.first() else { return out };
    let starts_motive = head.type_id() == "elliott_impulse";

    // chain_degree — every member must carry the head's own degree.
    if let Some(head_degree) = head.wave_degree() {
        for (i, &p) in chain.iter().enumerate().skip(1) {
            if p.wave_degree() != Some(head_degree) {
                if let Some(wave) = count::chain_slot_for_index(starts_motive, i) {
                    out.push(ChainViolation {
                        rule_id: "chain_degree",
                        member_index: i,
                        wave,
                        message: ConceptKey::RuleChainDegreeMismatch,
                    });
                }
            }
        }
    }

    // chain_alternation — no two adjacent members share a motive/corrective
    // class.
    for i in 1..chain.len() {
        let prev_motive = chain[i - 1].type_id() == "elliott_impulse";
        let this_motive = chain[i].type_id() == "elliott_impulse";
        if prev_motive == this_motive {
            if let Some(wave) = count::chain_slot_for_index(starts_motive, i) {
                out.push(ChainViolation {
                    rule_id: "chain_alternation",
                    member_index: i,
                    wave,
                    message: ConceptKey::RuleChainAlternationBroken,
                });
            }
        }
    }

    // The family rules — only once the chain actually resolves to a
    // well-formed coarser shape (see this function's own doc).
    if chain.len() >= 3 {
        if let Some(shape) = count::promote_chain_shape(chain) {
            let realized = shape.points.len();
            let family = match shape.type_id {
                // `truncated_fifth: false` and `impulse_kind: None`
                // unconditionally — a promoted chain's own coarser impulse
                // is SYNTHESIZED (`count::promote_chain_shape`), not drawn,
                // so there is no single `ElliottImpulse::truncated_fifth`/
                // `ElliottImpulse::impulse_kind` this assembled shape's own
                // wave 5/wave 4 could read from; the owner's ask
                // (2026-08-15) was a toggle on a drawn structure, not a
                // chain-level counterpart, and a diagonal is likewise a
                // per-structure claim no synthesized shape carries.
                // `"wave5_beyond_wave3"`/`"wave4_no_wave1_overlap"` both
                // stay ACTIVE here, same posture a chain member that was
                // never drawn as either already has.
                "elliott_impulse" => impulse_violations(&shape.points, realized, false, None),
                "elliott_correction" => {
                    let subtype = head.correction_subtype().unwrap_or("zigzag");
                    correction_violations(&shape.points, realized, subtype)
                }
                _ => Vec::new(),
            };
            for v in family {
                if let Some(member_index) = shape.slots.iter().position(|&s| s == v.wave) {
                    out.push(ChainViolation { rule_id: v.rule_id, member_index, wave: v.wave, message: v.message });
                }
            }
        }
    }

    out.retain(|v| family_enabled(v.rule_id));
    out
}

/// This primitive's own catalogue pattern kind — [`count::kind_pattern`]
/// applied to [`count::kind_id_of`], so `d)`'s combination-grammar checks
/// read the SAME mapping [`count::child_kinds`] itself is built from.
/// `None` for a primitive [`count::kind_id_of`] cannot name (mirrors
/// [`count::kind_pattern`]'s own `None` contract for an unrecognized kind
/// id). Was a private `child_kind_id` formatting the same string inline —
/// moved to `count::kind_id_of` (owner report 2026-08-14) so the
/// conversion picker (`count::conversion_kinds`) reads the SAME "what kind
/// is this primitive" computation, not a second copy of it.
fn child_pattern(prim: &dyn Primitive) -> Option<EwaPatternKind> {
    count::kind_id_of(prim).and_then(|k| count::kind_pattern(&k))
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

/// Whether `slot`'s own child (if any) among `children` is a RUNNING
/// variant — `EwaPatternKind::RunningFlat`/`RunningTriangle`/
/// `RunningZigzag`, the catalogue's own definition of "legitimately ends in
/// the trend direction instead of retracing against it" (a running flat's
/// own catalogue rule is literally named `"c_fails_to_exceed_a_end"` — see
/// `mli::ewa::primitives::EWA_PRIMITIVES`'s own `RunningFlat`/
/// `RunningTriangle` entries — and `RunningZigzag` reuses the same rule
/// under `zigzag_core`). Reads the child's own kind through [`child_pattern`]
/// (`count::kind_id_of` + `count::kind_pattern`), never a string match on
/// the chart-side type id — the exact vocabulary the owner ask for this
/// check named (see [`violations_in`]'s own doc, 2026-08-14 note). `false`
/// when `slot` has no child, or the child's own kind cannot be named at all
/// — an absent or unrecognized subdivision earns no exception; it must
/// actually BE a running structure to open one.
fn running_subdivision(children: &[(&dyn Primitive, WaveSlot)], slot: WaveSlot) -> bool {
    find_child(children, slot).and_then(child_pattern).is_some_and(|kind| {
        matches!(kind, EwaPatternKind::RunningFlat | EwaPatternKind::RunningTriangle | EwaPatternKind::RunningZigzag)
    })
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

#[cfg(test)]
mod retracement_tests {
    use super::*;
    use crate::drawing::primitives_v2::elliott::{CorrectionType, ElliottCorrection, ElliottImpulse, WaveLink};

    /// The owner's own report (2026-08-14): a plain downward impulse — point
    /// 0 ~65600, wave 1 end ~64800, wave 2 end ~65200, wave 3 end ~63800,
    /// wave 4 end ~63600, wave 5 end ~63000 (wave 3/5 nudged off the
    /// screenshot's own rounded values so `wave3_exceeds_wave1` and
    /// `wave3_not_shortest` stay clean and this fixture isolates exactly the
    /// violation the owner is asking about — wave 4 ends BELOW wave 3,
    /// continuing the downtrend instead of retracing against it).
    fn owner_report_impulse() -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 65_600.0),
            (1_000, 64_800.0),
            (2_000, 65_200.0),
            (3_000, 63_800.0),
            (4_000, 63_600.0),
            (5_000, 63_000.0),
        ];
        let mut impulse = ElliottImpulse::new(points, "#2196F3");
        impulse.data.id = 1;
        impulse
    }

    #[test]
    fn owner_report_impulse_flags_wave4_retraces_wave3() {
        let impulse = owner_report_impulse();

        // Self-only pass never sees this rule (see `violations_in`'s own
        // doc) — the impulse's OTHER self-only rules (`wave3_exceeds_wave1`,
        // `wave3_not_shortest`, `wave2_not_beyond_wave1_origin`,
        // `wave4_no_wave1_overlap`) all stay clean on these numbers.
        assert!(violations(&impulse).is_empty(), "self-only pass reports nothing for this fixture");

        let found = violations_in(&impulse, &[]);
        assert_eq!(found.len(), 1, "exactly the wave-4 retracement violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "wave4_retraces_wave3");
        assert!(matches!(found[0].wave, WaveSlot::Impulse4));
    }

    #[test]
    fn running_flat_in_wave4_clears_the_retracement_violation() {
        let impulse = owner_report_impulse();

        let mut running_child = ElliottCorrection::new(
            [(3_000, 63_800.0), (3_300, 63_700.0), (3_600, 63_750.0), (4_000, 63_600.0)],
            "#FF5722",
        );
        running_child.correction_type = CorrectionType::Running;
        running_child.set_wave_link(Some(WaveLink { parent_id: impulse.data.id, slot: WaveSlot::Impulse4 }));

        let others: Vec<&dyn Primitive> = vec![&running_child];
        let found = violations_in(&impulse, &others);
        assert!(
            !found.iter().any(|v| v.rule_id == "wave4_retraces_wave3"),
            "a running-flat subdivision in wave 4 legitimately fails to retrace, so this rule must not fire: {found:?}"
        );
    }

    #[test]
    fn compliant_impulse_reports_nothing() {
        let points: [(i64, f64); 6] = [
            (0, 100.0),
            (1_000, 110.0),
            (2_000, 104.0),
            (3_000, 130.0),
            (4_000, 120.0),
            (5_000, 140.0),
        ];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        assert!(violations(&impulse).is_empty());
        assert!(violations_in(&impulse, &[]).is_empty());
    }
}

#[cfg(test)]
mod subdivision_degree_tests {
    //! `"subdivision_degree"` — coordinator review of higher-degree
    //! auto-adoption (2026-08-14): `elliott::count::adopted_parent_degree`'s
    //! own ceiling case (`WaveDegree::coarser` saturates at
    //! `Supermillennium`) left a new parent silently SHARING its own
    //! children's degree instead of sitting one step above them, and
    //! nothing reported it. These tests exercise the rule from the CHILD
    //! side, the way it is actually checked — see [`violations_in`]'s own
    //! `"a.1"` block.

    use super::*;
    use crate::drawing::primitives_v2::config::WaveDegree;
    use crate::drawing::primitives_v2::elliott::{ElliottCorrection, ElliottImpulse, WaveLink};
    use crate::state::ewa_engine::{EwaEngineConfig, set_ewa_engine_config};

    /// A plain impulse parent (default `WaveDegree::Intermediate`) with its
    /// own wave-2 span — every test below nests a correction into that
    /// slot and varies only the CHILD's own degree.
    fn parent() -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let mut p = ElliottImpulse::new(points, "#2196F3");
        p.data.id = 1;
        p
    }

    /// A zigzag correction nested into the parent's own wave-2 slot,
    /// carrying `degree` exactly as given — the fixture every test below
    /// varies only by which degree it passes.
    fn child_at(parent_id: u64, degree: WaveDegree) -> ElliottCorrection {
        let points: [(i64, f64); 4] = [(1_000, 110.0), (1_250, 107.0), (1_500, 109.0), (2_000, 105.0)];
        let mut c = ElliottCorrection::new(points, "#FF5722");
        c.data.id = 2;
        c.degree = degree;
        c.link = Some(WaveLink { parent_id, slot: WaveSlot::Impulse2 });
        c
    }

    /// Every scenario lives in ONE test function on purpose:
    /// `guard_families.grammar` is process-global mutable state
    /// (`mlc-core`'s own `OnceLock<RwLock<EwaEngineConfig>>`), and `cargo
    /// test` runs different `#[test]` functions on different threads by
    /// default. Any function here that flips it off would race every OTHER
    /// function in this module that assumes the family stays on
    /// (measured: it did, intermittently, splitting this into five
    /// functions made the "reports" cases occasionally see the family OFF
    /// mid-flight and read back an empty result). One function, one
    /// thread, deterministic — same reason `manager::
    /// auto_adoption_gate_tests::adoption_gated_by_detect_higher_degree`
    /// (`mlc-backend`) merges its own off/on halves for the SAME class of
    /// global.
    #[test]
    fn subdivision_degree_rule_behaves_correctly() {
        set_ewa_engine_config(EwaEngineConfig::default());
        let p = parent();

        // A child ONE step finer than its parent — the correct relation
        // `child_seed` itself always produces — must not report.
        let one_step_finer = child_at(p.data.id, p.wave_degree().expect("impulse carries a degree").finer());
        let found = violations_in(&one_step_finer, &[&p]);
        assert!(!found.iter().any(|v| v.rule_id == "subdivision_degree"), "one step finer is the correct relation: {found:?}");

        // A child at the SAME degree as its parent — the higher-degree
        // auto-adoption ceiling case (`count::adopted_parent_degree`'s own
        // doc: `WaveDegree::coarser` saturates at `Supermillennium`) —
        // must report, and nothing else.
        let same_degree = child_at(p.data.id, p.wave_degree().expect("impulse carries a degree"));
        let found = violations_in(&same_degree, &[&p]);
        assert_eq!(found.len(), 1, "exactly the degree violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "subdivision_degree");
        assert_eq!(found[0].wave, WaveSlot::CorrectionA, "the child's own FIRST slot stands in for \"the whole structure\", same as chain_continuity's own marker");

        // A child TWO steps finer — still wrong, not merely "finer than".
        let two_steps_finer = child_at(p.data.id, p.wave_degree().expect("impulse carries a degree").finer().finer());
        let found = violations_in(&two_steps_finer, &[&p]);
        assert!(found.iter().any(|v| v.rule_id == "subdivision_degree"), "two steps finer is still wrong: {found:?}");

        // Grammar family OFF: the loudest fixture (same-degree, already
        // proven to violate above) must now report nothing.
        let mut cfg = EwaEngineConfig::default();
        cfg.guard_families.grammar = false;
        set_ewa_engine_config(cfg);
        let found = violations_in(&same_degree, &[&p]);
        assert!(!found.iter().any(|v| v.rule_id == "subdivision_degree"), "grammar family off must silence this rule too: {found:?}");
        set_ewa_engine_config(EwaEngineConfig::default());

        // The normal hand-made-subdivision path (`DrawingManager::
        // create_wave_child`) seeds a NEW child through `count::
        // child_seed`, whose own degree is `parent.wave_degree()?.finer()`
        // by contract — this must stay exactly as clean under the new rule
        // as it already was, or the rule is wrong, not the seed.
        let seed = count::child_seed(&p, WaveSlot::Impulse2).expect("impulse has a wave-2 slot");
        let seeded_points = seed.interpolate(4);
        let points: [(i64, f64); 4] = [seeded_points[0], seeded_points[1], seeded_points[2], seeded_points[3]];
        let mut seeded_child = ElliottCorrection::new(points, "#FF5722");
        seeded_child.data.id = 2;
        seeded_child.degree = seed.degree;
        seeded_child.link = Some(WaveLink { parent_id: p.data.id, slot: WaveSlot::Impulse2 });
        let found = violations_in(&seeded_child, &[&p]);
        assert!(found.is_empty(), "child_seed's own .finer() contract must still produce zero violations: {found:?}");
    }
}

#[cfg(test)]
mod wave5_truncation_tests {
    //! `"wave5_beyond_wave3"` / `"wave5_not_truncated"` +
    //! `ElliottImpulse::truncated_fifth` (owner ruling 2026-08-15, corrected
    //! same day: the flag is a CLAIM about the shape, not permission to
    //! stop checking it — both halves of the toggle are judged). None of
    //! these touch `guard_families` — unlike `subdivision_degree_tests`
    //! above, there is no process-global `EwaEngineConfig` state to race
    //! here, so each scenario is free to stay its own `#[test]` function.

    use super::*;
    use crate::drawing::primitives_v2::elliott::ElliottImpulse;

    /// Wave 5 ends beyond wave 3 (140 > 130) — the compliant case for
    /// `truncated_fifth == false`, every other self-only rule already
    /// clean on this shape (same fixture
    /// `retracement_tests::compliant_impulse_reports_nothing` uses).
    fn wave5_exceeds_wave3() -> ElliottImpulse {
        let points: [(i64, f64); 6] =
            [(0, 100.0), (1_000, 110.0), (2_000, 104.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0)];
        ElliottImpulse::new(points, "#2196F3")
    }

    /// Wave 5 ends SHORT of wave 3 (125 < 130) — every OTHER self-only rule
    /// still clean (wave 3 not shortest: lengths 10/26/5; wave 2/4 stay on
    /// their own side of the origin; wave 3 still exceeds wave 1), isolating
    /// exactly the one violation this module exists to test.
    fn wave5_falls_short_of_wave3() -> ElliottImpulse {
        let points: [(i64, f64); 6] =
            [(0, 100.0), (1_000, 110.0), (2_000, 104.0), (3_000, 130.0), (4_000, 120.0), (5_000, 125.0)];
        ElliottImpulse::new(points, "#2196F3")
    }

    #[test]
    fn fifth_exceeding_third_reports_nothing() {
        let impulse = wave5_exceeds_wave3();
        let found = violations(&impulse);
        assert!(found.is_empty(), "flag off, a fifth that exceeds wave 3 is compliant progression, nothing must report: {found:?}");
    }

    #[test]
    fn short_fifth_reports_wave5_beyond_wave3() {
        let impulse = wave5_falls_short_of_wave3();
        let found = violations(&impulse);
        assert_eq!(found.len(), 1, "exactly the wave-5 violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "wave5_beyond_wave3");
        assert_eq!(found[0].wave, WaveSlot::Impulse5);
    }

    /// `truncated_fifth == true` and the measured fifth really is short —
    /// the analyst's claim matches the shape, so neither wave-5 rule fires.
    #[test]
    fn truncated_fifth_flag_on_with_short_fifth_reports_nothing() {
        let mut impulse = wave5_falls_short_of_wave3();
        impulse.truncated_fifth = true;
        let found = violations(&impulse);
        assert!(found.is_empty(), "the declared truncation matches the measured shape, nothing must report: {found:?}");
    }

    /// `truncated_fifth == true` but the measured fifth actually exceeds
    /// wave 3 — the analyst's own claim is contradicted by the shape, so
    /// the GRAMMAR-family `"wave5_not_truncated"` fires, and its GEOMETRY
    /// sibling `"wave5_beyond_wave3"` must not (only one of the two rules
    /// is ever active for a given structure).
    #[test]
    fn truncated_fifth_flag_on_with_exceeding_fifth_reports_wave5_not_truncated() {
        let mut impulse = wave5_exceeds_wave3();
        impulse.truncated_fifth = true;
        let found = violations(&impulse);
        assert_eq!(found.len(), 1, "exactly the contradicted-claim violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "wave5_not_truncated");
        assert_eq!(found[0].wave, WaveSlot::Impulse5);
        assert!(!found.iter().any(|v| v.rule_id == "wave5_beyond_wave3"), "the geometry sibling must not also fire: {found:?}");
    }

    /// The flag's own contract (field doc): it switches which of the two
    /// wave-5 rules is judged, it does not touch anything else — a
    /// SEPARATE violation on the same structure (wave 4 re-entering wave
    /// 1's own territory) must keep reporting regardless of the flag.
    #[test]
    fn truncated_fifth_flag_does_not_silence_a_different_violation() {
        // Same short fifth (130 -> 125) PLUS wave 4 pulled back to 105,
        // inside wave 1's own [100, 110] span — `wave4_no_wave1_overlap`
        // fires independently of wave 5. `truncated_fifth = true` and the
        // fifth really is short, so the wave-5 claim matches the shape and
        // neither wave-5 rule fires alongside it.
        let points: [(i64, f64); 6] =
            [(0, 100.0), (1_000, 110.0), (2_000, 104.0), (3_000, 130.0), (4_000, 105.0), (5_000, 125.0)];
        let mut impulse = ElliottImpulse::new(points, "#2196F3");
        impulse.truncated_fifth = true;

        let found = violations(&impulse);
        assert!(found.iter().any(|v| v.rule_id == "wave4_no_wave1_overlap"), "the unrelated violation must still report: {found:?}");
        assert!(!found.iter().any(|v| v.rule_id == "wave5_beyond_wave3"), "the claim matches the shape, this half must stay silent: {found:?}");
        assert!(!found.iter().any(|v| v.rule_id == "wave5_not_truncated"), "the claim matches the shape, this half must stay silent too: {found:?}");
    }

    /// A downward impulse — the same short-fifth geometry mirrored around
    /// the origin (every price delta negated) — must report the same rule.
    /// `beyond`'s own direction derivation (`reference >= origin`) flips
    /// branch on a downward structure, so this is the check that the rule
    /// is not accidentally direction-locked to an uptrend.
    #[test]
    fn downward_impulse_reports_wave5_beyond_wave3() {
        let points: [(i64, f64); 6] =
            [(0, 100.0), (1_000, 90.0), (2_000, 96.0), (3_000, 70.0), (4_000, 80.0), (5_000, 75.0)];
        let impulse = ElliottImpulse::new(points, "#2196F3");
        let found = violations(&impulse);
        assert_eq!(found.len(), 1, "exactly the wave-5 violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "wave5_beyond_wave3");
        assert_eq!(found[0].wave, WaveSlot::Impulse5);
    }

    /// The downward mirror of `truncated_fifth_flag_on_with_exceeding_fifth_
    /// reports_wave5_not_truncated`: same downward shape as the test above,
    /// but wave 5 pushed further past wave 3 (60 < 70, continuing the
    /// downtrend past wave 3's own end) with the flag on — the claim is
    /// contradicted on a downward structure too, not only an uptrend.
    #[test]
    fn downward_impulse_with_flag_on_reports_wave5_not_truncated() {
        let points: [(i64, f64); 6] =
            [(0, 100.0), (1_000, 90.0), (2_000, 96.0), (3_000, 70.0), (4_000, 80.0), (5_000, 60.0)];
        let mut impulse = ElliottImpulse::new(points, "#2196F3");
        impulse.truncated_fifth = true;
        let found = violations(&impulse);
        assert_eq!(found.len(), 1, "exactly the contradicted-claim violation, nothing else: {found:?}");
        assert_eq!(found[0].rule_id, "wave5_not_truncated");
        assert_eq!(found[0].wave, WaveSlot::Impulse5);
    }
}

#[cfg(test)]
mod diagonal_overlap_tests {
    //! `ElliottImpulse::impulse_kind` (the diagonal arc) — the ONE guard
    //! rule that changes for a diagonal: wave 4 legally overlaps wave 1's
    //! own territory (see the module doc's own "Diagonals" paragraph).

    use super::*;
    use crate::drawing::primitives_v2::elliott::{ElliottImpulse, ImpulseKind};

    /// Wave 4 ends at 105, inside wave 1's own [100, 110] span — the
    /// classic overlap shape: illegal for a plain impulse, legal for a
    /// diagonal.
    fn overlapping_wave4_impulse(kind: ImpulseKind) -> ElliottImpulse {
        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 104.0), (3_000, 130.0), (4_000, 105.0), (5_000, 140.0),
        ];
        let mut impulse = ElliottImpulse::new(points, "#2196F3");
        impulse.impulse_kind = kind;
        impulse
    }

    #[test]
    fn plain_impulse_still_reports_the_overlap() {
        let impulse = overlapping_wave4_impulse(ImpulseKind::Plain);
        let found = violations(&impulse);
        assert!(
            found.iter().any(|v| v.rule_id == "wave4_no_wave1_overlap"),
            "the SAME geometry, as a plain impulse, must still flag the overlap: {found:?}"
        );
    }

    #[test]
    fn ending_diagonal_reports_no_overlap_violation() {
        let impulse = overlapping_wave4_impulse(ImpulseKind::EndingDiagonal);
        let found = violations(&impulse);
        assert!(
            !found.iter().any(|v| v.rule_id == "wave4_no_wave1_overlap"),
            "an ending diagonal's own wave-4/wave-1 overlap is legal, this must not report: {found:?}"
        );
    }

    #[test]
    fn leading_diagonal_reports_no_overlap_violation() {
        let impulse = overlapping_wave4_impulse(ImpulseKind::LeadingDiagonal);
        let found = violations(&impulse);
        assert!(
            !found.iter().any(|v| v.rule_id == "wave4_no_wave1_overlap"),
            "a leading diagonal's own wave-4/wave-1 overlap is legal, this must not report: {found:?}"
        );
    }
}
