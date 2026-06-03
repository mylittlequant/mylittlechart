//! Phase 4 — `ChartPanelState` skeleton (S2.1).
//!
//! Empty stub today. Per the S2 handoff
//! (`docs/plans/phase4-s2-handoff-2026-06-04.md`), subsequent S2.X commits
//! migrate canvas-leaf fields out of `ChartContainer` into this struct, stored
//! in `TradingPanelsStore.chart_panel` keyed by `ChartPanelId`.
//!
//! Until then this is intentionally trivial — its presence lets S2.2..S2.6
//! land field migrations behind a stable storage map without churning the
//! type signature each commit.

use zengeld_chart::state::ChartPanelId;

/// State of one chart canvas leaf.
///
/// Stored in `TradingPanelsStore.chart_panel`, keyed by [`ChartPanelId`].
/// Phase 4 / S2.6 migrates the bulk of [`zengeld_chart::state::ChartContainer`]
/// fields here (bars / viewport / scales / interaction / display options /
/// scale_settings / toolbar / async lifecycle / price_lines / compare_overlay
/// / command_history / drawing_manager). S2.1 keeps the body empty so the
/// storage map can be wired with zero risk to consumers.
#[derive(Debug)]
pub struct ChartPanelState {
    pub chart_panel_id: ChartPanelId,
}

impl ChartPanelState {
    /// Construct an empty state for a freshly-allocated canvas leaf.
    pub fn new(chart_panel_id: ChartPanelId) -> Self {
        Self { chart_panel_id }
    }
}
