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

/// Call [`Primitive::refresh_bar_derived_cache`] on every primitive in `b`
/// with `b`'s own current `bars`.
///
/// Cheap to call unconditionally: the trait default is a no-op, and a type
/// that DOES override it (`RegressionTrend` today) is contractually
/// required to stay cheap when nothing changed (see the trait method's own
/// doc) — this function itself does no additional filtering or
/// short-circuiting, exactly like [`super::wave_guards::refresh_wave_guards`]
/// does not either.
pub(crate) fn refresh_bar_derived_caches(b: &mut DataBubble) {
    for p in b.primitives.iter_mut() {
        p.refresh_bar_derived_cache(&b.bars);
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
