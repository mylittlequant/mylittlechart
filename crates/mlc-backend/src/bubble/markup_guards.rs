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
use mlc_core::drawing::primitives_v2::markup::{
    chain, ChainFact, MarkupFamily, MarkupFollows, MarkupLink,
};
use mlc_core::i18n::ConceptKey;
use mlc_core::state::markup_engine::guard_families_for;

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
/// Recompute every claim's own containment edge — which dealing range or
/// AMD cycle it sits inside, and in which of that parent's slots.
///
/// The whole edge is rebuilt from geometry on every pass, which is what
/// keeps a PERSISTED link from lying: dragging a range so it no longer
/// covers a block clears that block's link on the same gesture, and
/// dragging the block across equilibrium re-slots it from premium to
/// discount. `markup::link`'s own module doc states this contract from the
/// other side.
///
/// **Tightest parent wins.** A dealing range and an AMD cycle may cover the
/// same bars, and a child carries exactly one link, so the container with
/// the narrower timestamp span takes it — the more specific reading, and
/// the one that changes if either is redrawn. Ties break on the lower
/// primitive id so the answer does not depend on collection order.
fn refresh_markup_links(b: &mut DataBubble) {
    let bars = &b.bars;

    // Every primitive that can hold something: its own index, id, type and
    // how wide it is. Width is the tiebreak, so it is computed once here
    // rather than per candidate child.
    let containers: Vec<(usize, u64, String, i64)> = b
        .primitives
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let (from, to) = p.markup_span(bars)?;
            Some((i, p.data().id, p.type_id().to_string(), (to - from).abs()))
        })
        .collect();

    let updates: Vec<(usize, Option<MarkupLink>)> = b
        .primitives
        .iter()
        .enumerate()
        .filter_map(|(i, child)| {
            child.markup_family()?;
            let resolved = child.markup_anchor(bars).and_then(|(ts, price)| {
                let mut hits: Vec<(i64, u64, MarkupLink)> = containers
                    .iter()
                    .filter(|(j, ..)| *j != i)
                    .filter_map(|(j, id, type_id, width)| {
                        let slot = b.primitives[*j].slot_containing(bars, ts, price)?;
                        Some((*width, *id, MarkupLink::new(*id, type_id, slot)))
                    })
                    .collect();
                hits.sort_by_key(|(width, id, _)| (*width, *id));
                hits.into_iter().next().map(|(.., link)| link)
            });
            (resolved.as_ref() != child.markup_link()).then_some((i, resolved))
        })
        .collect();

    for (i, link) in updates {
        b.primitives[i].set_markup_link(link);
    }
}

/// How far back a claim of `family` may look for the claim it continues.
///
/// Per family, not one shared number: the smart-money narrative plays out
/// over swings and order flow's over a handful of bars, and each engine
/// config owns its own window for that reason. Candle patterns answer `0` —
/// a pattern is a self-contained reading of one to three bars with no
/// succession of its own, which the plan states and `chain::successor_kinds`
/// enforces by giving them no successors at all.
fn chain_window(family: MarkupFamily) -> usize {
    match family {
        MarkupFamily::SmartMoney => mlc_core::state::smc_engine::smc_engine_config().chain_window_bars,
        MarkupFamily::OrderFlow => {
            mlc_core::state::orderflow_engine::orderflow_engine_config().chain_window_bars
        }
        MarkupFamily::CandlePattern => 0,
    }
}

/// One claim as the chain pass sees it.
struct ChainSeen {
    id: u64,
    type_id: String,
    family: MarkupFamily,
    fact: ChainFact,
}

/// Recompute every claim's own succession edge, then the rules that edge
/// makes possible.
///
/// Derived, not declared — `markup::chain`'s own module doc argues why, and
/// the consequence is here: each claim links to the NEAREST earlier claim
/// its family's grammar allows, inside that family's own chain window. Ties
/// break on the lower primitive id so the answer does not depend on
/// collection order.
///
/// Because the edge is rebuilt from the grammar every pass, a chain cannot
/// contain a cycle by construction (every predecessor is strictly earlier
/// in bars). The walk still carries a visited set — a persisted edge from
/// another build reaches this code before it is overwritten, and a pass
/// that can hang on malformed stored state is not one to ship.
fn refresh_markup_chains(b: &mut DataBubble) {
    let bars = &b.bars;

    let seen: Vec<Option<ChainSeen>> = b
        .primitives
        .iter()
        .map(|p| {
            let family = p.markup_family()?;
            let fact = p.chain_fact(bars)?;
            Some(ChainSeen { id: p.data().id, type_id: p.type_id().to_string(), family, fact })
        })
        .collect();

    // 1. The edge itself.
    let mut edges: Vec<(usize, Option<MarkupFollows>)> = Vec::new();
    for (i, own) in seen.iter().enumerate() {
        let Some(own) = own else { continue };
        let window = chain_window(own.family);
        let resolved = (window > 0)
            .then(|| {
                seen.iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .filter_map(|(_, other)| other.as_ref())
                    .filter(|prev| {
                        chain::may_follow(&prev.type_id, &own.type_id)
                            && prev.fact.bar < own.fact.bar
                            && own.fact.bar - prev.fact.bar <= window
                    })
                    // Nearest first; the lower id breaks a tie.
                    .max_by_key(|prev| (prev.fact.bar, std::cmp::Reverse(prev.id)))
                    .map(|prev| MarkupFollows::new(prev.id, &prev.type_id))
            })
            .flatten();
        if resolved.as_ref() != b.primitives[i].markup_follows() {
            edges.push((i, resolved));
        }
    }
    for (i, follows) in edges {
        b.primitives[i].set_markup_follows(follows);
    }

    // 2. The rules that edge makes possible. Recomputed from the edges just
    //    written, never from what they were before this pass.
    let mut updates: Vec<(usize, Vec<Violation>)> = Vec::new();
    for (i, own) in seen.iter().enumerate() {
        let Some(own) = own else { continue };
        let Some(follows) = b.primitives[i].markup_follows().cloned() else { continue };
        let Some(prev) = seen.iter().flatten().find(|s| s.id == follows.prev_id) else { continue };

        // Every structure break earlier in this chain, nearest last — the
        // `choch_after_same_direction_break` rule compares two BREAKS, and
        // in a full narrative two breaks are two links apart.
        let mut prior_breaks: Vec<ChainFact> = Vec::new();
        let mut cursor = Some(follows.prev_id);
        let mut visited: std::collections::HashSet<u64> = std::collections::HashSet::new();
        while let Some(id) = cursor {
            if !visited.insert(id) {
                break;
            }
            let Some(node) = seen.iter().flatten().find(|s| s.id == id) else { break };
            if node.type_id == "structure_break" {
                prior_breaks.push(node.fact);
            }
            cursor = b
                .primitives
                .iter()
                .find(|p| p.data().id == id)
                .and_then(|p| p.markup_follows())
                .map(|f| f.prev_id);
        }
        prior_breaks.reverse();

        // Silenced per family, not per rule: the whole chain class is one
        // switch (`MarkupGuardFamilies::chain`), read fresh here rather than
        // threaded through `chain_violations` so the rules stay pure
        // functions of the facts.
        if !guard_families_for(own.family).chain {
            continue;
        }
        let violations =
            chain::chain_violations(&own.type_id, &own.fact, &prev.type_id, &prev.fact, &prior_breaks);
        if !violations.is_empty() {
            updates.push((i, violations));
        }
    }

    for (i, violations) in updates {
        b.primitives[i].push_context_violations(violations);
    }
}

pub(crate) fn refresh_markup_guards(b: &mut DataBubble) {
    // Containment first: it is collection state the rules below may read,
    // and rebuilding it is what keeps a persisted link honest.
    refresh_markup_links(b);
    refresh_markup_chains(b);

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
        if duplicated && guard_families_for(*family).neighbour {
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

    // ---------------------------------------------------------------
    // Containment (S2)
    // ---------------------------------------------------------------

    /// Bars at one-minute spacing from ts 1000s, each a doji at `close` so
    /// an unresolved claim's own `markup_anchor` fallback lands exactly on
    /// the price the test names.
    fn bars_at(closes: &[f64]) -> Vec<mlc_core::Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| mlc_core::Bar::new(1_000 + i as i64 * 60, *c, *c, *c, *c))
            .collect()
    }

    fn ts_ms(i: usize) -> i64 {
        (1_000 + i as i64 * 60) * 1_000
    }

    /// A bubble carrying `bars`, with each `(type_id, points)` created
    /// through the real registry — then run through the SAME two passes
    /// every call site runs, in the order this module's own contract fixes.
    fn contained_bubble(bars: Vec<mlc_core::Bar>, prims: Vec<(&str, Vec<(i64, f64)>)>) -> DataBubble {
        let mut store = crate::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), mlc_core::Timeframe::m1());
        let mut b = store.bubble(bid).cloned().expect("just-created bubble");
        b.bars = bars;
        let registry = PrimitiveRegistry::global().read().unwrap();
        for (n, (type_id, points)) in prims.into_iter().enumerate() {
            let mut prim = registry
                .create(type_id, &points, Some("#3ba3ff"))
                .unwrap_or_else(|| panic!("registry has no {type_id}"));
            // Distinct ids, for the reason `markup_weld`'s own helper
            // states: the registry leaves the id at its factory default, so
            // without this every by-id lookup finds the first primitive and
            // an assertion about WHICH neighbour was chosen proves nothing.
            prim.data_mut().id = n as u64 + 1;
            b.primitives.push(prim);
        }
        drop(registry);
        super::super::bar_derived_cache::refresh_bar_derived_caches(&mut b);
        refresh_markup_guards(&mut b);
        b
    }

    fn slot_of(b: &DataBubble, i: usize) -> Option<String> {
        b.primitives[i].markup_link().map(|l| l.slot.clone())
    }

    /// A range 100-200 over bars 0..4; the block sits on a bar closing at
    /// 180, above the 150 equilibrium.
    #[test]
    fn a_claim_in_the_upper_half_reports_premium() {
        let b = contained_bubble(
            bars_at(&[150.0, 180.0, 120.0, 150.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(4), 200.0)]),
                ("order_block", vec![(ts_ms(1), 180.0)]),
            ],
        );
        assert_eq!(slot_of(&b, 1).as_deref(), Some("premium"));
    }

    #[test]
    fn a_claim_in_the_lower_half_reports_discount() {
        let b = contained_bubble(
            bars_at(&[150.0, 180.0, 120.0, 150.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(4), 200.0)]),
                ("order_block", vec![(ts_ms(2), 120.0)]),
            ],
        );
        assert_eq!(slot_of(&b, 1).as_deref(), Some("discount"));
    }

    /// Exactly on the midpoint. Without its own token this would report no
    /// slot, which reads as "not contained" — the one wrong answer, and the
    /// reason `slots::EQUILIBRIUM` exists.
    #[test]
    fn a_claim_on_the_midpoint_is_still_inside_the_range() {
        let b = contained_bubble(
            bars_at(&[150.0, 150.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(2), 200.0)]),
                ("order_block", vec![(ts_ms(1), 150.0)]),
            ],
        );
        assert_eq!(slot_of(&b, 1).as_deref(), Some("equilibrium"));
    }

    #[test]
    fn a_claim_priced_outside_the_range_carries_no_link() {
        let b = contained_bubble(
            bars_at(&[150.0, 260.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(2), 200.0)]),
                ("order_block", vec![(ts_ms(1), 260.0)]),
            ],
        );
        assert!(b.primitives[1].markup_link().is_none());
    }

    /// The range's own render stops at its later point, so containment
    /// stops there too — see `DealingRange::slot_containing`'s own doc.
    #[test]
    fn a_claim_past_the_ranges_right_edge_carries_no_link() {
        let b = contained_bubble(
            bars_at(&[150.0, 150.0, 150.0, 180.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(1), 200.0)]),
                ("order_block", vec![(ts_ms(3), 180.0)]),
            ],
        );
        assert!(b.primitives[1].markup_link().is_none());
    }

    /// The whole reason the link may be persisted: the pass rebuilds it, so
    /// dragging the parent off the child clears a stale edge on the same
    /// gesture rather than leaving a link that outlived its geometry.
    #[test]
    fn dragging_the_parent_away_clears_the_link() {
        let mut b = contained_bubble(
            bars_at(&[150.0, 180.0, 150.0, 150.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(4), 200.0)]),
                ("order_block", vec![(ts_ms(1), 180.0)]),
            ],
        );
        assert!(b.primitives[0].markup_link().is_none(), "the range holds, it is not held");
        assert!(b.primitives[1].markup_link().is_some());

        b.primitives[0].set_points(&[(ts_ms(2), 100.0), (ts_ms(4), 200.0)]);
        super::super::bar_derived_cache::refresh_bar_derived_caches(&mut b);
        refresh_markup_guards(&mut b);
        assert!(b.primitives[1].markup_link().is_none(), "a stale edge survived the drag");
    }

    /// Re-slotting, not just clearing: the same parent, the other half.
    #[test]
    fn dragging_the_child_across_equilibrium_re_slots_it() {
        let mut b = contained_bubble(
            bars_at(&[150.0, 180.0, 120.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(2), 200.0)]),
                ("order_block", vec![(ts_ms(1), 180.0)]),
            ],
        );
        assert_eq!(slot_of(&b, 1).as_deref(), Some("premium"));

        b.primitives[1].set_points(&[(ts_ms(2), 120.0)]);
        super::super::bar_derived_cache::refresh_bar_derived_caches(&mut b);
        refresh_markup_guards(&mut b);
        assert_eq!(slot_of(&b, 1).as_deref(), Some("discount"));
    }

    /// Two ranges cover the block; the narrower one takes it.
    #[test]
    fn the_tighter_container_takes_the_child() {
        let b = contained_bubble(
            bars_at(&[150.0, 180.0, 150.0, 150.0, 150.0, 150.0, 150.0]),
            vec![
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(6), 200.0)]),
                ("dealing_range", vec![(ts_ms(0), 100.0), (ts_ms(2), 200.0)]),
                ("order_block", vec![(ts_ms(1), 180.0)]),
            ],
        );
        let parent = b.primitives[2].markup_link().map(|l| l.parent_id);
        assert_eq!(parent, Some(b.primitives[1].data().id), "the wide range won");
    }

    // ---------------------------------------------------------------
    // Succession (S3)
    // ---------------------------------------------------------------

    /// Forty flat bars — enough to put a predecessor outside the default
    /// thirty-bar window. Price is irrelevant to succession, which is about
    /// bar order and the grammar alone.
    fn flat_bars(n: usize) -> Vec<mlc_core::Bar> {
        (0..n).map(|i| mlc_core::Bar::new(1_000 + i as i64 * 60, 100.0, 100.0, 100.0, 100.0)).collect()
    }

    fn follows_of(b: &DataBubble, i: usize) -> Option<u64> {
        b.primitives[i].markup_follows().map(|f| f.prev_id)
    }

    #[test]
    fn a_break_after_a_sweep_continues_it() {
        let b = contained_bubble(
            flat_bars(40),
            vec![
                ("liquidity_sweep", vec![(ts_ms(5), 100.0)]),
                ("structure_break", vec![(ts_ms(9), 100.0)]),
            ],
        );
        assert_eq!(follows_of(&b, 1), Some(b.primitives[0].data().id));
        assert!(follows_of(&b, 0).is_none(), "the sweep opens the chain");
    }

    /// The shift is not optional: an origin claimed straight after a sweep
    /// skips the link that makes the sweep mean anything, so the grammar
    /// refuses the edge rather than inventing one.
    #[test]
    fn the_grammar_refuses_an_edge_the_setup_does_not_have() {
        let b = contained_bubble(
            flat_bars(40),
            vec![
                ("liquidity_sweep", vec![(ts_ms(5), 100.0)]),
                ("order_block", vec![(ts_ms(9), 100.0)]),
            ],
        );
        assert!(follows_of(&b, 1).is_none());
    }

    #[test]
    fn a_predecessor_outside_the_window_is_not_continued() {
        let b = contained_bubble(
            flat_bars(40),
            vec![
                ("liquidity_sweep", vec![(ts_ms(0), 100.0)]),
                ("structure_break", vec![(ts_ms(35), 100.0)]),
            ],
        );
        assert!(follows_of(&b, 1).is_none(), "35 bars apart is not one narrative");
    }

    #[test]
    fn the_nearest_legal_predecessor_wins() {
        let b = contained_bubble(
            flat_bars(40),
            vec![
                ("liquidity_sweep", vec![(ts_ms(3), 100.0)]),
                ("liquidity_sweep", vec![(ts_ms(12), 100.0)]),
                ("structure_break", vec![(ts_ms(15), 100.0)]),
            ],
        );
        assert_eq!(follows_of(&b, 2), Some(b.primitives[1].data().id));
    }

    /// The edge is rebuilt every pass, so moving a claim out of reach
    /// clears it — the same contract containment holds to.
    #[test]
    fn dragging_a_claim_out_of_the_window_clears_the_edge() {
        let mut b = contained_bubble(
            flat_bars(40),
            vec![
                ("liquidity_sweep", vec![(ts_ms(2), 100.0)]),
                ("structure_break", vec![(ts_ms(9), 100.0)]),
            ],
        );
        assert!(follows_of(&b, 1).is_some());

        b.primitives[1].set_points(&[(ts_ms(38), 100.0)]);
        super::super::bar_derived_cache::refresh_bar_derived_caches(&mut b);
        refresh_markup_guards(&mut b);
        assert!(follows_of(&b, 1).is_none(), "a stale edge survived the drag");
    }

    /// A pattern is a self-contained reading of its own bars — the plan
    /// says so, and `successor_kinds` gives the family no successors, so no
    /// window setting can make one appear.
    #[test]
    fn candle_patterns_form_no_chains() {
        let b = contained_bubble(
            flat_bars(40),
            vec![
                ("candle_pattern", vec![(ts_ms(5), 100.0)]),
                ("candle_pattern", vec![(ts_ms(9), 100.0)]),
            ],
        );
        assert!(follows_of(&b, 1).is_none());
    }

    // ---------------------------------------------------------------
    // Rule-family gating (S6)
    // ---------------------------------------------------------------

    /// Both gating tests mutate PROCESS-GLOBAL engine config, so they run
    /// under one mutex and restore what they found — the discipline
    /// `elliott::guard`'s own `guard_families` tests already keep.
    static ENGINE_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_smc_guard_families<T>(edit: impl FnOnce(&mut mlc_core::state::markup_engine::MarkupGuardFamilies), body: impl FnOnce() -> T) -> T {
        let _guard = ENGINE_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original = mlc_core::state::smc_engine::smc_engine_config();
        let mut cfg = original.clone();
        edit(&mut cfg.guard_families);
        mlc_core::state::smc_engine::set_smc_engine_config(cfg);
        let out = body();
        mlc_core::state::smc_engine::set_smc_engine_config(original);
        out
    }

    #[test]
    fn silencing_the_neighbour_class_stops_the_duplicate_rule() {
        with_smc_guard_families(|f| f.neighbour = false, || {
            let mut b = bubble_with(vec![("order_block", 1_000), ("order_block", 1_000)]);
            refresh_markup_guards(&mut b);
            assert!(!rule_ids(&b, 0).contains(&"overlapping_claim"));
        });
        // And the switch is the only reason it went quiet.
        let mut b = bubble_with(vec![("order_block", 1_000), ("order_block", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(rule_ids(&b, 0).contains(&"overlapping_claim"));
    }

    /// Silencing the chain class must leave the EDGE alone — the link is
    /// collection state a claim reports in its own Stat block, not a
    /// verdict, and a switch that turns off verdicts has no business
    /// hiding what a claim continues.
    #[test]
    fn silencing_the_chain_class_keeps_the_edge_and_drops_only_the_verdicts() {
        with_smc_guard_families(|f| f.chain = false, || {
            let b = contained_bubble(
                flat_bars(40),
                vec![
                    ("liquidity_sweep", vec![(ts_ms(5), 100.0)]),
                    ("structure_break", vec![(ts_ms(9), 100.0)]),
                ],
            );
            assert!(follows_of(&b, 1).is_some(), "the edge is not a verdict");
        });
    }

    #[test]
    fn a_drawing_outside_the_markup_families_is_left_alone() {
        let mut b = bubble_with(vec![("trend_line", 1_000), ("trend_line", 1_000)]);
        refresh_markup_guards(&mut b);
        assert!(rule_ids(&b, 0).is_empty());
    }
}
