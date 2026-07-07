//! Phase 4 — S2.6.A chart-panel series-visibility bridge.
//!
//! Free helpers used by every write site that mutates the 13 series-visibility
//! fields (`show_candles` / … / `chart_type`) on a
//! [`crate::chart_container::ChartContainer`]. Each helper mirrors the new
//! value into `TradingPanelsStore.chart_panel` via
//! `entry().or_insert_with(...)` so callers do not need to seed the entry
//! up-front (lazy-insert).
//!
//! Per the S2.6 audit (`docs/plans/phase4-s2.6-canvas-state-audit.md`):
//! `ChartContainer` fields remain read-authoritative through S2.7; S2.7 flips
//! reads to `panels_store.chart_panel` and removes the duplicate fields.
//!
//! These helpers take **disjoint** `&mut TradingPanelsStore` so callers can
//! run them after dropping any `panel_grid.containers_mut()` borrow — or can
//! capture `window.chart_panel_id` inside the mutating block before the borrow
//! ends and pass it here (split-block pattern matching S2.2..S2.5).
//!
use mlc_core::state::ChartPanelId;
use crate::chart_panel_state::ChartPanelState;

use crate::panels_store::TradingPanelsStore;

// =============================================================================
// ensure_chart_panel_state — lazy-insert helper
// =============================================================================

/// Ensure a [`ChartPanelState`] entry exists for `chart_panel_id`, inserting
/// default state on miss. Returns a mutable reference to the entry.
pub fn ensure_chart_panel_state(
    panels_store: &mut TradingPanelsStore,
    chart_panel_id: ChartPanelId,
) -> &mut ChartPanelState {
    panels_store
        .chart_panel
        .entry(chart_panel_id)
        .or_insert_with(|| ChartPanelState::new(chart_panel_id))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::state::ChartPanelId;

    fn store() -> TradingPanelsStore {
        TradingPanelsStore::new()
    }

    fn pid(n: u64) -> ChartPanelId {
        ChartPanelId(n)
    }

    #[test]
    fn ensure_lazy_inserts_with_defaults() {
        let mut s = store();
        let entry = ensure_chart_panel_state(&mut s, pid(1));
        assert_eq!(entry.chart_panel_id, pid(1));
        assert!(entry.show_candles, "default show_candles must be true");
        assert!(!entry.show_bars, "default show_bars must be false");
    }

    #[test]
    fn ensure_idempotent_on_second_call() {
        let mut s = store();
        ensure_chart_panel_state(&mut s, pid(2)).show_bars = true;
        // Second call must NOT re-insert (preserves the mutation).
        let entry = ensure_chart_panel_state(&mut s, pid(2));
        assert!(entry.show_bars, "existing entry must not be replaced");
    }
}
