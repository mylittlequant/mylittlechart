//! The one shared timestamp-span-to-bar-index resolver.
//!
//! Promoted (2026-08-25, markup-engines wave) from
//! `volume::profile_render::window_indices`, which `projection::
//! bars_pattern::BarsPattern::render` had a second, inline copy of.
//!
//! `volume::anchored_vwap::AnchoredVwap::end_bar_idx` deliberately does NOT
//! call this and is NOT a third copy: it resolves ONE endpoint — an upper
//! cap clamped so it can never fall before the anchor, degenerating to a
//! single-bar VWAP rather than inverting — and routing it through a
//! two-endpoint resolver would mean inventing a lower endpoint it does not
//! have, purely to reach a shared function. Sharing a computation two
//! callers genuinely perform is the point; manufacturing an argument to
//! qualify for the sharing is not.
//!
//! `bars_pattern`'s own copy differed from this one in two
//! details, neither of them behavioural: it converted `ms -> s` with
//! `div_euclid` instead of plain `/` (identical for the non-negative epoch
//! timestamps every bar in this crate carries — Unix seconds never go
//! negative here — so the difference is real only for input this crate
//! never produces), and it exposed the upper bound as an EXCLUSIVE slice end
//! rather than an inclusive `(start, end)` pair. Both are adapted at the
//! call site below, not carried into this shared function — see
//! [`projection::bars_pattern`](super::projection::bars_pattern)'s own call
//! site for the exact `end + 1` conversion.

use crate::Bar;

/// The inclusive bar-INDEX range covering every bar inside a closed
/// `[from_ms, to_ms]` window, in whichever order the two timestamps arrive
/// — `None` when the window holds no bar.
///
/// `Bar::timestamp` is SECONDS — callers carry milliseconds, and mixing the
/// two is how an anchor lands on bar zero.
pub(super) fn window_indices(bars: &[Bar], from_ms: i64, to_ms: i64) -> Option<(usize, usize)> {
    let (lo_ms, hi_ms) = if from_ms <= to_ms { (from_ms, to_ms) } else { (to_ms, from_ms) };
    let lo = lo_ms / 1000;
    let hi = hi_ms / 1000;
    let start = bars.partition_point(|b| b.timestamp < lo);
    let end = bars.partition_point(|b| b.timestamp <= hi);
    if start >= end { return None; }
    Some((start, end - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars() -> Vec<Bar> {
        (0..=5).map(|i| Bar::new(i, 100.0, 101.0, 99.0, 100.5)).collect()
    }

    #[test]
    fn resolves_a_plain_span() {
        let bars = bars();
        assert_eq!(window_indices(&bars, 0, 5_000), Some((0, 5)));
    }

    #[test]
    fn swaps_reversed_endpoints() {
        let bars = bars();
        assert_eq!(window_indices(&bars, 5_000, 0), Some((0, 5)));
    }

    #[test]
    fn empty_bars_is_none() {
        assert_eq!(window_indices(&[], 0, 5_000), None);
    }

    #[test]
    fn a_span_entirely_outside_the_data_is_none() {
        let bars = bars();
        assert_eq!(window_indices(&bars, 100_000, 200_000), None);
    }
}
