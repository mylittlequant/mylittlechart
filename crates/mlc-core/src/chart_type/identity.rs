//! Reseed last-wins merge (K0 contract, chart-type-engine-consolidation B2).
//!
//! Every fold/trade-fold machine emits a growing-element snapshot on EVERY
//! consumed upstream event (in-progress bar/brick/column/segment/session
//! walking toward close), not just on element-close — a full reseed replay
//! (`FoldRuntime::reseed`/`TradeFoldRuntime::reseed`) therefore returns many
//! consecutive same-identity outputs per real element. [`merge_last_wins`]
//! collapses those to the LAST snapshot per identity, exactly mirroring the
//! live-tick upsert rule each output shape's `append_*`/inline-upsert path
//! already applies (`mlc-backend::lib`'s `fold_on_live_1m` /
//! `trade_fold_on_live_trade`). Was hand-written per reseed arm before this
//! (`apply_fold_reseed_outputs` / `apply_trade_fold_reseed_outputs`) — one
//! shared generic + a small per-shape key fn replaces all of them.

use super::registry::IdentityKey;
use crate::types::{
    CvdSample, FootprintBar, KagiSegment, PnfColumn, RenkoBrick, ThreeLineBreakLine, TpoSession,
};
use crate::Bar;

/// Collapse consecutive same-identity items (by `key`) down to the LAST one,
/// preserving oldest-first order — the shape every reseed output arrives in.
/// `out` accumulates the result; call once per full reseed replay with an
/// empty `out`, or reuse across multiple output batches (each batch's first
/// item is compared against `out`'s current last element, so back-to-back
/// calls compose correctly).
pub fn merge_last_wins<T>(out: &mut Vec<T>, item: T, key: fn(&T) -> i64) {
    match out.last_mut() {
        Some(last) if key(last) == key(&item) => *last = item,
        _ => out.push(item),
    }
}

/// [`Bar`] identity — `derived_ts_ms` when non-zero, else `timestamp`. Used
/// by every counted-bar family (range/volume/dollar/tick/tick_imbalance/
/// volume_imbalance/run) — [`IdentityKey::DerivedTsMsOrTimestamp`].
pub fn bar_identity(b: &Bar) -> i64 {
    if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
}

/// [`RenkoBrick`] identity — `open_time`. [`IdentityKey::OpenTime`].
pub fn renko_identity(b: &RenkoBrick) -> i64 {
    b.open_time
}

/// [`PnfColumn`] identity — `open_time`. [`IdentityKey::OpenTime`].
pub fn pnf_identity(c: &PnfColumn) -> i64 {
    c.open_time
}

/// [`KagiSegment`] identity — `open_time`. [`IdentityKey::OpenTime`].
pub fn kagi_identity(s: &KagiSegment) -> i64 {
    s.open_time
}

/// [`ThreeLineBreakLine`] identity — `ts_open`. [`IdentityKey::TsOpen`].
pub fn three_line_break_identity(l: &ThreeLineBreakLine) -> i64 {
    l.ts_open
}

/// [`FootprintBar`] identity — `open_time`. [`IdentityKey::OpenTime`].
pub fn footprint_identity(b: &FootprintBar) -> i64 {
    b.open_time
}

/// [`TpoSession`] identity — `open_time`. [`IdentityKey::OpenTime`].
pub fn tpo_identity(s: &TpoSession) -> i64 {
    s.open_time
}

/// [`CvdSample`] identity — `ts_ms`. [`IdentityKey::TsMs`].
pub fn cvd_identity(s: &CvdSample) -> i64 {
    s.ts_ms
}

/// Resolve a registry [`IdentityKey`] to the [`bar_identity`] key fn — the
/// only shape that needs a *runtime* dispatch (every other output shape has
/// exactly one identity rule, so its caller calls the shape's key fn
/// directly). `None` for [`IdentityKey::None`]/any non-`Bar` key — callers
/// match on `IdentityKey` themselves when the output isn't `Bar`-shaped.
pub fn bar_identity_key_fn(key: IdentityKey) -> Option<fn(&Bar) -> i64> {
    match key {
        IdentityKey::DerivedTsMsOrTimestamp => Some(bar_identity),
        IdentityKey::None | IdentityKey::OpenTime | IdentityKey::TsOpen | IdentityKey::TsMs => None,
    }
}

// ── Per-shape `key_fn_for` resolvers (chart-type-engine-consolidation B3,
// S6) — one per reseed output shape, each mirroring `bar_identity_key_fn`'s
// pattern: resolve a registry-read `IdentityKey` to the shape's own key fn.
// Reseed arms (`mlc-backend::lib::apply_fold_reseed_outputs`/
// `apply_trade_fold_reseed_outputs`) call `by_id(chart_type).identity` then
// one of these instead of hardcoding "renko → renko_identity" as tribal
// per-arm knowledge — the identity RULE is now registry-driven even though
// the output-shape match (which field the fold machine actually produced)
// necessarily stays (that's data shape, not policy).

/// Resolve `key` to the [`renko_identity`] key fn. `None` for any key other
/// than [`IdentityKey::OpenTime`] (a registry/caller mismatch).
pub fn renko_identity_key_fn(key: IdentityKey) -> Option<fn(&RenkoBrick) -> i64> {
    matches!(key, IdentityKey::OpenTime).then_some(renko_identity)
}

/// Resolve `key` to the [`pnf_identity`] key fn. `None` for any key other
/// than [`IdentityKey::OpenTime`].
pub fn pnf_identity_key_fn(key: IdentityKey) -> Option<fn(&PnfColumn) -> i64> {
    matches!(key, IdentityKey::OpenTime).then_some(pnf_identity)
}

/// Resolve `key` to the [`kagi_identity`] key fn. `None` for any key other
/// than [`IdentityKey::OpenTime`].
pub fn kagi_identity_key_fn(key: IdentityKey) -> Option<fn(&KagiSegment) -> i64> {
    matches!(key, IdentityKey::OpenTime).then_some(kagi_identity)
}

/// Resolve `key` to the [`three_line_break_identity`] key fn. `None` for any
/// key other than [`IdentityKey::TsOpen`].
pub fn three_line_break_identity_key_fn(key: IdentityKey) -> Option<fn(&ThreeLineBreakLine) -> i64> {
    matches!(key, IdentityKey::TsOpen).then_some(three_line_break_identity)
}

/// Resolve `key` to the [`footprint_identity`] key fn. `None` for any key
/// other than [`IdentityKey::OpenTime`].
pub fn footprint_identity_key_fn(key: IdentityKey) -> Option<fn(&FootprintBar) -> i64> {
    matches!(key, IdentityKey::OpenTime).then_some(footprint_identity)
}

/// Resolve `key` to the [`tpo_identity`] key fn. `None` for any key other
/// than [`IdentityKey::OpenTime`].
pub fn tpo_identity_key_fn(key: IdentityKey) -> Option<fn(&TpoSession) -> i64> {
    matches!(key, IdentityKey::OpenTime).then_some(tpo_identity)
}

/// Resolve `key` to the [`cvd_identity`] key fn. `None` for any key other
/// than [`IdentityKey::TsMs`]. `"cvd"` is not a registry row (see
/// `view_fold.rs`'s doc comment on the synthetic id) — its caller passes
/// `IdentityKey::TsMs` directly rather than reading it from a `by_id` lookup.
pub fn cvd_identity_key_fn(key: IdentityKey) -> Option<fn(&CvdSample) -> i64> {
    matches!(key, IdentityKey::TsMs).then_some(cvd_identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_last_wins_collapses_same_identity_bars() {
        let mut out: Vec<Bar> = Vec::new();
        let a = Bar { timestamp: 1, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0, derived_ts_ms: 0 };
        let a2 = Bar { timestamp: 1, open: 1.0, high: 2.0, low: 1.0, close: 1.5, volume: 2.0, derived_ts_ms: 0 };
        let b = Bar { timestamp: 2, open: 1.5, high: 1.5, low: 1.5, close: 1.5, volume: 1.0, derived_ts_ms: 0 };
        merge_last_wins(&mut out, a, bar_identity);
        merge_last_wins(&mut out, a2, bar_identity);
        merge_last_wins(&mut out, b, bar_identity);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].close, 1.5);
        assert_eq!(out[0].volume, 2.0);
        assert_eq!(out[1].timestamp, 2);
    }

    #[test]
    fn merge_last_wins_prefers_derived_ts_ms_over_timestamp() {
        let mut out: Vec<Bar> = Vec::new();
        let a = Bar { timestamp: 100, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0, derived_ts_ms: 5000 };
        let a2 = Bar { timestamp: 200, open: 1.0, high: 1.0, low: 1.0, close: 2.0, volume: 1.0, derived_ts_ms: 5000 };
        merge_last_wins(&mut out, a, bar_identity);
        merge_last_wins(&mut out, a2, bar_identity);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].close, 2.0);
    }

    #[test]
    fn merge_last_wins_empty_input_stays_empty() {
        let out: Vec<Bar> = Vec::new();
        assert!(out.is_empty());
    }

    #[test]
    fn bar_identity_key_fn_resolves_derived_ts_ms_or_timestamp_only() {
        assert!(bar_identity_key_fn(IdentityKey::DerivedTsMsOrTimestamp).is_some());
        assert!(bar_identity_key_fn(IdentityKey::None).is_none());
        assert!(bar_identity_key_fn(IdentityKey::OpenTime).is_none());
        assert!(bar_identity_key_fn(IdentityKey::TsOpen).is_none());
        assert!(bar_identity_key_fn(IdentityKey::TsMs).is_none());
    }
}
