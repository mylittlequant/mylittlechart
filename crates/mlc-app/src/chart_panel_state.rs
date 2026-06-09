//! Phase 4 S2.7 — `ChartPanelState` re-export.
//!
//! `ChartPanelState` now lives in `mlc_core::state::chart_panel_state`
//! and is re-exported here for backwards compatibility with any import sites
//! inside chart-app that use `crate::chart_panel_state::ChartPanelState`.

pub use mlc_core::ChartPanelState;
