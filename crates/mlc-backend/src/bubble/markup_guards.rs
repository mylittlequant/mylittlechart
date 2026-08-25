//! The cross-primitive rule pass for the non-Elliott markup families.
//!
//! # Why this is a bubble-scoped pass and not a method on a primitive
//!
//! Same reason [`super::wave_guards`] is: a primitive cannot see its own
//! siblings. A rule that is ABOUT the neighbours — two claims reading the
//! same bar, a claim contradicting the one it follows — has nowhere to be
//! computed except a pass that holds the whole collection at once. What a
//! claim keeps is the RESULT, in the same `context_violations` list its own
//! bars-derived rules already live in (see
//! `Primitive::push_context_violations`).
//!
//! # The ordering contract
//!
//! This pass APPENDS. Every call site must run
//! [`super::bar_derived_cache::refresh_bar_derived_caches`] (and, where the
//! bubble carries one, its footprint counterpart) FIRST — those rebuild each
//! claim's self-derived violations wholesale — and this pass immediately
//! after. Running it twice without a refresh in between would report the
//! same neighbour twice; running the refresh without this pass afterwards
//! silently drops the cross-primitive half until the next gesture.
//!
//! # Rhythm
//!
//! Once per gesture, never per frame — the rhythm `wave_guards`'s own module
//! doc argues for, for the same reason: this walks every primitive against
//! every other one of its family, which is fine once per edit and wrong
//! sixty times a second.

use mlc_core::drawing::primitives_v2::elliott::guard::Violation;
use mlc_core::drawing::primitives_v2::markup::MarkupFamily;
use mlc_core::i18n::ConceptKey;

use super::data_bubble::DataBubble;

/// Two claims of the same kind anchored on the same bar.
///
/// Not a style objection — a claim is an assertion about one bar, so a
/// second identical assertion on that bar is either a stray double-click or
/// a leftover the analyst meant to delete, and either way the chart is now
/// drawing the same reading twice. Both are flagged: neither is more
/// original than the other, and the analyst is the one who knows which to
/// keep.
const RULE_OVERLAPPING_CLAIM: &str = "overlapping_claim";

/// Recompute every markup claim's own cross-primitive violations for `b`
/// and append them onto what its bars/footprint pass already reported.
///
/// Elliott is deliberately not touched here — it answers `None` to
/// `markup_family` and keeps [`super::wave_guards`], a working engine this
/// pass has no business rewriting.
pub(crate) fn refresh_markup_guards(b: &mut DataBubble) {
    // One immutable pass to decide, one mutable pass to write — the shape
    // `wave_guards::refresh_wave_guards` uses, for the same borrow reason.
    let anchors: Vec<Option<(MarkupFamily, String, i64)>> = b
        .primitives
        .iter()
        .map(|p| {
            let family = p.markup_family()?;
            // A claim's own anchor is its first point's timestamp — the
            // same point every single-anchor claim in these families stores
            // and every two-point survey starts at.
            let anchor_ts = p.points().first().map(|(ts, _)| *ts)?;
            Some((family, p.type_id().to_string(), anchor_ts))
        })
        .collect();

    let mut updates: Vec<(usize, Vec<Violation>)> = Vec::new();
    for (i, own) in anchors.iter().enumerate() {
        let Some((family, type_id, anchor_ts)) = own else { continue };
        let duplicated = anchors.iter().enumerate().any(|(j, other)| {
            j != i
                && matches!(other, Some((f, t, ts)) if f == family && t == type_id && ts == anchor_ts)
        });
        if duplicated {
            updates.push((
                i,
                vec![Violation::whole(
                    RULE_OVERLAPPING_CLAIM,
                    ConceptKey::RuleOverlappingClaim,
                )],
            ));
        }
    }

    for (i, violations) in updates {
        b.primitives[i].push_context_violations(violations);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::drawing::primitives_v2::PrimitiveRegistry;

    fn bubble_with(prims: Vec<(&str, i64)>) -> DataBubble {
        // Same construction the bubble's own tests use — a real store, so
        // nothing here depends on a `Default` this type deliberately lacks.
        let mut store = crate::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), mlc_core::Timeframe::m1());
        let mut b = store.bubble(bid).cloned().expect("just-created bubble");
        let registry = PrimitiveRegistry::global().read().unwrap();
        for (type_id, ts) in prims {
            let prim = registry
                .create(type_id, &[(ts, 0.0), (ts + 60_000, 0.0)], Some("#3ba3ff"))
                .unwrap_or_else(|| panic!("registry has no {type_id}"));
            b.primitives.push(prim);
        }
        b
    }

    fn rule_ids(b: &DataBubble, i: usize) -> Vec<&'static str> {
        b.primitives[i].context_violations().iter().map(|v| v.rule_id).collect()
    }

    #[test]
    fn two_claims_of_the_same_kind_on_one_bar_are_both_flagged() {
        let mut b = bubble_with(vec![("order_block", 1_000), ("order_block", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(rule_ids(&b, 0).contains(&"overlapping_claim"));
        assert!(rule_ids(&b, 1).contains(&"overlapping_claim"), "both, not just the second");
    }

    #[test]
    fn the_same_kind_on_different_bars_is_not_a_duplicate() {
        let mut b = bubble_with(vec![("order_block", 1_000), ("order_block", 2_000)]);
        refresh_markup_guards(&mut b);
        assert!(!rule_ids(&b, 0).contains(&"overlapping_claim"));
        assert!(!rule_ids(&b, 1).contains(&"overlapping_claim"));
    }

    #[test]
    fn different_kinds_on_one_bar_are_two_readings_not_a_duplicate() {
        let mut b = bubble_with(vec![("order_block", 1_000), ("fair_value_gap", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(!rule_ids(&b, 0).contains(&"overlapping_claim"));
        assert!(!rule_ids(&b, 1).contains(&"overlapping_claim"));
    }

    #[test]
    fn a_survey_declares_its_family_but_reports_no_neighbour_rule() {
        // Two identical surveys: they make no claim, so neither contradicts
        // the other — see `Primitive::push_context_violations`'s own default.
        let mut b = bubble_with(vec![("smc_scan", 1_000), ("smc_scan", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(rule_ids(&b, 0).is_empty());
    }

    #[test]
    fn a_drawing_outside_the_markup_families_is_left_alone() {
        let mut b = bubble_with(vec![("trend_line", 1_000), ("trend_line", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(rule_ids(&b, 0).is_empty());
    }
}
