//! Welding a freshly placed container claim onto the claim it is about.
//!
//! # Why this is a creation-time move and not a guard pass
//!
//! [`super::markup_guards`] runs on every gesture and rebuilds derived
//! state — links, chains, violations. This does none of that: it MOVES a
//! primitive's own anchor, which is the analyst's geometry, not derived
//! state. Running that on every refresh would fight a drag, re-snapping the
//! claim back the instant it was pulled away. So it runs exactly where
//! `Primitive::snap_to_detected` runs — once, when the claim is created.
//! Elliott draws the same line for the same reason (`restitch_wave_joints`
//! is a drag-time operation, not a refresh-time one).
//!
//! # Order against the detector snap
//!
//! Weld FIRST, then let `snap_to_detected` run as it always has.
//!
//! The two compose correctly in that order and only in that order. If the
//! welded bar resolves, the detector snap sees a resolved claim and does
//! nothing. If it does not, the snap searches outward from the welded bar
//! and — since both radii are the same `claim_snap_bars` — can reach back
//! to where the analyst originally clicked. So a weld that would make the
//! claim worse undoes itself, and no restore bookkeeping is needed to get
//! that.

use mlc_core::drawing::primitives_v2::markup;
use mlc_core::state::smc_engine::smc_engine_config;

use super::data_bubble::DataBubble;

/// The bar a primitive's own first point names, or `None` when that point
/// falls outside the loaded series.
fn anchor_bar(b: &DataBubble, idx: usize) -> Option<usize> {
    let &(ts, _) = b.primitives.get(idx)?.points().first()?;
    mlc_core::find_bar_for_timestamp_ms(&b.bars, ts)
}

/// Anchor the just-created primitive `id` onto the sibling claim it is
/// about, when one is already drawn within `claim_snap_bars`.
///
/// Returns whether anything moved. No-op for every type
/// [`markup::adopt::welds_onto`] answers `None` for — which is every type
/// but `amd_cycle` today, and deliberately so (that table's own doc).
pub(crate) fn weld_markup_claim(b: &mut DataBubble, id: u64) -> bool {
    let radius = smc_engine_config().claim_snap_bars;
    if radius == 0 {
        return false;
    }

    let Some(idx) = b.primitives.iter().position(|p| p.data().id == id) else { return false };
    let Some(target_type) = markup::adopt::welds_onto(b.primitives[idx].type_id()) else { return false };
    // The claim's own FIRST POINT, not `markup_anchor`: welding is about
    // where the analyst put the claim, and `markup_anchor` answers a
    // different question (the price a container should test for
    // containment) that a container itself has no reason to answer.
    let Some(own_bar) = anchor_bar(b, idx) else { return false };

    // The nearest already-drawn claim of that type, inside the radius. Ties
    // break on the lower primitive id so two siblings equidistant on either
    // side resolve the same way every time rather than by list order.
    let mut candidates: Vec<(usize, u64, i64)> = Vec::new();
    for (j, other) in b.primitives.iter().enumerate() {
        if j == idx || other.type_id() != target_type {
            continue;
        }
        let Some(&(ts, _)) = other.points().first() else { continue };
        let Some(bar) = anchor_bar(b, j) else { continue };
        let distance = bar.abs_diff(own_bar);
        if distance == 0 || distance > radius {
            continue;
        }
        candidates.push((distance, other.data().id, ts));
    }
    candidates.sort_by_key(|(distance, other_id, _)| (*distance, *other_id));
    let Some(&(_, _, target_ts)) = candidates.first() else { return false };

    let points = b.primitives[idx].points();
    let Some(&(_, price)) = points.first() else { return false };
    b.primitives[idx].set_points(&[(target_ts, price)]);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::drawing::primitives_v2::PrimitiveRegistry;

    fn bars_at(n: usize) -> Vec<mlc_core::Bar> {
        (0..n).map(|i| mlc_core::Bar::new(1_000 + i as i64 * 60, 100.0, 100.0, 100.0, 100.0)).collect()
    }

    fn ts_ms(i: usize) -> i64 {
        (1_000 + i as i64 * 60) * 1_000
    }

    fn bubble_with(prims: Vec<(&str, i64)>) -> (DataBubble, Vec<u64>) {
        let mut store = crate::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), mlc_core::Timeframe::m1());
        let mut b = store.bubble(bid).cloned().expect("just-created bubble");
        b.bars = bars_at(40);
        let registry = PrimitiveRegistry::global().read().unwrap();
        let mut ids = Vec::new();
        for (n, (type_id, ts)) in prims.into_iter().enumerate() {
            let mut prim = registry
                .create(type_id, &[(ts, 100.0)], Some("#3ba3ff"))
                .unwrap_or_else(|| panic!("registry has no {type_id}"));
            // The registry leaves the id at its factory default; ids are
            // allocated by `finalize_new_primitive`, which these tests do
            // not go through. Without distinct ids every lookup by id finds
            // the first primitive and the assertions below prove nothing.
            prim.data_mut().id = n as u64 + 1;
            ids.push(prim.data().id);
            b.primitives.push(prim);
        }
        drop(registry);
        super::super::bar_derived_cache::refresh_bar_derived_caches(&mut b);
        (b, ids)
    }

    fn anchor_ts(b: &DataBubble, id: u64) -> i64 {
        b.primitives
            .iter()
            .find(|p| p.data().id == id)
            .and_then(|p| p.points().first().map(|(ts, _)| *ts))
            .expect("primitive present")
    }

    #[test]
    fn a_cycle_placed_beside_a_sweep_anchors_on_it() {
        let (mut b, ids) = bubble_with(vec![("liquidity_sweep", ts_ms(10)), ("amd_cycle", ts_ms(12))]);
        assert!(weld_markup_claim(&mut b, ids[1]));
        assert_eq!(anchor_ts(&b, ids[1]), ts_ms(10), "the cycle must name the sweep's own bar");
        assert_eq!(anchor_ts(&b, ids[0]), ts_ms(10), "the sweep is the subject, it does not move");
    }

    #[test]
    fn a_sweep_out_of_reach_is_left_alone() {
        let (mut b, ids) = bubble_with(vec![("liquidity_sweep", ts_ms(10)), ("amd_cycle", ts_ms(30))]);
        assert!(!weld_markup_claim(&mut b, ids[1]));
        assert_eq!(anchor_ts(&b, ids[1]), ts_ms(30));
    }

    #[test]
    fn the_nearest_sweep_wins() {
        let (mut b, ids) = bubble_with(vec![
            ("liquidity_sweep", ts_ms(8)),
            ("liquidity_sweep", ts_ms(11)),
            ("amd_cycle", ts_ms(12)),
        ]);
        assert!(weld_markup_claim(&mut b, ids[2]));
        assert_eq!(anchor_ts(&b, ids[2]), ts_ms(11));
    }

    /// Nearby is not the same event — nothing but the cycle welds, so an
    /// order block dropped beside a gap stays exactly where it was put.
    #[test]
    fn a_type_that_welds_onto_nothing_never_moves() {
        let (mut b, ids) = bubble_with(vec![("fair_value_gap", ts_ms(10)), ("order_block", ts_ms(12))]);
        assert!(!weld_markup_claim(&mut b, ids[1]));
        assert_eq!(anchor_ts(&b, ids[1]), ts_ms(12));
    }

    #[test]
    fn a_cycle_already_on_the_sweeps_bar_does_not_move() {
        let (mut b, ids) = bubble_with(vec![("liquidity_sweep", ts_ms(10)), ("amd_cycle", ts_ms(10))]);
        assert!(!weld_markup_claim(&mut b, ids[1]), "a zero-bar move is not a weld");
        assert_eq!(anchor_ts(&b, ids[1]), ts_ms(10));
    }
}
