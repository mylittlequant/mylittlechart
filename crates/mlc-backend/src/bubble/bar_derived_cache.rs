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
