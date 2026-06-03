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

use crate::chart_panel_state::ChartPanelState;
use crate::panels_store::TradingPanelsStore;

// =============================================================================
// SeriesVisibilitySnapshot — Copy transfer struct
// =============================================================================

/// Lightweight snapshot of all 13 series-visibility fields that can be built
/// from a [`zengeld_chart::state::ChartContainer`] reference inside a
/// mutating block, then passed to [`mirror_series_visibility`] after the
/// panel-grid borrow is released.
///
/// All fields are `Copy` so the struct itself is `Copy`.
#[derive(Debug, Clone, Copy)]
pub struct SeriesVisibilitySnapshot {
    pub show_candles: bool,
    pub show_bars: bool,
    pub show_hollow_candles: bool,
    pub show_heikin_ashi: bool,
    pub show_line: bool,
    pub show_step_line: bool,
    pub show_line_markers: bool,
    pub show_area: bool,
    pub show_hlc_area: bool,
    pub show_histogram: bool,
    pub show_columns: bool,
    pub show_baseline: bool,
    pub chart_type: &'static str,
}

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
// mirror_series_visibility — bulk mirror
// =============================================================================

/// Mirror all 13 series-visibility fields from `src` into
/// `panels_store.chart_panel[chart_panel_id]`. Lazy-inserts the entry if
/// missing (e.g. first-time restore before any chart-type user action).
pub fn mirror_series_visibility(
    panels_store: &mut TradingPanelsStore,
    chart_panel_id: ChartPanelId,
    src: SeriesVisibilitySnapshot,
) {
    let entry = ensure_chart_panel_state(panels_store, chart_panel_id);
    entry.show_candles = src.show_candles;
    entry.show_bars = src.show_bars;
    entry.show_hollow_candles = src.show_hollow_candles;
    entry.show_heikin_ashi = src.show_heikin_ashi;
    entry.show_line = src.show_line;
    entry.show_step_line = src.show_step_line;
    entry.show_line_markers = src.show_line_markers;
    entry.show_area = src.show_area;
    entry.show_hlc_area = src.show_hlc_area;
    entry.show_histogram = src.show_histogram;
    entry.show_columns = src.show_columns;
    entry.show_baseline = src.show_baseline;
    entry.chart_type = src.chart_type;
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

    #[test]
    fn mirror_series_visibility_copies_all_fields() {
        let mut s = store();
        let snap = SeriesVisibilitySnapshot {
            show_candles: false,
            show_bars: true,
            show_hollow_candles: false,
            show_heikin_ashi: false,
            show_line: false,
            show_step_line: false,
            show_line_markers: false,
            show_area: false,
            show_hlc_area: false,
            show_histogram: false,
            show_columns: false,
            show_baseline: false,
            chart_type: "bars",
        };
        mirror_series_visibility(&mut s, pid(4), snap);
        let entry = s.chart_panel.get(&pid(4)).expect("entry must exist");
        assert!(!entry.show_candles);
        assert!(entry.show_bars);
        assert_eq!(entry.chart_type, "bars");
    }

    #[test]
    fn mirror_series_visibility_updates_existing_entry() {
        let mut s = store();
        // Seed entry first.
        ensure_chart_panel_state(&mut s, pid(5));
        let snap = SeriesVisibilitySnapshot {
            show_candles: false,
            show_bars: false,
            show_hollow_candles: true,
            show_heikin_ashi: false,
            show_line: false,
            show_step_line: false,
            show_line_markers: false,
            show_area: false,
            show_hlc_area: false,
            show_histogram: false,
            show_columns: false,
            show_baseline: false,
            chart_type: "hollow_candles",
        };
        mirror_series_visibility(&mut s, pid(5), snap);
        let entry = s.chart_panel.get(&pid(5)).expect("entry must exist");
        assert!(entry.show_hollow_candles);
        assert_eq!(entry.chart_type, "hollow_candles");
        assert!(!entry.show_candles, "show_candles updated from snap");
    }
}
