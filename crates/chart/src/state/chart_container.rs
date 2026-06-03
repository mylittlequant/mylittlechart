//! ChartContainer — the new home for a chart-and-its-sub-panes unit.
//!
//! A `ChartContainer` is the logical grouping of one chart canvas
//! (`ChartPanel`) plus 0..N indicator sub-panes. It lives as one leaf in
//! the outer docking tree (`ChartPanelGrid.docking`); inside, it carries a
//! small uzor `DockState<ChartContainerLeaf>` that arranges the chart and
//! its sub-panes vertically.
//!
//! Phase 1 (C1.2) lands the skeleton: id + inner_docking with exactly one
//! `MainChart` leaf. Container-level fields (symbol, timeframe, exchange,
//! account_type, indicator_source, signal/trade_manager, group_id, etc.)
//! are moved out of `ChartWindow` in C1.4. The full chart panel migration
//! (rename `ChartWindow` -> `ChartPanel`, field ownership transfer) happens
//! across C1.3 + C1.4 + C1.7.
//!
//! See: docs/plans/chart-container-cut-plan-2026-06-03.md §3 Phase 1.

use uzor::layout::DockState;
use uzor::panels::LeafId;

use crate::state::{
    ChartContainerLeaf,
    ChartContainerId,
    ChartPanelId,
    generate_chart_panel_id,
};

/// One chart container (= one chart canvas + its sub-panes), hosted as one
/// leaf in the outer `ChartPanelGrid` docking tree.
pub struct ChartContainer {
    /// Container identity. Reuses `ChartId` (== `ChartContainerId`) so
    /// presets / TagManager / alerts continue to key by it unchanged.
    pub id: ChartContainerId,

    /// Inner uzor docking tree holding the `MainChart` leaf and any
    /// `IndicatorPanel` sub-panes. C1.2 ships this with a single seeded
    /// `MainChart` leaf; Phase 2 wires `split_leaf` / `remove_leaf` on
    /// indicator add / remove.
    pub inner_docking: DockState<ChartContainerLeaf>,

    /// Leaf id of the seeded `MainChart` inside `inner_docking`. Cached so
    /// later phases can find the chart canvas without walking the tree.
    pub main_leaf_id: LeafId,

    /// Identity of the chart canvas itself (the `ChartPanel`). Distinct
    /// from the container id (plan D1). C1.4 attaches the actual
    /// `ChartPanel` struct to the container; this id is its key.
    pub chart_panel_id: ChartPanelId,
}

impl ChartContainer {
    /// Construct an empty container with a freshly seeded `MainChart` leaf.
    ///
    /// The caller supplies the `ChartContainerId`. The `ChartPanelId` is
    /// freshly generated.
    pub fn new(id: ChartContainerId) -> Self {
        let chart_panel_id = generate_chart_panel_id();
        let mut inner_docking: DockState<ChartContainerLeaf> = DockState::new();
        let main_leaf_id = inner_docking
            .tree_mut()
            .add_leaf(ChartContainerLeaf::MainChart { chart_panel_id });
        Self {
            id,
            inner_docking,
            main_leaf_id,
            chart_panel_id,
        }
    }

    /// Construct a container with a caller-supplied `ChartPanelId` (used
    /// when restoring from a preset to preserve the panel id).
    pub fn with_chart_panel_id(
        id: ChartContainerId,
        chart_panel_id: ChartPanelId,
    ) -> Self {
        let mut inner_docking: DockState<ChartContainerLeaf> = DockState::new();
        let main_leaf_id = inner_docking
            .tree_mut()
            .add_leaf(ChartContainerLeaf::MainChart { chart_panel_id });
        Self {
            id,
            inner_docking,
            main_leaf_id,
            chart_panel_id,
        }
    }

    pub fn id(&self) -> ChartContainerId {
        self.id
    }

    pub fn chart_panel_id(&self) -> ChartPanelId {
        self.chart_panel_id
    }

    pub fn main_leaf_id(&self) -> LeafId {
        self.main_leaf_id
    }
}
