//! Bars-generation stamp (K1 lifecycle groundwork, chart-type-engine-
//! consolidation B2) — the stale-array killer.
//!
//! `DataBubble.bars` (and its derived-slot siblings) is written by whichever
//! chart_type/param combination is currently active; nothing today asserts
//! that a render snapshot's array actually belongs to that combination. A
//! [`BarsGeneration`] is a cheap fingerprint of "which (chart_type, param)
//! last replaced/cleared this bubble's display array" — every
//! `replace_bars`/`clear_bars` call bumps it, every subscribe/install path
//! records the generation it installed, and a render-time comparison
//! between "active generation" and "bubble's stamped generation" turns a
//! silent stale-array bug into a loud dev-log warning (B2 scope: warn only,
//! never panic — full K1 lifecycle enforcement is a later brick).

/// One stamp: which chart_type/param combination produced the array
/// currently sitting in `DataBubble.bars`, plus a monotonic sequence number
/// (bumped on every stamp, even a same-chart_type/param repeat — e.g. a
/// scroll-deepen refold) so two consecutive stamps for the identical
/// combination are still distinguishable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BarsGeneration {
    pub chart_type: &'static str,
    /// Coarse fingerprint of the active fold/subscribe params (ticks/
    /// threshold/interval/alpha — whatever the installing call site has in
    /// scope), NOT a full param struct — this is a diagnostic tripwire, not
    /// a replay key. `0` when the chart_type carries no numeric param.
    pub param_bits: u64,
    pub seq: u32,
}

impl BarsGeneration {
    pub const NONE: BarsGeneration = BarsGeneration { chart_type: "", param_bits: 0, seq: 0 };

    /// Bump to a fresh generation for `chart_type`/`param_bits`, monotonic
    /// across ANY prior stamp (including same chart_type/param repeats).
    pub fn bump(&self, chart_type: &'static str, param_bits: u64) -> BarsGeneration {
        BarsGeneration { chart_type, param_bits, seq: self.seq.wrapping_add(1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_increments_seq_monotonically() {
        let g0 = BarsGeneration::NONE;
        let g1 = g0.bump("candles", 0);
        let g2 = g1.bump("candles", 0);
        assert_eq!(g1.seq, 1);
        assert_eq!(g2.seq, 2);
        assert_ne!(g1, g2, "same chart_type/param repeat must still produce a distinguishable stamp");
    }

    #[test]
    fn bump_carries_chart_type_and_param_bits() {
        let g = BarsGeneration::NONE.bump("range_bar", 8);
        assert_eq!(g.chart_type, "range_bar");
        assert_eq!(g.param_bits, 8);
    }
}
