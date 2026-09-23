//! Freshness decision for the alert-evaluator's `current_price` input.
//!
//! `AlertManager::on_tick`'s `Option<f64>` contract (94c8e4fa,
//! `docs/mlc/research/owner-review-2026-09-23/
//! alert-bubbles-on-cold-apply.md` item 2) only covered ONE way a bubble
//! can be "no real data yet": an empty `bars` array (a bubble just minted
//! by `ensure_alert_bubble`, never backfilled). It left a second way
//! uncovered — a bubble whose live feed was torn down by some OTHER path
//! (window close, panel close, symbol switch, ...) after real bars had
//! already landed keeps a NON-empty `bars` array that simply stops
//! updating. Fed straight through the old code, that reads as a real,
//! permanently-frozen price forever — indistinguishable from a live tick
//! sitting still. [`price_if_fresh`] closes that gap: it is the one place
//! that turns "bars + timeframe + now" into the `Option<f64>` the
//! evaluator actually wants.

use mlc_core::state::Timeframe;
use mlc_core::Bar;

/// How many worst-case bar intervals the LAST bar may lag `now` before its
/// price counts as stale ("no data") rather than a live tick.
///
/// A bar's own `timestamp` is its OPEN time, not a last-update stamp — even
/// a perfectly live feed can carry a last bar up to one full `interval_ms`
/// old right before it rolls to the next bucket. The bound has to clear
/// that plus slack for ordinary feed/backfill jitter before "old" means
/// anything; 3 intervals is generous enough to never trip on live data
/// (verified against the tightest real timeframe, 1s) while still being a
/// bounded multiple of MINUTES-to-HOURS for a genuinely dead feed, not an
/// unrelated constant.
const STALE_INTERVAL_MULTIPLIER: i64 = 3;

/// Decide the alert evaluator's `current_price` from a bubble's own bars +
/// timeframe, against `now_secs` (caller-supplied so this stays a pure,
/// deterministic function with no wall-clock read inside — the freshness
/// decision itself is what needs testing, not the clock).
///
/// `None` when: no bars yet (the original 94c8e4fa "no data" case), OR a
/// Time-kind timeframe's last bar is older than the staleness bound above
/// (this item's addition, owner-review-2026-09-23 item 2).
///
/// Tick timeframes (`Timeframe::is_tick`) have no wall-clock duration —
/// `interval_ms` is a synthetic sort weight, not a real period (see
/// `Timeframe`'s own doc) — so age-based staleness does not apply to them;
/// only the empty-bars case counts as "no data". A tick bar can
/// legitimately stay open a long wall-clock time in a quiet market without
/// that meaning the feed died.
pub fn price_if_fresh(bars: &[Bar], timeframe: &Timeframe, now_secs: i64) -> Option<f64> {
    let last = bars.last()?;
    if !timeframe.is_tick() {
        let interval_secs = (timeframe.interval_ms.max(0) / 1000).max(1);
        let bound_secs = interval_secs.saturating_mul(STALE_INTERVAL_MULTIPLIER);
        let age_secs = now_secs.saturating_sub(last.timestamp);
        if age_secs > bound_secs {
            return None;
        }
    }
    Some(last.close)
}

#[cfg(test)]
mod tests {
    //! Item 2 coverage (owner-review-2026-09-23,
    //! `docs/mlc/research/owner-review-2026-09-23/
    //! alert-bubbles-on-cold-apply.md` item 2): every branch of the
    //! freshness decision, isolated from `ChartApp`/the tick loop — pure
    //! function, no bridge/backend construction needed.

    use super::*;

    fn bar_at(timestamp: i64, close: f64) -> Bar {
        Bar { timestamp, close, ..Default::default() }
    }

    #[test]
    fn no_bars_is_no_data() {
        let tf = Timeframe::h1();
        assert_eq!(price_if_fresh(&[], &tf, 1_000_000), None);
    }

    #[test]
    fn a_bar_within_the_staleness_bound_is_a_real_price() {
        let tf = Timeframe::new("1m", 60_000); // 60s interval, bound = 180s
        let bars = [bar_at(1_000, 42.0)];
        // 150s old: inside the 180s bound.
        assert_eq!(price_if_fresh(&bars, &tf, 1_150), Some(42.0));
    }

    #[test]
    fn a_bar_right_at_the_bound_is_still_fresh() {
        let tf = Timeframe::new("1m", 60_000); // bound = 180s
        let bars = [bar_at(1_000, 42.0)];
        assert_eq!(price_if_fresh(&bars, &tf, 1_000 + 180), Some(42.0));
    }

    #[test]
    fn a_bar_past_the_staleness_bound_is_no_data() {
        let tf = Timeframe::new("1m", 60_000); // bound = 180s
        let bars = [bar_at(1_000, 42.0)];
        // 181s old: one second past the bound.
        assert_eq!(price_if_fresh(&bars, &tf, 1_000 + 181), None);
    }

    #[test]
    fn a_frozen_feed_on_a_slow_timeframe_eventually_reads_as_no_data() {
        // The exact defect this closes: a 1h-timeframe bubble whose feed
        // was torn down elsewhere keeps a non-empty `bars` array forever —
        // without this check, `current_price` stays pinned at the frozen
        // last close indefinitely. h1 = 3600s interval, bound = 10800s (3h).
        let tf = Timeframe::h1();
        let bars = [bar_at(0, 42.0)];
        assert_eq!(price_if_fresh(&bars, &tf, 10_800), Some(42.0), "still inside the bound");
        assert_eq!(price_if_fresh(&bars, &tf, 10_801), None, "one second past the bound must read as no-data");
    }

    #[test]
    fn tick_timeframes_never_go_stale_by_age() {
        // `interval_ms` on a tick TF is a synthetic sort weight, not a real
        // duration — a tick bar sitting open for a long wall-clock time in
        // a quiet market must not be mistaken for a dead feed.
        let tf = Timeframe::tick(100);
        let bars = [bar_at(0, 42.0)];
        assert_eq!(price_if_fresh(&bars, &tf, 1_000_000_000), Some(42.0));
    }

    #[test]
    fn tick_timeframe_with_no_bars_is_still_no_data() {
        let tf = Timeframe::tick(100);
        assert_eq!(price_if_fresh(&[], &tf, 0), None);
    }
}
