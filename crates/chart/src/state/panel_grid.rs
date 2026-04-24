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
    /// Per-leaf minimum pixel width used as a lower bound during separator drag.
    ///
    /// Set by the caller before each drag via `set_leaf_min_width`.
    /// Any leaf not present in the map is treated as having min_width = 0.
    leaf_min_widths: HashMap<LeafId, f32>,
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
            leaf_min_widths: HashMap::new(),
        }
    }

    /// Minimal grid used as temporary stand-in during `mem::replace` in preset cache swaps.
    pub fn placeholder() -> Self {
        use crate::state::Timeframe;
        Self::new(ChartWindow::new("PLACEHOLDER", Timeframe::h1()))
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

    // =========================================================================
    // Per-leaf minimum width
    // =========================================================================

    /// Set a minimum pixel width for the given leaf.
    ///
    /// This is consulted by [`apply_separator_drag`] to prevent a leaf from
    /// shrinking below the price-scale width + padding.  Call this before each
    /// separator drag (or whenever scale widths change).
    pub fn set_leaf_min_width(&mut self, leaf_id: LeafId, min_width: f32) {
        self.leaf_min_widths.insert(leaf_id, min_width);
    }

    /// Get the minimum pixel width for the given leaf (0.0 if not set).
    pub fn leaf_min_width(&self, leaf_id: LeafId) -> f32 {
        self.leaf_min_widths.get(&leaf_id).copied().unwrap_or(0.0)
    }

    /// Recursively compute the minimum pixel width for a subtree node.
    ///
    /// For a leaf node: looks up `leaf_min_widths`.
    /// For a branch node: returns the maximum of all leaf min-widths in the subtree
    /// (because the branch as a whole cannot be smaller than its widest required leaf).
    fn min_width_for_node(&self, node: &uzor::panels::PanelNode<ChartSubPanel>) -> f32 {
        use uzor::panels::PanelNode;
        match node {
            PanelNode::Leaf(l) => {
                const CHART_AREA_MIN: f64 = 10.0;
                let scale_w = self
                    .leaf_to_chart
                    .get(&l.id)
                    .and_then(|cid| self.windows.get(cid))
                    .map(|w| w.scale_settings.effective_price_scale_width())
                    .unwrap_or(crate::scale_settings::DEFAULT_PRICE_SCALE_WIDTH);
                ((scale_w + CHART_AREA_MIN) as f32).max(Self::LEAF_MIN_WIDTH)
            }
            PanelNode::Branch(branch) => {
                use uzor::panels::WindowLayout;
                let child_mins = branch.children.iter().map(|c| self.min_width_for_node(c));
                match branch.layout {
                    // Side-by-side layouts: total min width is the sum of children.
                    WindowLayout::SplitHorizontal
                    | WindowLayout::ThreeColumns
                    | WindowLayout::OneLeftTwoRight
                    | WindowLayout::TwoLeftOneRight
                    | WindowLayout::Grid2x2 => child_mins.sum(),
                    // Stacked layouts: min width is the max of children.
                    _ => child_mins.fold(0.0_f32, f32::max),
                }
            }
        }
    }

    /// Recursively compute the minimum pixel HEIGHT for a subtree node.
    /// Mirror of `min_width_for_node` but with axes swapped:
    /// - stacked layouts (horizontal separators) sum children
    /// - side-by-side layouts take the max
    fn min_height_for_node(&self, node: &uzor::panels::PanelNode<ChartSubPanel>) -> f32 {
        use uzor::panels::PanelNode;
        match node {
            PanelNode::Leaf(_) => Self::LEAF_MIN_HEIGHT,
            PanelNode::Branch(branch) => {
                use uzor::panels::WindowLayout;
                let child_mins = branch.children.iter().map(|c| self.min_height_for_node(c));
                match branch.layout {
                    // Side-by-side layouts share a horizontal strip — max height.
                    WindowLayout::SplitHorizontal
                    | WindowLayout::ThreeColumns
                    | WindowLayout::OneLeftTwoRight
                    | WindowLayout::TwoLeftOneRight => child_mins.fold(0.0_f32, f32::max),
                    // Grid2x2 stacks rows — treat as sum.
                    WindowLayout::Grid2x2 => child_mins.sum(),
                    // Stacked layouts (SplitVertical / ThreeRows etc.) sum.
                    _ => child_mins.sum(),
                }
            }
        }
    }

    /// Absolute hard-coded minimum width of any single chart leaf (pixels).
    /// A leaf can NEVER be squished below this — neither by its own separator
    /// nor by the sidebar separator.
    pub const LEAF_MIN_WIDTH: f32 = 80.0;

    /// Absolute hard-coded minimum height of any single chart leaf (pixels).
    pub const LEAF_MIN_HEIGHT: f32 = 60.0;

    /// Compute the minimum chart width that must remain visible when the sidebar
    /// separator is dragged.
    ///
    /// Walks the docking tree: horizontal layouts (side-by-side columns) sum
    /// children; stacked layouts take the max.  Each leaf contributes exactly
    /// `LEAF_MIN_WIDTH`.  No per-leaf scale math, no padding — simple and
    /// predictable.
    pub fn min_sidebar_chart_width(&self) -> f32 {
        use uzor::panels::{PanelNode, WindowLayout};
        // Build per-leaf min width (mirrors normalize_proportions_for_size).
        let mut leaf_min_w: HashMap<LeafId, f32> = HashMap::new();
        const CHART_AREA_MIN: f64 = 10.0;
        for (&leaf_id, &chart_id) in &self.leaf_to_chart {
            let scale_w = self
                .windows
                .get(&chart_id)
                .map(|w| w.scale_settings.effective_price_scale_width())
                .unwrap_or(crate::scale_settings::DEFAULT_PRICE_SCALE_WIDTH);
            let leaf_w = (scale_w + CHART_AREA_MIN) as f32;
            leaf_min_w.insert(leaf_id, leaf_w.max(Self::LEAF_MIN_WIDTH));
        }
        fn walk(
            node: &PanelNode<ChartSubPanel>,
            leaf_min_w: &HashMap<LeafId, f32>,
        ) -> f32 {
            match node {
                PanelNode::Leaf(l) => leaf_min_w
                    .get(&l.id)
                    .copied()
                    .unwrap_or(ChartPanelGrid::LEAF_MIN_WIDTH),
                PanelNode::Branch(branch) => {
                    let children = branch.children.iter().map(|c| walk(c, leaf_min_w));
                    match branch.layout {
                        WindowLayout::SplitHorizontal
                        | WindowLayout::ThreeColumns
                        | WindowLayout::OneLeftTwoRight
                        | WindowLayout::TwoLeftOneRight
                        | WindowLayout::Grid2x2 => children.sum::<f32>(),
                        _ => children.fold(0.0_f32, f32::max),
                    }
                }
            }
        }
        let root = self.docking.tree().root().clone();
        walk(&PanelNode::Branch(root), &leaf_min_w)
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
        // Normalize split proportions so that no single leaf ever shrinks
        // below LEAF_MIN_WIDTH / LEAF_MIN_HEIGHT, regardless of which
        // separator (chart internal OR sidebar) caused the compression.
        self.normalize_proportions_for_size(area.width, area.height);
        self.docking.layout(area);
    }

    /// Water-fill normalization: for every branch in the docking tree,
    /// rewrite `proportions` so that each child receives at least its
    /// subtree's minimum pixel size (sum of LEAF_MIN_W/H for contained
    /// leaves, as computed by `min_width_for_node` / `min_height_for_node`).
    /// Remaining space is distributed by the branch's existing proportions.
    ///
    /// This is the single source of truth that makes the sidebar separator
    /// respect per-leaf mins: when the sidebar compresses total chart
    /// width, this pass re-flows internal splits so no leaf disappears.
    fn normalize_proportions_for_size(&mut self, total_w: f32, total_h: f32) {
        use uzor::panels::{PanelNode, WindowLayout};

        struct Pending {
            id: uzor::panels::BranchId,
            props: Vec<f64>,
        }

        fn is_horizontal(layout: WindowLayout) -> bool {
            matches!(
                layout,
                WindowLayout::SplitHorizontal
                    | WindowLayout::ThreeColumns
                    | WindowLayout::OneLeftTwoRight
                    | WindowLayout::TwoLeftOneRight
            )
        }
        fn is_vertical(layout: WindowLayout) -> bool {
            matches!(layout, WindowLayout::SplitVertical | WindowLayout::ThreeRows)
        }

        // Build per-leaf min width: each leaf must be at least wide enough
        // to hold its own price scale plus a small chart area. Without this,
        // a leaf whose price scale was resized to 90px gets clamped to the
        // hardcoded LEAF_MIN_WIDTH=80 and the last-price label (which is
        // `price_scale_width - 2`) overflows into the chart area.
        let mut leaf_min_w: HashMap<LeafId, f32> = HashMap::new();
        const CHART_AREA_MIN: f64 = 10.0;
        for (&leaf_id, &chart_id) in &self.leaf_to_chart {
            let scale_w = self
                .windows
                .get(&chart_id)
                .map(|w| w.scale_settings.effective_price_scale_width())
                .unwrap_or(crate::scale_settings::DEFAULT_PRICE_SCALE_WIDTH);
            let leaf_w = (scale_w + CHART_AREA_MIN) as f32;
            leaf_min_w.insert(leaf_id, leaf_w.max(Self::LEAF_MIN_WIDTH));
        }

        // Snapshot min sizes per child so we can water-fill.
        fn min_w(
            node: &PanelNode<ChartSubPanel>,
            leaf_min_w: &HashMap<LeafId, f32>,
        ) -> f32 {
            match node {
                PanelNode::Leaf(l) => leaf_min_w
                    .get(&l.id)
                    .copied()
                    .unwrap_or(ChartPanelGrid::LEAF_MIN_WIDTH),
                PanelNode::Branch(b) => {
                    let it = b.children.iter().map(|c| min_w(c, leaf_min_w));
                    if is_horizontal(b.layout) || matches!(b.layout, WindowLayout::Grid2x2) {
                        it.sum()
                    } else {
                        it.fold(0.0_f32, f32::max)
                    }
                }
            }
        }
        fn min_h(node: &PanelNode<ChartSubPanel>) -> f32 {
            match node {
                PanelNode::Leaf(_) => ChartPanelGrid::LEAF_MIN_HEIGHT,
                PanelNode::Branch(b) => {
                    let it = b.children.iter().map(min_h);
                    if is_vertical(b.layout) || matches!(b.layout, WindowLayout::Grid2x2) {
                        it.sum()
                    } else {
                        it.fold(0.0_f32, f32::max)
                    }
                }
            }
        }

        // Fixed-point water-fill: returns `Some(new_props)` ONLY when the
        // current proportions violate per-child minima. When the existing
        // proportions already satisfy `prop[i] * available >= mins[i]` for
        // every child, returns `None` — caller leaves the branch untouched.
        //
        // This idempotency is critical: `apply_separator_drag` also mutates
        // proportions, and if `layout()` unconditionally rewrote them every
        // frame, drag would fight normalize, producing a jitter/rubber-band
        // feeling ("separator snaps back to where it was").
        //
        // Violating children are frozen at their min share; the remaining
        // available space is redistributed between the free children
        // proportionally to their current weights. This repeats until the
        // set of violators stabilises (classic water-filling).
        fn water_fill(
            available: f32,
            weights: &[f64],
            mins: &[f32],
        ) -> Option<Vec<f64>> {
            let n = weights.len();
            if n == 0 || available <= 0.0 {
                return None;
            }

            // First, normalize weights so they sum to 1.0 (caller may pass
            // raw branch proportions that already sum to 1 or arbitrary).
            let w_sum: f64 = weights.iter().sum::<f64>().max(f64::EPSILON);
            let norm: Vec<f64> = weights.iter().map(|w| w / w_sum).collect();

            // Quick-exit: are all children already above their min?
            let avail_f = available as f64;
            let all_ok = norm
                .iter()
                .zip(mins.iter())
                .all(|(p, m)| p * avail_f + 1e-6 >= *m as f64);
            if all_ok {
                return None;
            }

            // Degenerate: mins don't even fit — give each its min share.
            let total_min: f32 = mins.iter().sum();
            if total_min >= available {
                let sum_min = total_min.max(f32::EPSILON) as f64;
                return Some(mins.iter().map(|&m| m as f64 / sum_min).collect());
            }

            // Fixed-point water-fill: repeatedly freeze violators at their
            // min and redistribute the remainder among free children.
            let mut frozen = vec![false; n];
            let mut out = norm.clone();

            loop {
                // Sum of min shares of currently frozen children.
                let frozen_min: f64 = (0..n)
                    .filter(|&i| frozen[i])
                    .map(|i| mins[i] as f64 / avail_f)
                    .sum();

                let free_indices: Vec<usize> =
                    (0..n).filter(|&i| !frozen[i]).collect();
                if free_indices.is_empty() {
                    break;
                }

                // Pool of share space available to free children.
                let free_pool = (1.0 - frozen_min).max(0.0);

                // Free-children weight total (from original normalized weights).
                let free_w_sum: f64 =
                    free_indices.iter().map(|&i| norm[i]).sum::<f64>().max(f64::EPSILON);

                // Tentative allocation.
                let mut newly_frozen = false;
                for &i in &free_indices {
                    let share = free_pool * norm[i] / free_w_sum;
                    let min_share = mins[i] as f64 / avail_f;
                    if share + 1e-9 < min_share {
                        frozen[i] = true;
                        out[i] = min_share;
                        newly_frozen = true;
                    } else {
                        out[i] = share;
                    }
                }
                // Frozen children are already fixed at their min share.
                for i in 0..n {
                    if frozen[i] && (!newly_frozen || out[i] == 0.0) {
                        out[i] = mins[i] as f64 / avail_f;
                    }
                }

                if !newly_frozen {
                    break;
                }
            }

            // Final renormalize to absorb f64 drift.
            let s: f64 = out.iter().sum();
            if s > f64::EPSILON {
                for v in &mut out {
                    *v /= s;
                }
            }
            Some(out)
        }

        // Recursive immutable walker — collects pending updates and child
        // rects using the freshly-computed proportions (so descendants see
        // the correct parent size).
        fn walk(
            node: &PanelNode<ChartSubPanel>,
            rect_w: f32,
            rect_h: f32,
            pending: &mut Vec<Pending>,
            leaf_min_w: &HashMap<LeafId, f32>,
        ) {
            let branch = match node {
                PanelNode::Branch(b) => b,
                _ => return,
            };
            let n = branch.children.len();
            if n < 2 {
                // Still recurse into single-child branches.
                for c in &branch.children {
                    walk(c, rect_w, rect_h, pending, leaf_min_w);
                }
                return;
            }

            let horizontal = is_horizontal(branch.layout);
            let vertical = is_vertical(branch.layout);

            if horizontal || vertical {
                let available = if horizontal { rect_w } else { rect_h };
                let mins: Vec<f32> = if horizontal {
                    branch.children.iter().map(|c| min_w(c, leaf_min_w)).collect()
                } else {
                    branch.children.iter().map(min_h).collect()
                };
                // Use existing proportions as weights, or equal weights.
                let weights: Vec<f64> = if branch.proportions.len() == n {
                    branch.proportions.clone()
                } else {
                    vec![1.0; n]
                };
                // Idempotent: returns None when current proportions are
                // already valid, so we leave the branch alone and don't
                // fight `apply_separator_drag`.
                let effective_props: Vec<f64> =
                    match water_fill(available, &weights, &mins) {
                        Some(new_props) => {
                            pending.push(Pending {
                                id: branch.id,
                                props: new_props.clone(),
                            });
                            new_props
                        }
                        None => {
                            // Existing proportions are valid; use them
                            // (normalized) for recursing into children.
                            let s: f64 =
                                weights.iter().sum::<f64>().max(f64::EPSILON);
                            weights.iter().map(|w| w / s).collect()
                        }
                    };

                // Recurse with the resulting per-child sizes.
                for (i, child) in branch.children.iter().enumerate() {
                    let frac = effective_props[i] as f32;
                    let (cw, ch) = if horizontal {
                        (rect_w * frac, rect_h)
                    } else {
                        (rect_w, rect_h * frac)
                    };
                    walk(child, cw, ch, pending, leaf_min_w);
                }
            } else {
                // Non-proportional layouts (Grid2x2, presets) — just recurse
                // with the whole parent rect. Leaf min enforcement on those
                // is best-effort via the total-width clamp at the sidebar.
                for c in &branch.children {
                    walk(c, rect_w, rect_h, pending, leaf_min_w);
                }
            }
        }

        let mut pending = Vec::new();
        let root_clone = self.docking.tree().root().clone();
        walk(
            &PanelNode::Branch(root_clone),
            total_w,
            total_h,
            &mut pending,
            &leaf_min_w,
        );

        // Apply updates via public API.
        let tree = self.docking.tree_mut();
        for upd in pending {
            tree.set_branch_proportions(upd.id, upd.props);
        }
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
    /// Per-leaf minimum widths are read from `self.leaf_min_widths` (set via
    /// [`set_leaf_min_width`](Self::set_leaf_min_width)). Cascading resizing
    /// works in pixel space against the actual parent-branch pixel rect so that
    /// nested branches are correctly constrained.
    pub fn apply_separator_drag(
        &mut self,
        sep_idx: usize,
        delta: f32,
        content_width: f32,
        content_height: f32,
    ) {
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

        // Use the parent-branch pixel size for correct proportion math.
        // `rect_for_branch` walks the layout tree to find the actual rect of
        // this branch within the content area, so nested branches work correctly.
        let branch_rect = self.docking.tree()
            .rect_for_branch(parent_id, content_width, content_height);

        let branch_size = match branch_rect {
            Some(r) => match orientation {
                SeparatorOrientation::Horizontal => r.height,
                SeparatorOrientation::Vertical => r.width,
            },
            None => match orientation {
                SeparatorOrientation::Horizontal => content_height,
                SeparatorOrientation::Vertical => content_width,
            },
        };

        // Retrieve the current proportions of the parent branch.
        let (n, raw_props, children_min_px, pos_a, pos_b) = {
            let branch = match self.docking.tree().find_branch(parent_id) {
                Some(b) => b,
                None => return,
            };

            let n = branch.children.len();
            if n < 2 {
                return;
            }

            let raw_props: Vec<f64> = if branch.proportions.len() == n {
                branch.proportions.clone()
            } else {
                vec![1.0_f64 / n as f64; n]
            };

            // Per-child minimum in pixels — vertical separator = width guard,
            // horizontal separator = height guard.
            let children_min_px: Vec<f32> = if orientation == SeparatorOrientation::Vertical {
                branch.children.iter()
                    .map(|c| self.min_width_for_node(c))
                    .collect()
            } else {
                branch.children.iter()
                    .map(|c| self.min_height_for_node(c))
                    .collect()
            };

            let pos_a = branch.children.iter().position(|c| c.raw_id() == child_a_raw);
            let pos_b = branch.children.iter().position(|c| c.raw_id() == child_b_raw);
            let (pos_a, pos_b) = match (pos_a, pos_b) {
                (Some(a), Some(b)) => (a, b),
                _ => return,
            };

            (n, raw_props, children_min_px, pos_a, pos_b)
        };

        if branch_size <= 0.0 {
            return;
        }

        // Convert pixel delta to a proportion delta relative to the total share sum.
        let total_share: f64 = raw_props.iter().sum();
        let delta_share = (delta as f64 / branch_size as f64) * total_share;

        // Per-child minimum in share space (derived from pixel min via branch_size).
        let min_shares: Vec<f64> = children_min_px.iter()
            .map(|&px| (px as f64 / branch_size as f64) * total_share)
            .collect();

        // --- Cascading resize in share space ---
        //
        // When dragging in the positive direction (pos_a grows, pos_b shrinks):
        //   - Walk siblings from pos_b rightward; take shrinkage from each.
        //   - Give all taken shrinkage to pos_a.
        //
        // When dragging in the negative direction (pos_a shrinks, pos_b grows):
        //   - Walk siblings from pos_a leftward; take shrinkage from each.
        //   - Give all taken shrinkage to pos_b.

        let mut new_props = raw_props.clone();

        if delta_share >= 0.0 {
            // pos_a grows — cascade shrink across pos_b, pos_b+1, pos_b+2, ...
            let mut remaining = delta_share;
            for i in pos_b..n {
                if new_props[i] <= 0.0 { continue; }
                let available = (new_props[i] - min_shares[i]).max(0.0);
                let take = remaining.min(available);
                new_props[i] -= take;
                remaining -= take;
                if remaining <= 0.0 {
                    break;
                }
            }
            // pos_a absorbs however much was actually freed.
            new_props[pos_a] += delta_share - remaining;
        } else {
            // pos_a shrinks — cascade shrink across pos_a-1, pos_a-2, ...
            // (walking leftward from pos_a inclusive)
            let mut remaining = (-delta_share).abs();
            let indices: Vec<usize> = (0..=pos_a).rev().collect();
            for i in indices {
                if new_props[i] <= 0.0 { continue; }
                let available = (new_props[i] - min_shares[i]).max(0.0);
                let take = remaining.min(available);
                new_props[i] -= take;
                remaining -= take;
                if remaining <= 0.0 {
                    break;
                }
            }
            // pos_b absorbs however much was actually freed.
            new_props[pos_b] += (-delta_share) - remaining;
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
// Right-click hit-test
// =========================================================================

/// Result of a right-click hit-test on the chart canvas.
///
/// Returned by [`ChartPanelGrid::handle_right_click`].
/// The caller (`chart-app`) maps each variant to the appropriate context-menu
/// action without knowing the hit-test internals.
#[derive(Debug)]
pub enum ChartRightClickHit {
    /// Hit a drawing primitive.  Caller should select it and open the
    /// primitive context menu.
    Primitive {
        /// Raw index returned by `DrawingManager::hit_test*`.
        prim_idx: usize,
    },
    /// Hit an indicator line (overlay or sub-pane).  Caller should open the
    /// indicator settings panel.
    Indicator {
        /// Instance ID of the hit indicator.
        indicator_id: u64,
    },
    /// Click landed on the empty chart background.  Caller should open the
    /// chart background context menu.
    Background,
    /// Click did not land on any chart pane (outside all panels).
    Miss,
}

impl ChartPanelGrid {
    /// Perform a right-click hit-test in screen coordinates.
    ///
    /// Steps:
    /// 1. Check drawing primitives on the main chart.
    /// 2. If no primitive hit, check sub-panes for primitives.
    /// 3. If still no hit, delegate indicator overlay hit-test to `indicator_overlay_hit`.
    /// 4. If no overlay hit, delegate sub-pane indicator hit-test to `indicator_subpane_hit`.
    /// 5. Fall back to [`ChartRightClickHit::Background`] if inside a chart pane,
    ///    or [`ChartRightClickHit::Miss`] if outside all panes.
    ///
    /// The two indicator closures are supplied by `chart-app` (which owns
    /// `IndicatorManager`).  This keeps the crate boundary clean: `zengeld-chart`
    /// never depends on `zengeld-terminal-indicators`.
    ///
    /// `indicator_overlay_hit(local_x, local_y, chart_height) -> Option<u64>`
    /// `indicator_subpane_hit(instance_id, local_x, local_y, pane_height) -> bool`
    pub fn handle_right_click(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        extended: &crate::layout::ExtendedFrameLayout,
        indicator_overlay_hit: impl Fn(f64, f64, f64) -> Option<u64>,
        indicator_subpane_hit: impl Fn(u64, f64, f64, f64) -> bool,
    ) -> ChartRightClickHit {
        // ----------------------------------------------------------------
        // Step 1 & 2: primitive hit-test (main chart then sub-panes).
        // ----------------------------------------------------------------
        let chart_rect = extended.main_chart.chart;
        let main_local_x = screen_x - chart_rect.x;
        let main_local_y = screen_y - chart_rect.y;

        let primitive_hit: Option<usize> = {
            // Main chart.
            let main_hit = if main_local_x >= 0.0
                && main_local_x <= chart_rect.width
                && main_local_y >= 0.0
                && main_local_y <= chart_rect.height
            {
                if let Some(window) = self.active_window() {
                    let mut corrected_vp = window.viewport.clone();
                    corrected_vp.chart_width = chart_rect.width;
                    corrected_vp.chart_height = chart_rect.height;
                    window.drawing_manager.hit_test(
                        main_local_x,
                        main_local_y,
                        &corrected_vp,
                        &window.price_scale,
                    )
                } else {
                    None
                }
            } else {
                None
            };

            if main_hit.is_some() {
                main_hit
            } else {
                // Sub-panes.
                let mut sub_hit = None;
                for pane_layout in extended.sub_panes.iter() {
                    let content = pane_layout.content;
                    let plx = screen_x - content.x;
                    let ply = screen_y - content.y;
                    if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                        continue;
                    }
                    let instance_id = pane_layout.instance_id;
                    let (price_min, price_max) = self
                        .active_window()
                        .and_then(|w| {
                            w.sub_panes
                                .iter()
                                .find(|sp| sp.instance_id == instance_id)
                                .map(|sp| (sp.price_min, sp.price_max))
                        })
                        .unwrap_or((0.0, 100.0));
                    let sub_price_scale =
                        crate::PriceScale::new(price_min, price_max);
                    let sub_viewport = self.active_window().map(|w| {
                        let mut vp = w.viewport.clone();
                        vp.chart_height = content.height;
                        vp
                    });
                    if let (Some(sub_viewport), Some(window)) =
                        (sub_viewport, self.active_window())
                    {
                        if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                            plx,
                            ply,
                            instance_id,
                            &sub_viewport,
                            &sub_price_scale,
                        ) {
                            // Record active pane so context-menu actions know which pane is active.
                            if let Some(win) = self.active_window_mut() {
                                win.drawing_manager.set_current_pane(Some(instance_id));
                            }
                            sub_hit = Some(prim_idx);
                            break;
                        }
                    }
                }
                sub_hit
            }
        };

        if let Some(prim_idx) = primitive_hit {
            return ChartRightClickHit::Primitive { prim_idx };
        }

        // ----------------------------------------------------------------
        // Step 3 & 4: indicator hit-test (overlay then sub-panes).
        // ----------------------------------------------------------------
        let indicator_hit: Option<u64> = {
            // Main chart overlay.
            let overlay_hit = if main_local_x >= 0.0
                && main_local_x <= chart_rect.width
                && main_local_y >= 0.0
                && main_local_y <= chart_rect.height
            {
                indicator_overlay_hit(main_local_x, main_local_y, chart_rect.height)
            } else {
                None
            };

            if overlay_hit.is_some() {
                overlay_hit
            } else {
                // Sub-pane indicators.
                let mut sub_hit = None;
                for sp in &extended.sub_panes {
                    let pane_rect = sp.content;
                    let plx = screen_x - pane_rect.x;
                    let ply = screen_y - pane_rect.y;
                    if plx < 0.0 || plx > pane_rect.width || ply < 0.0 || ply > pane_rect.height {
                        continue;
                    }
                    let instance_id = sp.instance_id;
                    if indicator_subpane_hit(instance_id, plx, ply, pane_rect.height) {
                        sub_hit = Some(instance_id);
                        break;
                    }
                }
                sub_hit
            }
        };

        if let Some(ind_id) = indicator_hit {
            return ChartRightClickHit::Indicator { indicator_id: ind_id };
        }

        // ----------------------------------------------------------------
        // Step 5: determine if inside any pane at all.
        // ----------------------------------------------------------------
        let inside_main = main_local_x >= 0.0
            && main_local_x <= chart_rect.width
            && main_local_y >= 0.0
            && main_local_y <= chart_rect.height;

        let inside_sub = !inside_main
            && extended.sub_panes.iter().any(|sp| {
                let plx = screen_x - sp.content.x;
                let ply = screen_y - sp.content.y;
                plx >= 0.0 && plx <= sp.content.width && ply >= 0.0 && ply <= sp.content.height
            });

        if inside_main || inside_sub {
            ChartRightClickHit::Background
        } else {
            ChartRightClickHit::Miss
        }
    }
}

// =========================================================================
// Drag-start hit-test
// =========================================================================

/// Result of a drag-start hit-test on the chart canvas.
///
/// Returned by [`ChartPanelGrid::handle_drag_start`].
/// The caller (`chart-app`) maps each variant to the appropriate drag mode
/// without knowing the hit-test internals.
#[derive(Debug)]
pub enum ChartDragStartHit {
    /// Hit a control point of the currently selected primitive.
    /// Caller should set `DragMode::ControlPoint` and emit `StartControlPointDrag`.
    ControlPoint {
        /// ID of the selected primitive that owns the control point.
        primitive_id: u64,
        /// Which control point was hit.
        control_point: crate::drawing::ControlPointType,
    },
    /// Hit a primitive body (no control point).
    /// Caller should set `DragMode::Primitive` and emit `StartPrimitiveDrag`.
    Primitive {
        /// ID of the hit primitive.
        primitive_id: u64,
    },
    /// Freehand drawing tool was active and a stroke was started on the main chart canvas.
    /// Caller should return early — `drawing_manager.start_freehand()` already ran.
    FreehandStarted,
    /// Drag started on the chart background (no primitive, no freehand).
    /// Caller should capture `viewport_before_drag` for undo, then fall through.
    Background,
    /// Drag started outside all chart panes.
    Miss,
}

impl ChartPanelGrid {
    /// Perform a drag-start hit-test in screen coordinates.
    ///
    /// Steps (in priority order):
    /// 1. If freehand tool is active and the point lands on the main chart canvas,
    ///    call `drawing_manager.start_freehand()` and return [`ChartDragStartHit::FreehandStarted`].
    /// 2. Control-point hit-test on the main chart (selected primitive only).
    /// 3. Primitive body hit-test on the main chart.
    /// 4. Control-point hit-test across all sub-panes.
    /// 5. Primitive body hit-test across all sub-panes.
    /// 6. Returns [`ChartDragStartHit::Background`] if inside a pane, or
    ///    [`ChartDragStartHit::Miss`] if outside all panes.
    ///
    /// The `extended` layout is supplied by the caller (already computed for the frame).
    pub fn handle_drag_start(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        extended: &crate::layout::ExtendedFrameLayout,
    ) -> ChartDragStartHit {
        let chart_rect = extended.main_chart.chart;
        let local_x = screen_x - chart_rect.x;
        let local_y = screen_y - chart_rect.y;
        let in_main = local_x >= 0.0
            && local_x <= chart_rect.width
            && local_y >= 0.0
            && local_y <= chart_rect.height;

        // ----------------------------------------------------------------
        // Step 1: freehand tool
        // ----------------------------------------------------------------
        if in_main {
            let is_freehand = self
                .active_window()
                .map(|w| w.drawing_manager.is_freehand_tool())
                .unwrap_or(false);
            if is_freehand {
                let (price_min, price_max) = self
                    .active_window()
                    .map(|w| (w.price_scale.price_min, w.price_scale.price_max))
                    .unwrap_or((0.0, 1.0));
                let bar = self
                    .active_window()
                    .map(|w| w.viewport.x_to_bar_f64(local_x))
                    .unwrap_or(0.0);
                let price =
                    price_max - (local_y / chart_rect.height) * (price_max - price_min);
                if let Some(window) = self.active_window_mut() {
                    window.drawing_manager.start_freehand(bar, price);
                }
                return ChartDragStartHit::FreehandStarted;
            }
        }

        // ----------------------------------------------------------------
        // Steps 2 & 3: main chart primitive / control-point hit-test
        // ----------------------------------------------------------------
        if in_main {
            if let Some(window) = self.active_window() {
                let mut corrected_vp = window.viewport.clone();
                corrected_vp.chart_width = chart_rect.width;
                corrected_vp.chart_height = chart_rect.height;

                // Control points have higher priority than primitive body.
                if let Some(cp_type) = window.drawing_manager.hit_test_control_point(
                    local_x,
                    local_y,
                    &corrected_vp,
                    &window.price_scale,
                ) {
                    if let Some(selected_idx) = window.drawing_manager.selected() {
                        if let Some(id) = window
                            .drawing_manager
                            .primitives()
                            .get(selected_idx)
                            .map(|p| p.data().id)
                        {
                            return ChartDragStartHit::ControlPoint {
                                primitive_id: id,
                                control_point: cp_type,
                            };
                        }
                    }
                }

                if let Some(prim_idx) = window.drawing_manager.hit_test(
                    local_x,
                    local_y,
                    &corrected_vp,
                    &window.price_scale,
                ) {
                    if let Some(id) = window
                        .drawing_manager
                        .primitives()
                        .get(prim_idx)
                        .map(|p| p.data().id)
                    {
                        return ChartDragStartHit::Primitive { primitive_id: id };
                    }
                }
            }
        }

        // ----------------------------------------------------------------
        // Steps 4 & 5: sub-pane primitive / control-point hit-test
        // ----------------------------------------------------------------
        let mut sub_pane_hit: Option<ChartDragStartHit> = None;
        'sub_pane_loop: for pane_layout in extended.sub_panes.iter() {
            let content = pane_layout.content;
            let plx = screen_x - content.x;
            let ply = screen_y - content.y;
            if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                continue;
            }
            let instance_id = pane_layout.instance_id;
            let (price_min, price_max) = self
                .active_window()
                .and_then(|w| {
                    w.sub_panes
                        .iter()
                        .find(|sp| sp.instance_id == instance_id)
                        .map(|sp| (sp.price_min, sp.price_max))
                })
                .unwrap_or((0.0, 100.0));
            let sub_price_scale = crate::PriceScale::new(price_min, price_max);
            let sub_viewport = self.active_window().map(|w| {
                let mut vp = w.viewport.clone();
                vp.chart_height = content.height;
                vp
            });

            if let Some(sub_viewport) = sub_viewport {
                if let Some(window) = self.active_window() {
                    // Control points first.
                    if let Some(cp_type) = window.drawing_manager.hit_test_control_point_in_pane(
                        plx,
                        ply,
                        instance_id,
                        &sub_viewport,
                        &sub_price_scale,
                    ) {
                        if let Some(selected_idx) = window.drawing_manager.selected() {
                            if let Some(id) = window
                                .drawing_manager
                                .primitives()
                                .get(selected_idx)
                                .map(|p| p.data().id)
                            {
                                sub_pane_hit = Some(ChartDragStartHit::ControlPoint {
                                    primitive_id: id,
                                    control_point: cp_type,
                                });
                                break 'sub_pane_loop;
                            }
                        }
                    } else if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                        plx,
                        ply,
                        instance_id,
                        &sub_viewport,
                        &sub_price_scale,
                    ) {
                        if let Some(id) = window
                            .drawing_manager
                            .primitives()
                            .get(prim_idx)
                            .map(|p| p.data().id)
                        {
                            sub_pane_hit = Some(ChartDragStartHit::Primitive { primitive_id: id });
                            break 'sub_pane_loop;
                        }
                    }
                }
            }
        }

        if let Some(hit) = sub_pane_hit {
            return hit;
        }

        // ----------------------------------------------------------------
        // Step 6: background or miss
        // ----------------------------------------------------------------
        let inside_sub = extended.sub_panes.iter().any(|sp| {
            let plx = screen_x - sp.content.x;
            let ply = screen_y - sp.content.y;
            plx >= 0.0 && plx <= sp.content.width && ply >= 0.0 && ply <= sp.content.height
        });

        if in_main || inside_sub {
            ChartDragStartHit::Background
        } else {
            ChartDragStartHit::Miss
        }
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
