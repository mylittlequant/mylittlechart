//! Shared bars/window resolvers and small resolved-fact types every
//! order-flow primitive (`imbalance_zone`, `absorption_mark`,
//! `delta_divergence`, `order_flow_scan`) reads — the order-flow family's
//! own counterpart of `smc::smc_judge`.
//!
//! **Not the same helper `smc::smc_judge::judge_window` uses.** That
//! function is `pub(super)` to `smc` — visible inside `smc` and its own
//! descendants only, not to a sibling family under `primitives_v2` — so it
//! is unreachable from here. This module writes its own, equally small,
//! bars-window-to-`mli` conversion rather than widening `smc_judge`'s own
//! visibility for one outside caller.
//!
//! Every function here reads flags [`crate::types::FootprintBar`] already
//! carries (`stacked_ask_start`/`stacked_bid_start`/`is_absorption`/
//! `cumulative_delta`/per-level `is_single_print`/`is_unfinished`) —
//! computed upstream by `FootprintBar::compute_extras` from the user's own
//! configured thresholds. NONE of those thresholds are recomputed here: no
//! imbalance ratio, no absorption threshold, no stacked-run detection. The
//! only computed fact in this whole family is delta divergence, which comes
//! from `mli::orderflow::delta_divergences` — see [`footprint_judge_window`].

use crate::types::FootprintBar;
use crate::i18n::ConceptKey;
use super::super::elliott::guard::Violation;

/// The shared "no order-flow data for this series" violation every
/// primitive in this family reports FIRST (Appendix B's own honest-
/// degradation rule) — a single `'static` slice rather than a per-call
/// `Vec`, so [`Primitive::context_violations`](super::super::Primitive::context_violations)
/// can return it by reference even before any sweep has ever run (a
/// primitive's own `cached_footprint_present` starts `false` — plain
/// `#[serde(skip)]`/`bool::default()` — for both a freshly constructed
/// primitive AND a freshly deserialized one, with no special-cased default
/// function needed on either path).
pub const MISSING_VIOLATION: [Violation; 1] =
    [Violation { rule_id: "order_flow_data_missing", wave: None, message: ConceptKey::RuleOrderFlowDataMissing }];

/// Which side of the book a stacked-imbalance run sits on — the chart's own
/// small enum for [`ImbalanceRun::side`], distinct from `mli::smc::
/// Direction` (a price-bias direction, not a book side) so the two are never
/// confused at a call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImbalanceSide {
    /// Stacked ask imbalance — `FootprintBar::stacked_ask_start`. The
    /// existing footprint renderer marks this with `▲` (F1) — the same
    /// "aggressive buying pressure" reading this side's own render label
    /// reuses (`ConceptKey::PatternBiasBullish`).
    Ask,
    /// Stacked bid imbalance — `FootprintBar::stacked_bid_start`. Marked
    /// `▼` by the existing footprint renderer (F1) — read here as
    /// `ConceptKey::PatternBiasBearish`.
    Bid,
}

/// [`imbalance_zone::ImbalanceZone`](super::imbalance_zone::ImbalanceZone)'s
/// own resolved fact — one bar's stacked-imbalance run, read verbatim off
/// `FootprintBar::stacked_ask_start`/`stacked_ask_len` (or the bid pair),
/// with the run's own price bounds already resolved from `levels`. Also the
/// per-hit shape [`order_flow_scan`](super::order_flow_scan) surveys with —
/// the ONE place this crate turns a stacked-run flag into a price range, so
/// a claim and a survey hit of the same kind can never disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImbalanceRun {
    /// The footprint bar's own `open_time` (ms) — carries no OHLC bar index
    /// of its own, since a footprint bucket and the displayed candle series
    /// are not guaranteed to share one; `RenderContext::ts_to_x_ms` resolves
    /// a screen X from this directly.
    pub bar_ts: i64,
    pub side: ImbalanceSide,
    /// The run's own level count (`stacked_ask_len`/`stacked_bid_len`).
    pub len: usize,
    /// The lowest and highest PRICE the run's own levels span
    /// (`FootprintLevel::price`, ascending order already guaranteed by
    /// `FootprintBar`'s own doc).
    pub low: f64,
    pub high: f64,
}

/// Resolve `bar`'s own stacked-imbalance run, if it carries one — ask
/// checked first only because a bar with both sides stacked is not a shape
/// `FootprintBar::compute_extras`'s own doc describes (it records the
/// LONGEST run per side, independently; nothing arbitrates "the" run when
/// both exist, so this function does not invent an arbitration either).
pub fn resolve_imbalance_run(bar: &FootprintBar) -> Option<ImbalanceRun> {
    let (side, start, len) = if let (Some(s), Some(l)) = (bar.stacked_ask_start, bar.stacked_ask_len) {
        (ImbalanceSide::Ask, s, l)
    } else if let (Some(s), Some(l)) = (bar.stacked_bid_start, bar.stacked_bid_len) {
        (ImbalanceSide::Bid, s, l)
    } else {
        return None;
    };
    if len == 0 {
        return None;
    }
    let end = start.checked_add(len)?.checked_sub(1)?;
    let run_levels = bar.levels.get(start..=end)?;
    let low = run_levels.iter().fold(f64::INFINITY, |m, l| m.min(l.price));
    let high = run_levels.iter().fold(f64::NEG_INFINITY, |m, l| m.max(l.price));
    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    Some(ImbalanceRun { bar_ts: bar.ts_ms(), side, len, low, high })
}

/// [`absorption_mark::AbsorptionMark`](super::absorption_mark::AbsorptionMark)'s
/// own resolved fact — one bar's absorption snapshot, read verbatim off
/// `FootprintBar::is_absorption`/`total_delta`/`volume`. `mid_price` is a
/// render-only anchor (the bar's own `(high + low) / 2`), never part of the
/// claim's own Stat block, which reports `delta`/`volume`/their ratio only
/// — the task's own "all read off the bar, not recomputed thresholds".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbsorptionSnapshot {
    pub bar_ts: i64,
    pub delta: f64,
    pub volume: f64,
    pub mid_price: f64,
}

/// Resolve `bar`'s own absorption snapshot — `None` when
/// `FootprintBar::is_absorption` is false, so `None` reads identically
/// whether the bar was never absorption or was never found at all (the same
/// single-rule shape `smc::order_block::OrderBlock::detect` already uses).
pub fn resolve_absorption(bar: &FootprintBar) -> Option<AbsorptionSnapshot> {
    if !bar.is_absorption {
        return None;
    }
    Some(AbsorptionSnapshot {
        bar_ts: bar.ts_ms(),
        delta: bar.total_delta(),
        volume: bar.volume,
        mid_price: (bar.high + bar.low) / 2.0,
    })
}

/// One price LEVEL flagged `is_single_print`/`is_unfinished` — the shape
/// [`order_flow_scan`](super::order_flow_scan) surveys both of those
/// structures with (they carry no standalone claim primitive of their own,
/// so this type exists only for the survey).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelHit {
    pub bar_ts: i64,
    pub price: f64,
}

/// Every level on `bar` flagged `is_single_print`, and every level flagged
/// `is_unfinished` — read verbatim, nothing recomputed (see this module's
/// own doc).
pub fn resolve_level_flags(bar: &FootprintBar) -> (Vec<LevelHit>, Vec<LevelHit>) {
    let mut single_print = Vec::new();
    let mut unfinished = Vec::new();
    for level in &bar.levels {
        if level.is_single_print {
            single_print.push(LevelHit { bar_ts: bar.ts_ms(), price: level.price });
        }
        if level.is_unfinished {
            unfinished.push(LevelHit { bar_ts: bar.ts_ms(), price: level.price });
        }
    }
    (single_print, unfinished)
}

/// Resolve the footprint-bar INDEX whose own bucket contains `ts_ms` —
/// `None` when `footprint` is empty, `ts_ms` lands before the first
/// bucket's own start, or `ts_ms` lands strictly beyond the last loaded
/// bucket (mirrors `smc::order_block::OrderBlock::anchor_bar_idx`'s own
/// "None when the anchor lands strictly beyond the last loaded bar"
/// contract — a footprint bucket carries no fixed interval this crate can
/// assume, so there is nothing honest to extrapolate INTO past the last
/// bucket's own start).
pub fn find_footprint_bar_for_ts(footprint: &[FootprintBar], ts_ms: i64) -> Option<usize> {
    if footprint.is_empty() {
        return None;
    }
    let last_open = footprint[footprint.len() - 1].open_time;
    if ts_ms > last_open {
        return None;
    }
    let idx = footprint.partition_point(|fb| fb.open_time <= ts_ms);
    if idx == 0 {
        return None;
    }
    Some(idx - 1)
}

/// The inclusive footprint-bar-INDEX range covering every bucket inside a
/// closed `[from_ms, to_ms]` window, in whichever order the two timestamps
/// arrive — `None` when the window holds no bucket. The footprint-bar
/// counterpart of `super::super::bar_window::window_indices`, keyed on
/// `FootprintBar::open_time` (already milliseconds — no `/1000` needed,
/// unlike `Bar::timestamp`).
pub fn footprint_window_indices(footprint: &[FootprintBar], from_ms: i64, to_ms: i64) -> Option<(usize, usize)> {
    let (lo, hi) = if from_ms <= to_ms { (from_ms, to_ms) } else { (to_ms, from_ms) };
    let start = footprint.partition_point(|fb| fb.open_time < lo);
    let end = footprint.partition_point(|fb| fb.open_time <= hi);
    if start >= end {
        return None;
    }
    Some((start, end - 1))
}

/// Resolve `footprint` down to the window `mli::orderflow::
/// delta_divergences` should run over to judge a claim/survey anchored at
/// `anchor_idx`, plus the GLOBAL footprint-bar index the window's own index
/// 0 corresponds to — the delta-divergence counterpart of `smc::smc_judge::
/// judge_window`.
///
/// Backward bound: `anchor_idx.saturating_sub(lookback_bars)` — see
/// `OrderFlowEngineConfig::judge_lookback_bars`'s own doc. Forward bound:
/// every bucket loaded from there to the end of `footprint` — swing
/// confirmation looks FORWARD from the bar that forms it (Appendix A: "a
/// swing is CONFIRMED only n bars after it forms"), so capping the forward
/// side would make a claim's own outcome depend on how much history happens
/// to be loaded past it, not on the market.
///
/// `bars`/`series` are built from `footprint`'s OWN OHLC and
/// `cumulative_delta` — never from a separately-loaded `Bar` slice, so the
/// two stay aligned 1:1 by construction (`mli::orderflow::
/// delta_divergences` requires `series.len() == bars.len()`) regardless of
/// whether the footprint's own bucketing matches the displayed candle
/// series.
///
/// `(Vec::new(), Vec::new(), 0)` for empty `footprint` or an out-of-range
/// `anchor_idx`.
pub fn footprint_judge_window(
    footprint: &[FootprintBar],
    anchor_idx: usize,
    lookback_bars: usize,
) -> (Vec<mli::smc::SmcBar>, Vec<f64>, usize) {
    if footprint.is_empty() || anchor_idx >= footprint.len() {
        return (Vec::new(), Vec::new(), 0);
    }
    let start = anchor_idx.saturating_sub(lookback_bars);
    let window = &footprint[start..];
    let bars = window
        .iter()
        .map(|fb| mli::smc::SmcBar { open: fb.open, high: fb.high, low: fb.low, close: fb.close })
        .collect();
    let series = window.iter().map(|fb| fb.cumulative_delta).collect();
    (bars, series, start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FootprintLevel;

    fn bar_with_levels(open_time: i64, prices: &[(f64, f64, f64)]) -> FootprintBar {
        // prices: (price, buy_vol, sell_vol)
        let levels: Vec<FootprintLevel> = prices
            .iter()
            .map(|&(price, buy_vol, sell_vol)| FootprintLevel { price, buy_vol, sell_vol, is_single_print: false, is_unfinished: false })
            .collect();
        FootprintBar { open_time, open: 100.0, high: 103.0, low: 99.0, close: 101.0, volume: 10.0, levels, ..Default::default() }
    }

    #[test]
    fn resolve_imbalance_run_reads_the_ask_run_verbatim() {
        let mut bar = bar_with_levels(1_000, &[(99.0, 1.0, 1.0), (100.0, 5.0, 1.0), (101.0, 5.0, 1.0), (102.0, 1.0, 1.0)]);
        bar.stacked_ask_start = Some(1);
        bar.stacked_ask_len = Some(2);
        let run = resolve_imbalance_run(&bar).expect("expected an ask run");
        assert_eq!(run.side, ImbalanceSide::Ask);
        assert_eq!(run.len, 2);
        assert_eq!(run.low, 100.0);
        assert_eq!(run.high, 101.0);
        assert_eq!(run.bar_ts, 1_000);
    }

    #[test]
    fn resolve_imbalance_run_is_none_without_a_flagged_run() {
        let bar = bar_with_levels(1_000, &[(99.0, 1.0, 1.0)]);
        assert!(resolve_imbalance_run(&bar).is_none());
    }

    #[test]
    fn resolve_absorption_reads_the_flag_verbatim() {
        let mut bar = bar_with_levels(2_000, &[(100.0, 1.0, 1.0)]);
        bar.is_absorption = true;
        bar.volume = 20.0;
        let snap = resolve_absorption(&bar).expect("expected absorption");
        assert_eq!(snap.bar_ts, 2_000);
        assert_eq!(snap.volume, 20.0);
        assert_eq!(snap.delta, bar.total_delta());
    }

    #[test]
    fn resolve_absorption_is_none_when_flag_is_false() {
        let bar = bar_with_levels(2_000, &[(100.0, 1.0, 1.0)]);
        assert!(resolve_absorption(&bar).is_none());
    }

    #[test]
    fn find_footprint_bar_for_ts_resolves_the_containing_bucket() {
        let footprint = vec![bar_with_levels(1_000, &[]), bar_with_levels(2_000, &[]), bar_with_levels(3_000, &[])];
        assert_eq!(find_footprint_bar_for_ts(&footprint, 2_000), Some(1));
        assert_eq!(find_footprint_bar_for_ts(&footprint, 2_500), Some(1));
        assert_eq!(find_footprint_bar_for_ts(&footprint, 500), None);
        assert_eq!(find_footprint_bar_for_ts(&footprint, 3_001), None);
    }

    #[test]
    fn find_footprint_bar_for_ts_empty_never_panics() {
        assert_eq!(find_footprint_bar_for_ts(&[], 0), None);
    }

    #[test]
    fn footprint_window_indices_resolves_a_plain_span() {
        let footprint = vec![bar_with_levels(0, &[]), bar_with_levels(1_000, &[]), bar_with_levels(2_000, &[])];
        assert_eq!(footprint_window_indices(&footprint, 0, 2_000), Some((0, 2)));
        assert_eq!(footprint_window_indices(&footprint, 2_000, 0), Some((0, 2)));
        assert_eq!(footprint_window_indices(&footprint, 5_000, 6_000), None);
    }

    #[test]
    fn footprint_judge_window_clamps_at_the_start_and_never_caps_forward() {
        let footprint: Vec<FootprintBar> = (0..20).map(|i| bar_with_levels(i * 1_000, &[])).collect();
        let (bars, series, offset) = footprint_judge_window(&footprint, 15, 5);
        assert_eq!(offset, 10);
        assert_eq!(bars.len(), 10);
        assert_eq!(series.len(), 10);
    }

    #[test]
    fn footprint_judge_window_out_of_range_anchor_never_panics() {
        let footprint = vec![bar_with_levels(0, &[])];
        assert_eq!(footprint_judge_window(&footprint, 5, 300), (Vec::new(), Vec::new(), 0));
        assert_eq!(footprint_judge_window(&[], 0, 300), (Vec::new(), Vec::new(), 0));
    }
}
