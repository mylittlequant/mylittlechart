//! The one shared bars -> judgement-window resolver every SMC claim
//! primitive (`order_block`, `fair_value_gap`, `liquidity_sweep`,
//! `structure_break`) calls — see
//! `crate::state::smc_engine::SmcEngineConfig::judge_lookback_bars`'s own
//! doc for why a bounded window exists at all rather than handing every
//! detector the entire loaded series on every refresh.

use crate::Bar;

/// Resolve `bars` down to the window `mli::smc` detectors should run over to
/// judge a claim anchored at `anchor_bar_idx`, plus the GLOBAL bar index the
/// window's own index 0 corresponds to.
///
/// Backward bound: `anchor_bar_idx.saturating_sub(lookback_bars)` — see
/// `SmcEngineConfig::judge_lookback_bars`'s own doc. Forward bound: every
/// bar loaded from there to the end of `bars` — mitigation/breaker/recovery
/// state all look FORWARD from the event that creates them, so capping the
/// forward side would make a claim's own outcome depend on how much history
/// happens to be loaded past it, not on the market.
///
/// A caller converts a detector's own window-local index back to a GLOBAL
/// bar index via `offset + local_index`, and a global bar index into a
/// window-local one via `global_index.checked_sub(offset)` (`None` when the
/// global index falls before the window — cannot happen for the anchor bar
/// itself, since the window always starts at or before it, but can for an
/// older reference a detector names past the lookback bound).
///
/// `(Vec::new(), 0)` for empty `bars` or an out-of-range `anchor_bar_idx` —
/// there is nothing for a detector to run over either way.
pub(super) fn judge_window(
    bars: &[Bar],
    anchor_bar_idx: usize,
    lookback_bars: usize,
) -> (Vec<mli::smc::SmcBar>, usize) {
    if bars.is_empty() || anchor_bar_idx >= bars.len() {
        return (Vec::new(), 0);
    }
    let start = anchor_bar_idx.saturating_sub(lookback_bars);
    let window = bars[start..]
        .iter()
        .map(|b| mli::smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close })
        .collect();
    (window, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(n: usize) -> Vec<Bar> {
        (0..n as i64).map(|i| Bar::new(i, 100.0, 101.0, 99.0, 100.5)).collect()
    }

    #[test]
    fn window_starts_at_the_lookback_bound_before_the_anchor() {
        let bars = bars(20);
        let (window, offset) = judge_window(&bars, 15, 5);
        assert_eq!(offset, 10);
        assert_eq!(window.len(), 10); // bars 10..20
    }

    #[test]
    fn lookback_clamps_at_the_start_of_the_series() {
        let bars = bars(10);
        let (window, offset) = judge_window(&bars, 2, 300);
        assert_eq!(offset, 0);
        assert_eq!(window.len(), 10);
    }

    #[test]
    fn empty_bars_never_panics() {
        let (window, offset) = judge_window(&[], 0, 300);
        assert!(window.is_empty());
        assert_eq!(offset, 0);
    }

    #[test]
    fn anchor_past_the_series_never_panics() {
        let bars = bars(5);
        let (window, offset) = judge_window(&bars, 10, 300);
        assert!(window.is_empty());
        assert_eq!(offset, 0);
    }
}
