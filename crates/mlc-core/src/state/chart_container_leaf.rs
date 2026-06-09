//! Inner-docking leaf payload for ChartContainer.
//!
//! A `ChartContainer` owns a small uzor `DockState<ChartContainerLeaf>` that
//! describes how the main chart canvas and its sub-panes (indicator panels)
//! are stacked vertically inside the container's outer leaf rect.
//!
//! - `MainChart` — the candle canvas (exactly one leaf per container).
//! - `IndicatorPanel { instance_id }` — a sub-pane rendering one indicator
//!   instance. The `instance_id` matches the key used by the indicator
//!   manager and preset persistence, so it round-trips across save/load.
//!
//! Stage 1 of the chart-container cut: the type exists, but containers still
//! have exactly one `MainChart` leaf — Phase 2 wires sub-pane add/remove to
//! `split_leaf` / `remove_leaf` on the inner docking tree.

use uzor::panels::DockPanel;
use crate::state::ChartId;

/// Identifier of a `ChartContainer`.
///
/// Plan D1: containers reuse the existing `ChartId` so external systems
/// (presets, TagManager, alerts) that already key by `ChartId` keep working
/// without a renaming cascade. `ChartContainerId` is the canonical alias to
/// use in container-level code; `ChartId` is the historical name kept for
/// compatibility.
pub type ChartContainerId = ChartId;

/// Identifier of a `ChartPanel` (the candle canvas leaf inside a container).
///
/// Distinct from `ChartId` (which identifies a `ChartContainer`). Plan D1:
/// containers keep the `ChartId` name because external systems (presets,
/// TagManager, alerts) already key by it; the canvas gets its own id.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChartPanelId(pub u64);

impl std::fmt::Display for ChartPanelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

static NEXT_CHART_PANEL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn generate_chart_panel_id() -> ChartPanelId {
    ChartPanelId(NEXT_CHART_PANEL_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

pub fn bump_chart_panel_id_past(min_id: u64) {
    NEXT_CHART_PANEL_ID.fetch_max(min_id + 1, std::sync::atomic::Ordering::SeqCst);
}

/// Leaf payload inside `ChartContainer.inner_docking`.
#[derive(Clone, Debug)]
pub enum ChartContainerLeaf {
    /// The main candle canvas leaf. Exactly one per container.
    MainChart {
        chart_panel_id: ChartPanelId,
    },
    /// An indicator panel rendered as a sub-pane of the container's chart.
    ///
    /// `instance_id` matches `IndicatorInstance::id` from the indicator
    /// manager and `sub_pane_height_ratios` keys in presets.
    IndicatorPanel {
        instance_id: u64,
    },
}

impl ChartContainerLeaf {
    pub fn is_main_chart(&self) -> bool {
        matches!(self, ChartContainerLeaf::MainChart { .. })
    }

    pub fn indicator_instance_id(&self) -> Option<u64> {
        match self {
            ChartContainerLeaf::IndicatorPanel { instance_id } => Some(*instance_id),
            _ => None,
        }
    }
}

impl DockPanel for ChartContainerLeaf {
    fn title(&self) -> &str {
        match self {
            ChartContainerLeaf::MainChart { .. } => "Chart",
            ChartContainerLeaf::IndicatorPanel { .. } => "Indicator",
        }
    }

    fn type_id(&self) -> &'static str {
        match self {
            ChartContainerLeaf::MainChart { .. } => "container_main_chart",
            ChartContainerLeaf::IndicatorPanel { .. } => "container_indicator_panel",
        }
    }

    fn min_size(&self) -> (f32, f32) {
        // Main chart min matches min_main_chart floor used in rects.rs;
        // indicator panels match the existing 30 px floor for sub-pane content.
        match self {
            ChartContainerLeaf::MainChart { .. } => (80.0, 80.0),
            ChartContainerLeaf::IndicatorPanel { .. } => (80.0, 30.0),
        }
    }

    fn closable(&self) -> bool {
        match self {
            ChartContainerLeaf::MainChart { .. } => false,
            ChartContainerLeaf::IndicatorPanel { .. } => true,
        }
    }
}
