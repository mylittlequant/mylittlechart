//! Client-side kline aggregation for non-native timeframes (granularity arc
//! defect fix 2026-07-16).
//!
//! Every venue's `map_kline_interval` in `digdigdig3` silently falls back to
//! some default native interval (e.g. Bybit `_ => "60"`) when asked for an
//! interval it does not natively serve — so a request for `"12m"` silently
//! returns `"1h"` bars mislabeled as `"12m"` (log-proven: 300 bars spanning
//! 1,076,400s = 3588s/bar). `digdigdig3` must not be patched (its version
//! only moves at publish) — the bridge instead detects non-native targets
//! BEFORE dialing dig3, fetches the largest native divisor interval, and
//! folds the results into honest target-TF buckets client-side.
//!
//! Two pure, fully unit-tested primitives:
//! - [`aggregation_base`] — resolve which native interval (if any) a
//!   non-native target should be built from.
//! - [`fold_bars_into_buckets`] — fold a sorted slice of base-interval bars
//!   into target-interval buckets (epoch-aligned, OHLCV cumulative rules).
//!
//! Both are pure (no I/O, no async) so `cargo test -p
//! mlc-backend-data-bridge --lib` exercises them directly.

use mlc_core::Bar;

/// Resolved aggregation plan for one non-native target TF — the TYPED
/// return of `DataBridge::resolve_kline_aggregation` (was a positional
/// `(String, i64, i64)` tuple; the 2026-07-16 runaway-ramp live bug came
/// from a caller destructuring the middle field as the TARGET duration
/// when it was the BASE duration — a named struct makes that class of
/// mistake unrepresentable).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KlineAggregation {
    /// Canonical dig3 interval string to actually fetch/subscribe (e.g. "1m").
    pub base_interval: String,
    /// The base interval's duration in ms.
    pub base_ms: i64,
    /// Fan-in: how many base bars fold into one target bucket.
    pub ratio: i64,
}

impl KlineAggregation {
    /// The TARGET bucket duration in ms — the value every fold call must use.
    pub fn target_ms(&self) -> i64 {
        self.base_ms.saturating_mul(self.ratio)
    }
}

/// Ratio ceiling for treating a native interval as a usable aggregation
/// base. Above this, the fan-in (number of base bars folded into one
/// target bucket) is judged impractical — the caller falls back to the
/// existing no-divisor behavior (live trade aggregation, no REST history)
/// exactly as if no native divisor existed at all.
pub const MAX_AGGREGATION_RATIO: i64 = 120;

/// Resolve the aggregation BASE for a `target_ms`-duration timeframe that
/// the active venue does NOT natively serve.
///
/// `native` is the venue's own natively-served interval set as `(label,
/// duration_ms)` pairs — e.g. `[("1m", 60_000), ("5m", 300_000), ("1h",
/// 3_600_000)]`. Returns the LARGEST native interval whose duration evenly
/// divides `target_ms` with a ratio `2..=`[`MAX_AGGREGATION_RATIO`],
/// paired with that ratio (`target_ms / base_ms`) — the caller uses the
/// label to fetch base bars and the ratio to size the fetch (page count ×
/// ratio) and pre-size the fold.
///
/// Returns `None` when no native interval divides `target_ms` evenly
/// within the ratio window (e.g. a venue whose smallest native interval
/// is `"1m"` asked for `"45s"` — no integer number of 1m bars folds into
/// 45s) — callers must fall back to the existing live-only behavior for
/// that TF, unchanged.
///
/// Callers are expected to check nativeness FIRST (skip calling this
/// entirely when `target_ms` is itself in `native`) — this function does
/// not special-case an exact match; if `native` contains an entry equal
/// to `target_ms` its ratio is 1, which fails the `>= 2` floor below and
/// correctly returns `None` (an exact match is "native", not
/// "aggregated" — the caller's own nativeness check is the source of
/// truth for that distinction).
pub fn aggregation_base<'a>(
    target_ms: i64,
    native: &[(&'a str, i64)],
) -> Option<(&'a str, i64)> {
    if target_ms <= 0 {
        return None;
    }
    native
        .iter()
        .filter(|&&(_, base_ms)| base_ms > 0 && target_ms % base_ms == 0)
        .map(|&(label, base_ms)| (label, target_ms / base_ms))
        .filter(|&(_, ratio)| (2..=MAX_AGGREGATION_RATIO).contains(&ratio))
        // Largest base = smallest ratio. Tie-break on base_ms (unreachable
        // in practice — a venue's native set never repeats a duration
        // under two different labels — but keeps the choice deterministic).
        .max_by_key(|&(label, ratio)| {
            let base_ms = target_ms / ratio.max(1);
            (base_ms, std::cmp::Reverse(label))
        })
        .map(|(label, ratio)| (label, ratio))
}

/// Epoch-aligned bucket boundary (ms) containing `ts_ms` for a
/// `target_ms`-duration bucket grid — `ts_ms - ts_ms.rem_euclid(target_ms)`.
/// DST-agnostic (pure integer arithmetic on UTC epoch ms, no calendar/zone
/// involved) and stable for negative `ts_ms` (not expected in practice,
/// but `rem_euclid` keeps the floor direction correct either way).
pub fn bucket_start_ms(ts_ms: i64, target_ms: i64) -> i64 {
    if target_ms <= 0 {
        return ts_ms;
    }
    ts_ms - ts_ms.rem_euclid(target_ms)
}

/// Fold a slice of base-interval bars (sorted oldest→newest, as every dig3
/// kline fetch already returns them) into target-interval buckets.
///
/// Bucketing is keyed by each base bar's `derived_ts_ms` (the verbatim
/// upstream millisecond open-time — see `mlc_core::Bar::derived_ts_ms` doc
/// comment) rather than the lossy seconds `timestamp`, so bucket identity
/// never collides across bars that share the same wall-clock second.
///
/// Per bucket: `open` = first base bar's open, `close` = last base bar's
/// close, `high`/`low` = max/min across the bucket's base bars, `volume` =
/// sum. The output bucket's `timestamp`/`derived_ts_ms` are the bucket's
/// own epoch-aligned start (NOT copied from any input bar) — this is what
/// makes the folded series honest target-TF bars indistinguishable from a
/// native fetch.
///
/// The trailing bucket is included even if it is only partially filled
/// (fewer than `ratio` base bars) — that partial bucket IS the live
/// in-progress target bar, which downstream (kline layer / viewport / live
/// forwarder) already expects to see and update in place.
///
/// `target_ms <= 0` or an empty `base_bars` returns an empty `Vec`.
pub fn fold_bars_into_buckets(base_bars: &[Bar], target_ms: i64) -> Vec<Bar> {
    if target_ms <= 0 || base_bars.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<Bar> = Vec::new();
    let mut current_bucket_start: Option<i64> = None;

    for bar in base_bars {
        let bucket_start = bucket_start_ms(bar.derived_ts_ms, target_ms);
        let start_new_bucket = match current_bucket_start {
            Some(cur) => bucket_start != cur,
            None => true,
        };

        if start_new_bucket {
            out.push(Bar {
                timestamp: bucket_start / 1000,
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                volume: bar.volume,
                derived_ts_ms: bucket_start,
            });
            current_bucket_start = Some(bucket_start);
        } else if let Some(last) = out.last_mut() {
            last.high = last.high.max(bar.high);
            last.low = last.low.min(bar.low);
            last.close = bar.close;
            last.volume += bar.volume;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    //! Pure unit tests — no network, no runtime. Covers divisor choice
    //! (incl. ratio cap + native-set edge cases) and bucket fold (incl.
    //! alignment, partial tail, volume sum, DST-agnostic epoch alignment).

    use super::*;

    /// Regression pin for the 2026-07-16 runaway-ramp live bug: a caller
    /// folding with the plan's BASE duration instead of `target_ms()`
    /// produces zero folding (1000 base bars → 1000 "target" bars). The
    /// typed struct exists so the only ergonomic duration to reach for is
    /// the right one — pin its arithmetic here.
    #[test]
    fn plan_target_ms_is_base_times_ratio() {
        let plan = KlineAggregation {
            base_interval: "1m".to_string(),
            base_ms: 60_000,
            ratio: 7,
        };
        assert_eq!(plan.target_ms(), 420_000);
        // Folding at target_ms() actually folds; folding at base_ms would not.
        let bars: Vec<Bar> = (0..14).map(|i| bar(i * 60_000, 1.0, 2.0, 0.5, 1.5, 1.0)).collect();
        assert_eq!(fold_bars_into_buckets(&bars, plan.target_ms()).len(), 2);
        assert_eq!(fold_bars_into_buckets(&bars, plan.base_ms).len(), 14);
    }

    fn bar(derived_ts_ms: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
        Bar {
            timestamp: derived_ts_ms / 1000,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: v,
            derived_ts_ms,
        }
    }

    // ── aggregation_base ─────────────────────────────────────────────────

    #[test]
    fn picks_largest_divisor_1m_5m_1h_for_12m_target() {
        // 12m target: divisible by 1m (ratio 12) and... not by 5m (12 % 5 != 0),
        // not by 1h (12 < 60). Only 1m divides.
        let native = [("1m", 60_000), ("5m", 300_000), ("1h", 3_600_000)];
        let target_ms = 12 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("1m", 12)));
    }

    #[test]
    fn picks_largest_divisor_when_multiple_qualify() {
        // 90m target: divisible by 1m (ratio 90), 5m (ratio 18), 30m (ratio 3).
        // Largest base = 30m.
        let native = [("1m", 60_000), ("5m", 300_000), ("30m", 1_800_000), ("1h", 3_600_000)];
        let target_ms = 90 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("30m", 3)));
    }

    #[test]
    fn seven_minute_target_bases_on_1m() {
        let native = [("1m", 60_000), ("5m", 300_000), ("1h", 3_600_000)];
        let target_ms = 7 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("1m", 7)));
    }

    #[test]
    fn two_day_target_bases_on_1d() {
        let native = [("1h", 3_600_000), ("4h", 14_400_000), ("1d", 86_400_000)];
        let target_ms = 2 * 86_400_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("1d", 2)));
    }

    #[test]
    fn no_native_divisor_returns_none() {
        // Smallest native is 1m; 45s does not divide evenly into any
        // multiple of 1m (target < base, and non-integer ratio either way).
        let native = [("1m", 60_000), ("5m", 300_000)];
        let target_ms = 45_000;
        assert_eq!(aggregation_base(target_ms, &native), None);
    }

    #[test]
    fn empty_native_set_returns_none() {
        let native: [(&str, i64); 0] = [];
        assert_eq!(aggregation_base(12 * 60_000, &native), None);
    }

    #[test]
    fn exact_native_match_returns_none_ratio_one_excluded() {
        // Caller is expected to check nativeness first; this defends the
        // documented fallback behavior when they don't — ratio 1 is
        // filtered by the `>= 2` floor.
        let native = [("1m", 60_000), ("5m", 300_000)];
        assert_eq!(aggregation_base(60_000, &native), None);
    }

    #[test]
    fn ratio_above_cap_is_rejected() {
        // Only native interval is 1m; target is 121m (ratio 121 > 120 cap)
        // — must return None, not silently aggregate a huge fan-in.
        let native = [("1m", 60_000)];
        let target_ms = 121 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), None);
    }

    #[test]
    fn ratio_at_cap_boundary_is_accepted() {
        let native = [("1m", 60_000)];
        let target_ms = 120 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("1m", 120)));
    }

    #[test]
    fn ratio_just_above_cap_boundary_is_rejected() {
        let native = [("1m", 60_000)];
        let target_ms = 121 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), None);
    }

    #[test]
    fn zero_or_negative_target_returns_none() {
        let native = [("1m", 60_000)];
        assert_eq!(aggregation_base(0, &native), None);
        assert_eq!(aggregation_base(-60_000, &native), None);
    }

    #[test]
    fn zero_duration_native_entries_are_ignored() {
        let native = [("bogus", 0), ("1m", 60_000)];
        let target_ms = 5 * 60_000;
        assert_eq!(aggregation_base(target_ms, &native), Some(("1m", 5)));
    }

    // ── bucket_start_ms ──────────────────────────────────────────────────

    #[test]
    fn bucket_start_aligns_to_epoch_grid() {
        let target_ms = 7 * 60_000; // 7m
        // 10 * 7m = 4_200_000 exactly on a boundary.
        assert_eq!(bucket_start_ms(4_200_000, target_ms), 4_200_000);
        // One ms into the next bucket still floors to the same boundary.
        assert_eq!(bucket_start_ms(4_200_001, target_ms), 4_200_000);
        // One ms before the boundary floors to the PREVIOUS bucket.
        assert_eq!(bucket_start_ms(4_199_999, target_ms), 4_200_000 - target_ms);
    }

    #[test]
    fn bucket_start_is_dst_agnostic_pure_epoch_math() {
        // No calendar/timezone component anywhere — a bucket boundary
        // straddling a real-world DST transition instant is still just
        // integer epoch-ms arithmetic. Sanity-check across a wide span.
        let target_ms = 86_400_000; // 1d
        let ts = 1_700_000_000_000i64; // arbitrary real-world ms timestamp
        let start = bucket_start_ms(ts, target_ms);
        assert_eq!(start % target_ms, 0);
        assert!(start <= ts && ts - start < target_ms);
    }

    // ── fold_bars_into_buckets ───────────────────────────────────────────

    #[test]
    fn folds_exact_multiple_of_bars_into_clean_buckets() {
        // target = 3 * base; 6 base bars -> 2 full target buckets.
        let base_ms = 60_000i64;
        let target_ms = 3 * base_ms;
        let bars = vec![
            bar(0, 100.0, 105.0, 95.0, 102.0, 10.0),
            bar(60_000, 102.0, 110.0, 101.0, 108.0, 20.0),
            bar(120_000, 108.0, 109.0, 90.0, 95.0, 30.0),
            bar(180_000, 95.0, 96.0, 80.0, 85.0, 5.0),
            bar(240_000, 85.0, 200.0, 84.0, 150.0, 7.0),
            bar(300_000, 150.0, 160.0, 140.0, 155.0, 3.0),
        ];
        let folded = fold_bars_into_buckets(&bars, target_ms);
        assert_eq!(folded.len(), 2);

        // Bucket 1: [0, 60_000, 120_000)
        assert_eq!(folded[0].derived_ts_ms, 0);
        assert_eq!(folded[0].open, 100.0);
        assert_eq!(folded[0].close, 95.0);
        assert_eq!(folded[0].high, 110.0);
        assert_eq!(folded[0].low, 90.0);
        assert_eq!(folded[0].volume, 60.0);

        // Bucket 2: [180_000, 240_000, 300_000)
        assert_eq!(folded[1].derived_ts_ms, 180_000);
        assert_eq!(folded[1].open, 95.0);
        assert_eq!(folded[1].close, 155.0);
        assert_eq!(folded[1].high, 200.0);
        assert_eq!(folded[1].low, 80.0);
        assert_eq!(folded[1].volume, 15.0);
    }

    #[test]
    fn trailing_partial_bucket_is_included() {
        // target = 5 * base; only 3 base bars land in the (only, partial) bucket.
        let base_ms = 60_000i64;
        let target_ms = 5 * base_ms;
        let bars = vec![
            bar(0, 10.0, 12.0, 9.0, 11.0, 1.0),
            bar(60_000, 11.0, 13.0, 10.0, 12.0, 2.0),
            bar(120_000, 12.0, 14.0, 11.0, 13.0, 3.0),
        ];
        let folded = fold_bars_into_buckets(&bars, target_ms);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].derived_ts_ms, 0);
        assert_eq!(folded[0].open, 10.0);
        assert_eq!(folded[0].close, 13.0);
        assert_eq!(folded[0].high, 14.0);
        assert_eq!(folded[0].low, 9.0);
        assert_eq!(folded[0].volume, 6.0);
    }

    #[test]
    fn misaligned_first_bar_still_buckets_correctly() {
        // First base bar does not start on an epoch bucket boundary — the
        // fold must floor it to the correct (earlier) boundary, not treat
        // it as bucket-start itself.
        let base_ms = 60_000i64;
        let target_ms = 5 * base_ms; // 300_000
        let bars = vec![
            bar(120_000, 1.0, 1.0, 1.0, 1.0, 1.0), // floors to bucket 0
            bar(180_000, 2.0, 2.0, 2.0, 2.0, 1.0),
            bar(240_000, 3.0, 3.0, 3.0, 3.0, 1.0),
        ];
        let folded = fold_bars_into_buckets(&bars, target_ms);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].derived_ts_ms, 0);
        assert_eq!(folded[0].volume, 3.0);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(fold_bars_into_buckets(&[], 60_000).is_empty());
    }

    #[test]
    fn zero_target_ms_returns_empty() {
        let bars = vec![bar(0, 1.0, 1.0, 1.0, 1.0, 1.0)];
        assert!(fold_bars_into_buckets(&bars, 0).is_empty());
    }

    #[test]
    fn single_bar_folds_to_single_partial_bucket() {
        let bars = vec![bar(0, 5.0, 6.0, 4.0, 5.5, 2.0)];
        let folded = fold_bars_into_buckets(&bars, 7 * 60_000);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].open, 5.0);
        assert_eq!(folded[0].close, 5.5);
        assert_eq!(folded[0].high, 6.0);
        assert_eq!(folded[0].low, 4.0);
        assert_eq!(folded[0].volume, 2.0);
    }

    #[test]
    fn timestamp_seconds_field_matches_derived_ts_ms_bucket() {
        let base_ms = 60_000i64;
        let target_ms = 3 * base_ms;
        let bars = vec![
            bar(0, 1.0, 1.0, 1.0, 1.0, 1.0),
            bar(60_000, 1.0, 1.0, 1.0, 1.0, 1.0),
        ];
        let folded = fold_bars_into_buckets(&bars, target_ms);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].timestamp, folded[0].derived_ts_ms / 1000);
    }
}
