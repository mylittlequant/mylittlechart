//! `ChartView` — the persistent half of a `Chart`-kind `UiContainer`.
//!
//! Phase 1 stub: empty placeholder.  Phase 4 RENAMES
//! `crate::chart_panel_state::ChartPanelState` → `ChartView`, MOVES the file
//! here, and GROWS it with `scale_settings`, `inner_docking`,
//! `sub_pane_above_main`, the `compare_overlay` display half, and
//! the existing 19 panel-state fields.
//!
//! Phase 4C also adds `chart_type_param` + `chart_type_interval`
//! (fixes existing chart-type reload bug — see audit subplan Bug 2).
//!
//! Shell-agnostic AND serde-clean by invariant — no winit / web_sys
//! / wgpu / vello types may live here.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChartView {
    // Phase 4A: scale_settings
    // Phase 4B: compare_overlay display half
    // Phase 4C: chart_type, chart_type_param, chart_type_interval, 12 show_*
    //          flags, grid_options, crosshair_options, legend, watermark,
    //          tooltip
    // Phase 4D: inner_docking, sub_pane_above_main
    //
    // Phase 1 leaves this empty so the type exists for downstream
    // imports (UiContainer.chart) without committing to a layout yet.
}
