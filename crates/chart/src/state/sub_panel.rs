//! ChartSubPanel — lightweight panel type for chart-internal docking.
//!
//! Implements `DockPanel` so a `DockingManager<ChartSubPanel>` can manage
//! multiple chart windows within the single rectangle that the terminal
//! allocates to the chart crate.

use uzor::panels::DockPanel;
use crate::state::ChartId;

/// A sub-panel slot inside the chart's internal split layout.
///
/// Each slot corresponds to exactly one [`ChartWindow`] identified by
/// its [`ChartId`].  The docking engine only needs the title and minimum
/// size — all chart-specific state lives in `ChartPanelGrid`'s window map.
#[derive(Clone, Debug)]
pub struct ChartSubPanel {
    /// Identifies which `ChartWindow` this slot belongs to.
    pub chart_id: ChartId,
    /// Display title shown in the panel header (mirrors `ChartWindow::title`).
    pub title: String,
}

impl ChartSubPanel {
    /// Create a new sub-panel linked to the given chart window.
    pub fn new(chart_id: ChartId, title: impl Into<String>) -> Self {
        Self {
            chart_id,
            title: title.into(),
        }
    }
}

impl DockPanel for ChartSubPanel {
    fn title(&self) -> &str {
        &self.title
    }

    fn type_id(&self) -> &'static str {
        "chart_sub"
    }

    fn min_size(&self) -> (f32, f32) {
        (80.0, 60.0)
    }

    fn closable(&self) -> bool {
        true
    }
}
