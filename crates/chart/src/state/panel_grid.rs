//! ChartPanelGrid — input-aware panel grid for the chart crate.
//!
//! Handles all split / expand / layout functionality for chart sub-windows.
//! [`ChartPanelGrid::resolve_input`] maps an absolute screen coordinate to a
//! [`ChartInputTarget`] describing the exact chart element that was hit.

use std::collections::HashMap;

use uzor::panels::{DockingManager, DockingTree, LeafId, PanelRect, SplitKind, SeparatorOrientation};

use crate::state::{ChartWindow, ChartId, Timeframe};
use crate::state::generate_chart_id;
use super::sub_panel::ChartSubPanel;

// Re-export so callers that import from this module get everything they need.
pub use uzor::panels::SeparatorOrientation as PanelSeparatorOrientation;

// =========================================================================
// ChartInputTarget
// =========================================================================

/// Result of resolving an input point against the chart panel grid.
///
/// This replaces the scattered hit-testing logic that was duplicated
/// across app/src/input/{drag,scroll,click,mouse,mod}.rs.
#[derive(Debug, Clone)]
pub enum ChartInputTarget {
    /// Point falls inside a sub-chart's main chart canvas.
    Chart {
        leaf_id: LeafId,
    },
    /// Point is on the price scale of a sub-chart.
    PriceScale {
        leaf_id: LeafId,
    },
    /// Point is on the time scale of a sub-chart.
    TimeScale {
        leaf_id: LeafId,
    },
    /// Point is on the scale corner button area.
    ScaleCorner {
        leaf_id: LeafId,
        button: crate::layout::ScaleCornerButton,
    },
    /// Point is on a separator between sub-charts.
    Separator {
        idx: usize,
        orientation: SeparatorOrientation,
    },
    /// Point is outside all sub-chart panels and separators.
    None,
}

// =========================================================================
// SplitHitResult (kept for backward-compat during migration)
// =========================================================================

/// Hit-test result for chart-internal splits.
///
/// Kept for backward compatibility while migrating callers to
/// [`ChartInputTarget`] / [`ChartPanelGrid::resolve_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitHitResult {
    /// Point falls inside the given leaf sub-chart.
    Leaf(LeafId),
    /// Point is on a separator between sub-charts (index into `docking.separators()`).
    Separator(usize),
    /// Point is outside all sub-chart panels and separators.
    None,
}

// =========================================================================
// ChartPanelGrid
// =========================================================================

/// Manages chart-internal splits using `uzor-panels`.
///
/// One instance lives inside the chart panel.  The terminal only sees the
/// outer rectangle; `ChartPanelGrid` handles everything inside, including
/// [`resolve_input`](ChartPanelGrid::resolve_input) for unified hit-testing.
pub struct ChartPanelGrid {
    /// Panel tree (geometry + panel metadata).
    docking: DockingManager<ChartSubPanel>,
    /// All chart windows keyed by their `ChartId`.
    windows: HashMap<ChartId, ChartWindow>,
    /// Leaf-to-ChartId mapping (mirrors the tree's leaves).
    leaf_to_chart: HashMap<LeafId, ChartId>,
    /// Whether expand mode is active (all but active leaf hidden).
    expanded: bool,
}

impl ChartPanelGrid {
    // =========================================================================
    // Construction
    // =========================================================================

    /// Create a panel grid from an initial chart window.
    ///
    /// The window becomes the single visible leaf.
    pub fn new(initial_window: ChartWindow) -> Self {
        let chart_id = initial_window.id;
        let title = initial_window.title.clone();

        let panel = ChartSubPanel::new(chart_id, title);
        let docking = DockingManager::with_panel(panel);

        // Retrieve the leaf ID assigned to the panel we just added.
        let leaf_id = docking
            .active_leaf()
            .expect("DockingManager::with_panel always sets an active leaf");

        let mut windows = HashMap::new();
        windows.insert(chart_id, initial_window);

        let mut leaf_to_chart = HashMap::new();
        leaf_to_chart.insert(leaf_id, chart_id);

        Self {
            docking,
            windows,
            leaf_to_chart,
            expanded: false,
        }
    }

    // =========================================================================
    // Query
    // =========================================================================

    /// Returns `true` when more than one sub-chart exists.
    pub fn is_split(&self) -> bool {
        self.docking.tree().leaf_count() > 1
    }

    /// Returns `true` when expand mode is active.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Immutable reference to the active (focused) `ChartWindow`.
    ///
    /// Falls back to an arbitrary window if the active leaf is not mapped
    /// (should not happen in normal usage).
    pub fn active_window(&self) -> Option<&ChartWindow> {
        let chart_id = self.active_chart_id()?;
        self.windows.get(&chart_id)
    }

    /// Mutable reference to the active (focused) `ChartWindow`.
    pub fn active_window_mut(&mut self) -> Option<&mut ChartWindow> {
        let chart_id = self.active_chart_id()?;
        self.windows.get_mut(&chart_id)
    }

    /// `ChartId` of the active window, or `None` if no active leaf exists.
    pub fn active_chart_id(&self) -> Option<ChartId> {
        let leaf_id = self.docking.active_leaf()?;
        self.leaf_to_chart.get(&leaf_id).copied()
    }

    /// Immutable reference to a window by leaf ID.
    pub fn window_for_leaf(&self, leaf_id: LeafId) -> Option<&ChartWindow> {
        let chart_id = self.leaf_to_chart.get(&leaf_id)?;
        self.windows.get(chart_id)
    }

    /// Mutable reference to a window by leaf ID.
    pub fn window_for_leaf_mut(&mut self, leaf_id: LeafId) -> Option<&mut ChartWindow> {
        let chart_id = *self.leaf_to_chart.get(&leaf_id)?;
        self.windows.get_mut(&chart_id)
    }

    /// Iterate over all `(LeafId, &ChartWindow)` pairs in insertion order.
    pub fn iter_windows(&self) -> impl Iterator<Item = (LeafId, &ChartWindow)> {
        self.leaf_to_chart
            .iter()
            .filter_map(move |(&leaf_id, chart_id)| {
                self.windows.get(chart_id).map(|w| (leaf_id, w))
            })
    }

    /// Immutable reference to the underlying windows map.
    pub fn windows(&self) -> &HashMap<ChartId, ChartWindow> {
        &self.windows
    }

    /// Mutable reference to the underlying windows map.
    ///
    /// Callers can iterate or look up windows by `ChartId`.  This is the
    /// recommended pattern when mutable access to all sub-windows is needed,
    /// because returning a combined `(LeafId, &mut ChartWindow)` iterator
    /// would require simultaneous borrows of both `leaf_to_chart` and
    /// `windows`, which the borrow checker disallows.
    pub fn windows_mut(&mut self) -> &mut HashMap<ChartId, ChartWindow> {
        &mut self.windows
    }

    /// Resolve a `LeafId` to its associated `ChartId`, if any.
    pub fn chart_id_for_leaf(&self, leaf_id: LeafId) -> Option<ChartId> {
        self.leaf_to_chart.get(&leaf_id).copied()
    }

    /// Resolve a `ChartId` to the `LeafId` that hosts it, if any.
    pub fn leaf_for_chart_id(&self, chart_id: ChartId) -> Option<LeafId> {
        self.leaf_to_chart
            .iter()
            .find_map(|(&leaf, &cid)| if cid == chart_id { Some(leaf) } else { None })
    }

    /// Immutable reference to the underlying `DockingManager`.
    pub fn docking(&self) -> &DockingManager<ChartSubPanel> {
        &self.docking
    }

    /// Mutable reference to the underlying `DockingManager`.
    pub fn docking_mut(&mut self) -> &mut DockingManager<ChartSubPanel> {
        &mut self.docking
    }

    /// Replace the entire docking layout from a restored tree.
    ///
    /// This is the primary method for loading a saved layout.  The caller is
    /// responsible for building the `DockingTree` (typically via
    /// [`LayoutSnapshot::restore_tree`]) and for keeping `windows` /
    /// `leaf_to_chart` in sync with the new leaf IDs present in the tree.
    ///
    /// After this call the derived geometry (separators, rects, etc.) is
    /// cleared.  Call [`layout`](Self::layout) once before the next render to
    /// recompute positions.
    pub fn replace_docking(&mut self, tree: DockingTree<ChartSubPanel>) {
        self.docking = DockingManager::from_tree(tree);
    }

    /// Replace the entire windows map with the given one.
    ///
    /// Used during preset restore.  The caller is responsible for ensuring the
    /// new map is consistent with the docking tree and `leaf_to_chart`.
    pub fn replace_windows(&mut self, windows: HashMap<ChartId, ChartWindow>) {
        self.windows = windows;
    }

    /// Replace the leaf-to-chart mapping with the given one.
    ///
    /// Used during preset restore after [`replace_docking`] has been called
    /// with the new tree so that every `LeafId` in the new tree maps to the
    /// correct `ChartId`.
    pub fn replace_leaf_to_chart(&mut self, map: HashMap<LeafId, ChartId>) {
        self.leaf_to_chart = map;
    }

    pub fn reassign_active_chart_id(&mut self) {
        let active_leaf = match self.docking.active_leaf() {
            Some(id) => id,
            None => return,
        };
        let old_chart_id = match self.leaf_to_chart.get(&active_leaf).copied() {
            Some(id) => id,
            None => return,
        };
        let new_chart_id = generate_chart_id();
        if let Some(mut window) = self.windows.remove(&old_chart_id) {
            window.id = new_chart_id;
            self.windows.insert(new_chart_id, window);
        }
        self.leaf_to_chart.insert(active_leaf, new_chart_id);
    }

    // =========================================================================
    // Layout
    // =========================================================================

    /// Compute the layout of all sub-charts within `area`.
    ///
    /// Must be called every frame before calling [`panel_rects`].
    pub fn layout(&mut self, area: PanelRect) {
        self.docking.layout(area);
    }

    /// Get computed panel rects from the last [`layout`] call.
    ///
    /// Each `LeafId` maps to the screen rectangle its sub-chart should
    /// be rendered into.
    pub fn panel_rects(&self) -> &HashMap<LeafId, PanelRect> {
        self.docking.panel_rects()
    }

    // =========================================================================
    // Split / Close
    // =========================================================================

    /// Split the active leaf with the given `SplitKind`.
    ///
    /// Creates a cloned `ChartWindow` for each new leaf produced by the split.
    /// The original window remains associated with the first new leaf.
    ///
    /// Returns the `LeafId`s of the newly created leaves, or an empty `Vec`
    /// if there is no active leaf.
    pub fn split_active(&mut self, split: SplitKind) -> Vec<LeafId> {
        let active_leaf = match self.docking.active_leaf() {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Snapshot the source chart ID before we mutate the tree.
        let source_chart_id = match self.leaf_to_chart.get(&active_leaf).copied() {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Ask the docking tree to split the leaf.
        // We pass 0.0 for width/height — the tree uses those only for custom
        // rect computation which we don't use here.
        let new_leaf_ids = self
            .docking
            .tree_mut()
            .split_leaf(active_leaf, split, 0.0, 0.0);

        if new_leaf_ids.is_empty() {
            return Vec::new();
        }

        // The first new leaf inherits the original window's chart ID.
        // Subsequent leaves get fresh clones with new chart IDs.
        let source_window = match self.windows.get(&source_chart_id) {
            Some(w) => w,
            None => return Vec::new(),
        };

        // Re-map the first leaf to the existing window.
        if let Some(&first_id) = new_leaf_ids.first() {
            self.leaf_to_chart.remove(&active_leaf);
            self.leaf_to_chart.insert(first_id, source_chart_id);

            // Update the panel title in the docking tree for the first leaf.
            if let Some(leaf) = self.docking.tree_mut().leaf_mut(first_id) {
                if let Some(panel) = leaf.active_panel_mut() {
                    panel.title = source_window.title.clone();
                }
            }
        }

        // For every subsequent new leaf, clone the source window.
        let mut created_ids = Vec::with_capacity(new_leaf_ids.len().saturating_sub(1));
        for &leaf_id in new_leaf_ids.iter().skip(1) {
            let new_chart_id = generate_chart_id();

            // We need a fresh clone from the source window each time.
            // Re-borrow since the borrow above ended at the `first_id` block.
            let cloned = {
                let src = self
                    .windows
                    .get(&source_chart_id)
                    .expect("source window must exist");
                src.clone_for_split(new_chart_id, true)
            };

            let cloned_title = cloned.title.clone();

            self.windows.insert(new_chart_id, cloned);
            self.leaf_to_chart.insert(leaf_id, new_chart_id);

            // Update the panel stored inside the leaf.
            if let Some(leaf) = self.docking.tree_mut().leaf_mut(leaf_id) {
                if let Some(panel) = leaf.active_panel_mut() {
                    panel.chart_id = new_chart_id;
                    panel.title = cloned_title;
                }
            }

            created_ids.push(leaf_id);
        }

        // Set the first new leaf as active.
        if let Some(&first_id) = new_leaf_ids.first() {
            self.docking.set_active_leaf(first_id);
        }

        // Exit expand mode when splitting (the layout changed).
        self.expanded = false;

        new_leaf_ids
    }

    /// Close a leaf (remove its sub-chart).
    ///
    /// The last remaining leaf cannot be closed.
    /// Returns `true` if the leaf was removed, `false` otherwise.
    pub fn close_leaf(&mut self, leaf_id: LeafId) -> bool {
        if self.docking.tree().leaf_count() <= 1 {
            return false;
        }

        let chart_id = match self.leaf_to_chart.remove(&leaf_id) {
            Some(id) => id,
            None => return false,
        };

        self.windows.remove(&chart_id);
        self.docking.tree_mut().remove_leaf(leaf_id);

        // Exit expand mode; the layout changed.
        self.expanded = false;

        true
    }

    /// Reset to single-panel layout by closing all leaves except the active one.
    pub fn set_layout_single(&mut self) {
        if !self.is_split() {
            return;
        }
        let active_leaf = self.docking.active_leaf();
        let to_close: Vec<LeafId> = self.leaf_to_chart.keys()
            .filter(|&&id| Some(id) != active_leaf)
            .copied()
            .collect();
        for id in to_close {
            self.close_leaf(id);
        }
        self.expanded = false;
    }

    /// Reset all split proportions to equal sizes.
    pub fn reset_sizes(&mut self) {
        self.docking.tree_mut().reset_proportions();
    }

    // =========================================================================
    // Expand Toggle
    // =========================================================================

    /// Toggle expand mode.
    ///
    /// When entering expand mode, all leaves except the active one are hidden.
    /// When leaving expand mode, all hidden leaves are shown again.
    ///
    /// Does nothing if there is only one leaf (nothing to expand/collapse).
    pub fn toggle_expand(&mut self) {
        if !self.is_split() {
            return;
        }

        if self.expanded {
            // Show all hidden leaves.
            let all_leaf_ids: Vec<LeafId> = self.leaf_to_chart.keys().copied().collect();
            for leaf_id in all_leaf_ids {
                self.docking.tree_mut().show_leaf(leaf_id);
            }
            self.expanded = false;
        } else {
            // Hide all leaves except the active one.
            let active_leaf = self.docking.active_leaf();
            let all_leaf_ids: Vec<LeafId> = self.leaf_to_chart.keys().copied().collect();
            for leaf_id in all_leaf_ids {
                if Some(leaf_id) != active_leaf {
                    self.docking.tree_mut().hide_leaf(leaf_id);
                }
            }
            self.expanded = true;
        }
    }

    // =========================================================================
    // Hit Testing (legacy — use resolve_input for new code)
    // =========================================================================

    /// Hit-test a point in content-area coordinates against split panel rects and separators.
    ///
    /// Coordinates must be relative to the top-left of the content area passed
    /// to [`layout`].  Separators have higher priority than leaf panels so that
    /// thin separator areas are always reachable for dragging.
    ///
    /// Returns:
    /// - [`SplitHitResult::Separator`] when cursor is on a divider
    /// - [`SplitHitResult::Leaf`] when cursor is inside a sub-chart
    /// - [`SplitHitResult::None`] otherwise
    pub fn hit_test_point(&self, x: f32, y: f32) -> SplitHitResult {
        // Separators take priority (they sit between leaf rects and are thin).
        for (idx, sep) in self.docking.separators().iter().enumerate() {
            if sep.hit_test(x, y) {
                return SplitHitResult::Separator(idx);
            }
        }

        for (&leaf_id, rect) in self.panel_rects() {
            if x >= rect.x
                && x < rect.x + rect.width
                && y >= rect.y
                && y < rect.y + rect.height
            {
                return SplitHitResult::Leaf(leaf_id);
            }
        }
        SplitHitResult::None
    }

    /// Apply a pixel delta to a separator to resize the adjacent sub-charts.
    ///
    /// `sep_idx` is an index into `docking.separators()`.  `delta` is the
    /// signed pixel movement along the separator's axis (positive = right/down).
    ///
    /// The content area width/height must be supplied so that the parent
    /// branch rect can be computed for correct proportion calculation.
    pub fn apply_separator_drag(&mut self, sep_idx: usize, delta: f32, content_width: f32, content_height: f32) {
        use uzor::panels::SeparatorLevel;

        // Snapshot separator info to avoid borrow conflicts.
        let (parent_id, child_a_raw, child_b_raw, orientation) = {
            let sep = match self.docking.separators().get(sep_idx) {
                Some(s) => s,
                None => return,
            };
            let (parent_id, child_a, child_b) = match &sep.level {
                SeparatorLevel::Node { parent_id, child_a, child_b } => {
                    (*parent_id, *child_a, *child_b)
                }
            };
            (parent_id, child_a, child_b, sep.orientation)
        };

        // Always use the full content area (root rect) for proportion
        // calculation.  Proportions are relative to the root, not to
        // the parent branch.
        let total_size = match orientation {
            SeparatorOrientation::Horizontal => content_height,
            SeparatorOrientation::Vertical => content_width,
        };

        // Retrieve the current proportions of the parent branch.
        let branch = match self.docking.tree().find_branch(parent_id) {
            Some(b) => b,
            None => return,
        };

        // Build ordered (child_id, proportion) list for the non-hidden children.
        // `proportions` is parallel to `children` (hidden and visible combined),
        // so we must skip hidden entries to get the correct visual ordering.
        let n = branch.children.len();
        if n < 2 {
            return;
        }

        // Retrieve the raw proportion slice (one entry per child, including hidden).
        let raw_props: Vec<f64> = if branch.proportions.len() == n {
            branch.proportions.clone()
        } else {
            vec![1.0_f64 / n as f64; n]
        };

        // Locate child_a and child_b by their raw IDs.
        let pos_a = branch.children.iter().position(|c| c.raw_id() == child_a_raw);
        let pos_b = branch.children.iter().position(|c| c.raw_id() == child_b_raw);

        let (pos_a, pos_b) = match (pos_a, pos_b) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        };

        // Ensure total_size is usable.
        if total_size <= 0.0 {
            return;
        }

        // Convert pixel delta to a proportion delta relative to the total share sum.
        let total_share: f64 = raw_props.iter().sum();
        let delta_share = (delta as f64 / total_size as f64) * total_share;

        // Apply the delta: left/top child gains, right/bottom child loses.
        let mut new_props = raw_props.clone();
        new_props[pos_a] += delta_share;
        new_props[pos_b] -= delta_share;

        // Clamp to a minimum of 5% each (prevent collapsing panels).
        let min_share = 0.05 * total_share;
        if new_props[pos_a] < min_share || new_props[pos_b] < min_share {
            return;
        }

        // Commit new proportions.
        self.docking.tree_mut().set_branch_proportions(parent_id, new_props);
    }

    /// Return the orientation of separator at `sep_idx`, if it exists.
    pub fn separator_orientation(&self, sep_idx: usize) -> Option<SeparatorOrientation> {
        self.docking.separators().get(sep_idx).map(|s| s.orientation)
    }

    /// Update separator hover state based on cursor position in content-area coordinates.
    ///
    /// Returns `true` when any separator is under the cursor (so the caller can
    /// change the cursor to a resize cursor).
    pub fn update_separator_hover(&mut self, x: f32, y: f32) -> bool {
        self.docking.update_separator_hover(x, y)
    }

    // =========================================================================
    // Active Leaf Selection
    // =========================================================================

    /// Set the active leaf by leaf ID.
    pub fn set_active_leaf(&mut self, leaf_id: LeafId) {
        self.docking.set_active_leaf(leaf_id);
    }

    /// Set the active leaf and return a mutable reference to its `ChartWindow`.
    ///
    /// Updates the docking manager's focus and looks up the window associated
    /// with the given leaf.  Returns `None` if the leaf is not mapped.
    pub fn activate_leaf(&mut self, leaf_id: LeafId) -> Option<&mut ChartWindow> {
        self.docking.set_active_leaf(leaf_id);
        let chart_id = *self.leaf_to_chart.get(&leaf_id)?;
        self.windows.get_mut(&chart_id)
    }

    // =========================================================================
    // resolve_input — unified input hit-testing
    // =========================================================================

    /// Resolve an input point to determine what chart element was hit.
    ///
    /// `x`, `y` are in absolute screen coordinates.
    /// `content_origin_x`, `content_origin_y` are the absolute coordinates of
    /// the content area top-left (the rectangle passed to [`layout`]).
    ///
    /// This method:
    /// 1. Converts to content-local coordinates.
    /// 2. Checks separators first (they have priority over leaf panels).
    /// 3. For each leaf, computes `ChartAreaLayout` and checks sub-areas
    ///    (scale corner, price scale, time scale, chart canvas).
    ///
    /// The caller does NOT need to know about chart internals — all sub-area
    /// discrimination is done here.
    pub fn resolve_input(
        &self,
        x: f64,
        y: f64,
        content_origin_x: f64,
        content_origin_y: f64,
    ) -> ChartInputTarget {
        let local_x = (x - content_origin_x) as f32;
        let local_y = (y - content_origin_y) as f32;

        // 1. Separators take priority (they are thin and easily missed otherwise).
        for (idx, sep) in self.docking.separators().iter().enumerate() {
            if sep.hit_test(local_x, local_y) {
                return ChartInputTarget::Separator {
                    idx,
                    orientation: sep.orientation,
                };
            }
        }

        // 2. Check each leaf's sub-areas.
        for (&leaf_id, rect) in self.panel_rects() {
            // Quick bounds check in local (f32) space.
            if local_x < rect.x
                || local_x >= rect.x + rect.width
                || local_y < rect.y
                || local_y >= rect.y + rect.height
            {
                continue;
            }

            // This leaf was hit — resolve scale settings for sub-area layout.
            let window = match self.window_for_leaf(leaf_id) {
                Some(w) => w,
                // No window mapped: treat the whole leaf as the chart canvas.
                None => return ChartInputTarget::Chart { leaf_id },
            };

            // Build the absolute LayoutRect for this leaf so that
            // ChartAreaLayout::compute works in absolute screen coordinates
            // (matching the absolute `x`, `y` passed in).
            let available = crate::layout::LayoutRect {
                x: content_origin_x + rect.x as f64,
                y: content_origin_y + rect.y as f64,
                width: rect.width as f64,
                height: rect.height as f64,
            };

            let chart_layout = crate::layout::ChartAreaLayout::compute(
                available,
                window.scale_settings.price_scale_width,
                window.scale_settings.time_scale_height,
            );

            // Check scale corner first (smallest area → highest priority).
            if chart_layout.scale_corner.contains(x, y) {
                // Build hit zones that match the rendered button layout.
                let corner = &chart_layout.scale_corner;
                let am_width = 14.0_f64;
                let spacing = 4.0_f64;
                let mode_width = 20.0_f64;
                let total_width = am_width + spacing + mode_width;
                let start_x = corner.center_x() - total_width / 2.0;
                let zones = crate::layout::ScaleCornerHitZones {
                    am_button: crate::layout::LayoutRect::new(
                        start_x,
                        corner.y,
                        am_width,
                        corner.height,
                    ),
                    mode_button: crate::layout::LayoutRect::new(
                        start_x + am_width + spacing,
                        corner.y,
                        mode_width,
                        corner.height,
                    ),
                };
                return ChartInputTarget::ScaleCorner {
                    leaf_id,
                    button: zones.hit_test(x, y),
                };
            }

            // Check price scale.
            if chart_layout.price_scale.contains(x, y) {
                return ChartInputTarget::PriceScale { leaf_id };
            }

            // Check time scale.
            if chart_layout.time_scale.contains(x, y) {
                return ChartInputTarget::TimeScale { leaf_id };
            }

            // Default: the chart canvas.
            return ChartInputTarget::Chart { leaf_id };
        }

        ChartInputTarget::None
    }
}

// =========================================================================
// Default
// =========================================================================

impl Default for ChartPanelGrid {
    fn default() -> Self {
        let window = ChartWindow::new("BTCUSD", Timeframe::h1());
        Self::new(window)
    }
}
