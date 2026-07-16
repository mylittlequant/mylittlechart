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

use crate::bridge::RawTradeWire;

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

/// Whether a `target_ms`-duration timeframe must be built by folding raw
/// TRADES rather than fetched/aggregated from kline REST/WS (TRADE-BUILT
/// klines fix, 2026-07-16 — owner report: selecting 15s on Bybit silently
/// fell back to hourly bars mislabeled "15s", because Bybit's smallest
/// native kline is 1m and no native interval divides 15s evenly).
///
/// `true` iff `target_ms` is NOT itself in `native` AND
/// [`aggregation_base`] finds no usable native divisor for it. This is
/// intentionally NOT restricted to sub-minute targets: ANY non-native,
/// non-divisible target (e.g. a hypothetical 90s TF on a venue whose
/// smallest native interval is 1m — 90s doesn't divide by 60s either) is
/// trade-built by the same rule, because the underlying problem
/// (`aggregation_base` returning `None`) is identical regardless of
/// whether the target is smaller or larger than the smallest native
/// interval. Callers must check nativeness is not already established by
/// their own faster path first (same caller contract as
/// [`aggregation_base`]) — this function re-derives it internally so it is
/// safe to call standalone.
pub fn is_trade_built(target_ms: i64, native: &[(&str, i64)]) -> bool {
    if target_ms <= 0 {
        return false;
    }
    let is_native = native.iter().any(|&(_, base_ms)| base_ms == target_ms);
    if is_native {
        return false;
    }
    // SUB-MINUTE RULE (owner doctrine 2026-07-17): every non-native
    // sub-minute target is TRADE-BUILT unconditionally — divisor kline
    // aggregation (e.g. 1s→15s) is NOT consulted at this scale. Trades
    // are the canonical sub-minute source (zip archives + paginated REST
    // trades + live WS trades, all universally served), while sub-minute
    // kline endpoints are venue-exotic (one venue advertises them, its
    // REST map silently substituted hourly bars — live-caught 2026-07-17:
    // binance spot "1s" fell into a `_ => "1h"` connector default and 15s
    // rendered a wall of hourly bars while the trade plane STOOD DOWN
    // because this classifier said "the kline plane will serve"). One TF
    // = one source class; the trade plane must never stand down on a
    // sub-minute TF.
    if target_ms < 60_000 {
        return true;
    }
    aggregation_base(target_ms, native).is_none()
}

/// GRANULARITY VERIFICATION at the fetch boundary (owner invariant
/// 2026-07-17): do fetched bars actually carry the interval we asked
/// for? Venue connectors' `map_kline_interval` fall back SILENTLY on
/// unsupported intervals (live-caught: binance "1s" fell into a
/// `_ => "1h"` default and a 1s fetch returned hourly bars) — a
/// substituted page must be treated as an ERROR by the fetch path, never
/// emitted under the requested label.
///
/// Discriminator = the LOW percentile (p10) of consecutive steps, not
/// the median: pages glued from sparse session fragments have hours-wide
/// boundary gaps, but the steps INSIDE fragments equal the true
/// interval. Honest page → p10 == interval; substituted page → p10 ==
/// the substituted interval (out of tolerance either direction).
/// Empty / single-point pages carry no measurable step and pass.
pub fn bars_match_interval(bars: &[mlc_core::Bar], interval_ms: i64) -> bool {
    if bars.len() < 2 || interval_ms <= 0 {
        return true;
    }
    let ts_ms = |b: &mlc_core::Bar| if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp * 1000 };
    let mut steps: Vec<i64> = bars
        .windows(2)
        .map(|w| ts_ms(&w[1]) - ts_ms(&w[0]))
        .filter(|d| *d > 0)
        .collect();
    if steps.is_empty() {
        return true;
    }
    steps.sort_unstable();
    let p10 = steps[steps.len() / 10];
    p10 >= interval_ms / 2 && p10 <= interval_ms * 2
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

/// Fold a slice of raw trades (any order — sorted internally) into
/// `target_ms`-duration OHLCV buckets, for TRADE-BUILT timeframes (see
/// [`is_trade_built`]) — the trade-plane analogue of
/// [`fold_bars_into_buckets`], which folds kline bars instead.
///
/// Bucketing is keyed by each trade's `ts_ms`, epoch-aligned via
/// [`bucket_start_ms`] exactly like the kline fold. Per bucket: `open` =
/// first trade's price, `close` = last trade's price, `high`/`low` =
/// max/min price across the bucket's trades, `volume` = sum of `qty`. The
/// output bucket's `timestamp`/`derived_ts_ms` are the bucket's own
/// epoch-aligned start, never copied from any input trade.
///
/// **A bucket with no trades emits NO bar** — gaps are honest (unlike the
/// kline fold, there is no upstream bar to carry a flat OHLC forward for a
/// silent interval; a chart consumer sees a genuine hole exactly where no
/// trading happened). This is the one behavioral difference from
/// [`fold_bars_into_buckets`], which always emits a bucket for every base
/// bar it consumes (base bars themselves already encode "no gap" from the
/// venue). The trailing (possibly partial) bucket IS included as long as
/// at least one trade landed in it — that partial bucket is the live
/// in-progress bar.
///
/// `target_ms <= 0` or an empty `trades` slice returns an empty `Vec`.
pub fn fold_trades_into_bars(trades: &[RawTradeWire], target_ms: i64) -> Vec<Bar> {
    if target_ms <= 0 || trades.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<&RawTradeWire> = trades.iter().collect();
    sorted.sort_by_key(|t| t.ts_ms);

    let mut out: Vec<Bar> = Vec::new();
    let mut current_bucket_start: Option<i64> = None;

    for trade in sorted {
        let bucket_start = bucket_start_ms(trade.ts_ms, target_ms);
        let start_new_bucket = match current_bucket_start {
            Some(cur) => bucket_start != cur,
            None => true,
        };

        if start_new_bucket {
            out.push(Bar {
                timestamp: bucket_start / 1000,
                open: trade.price,
                high: trade.price,
                low: trade.price,
                close: trade.price,
                volume: trade.qty,
                derived_ts_ms: bucket_start,
            });
            current_bucket_start = Some(bucket_start);
        } else if let Some(last) = out.last_mut() {
            last.high = last.high.max(trade.price);
            last.low = last.low.min(trade.price);
            last.close = trade.price;
            last.volume += trade.qty;
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

    // ── is_trade_built (TRADE-BUILT klines fix, 2026-07-16) ─────────────────

    /// Bybit-like native set: smallest native kline is 1m.
    const BYBIT_LIKE_NATIVE: [(&str, i64); 3] = [("1m", 60_000), ("5m", 300_000), ("1h", 3_600_000)];

    #[test]
    fn sub_native_15s_is_trade_built() {
        assert!(is_trade_built(15_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn sub_native_1s_is_trade_built() {
        assert!(is_trade_built(1_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn sub_native_30s_is_trade_built() {
        assert!(is_trade_built(30_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn seven_minute_target_is_aggregated_not_trade_built() {
        // 7m has a native divisor (1m, ratio 7) — aggregated, not trade-built.
        assert!(!is_trade_built(7 * 60_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn thirty_minute_target_is_native_not_trade_built() {
        // Not in BYBIT_LIKE_NATIVE directly, but divisible by 1m (ratio 30) —
        // aggregated. A genuinely NATIVE match (exact same duration in the
        // native set) must also resolve false; cover that with 1h below.
        assert!(!is_trade_built(30 * 60_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn exact_native_match_is_not_trade_built() {
        assert!(!is_trade_built(3_600_000, &BYBIT_LIKE_NATIVE)); // == "1h"
    }

    #[test]
    fn supra_native_non_divisor_is_trade_built() {
        // 90s: larger than the smallest native interval (60s) but NOT an
        // integer multiple of it (90 % 60 != 0) — still trade-built. Any
        // no-divisor TF is trade-built regardless of whether it's smaller
        // or larger than the smallest native interval (doc comment on
        // `is_trade_built`).
        let native = [("1m", 60_000)];
        assert!(is_trade_built(90_000, &native));
    }

    #[test]
    fn zero_or_negative_target_is_not_trade_built() {
        assert!(!is_trade_built(0, &BYBIT_LIKE_NATIVE));
        assert!(!is_trade_built(-1_000, &BYBIT_LIKE_NATIVE));
    }

    #[test]
    fn empty_native_set_makes_everything_trade_built() {
        // A venue with zero known native intervals (not yet connected, or
        // genuinely has none) — every positive target is trade-built.
        let native: [(&str, i64); 0] = [];
        assert!(is_trade_built(60_000, &native));
        assert!(is_trade_built(15_000, &native));
    }

    // ── fold_trades_into_bars ────────────────────────────────────────────

    fn trade(ts_ms: i64, price: f64, qty: f64) -> RawTradeWire {
        RawTradeWire { ts_ms, price, qty, is_buyer_maker: false }
    }

    #[test]
    fn folds_trades_into_seconds_scale_buckets() {
        // 15s buckets — the exact owner-report scale (Bybit 15s sub-native).
        let target_ms = 15_000i64;
        let trades = vec![
            trade(0, 100.0, 1.0),
            trade(5_000, 101.0, 2.0),
            trade(14_999, 99.0, 3.0),
            // New bucket starts at 15_000.
            trade(15_000, 102.0, 1.0),
            trade(20_000, 103.0, 0.5),
        ];
        let bars = fold_trades_into_bars(&trades, target_ms);
        assert_eq!(bars.len(), 2);

        assert_eq!(bars[0].derived_ts_ms, 0);
        assert_eq!(bars[0].open, 100.0);
        assert_eq!(bars[0].close, 99.0);
        assert_eq!(bars[0].high, 101.0);
        assert_eq!(bars[0].low, 99.0);
        assert_eq!(bars[0].volume, 6.0);

        assert_eq!(bars[1].derived_ts_ms, 15_000);
        assert_eq!(bars[1].open, 102.0);
        assert_eq!(bars[1].close, 103.0);
        assert_eq!(bars[1].high, 103.0);
        assert_eq!(bars[1].low, 102.0);
        assert_eq!(bars[1].volume, 1.5);
    }

    #[test]
    fn gap_buckets_are_omitted_not_flat_filled() {
        // Trades in bucket 0 and bucket 3 (buckets 1, 2 have no trades) —
        // the fold must NOT synthesize flat bars for the empty buckets.
        let target_ms = 10_000i64;
        let trades = vec![
            trade(0, 50.0, 1.0),
            trade(30_000, 55.0, 1.0), // bucket index 3 (30_000 / 10_000)
        ];
        let bars = fold_trades_into_bars(&trades, target_ms);
        assert_eq!(bars.len(), 2, "only 2 buckets have trades — no gap-filled bars");
        assert_eq!(bars[0].derived_ts_ms, 0);
        assert_eq!(bars[1].derived_ts_ms, 30_000);
    }

    #[test]
    fn volume_is_sum_of_trade_qty_in_bucket() {
        let target_ms = 60_000i64;
        let trades = vec![
            trade(0, 10.0, 0.1),
            trade(10_000, 10.5, 0.2),
            trade(20_000, 10.2, 0.3),
            trade(30_000, 10.8, 0.05),
        ];
        let bars = fold_trades_into_bars(&trades, target_ms);
        assert_eq!(bars.len(), 1);
        assert!((bars[0].volume - 0.65).abs() < 1e-12);
    }

    #[test]
    fn open_close_ordering_is_first_and_last_trade_by_time_not_input_order() {
        // Input given out of chronological order — fold must sort by ts_ms
        // first, so open = earliest trade's price, close = latest trade's.
        let target_ms = 60_000i64;
        let trades = vec![
            trade(40_000, 30.0, 1.0), // latest -> should become close
            trade(0, 10.0, 1.0),      // earliest -> should become open
            trade(20_000, 50.0, 1.0), // highest -> should become high
            trade(10_000, 5.0, 1.0),  // lowest -> should become low
        ];
        let bars = fold_trades_into_bars(&trades, target_ms);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, 10.0);
        assert_eq!(bars[0].close, 30.0);
        assert_eq!(bars[0].high, 50.0);
        assert_eq!(bars[0].low, 5.0);
    }

    #[test]
    fn trade_fold_trailing_partial_bucket_is_the_live_bar() {
        let target_ms = 15_000i64;
        let trades = vec![trade(15_000, 100.0, 1.0), trade(16_000, 101.0, 1.0)];
        let bars = fold_trades_into_bars(&trades, target_ms);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].derived_ts_ms, 15_000);
        assert_eq!(bars[0].close, 101.0);
    }

    #[test]
    fn trade_fold_empty_input_returns_empty() {
        assert!(fold_trades_into_bars(&[], 15_000).is_empty());
    }

    #[test]
    fn trade_fold_zero_target_ms_returns_empty() {
        let trades = vec![trade(0, 1.0, 1.0)];
        assert!(fold_trades_into_bars(&trades, 0).is_empty());
    }

    #[test]
    fn trade_fold_single_trade_is_a_single_ohlc_point() {
        let bars = fold_trades_into_bars(&[trade(1_000, 42.0, 3.5)], 15_000);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, 42.0);
        assert_eq!(bars[0].high, 42.0);
        assert_eq!(bars[0].low, 42.0);
        assert_eq!(bars[0].close, 42.0);
        assert_eq!(bars[0].volume, 3.5);
    }

    #[test]
    fn trade_fold_timestamp_seconds_matches_derived_ts_ms_bucket() {
        let bars = fold_trades_into_bars(&[trade(1_000, 1.0, 1.0)], 15_000);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].timestamp, bars[0].derived_ts_ms / 1000);
    }
}
