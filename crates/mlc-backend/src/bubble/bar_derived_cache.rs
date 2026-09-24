//! Bars-derived cache refresh — recomputes whatever each primitive derives
//! from OHLC bars into its own `#[serde(skip)]` cache field(s). See
//! [`mlc_core::drawing::primitives_v2::Primitive::refresh_bar_derived_cache`]
//! for the trait contract this fills.
//!
//! # A separate concern from [`super::wave_guards`], even where the two end
//! up called from the same places
//!
//! Both are bubble-scoped free functions taking `&mut DataBubble` rather
//! than a `DrawingManager` method, for the same underlying reason
//! `wave_guards`'s own module doc gives: `DrawingManager` never owns
//! `bars`, `BubbleStore`/`DataBubble` do, so a caller that already holds
//! `&mut BubbleStore` + a `BubbleId` (or `&mut DataBubble` directly, at
//! `BubbleStore`'s own write choke points) is the natural convergence
//! point.
//!
//! That is where the similarity ends. `wave_guards::refresh_wave_guards`
//! filters to `p.wave_degree().is_some()` and reads every OTHER primitive
//! in the bubble (`violations_in`/`chain_violations` need every sibling at
//! once) but never touches `bars` — Elliott's violations come from sibling
//! PRIMITIVES, not price data. This pass is the mirror image: it walks
//! EVERY primitive (no family filter — the default hook is a no-op, so an
//! unrelated type pays only the cost of one empty vtable call) and hands
//! each one only `bars`, never its siblings. Neither pass could stand in
//! for the other, and this one does not widen `wave_guards` to also do
//! this — see this crate's own `refresh_bar_derived_caches` below.

use super::data_bubble::DataBubble;
use mlc_core::drawing::alloc_primitive_id;

/// Call [`Primitive::refresh_bar_derived_cache`] on every primitive in `b`
/// with `b`'s own current `bars`, then drain each primitive's own
/// [`Primitive::take_scan_placements`] queue — the "place as primitive"
/// mechanism (`docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`,
/// "New mechanism") — into real, independent primitives pushed onto this
/// SAME bubble's collection, immediately after the cache refresh a
/// placement's own geometry depends on.
///
/// Cheap to call unconditionally: the trait default is a no-op, and a type
/// that DOES override it (`RegressionTrend` today) is contractually
/// required to stay cheap when nothing changed (see the trait method's own
/// doc) — this function itself does no additional filtering or
/// short-circuiting, exactly like [`super::wave_guards::refresh_wave_guards`]
/// does not either. `take_scan_placements` is default-empty, so a primitive
/// that does not override it (the overwhelming majority) pays one empty-Vec
/// call and nothing more.
pub(crate) fn refresh_bar_derived_caches(b: &mut DataBubble) {
    let mut placements: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> = Vec::new();
    for p in b.primitives.iter_mut() {
        p.refresh_bar_derived_cache(&b.bars);
        placements.extend(p.take_scan_placements(&b.bars));
    }
    if placements.is_empty() {
        return;
    }
    let symbol = b.address.symbol.clone();
    for mut placed in placements {
        // Mirrors `DrawingManager::create_proposed_chains`'s own insertion
        // — stamp a fresh id + this bubble's symbol, push, nothing else (no
        // auto-select, no auto-delete of the scan that produced it).
        placed.data_mut().id = alloc_primitive_id();
        placed.data_mut().symbol = symbol.clone();
        b.primitives.push(placed);
    }
}

/// The footprint counterpart of [`refresh_bar_derived_caches`] — see
/// [`Primitive::refresh_footprint_derived_cache`]'s own contract
/// (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix B: "a second
/// hook ... called from the SAME sweep ... when the slot is `Some`").
///
/// Runs ONLY when `b.footprint_bars` is `Some` — there is nothing to call
/// [`Primitive::refresh_footprint_derived_cache`] with when it is `None`
/// (an empty slice would be ambiguous with "subscribed, zero buckets so far
/// yet"; see that trait method's own doc). A type that does not override
/// the hook pays one empty vtable call per primitive, same as
/// [`refresh_bar_derived_caches`] costs for the OHLC hook.
///
/// Call sites (measured, not assumed — every production write to
/// `DataBubble::footprint_bars` today): the three BATCH installs in
/// `chart_type::lifecycle` (`apply_trade_fold_reseed_outputs`'s
/// `"footprint"` arm, `splice_trade_fold_gap`, `incremental_prepend_trade_folds`)
/// call this unconditionally after installing the new vec — the same
/// "batch bar-array install" trigger `bar_derived_cache`'s own OHLC sweep
/// fires on for `replace_bars`/`set_bars`. The LIVE per-trade path
/// (`trade_fold_on_live_trade`'s `Footprint` arm) calls this only on the
/// branch that PUSHES a new bucket (a footprint bucket boundary — once per
/// interval, e.g. once a minute for a 1m footprint), never on the branch
/// that upserts the CURRENTLY FORMING bucket in place (once per trade) —
/// mirroring why `BubbleStore::append_bar` is deliberately NOT hooked to
/// the OHLC sweep at all (see that method's own doc): the per-trade branch
/// is the "many times a second" case the plan's own Appendix B names as the
/// one this cost model cannot absorb, where a once-per-interval bucket
/// boundary can. `DataBubble::clear_footprint` calls this too (a no-op
/// today — see that method's own doc for why it currently has no
/// production caller).
pub(crate) fn refresh_footprint_derived_caches(b: &mut DataBubble) {
    let Some(footprint) = b.footprint_bars.as_deref() else { return };
    for p in b.primitives.iter_mut() {
        p.refresh_footprint_derived_cache(footprint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::drawing::primitives_v2::{
        ClickBehavior, ControlPoint, ControlPointType, Primitive, PrimitiveData, PrimitiveKind,
        PrimitiveRegistry,
    };
    use mlc_core::{Bar, PriceScale, Viewport};

    /// A primitive whose `take_scan_placements` override fires exactly
    /// once — proof the sweep both drains a real override (S1) and leaves
    /// every default-empty primitive (every OTHER primitive in the crate,
    /// verified by the second test below with a real registry type) alone.
    #[derive(Clone)]
    struct OneShotPlacer {
        data: PrimitiveData,
        armed: bool,
    }

    impl Primitive for OneShotPlacer {
        fn type_id(&self) -> &'static str {
            "test_one_shot_placer"
        }
        fn display_name(&self) -> &str {
            "One-Shot Placer (test)"
        }
        fn kind(&self) -> PrimitiveKind {
            PrimitiveKind::Line
        }
        fn click_behavior(&self) -> ClickBehavior {
            ClickBehavior::TwoPoint
        }
        fn data(&self) -> &PrimitiveData {
            &self.data
        }
        fn data_mut(&mut self) -> &mut PrimitiveData {
            &mut self.data
        }
        fn points(&self) -> Vec<(i64, f64)> {
            Vec::new()
        }
        fn set_points(&mut self, _points: &[(i64, f64)]) {}
        fn translate(&mut self, _ts_delta_ms: i64, _price_delta: f64) {}
        fn move_control_point(&mut self, _point_type: ControlPointType, _ts_ms: i64, _price: f64) {}
        fn control_points(
            &self,
            _bars: &[Bar],
            _viewport: &Viewport,
            _price_scale: &PriceScale,
        ) -> Vec<ControlPoint> {
            Vec::new()
        }
        fn to_json(&self) -> String {
            "{}".to_string()
        }
        fn clone_box(&self) -> Box<dyn Primitive> {
            Box::new(self.clone())
        }
        fn take_scan_placements(&mut self, _bars: &[Bar]) -> Vec<Box<dyn Primitive>> {
            if !self.armed {
                return Vec::new();
            }
            self.armed = false;
            vec![Box::new(OneShotPlacer { data: PrimitiveData::default(), armed: false })]
        }
    }

    fn bubble_with(prims: Vec<Box<dyn Primitive>>) -> DataBubble {
        let mut store = crate::bubble::BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), mlc_core::Timeframe::m1());
        let mut b = store.bubble(bid).cloned().expect("just-created bubble");
        b.primitives = prims;
        b
    }

    #[test]
    fn take_scan_placements_grows_the_bubble_by_exactly_one() {
        let armed: Box<dyn Primitive> =
            Box::new(OneShotPlacer { data: PrimitiveData::default(), armed: true });
        let mut b = bubble_with(vec![armed]);
        refresh_bar_derived_caches(&mut b);
        assert_eq!(b.primitives.len(), 2, "one placed primitive inserted alongside the scan");

        // Draining again finds nothing left to place — the queue does not
        // refill itself.
        refresh_bar_derived_caches(&mut b);
        assert_eq!(b.primitives.len(), 2, "one-shot queue stays drained");
    }

    #[test]
    fn a_primitive_not_overriding_take_scan_placements_never_adds_anything() {
        let prim = {
            let registry = PrimitiveRegistry::global().read().unwrap();
            registry
                .create("horizontal_line", &[(0, 1.0)], Some("#3ba3ff"))
                .expect("registry has horizontal_line")
        };
        let mut b = bubble_with(vec![prim]);
        refresh_bar_derived_caches(&mut b);
        assert_eq!(b.primitives.len(), 1, "default take_scan_placements adds nothing");
    }
}
