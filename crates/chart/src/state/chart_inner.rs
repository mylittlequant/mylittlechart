//! Chart-internal nested layout — main chart + indicator sub-panes.
//!
//! Architecture (Stage 5 redo, NESTED):
//!
//! - `ChartPanelGrid.docking` = OUTER `DockState<ChartSubPanel>`.
//!   One leaf per `ChartWindow` (chart split: left | right etc.).
//!   Untouched by this module.
//!
//! - `ChartWindow.inner_layout: ChartInnerLayout` = INNER
//!   `DockState<ChartInnerLeaf>`.  Lives within one chart leaf rect.
//!   Contains:
//!     - One `MainChart` leaf (always present, top).
//!     - Zero or more `IndicatorPanel { instance_id }` leaves stacked
//!       beneath via SplitBottom.
//!
//! Separator drag between main and sub-pane routes through
//! `DockState::drag_separator` — same code path that handles agent /
//! slot / sidebar / outer chart-split separators.  No custom px math,
//! no `compute_from_chart_panel`, no rendered-heights cache, no
//! "30 px clamp" cascade.  uzor owns the geometry; we own the payload.

use serde::{Deserialize, Serialize};
use uzor::layout::docking::{LeafId, PanelRect, SplitKind};
use uzor::layout::DockState;
use uzor::panels::DockPanel;

use crate::layout::LayoutRect;

/// Inner-leaf payload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum ChartInnerLeaf {
    /// Main candle chart.  Exactly one always exists at the top.
    MainChart,
    /// Indicator sub-pane bound to an `IndicatorInstance`.
    IndicatorPanel { instance_id: u64 },
}

impl ChartInnerLeaf {
    pub fn is_main(&self) -> bool {
        matches!(self, ChartInnerLeaf::MainChart)
    }
    pub fn indicator_id(&self) -> Option<u64> {
        match self {
            ChartInnerLeaf::IndicatorPanel { instance_id } => Some(*instance_id),
            _ => None,
        }
    }
}

impl DockPanel for ChartInnerLeaf {
    fn title(&self) -> &str {
        match self {
            ChartInnerLeaf::MainChart => "chart",
            ChartInnerLeaf::IndicatorPanel { .. } => "ind",
        }
    }
    fn type_id(&self) -> &'static str {
        match self {
            ChartInnerLeaf::MainChart => "chart_main",
            ChartInnerLeaf::IndicatorPanel { .. } => "chart_ind_pane",
        }
    }
    fn min_size(&self) -> (f32, f32) {
        match self {
            ChartInnerLeaf::MainChart => (80.0, 80.0),
            ChartInnerLeaf::IndicatorPanel { .. } => (80.0, 40.0),
        }
    }
    fn closable(&self) -> bool {
        false
    }
}

/// Per-chart-window nested layout owning a uzor `DockState`.
///
/// `DockState` is not `Clone`/`Debug`; we provide both manually via a
/// snapshot round-trip (good enough — the only state that matters
/// across a clone is leaf layout + proportions).
pub struct ChartInnerLayout {
    docking: DockState<ChartInnerLeaf>,
}

impl Clone for ChartInnerLayout {
    fn clone(&self) -> Self {
        Self::from_snapshot(&self.snapshot())
    }
}

impl std::fmt::Debug for ChartInnerLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartInnerLayout")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl Default for ChartInnerLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartInnerLayout {
    pub fn new() -> Self {
        Self { docking: DockState::with_panel(ChartInnerLeaf::MainChart) }
    }

    pub fn docking(&self) -> &DockState<ChartInnerLeaf> {
        &self.docking
    }
    pub fn docking_mut(&mut self) -> &mut DockState<ChartInnerLeaf> {
        &mut self.docking
    }

    pub fn leaf_for_indicator(&self, instance_id: u64) -> Option<LeafId> {
        self.docking.tree().leaves().iter()
            .find(|l| matches!(
                l.panels.first(),
                Some(ChartInnerLeaf::IndicatorPanel { instance_id: id }) if *id == instance_id
            ))
            .map(|l| l.id)
    }

    pub fn main_leaf(&self) -> Option<LeafId> {
        self.docking.tree().leaves().iter()
            .find(|l| matches!(l.panels.first(), Some(ChartInnerLeaf::MainChart)))
            .map(|l| l.id)
    }

    /// Stack the indicator as a new SplitBottom leaf below the last
    /// existing leaf (so panes order = insertion order).  Idempotent.
    pub fn add_indicator(&mut self, instance_id: u64) -> bool {
        if self.leaf_for_indicator(instance_id).is_some() {
            return false;
        }
        let target = self.docking.tree().leaves().last().map(|l| l.id);
        let target = match target {
            Some(id) => id,
            None => return false,
        };
        self.docking.tree_mut().split_leaf(
            target,
            SplitKind::SplitBottom,
            ChartInnerLeaf::IndicatorPanel { instance_id },
        ).is_some()
    }

    pub fn remove_indicator(&mut self, instance_id: u64) -> bool {
        match self.leaf_for_indicator(instance_id) {
            Some(leaf_id) => {
                self.docking.tree_mut().remove_leaf(leaf_id);
                true
            }
            None => false,
        }
    }

    /// Apply uzor separator drag (delta in px).  Returns whether it
    /// hit a separator and was applied.
    pub fn drag_separator(
        &mut self,
        sep_idx: usize,
        delta: f32,
        content_width: f32,
        content_height: f32,
    ) -> bool {
        self.docking.drag_separator(sep_idx, delta, content_width, content_height)
    }

    /// Compute the layout for the given window rect and return
    /// (leaf payload, leaf id, absolute leaf rect) tuples.
    pub fn layout_leaves(&mut self, window_rect: LayoutRect) -> Vec<(ChartInnerLeaf, LeafId, LayoutRect)> {
        // uzor's DockState::layout takes a PanelRect (f32 coords) and
        // resolves rects relative to that area's origin.  We pass an
        // origin-zero area and add `window_rect.x/y` per leaf below.
        let area = PanelRect::new(0.0, 0.0, window_rect.width as f32, window_rect.height as f32);
        self.docking.layout(area);
        let panel_rects = self.docking.panel_rects().clone();
        self.docking.tree().leaves().iter().map(|leaf| {
            let payload = leaf.panels.first().cloned().unwrap_or(ChartInnerLeaf::MainChart);
            let rect = panel_rects.get(&leaf.id).cloned()
                .map(|r| LayoutRect {
                    x: window_rect.x + r.x as f64,
                    y: window_rect.y + r.y as f64,
                    width: r.width as f64,
                    height: r.height as f64,
                })
                .unwrap_or(LayoutRect {
                    x: window_rect.x,
                    y: window_rect.y,
                    width: window_rect.width,
                    height: window_rect.height,
                });
            (payload, leaf.id, rect)
        }).collect()
    }

    /// Hit-test the inner tree's separators.  Returns the separator
    /// index when (x, y) lands on one (with 6 px tolerance perpendicular
    /// to its axis — matches the historical sub-pane separator hit-zone).
    ///
    /// (x, y) are SCREEN coordinates; window_rect provides the offset.
    pub fn separator_at(
        &mut self,
        window_rect: LayoutRect,
        x: f64,
        y: f64,
    ) -> Option<usize> {
        let area = PanelRect::new(0.0, 0.0, window_rect.width as f32, window_rect.height as f32);
        self.docking.layout(area);
        let lx = (x - window_rect.x) as f32;
        let ly = (y - window_rect.y) as f32;
        const TOL: f32 = 6.0;
        for (idx, sep) in self.docking.separators().iter().enumerate() {
            // Separator geometry per uzor:
            // - Horizontal split (one above the other): orientation = Horizontal,
            //   position = y, start = x left, length = x width.
            // - Vertical split (side by side): orientation = Vertical,
            //   position = x, start = y top, length = y height.
            let hit = match sep.orientation {
                uzor::panels::SeparatorOrientation::Horizontal => {
                    lx >= sep.start && lx <= sep.start + sep.length &&
                    ly >= sep.position - TOL && ly <= sep.position + TOL
                }
                uzor::panels::SeparatorOrientation::Vertical => {
                    ly >= sep.start && ly <= sep.start + sep.length &&
                    lx >= sep.position - TOL && lx <= sep.position + TOL
                }
            };
            if hit {
                return Some(idx);
            }
        }
        None
    }

    pub fn snapshot(&self) -> ChartInnerSnapshot {
        let leaves: Vec<ChartInnerLeaf> = self.docking.tree().leaves().iter()
            .filter_map(|l| l.panels.first().cloned())
            .collect();
        // Root-level proportions only (the inner tree is a single
        // vertical stack — every leaf is a direct child of the root).
        let proportions = self.docking.tree().proportions();
        ChartInnerSnapshot { leaves, proportions }
    }

    pub fn from_snapshot(snap: &ChartInnerSnapshot) -> Self {
        let mut layout = ChartInnerLayout::new();
        for leaf in snap.leaves.iter().skip(1) {
            if let ChartInnerLeaf::IndicatorPanel { instance_id } = leaf {
                layout.add_indicator(*instance_id);
            }
        }
        if !snap.proportions.is_empty() {
            layout.docking.tree_mut().set_proportions(snap.proportions.clone());
        }
        layout
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChartInnerSnapshot {
    pub leaves: Vec<ChartInnerLeaf>,
    #[serde(default)]
    pub proportions: Vec<f64>,
}
