//! Phase 4 S2.7 — `ChartPanelState` re-export.
//!
//! `ChartPanelState` now lives in `zengeld_chart::state::chart_panel_state`
//! and is re-exported here for backwards compatibility with any import sites
//! inside chart-app that use `crate::chart_panel_state::ChartPanelState`.

pub use zengeld_chart::ChartPanelState;
