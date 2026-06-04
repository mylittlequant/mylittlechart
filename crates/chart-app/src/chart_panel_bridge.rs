//! Phase 4 — S2.6.A chart-panel series-visibility bridge.
//!
//! Free helpers used by every write site that mutates the 13 series-visibility
//! fields (`show_candles` / … / `chart_type`) on a
//! [`zengeld_chart::state::ChartContainer`]. Each helper mirrors the new
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

use zengeld_chart::state::ChartPanelId;
use zengeld_chart::{DrawingManager, ChartPanelState};

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
// mirror_chart_type — single-field mirror
// =============================================================================

/// Mirror only `chart_type` into `panels_store.chart_panel[chart_panel_id]`.
/// Lazy-inserts the entry on miss. Used by the `ChangeChartType` event handler
/// which only mutates `chart_type` (not the `show_*` bools).
pub fn mirror_chart_type(
    panels_store: &mut TradingPanelsStore,
    chart_panel_id: ChartPanelId,
    chart_type: &'static str,
) {
    let entry = ensure_chart_panel_state(panels_store, chart_panel_id);
    entry.chart_type = chart_type;
}

// =============================================================================
// S2.8 drawing_manager helpers
// =============================================================================

/// Get an immutable reference to the `DrawingManager` for the given panel id.
///
/// Returns `None` if the `ChartPanelState` entry is missing from the store
/// (e.g. the canvas leaf was not yet seeded — treat as no primitives).
pub fn drawing_manager_for_panel(
    panels_store: &TradingPanelsStore,
    chart_panel_id: ChartPanelId,
) -> Option<&DrawingManager> {
    panels_store
        .chart_panel
        .get(&chart_panel_id)
        .map(|panel| &panel.drawing_manager)
}

/// Get a mutable reference to the `DrawingManager` for the given panel id.
///
/// Lazy-inserts a default `ChartPanelState` entry when missing (same policy as
/// `ensure_chart_panel_state`) so callers do not need to pre-seed.
pub fn drawing_manager_for_panel_mut(
    panels_store: &mut TradingPanelsStore,
    chart_panel_id: ChartPanelId,
) -> &mut DrawingManager {
    let entry = panels_store
        .chart_panel
        .entry(chart_panel_id)
        .or_insert_with(|| ChartPanelState::new(chart_panel_id));
    &mut entry.drawing_manager
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use zengeld_chart::state::ChartPanelId;

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
        assert_eq!(entry.chart_type, "candles");
    }

    #[test]
    fn ensure_idempotent_on_second_call() {
        let mut s = store();
        ensure_chart_panel_state(&mut s, pid(2)).chart_type = "line";
        // Second call must NOT re-insert (preserves the mutation).
        let entry = ensure_chart_panel_state(&mut s, pid(2));
        assert_eq!(entry.chart_type, "line", "existing entry must not be replaced");
    }

    #[test]
    fn mirror_chart_type_lazy_inserts_and_sets() {
        let mut s = store();
        mirror_chart_type(&mut s, pid(3), "bars");
        let entry = s.chart_panel.get(&pid(3)).expect("entry must exist after mirror");
        assert_eq!(entry.chart_type, "bars");
        // Other defaults preserved on lazy insert.
        assert!(entry.show_candles);
    }

}
