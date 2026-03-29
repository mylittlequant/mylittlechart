//! Pane Manager - Multi-pane chart support
//!
//! Manages multiple chart panes (main price pane + indicator sub-panes).
//! Each pane has its own viewport, price scale, and height.

use std::collections::HashMap;

/// Pane identifier
pub type PaneId = usize;

// =============================================================================
// InteractionRegion - Centralized hit-test decision logic
// =============================================================================

/// Describes which region of the chart the cursor is interacting with.
/// Applications call `identify_region()` and handle the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InteractionRegion {
    /// Main chart area (price pane)
    MainChart {
        /// Local Y coordinate within the pane
        local_y: f64,
    },
    /// Main pane's price scale (Y-axis)
    MainPriceScale {
        /// Local Y coordinate within the scale area
        local_y: f64,
    },
    /// Sub-pane chart area (indicator pane)
    SubPane {
        /// Pane ID
        pane_id: PaneId,
        /// Local Y coordinate within the pane
        local_y: f64,
    },
    /// Sub-pane's price scale
    SubPanePriceScale {
        /// Pane ID
        pane_id: PaneId,
        /// Local Y coordinate within the scale area
        local_y: f64,
    },
    /// Separator between panes (for resizing)
    Separator {
        /// Pane ID above the separator
        above_pane_id: PaneId,
    },
    /// Time scale (X-axis) at the bottom
    TimeScale,
    /// Outside chart bounds
    Outside,
}

/// Main price pane ID (always 0)
pub const MAIN_PANE: PaneId = 0;

/// Individual chart pane configuration
#[derive(Clone, Debug)]
pub struct Pane {
    /// Unique pane identifier
    pub id: PaneId,
    /// Pane title (e.g., "RSI", "Volume")
    pub title: String,
    /// Height in pixels (0 = auto/proportional)
    pub height: f64,
    /// Minimum height
    pub min_height: f64,
    /// Maximum height (0 = unlimited)
    pub max_height: f64,
    /// Height as ratio of total chart (for proportional sizing)
    pub height_ratio: f64,
    /// Whether this pane is collapsed
    pub collapsed: bool,
    /// Price range for this pane (separate Y-axis)
    pub price_min: f64,
    pub price_max: f64,
    /// Auto-scale Y-axis to fit data
    pub auto_scale: bool,
    /// Show price labels on Y-axis
    pub show_price_labels: bool,
    /// Y-axis position (left or right)
    pub y_axis_right: bool,
    /// Saved price range at drag start (for drag operations)
    drag_start_price_min: f64,
    drag_start_price_max: f64,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            height: 100.0,
            min_height: 50.0,
            max_height: 0.0,
            height_ratio: 0.0,
            collapsed: false,
            price_min: 0.0,
            price_max: 100.0,
            auto_scale: true,
            show_price_labels: true,
            y_axis_right: true,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
        }
    }
}

impl Pane {
    /// Create main price pane
    pub fn main() -> Self {
        Self {
            id: MAIN_PANE,
            title: "Price".to_string(),
            height: 0.0, // Will be calculated as remaining space
            min_height: 200.0,
            height_ratio: 0.7, // 70% of total height
            auto_scale: true,
            ..Default::default()
        }
    }

    /// Create sub-pane for indicator
    pub fn indicator(id: PaneId, title: &str) -> Self {
        Self {
            id,
            title: title.to_string(),
            height: 100.0,
            min_height: 50.0,
            max_height: 300.0,
            height_ratio: 0.15, // 15% of total height
            auto_scale: true,
            ..Default::default()
        }
    }

    /// Create volume pane (special case - histogram)
    pub fn volume(id: PaneId) -> Self {
        Self {
            id,
            title: "Volume".to_string(),
            height: 80.0,
            min_height: 40.0,
            max_height: 150.0,
            height_ratio: 0.1, // 10% of total height
            auto_scale: true,
            price_min: 0.0,
            ..Default::default()
        }
    }

    /// Update price range from data
    pub fn update_price_range(&mut self, values: &[f64]) {
        if !self.auto_scale || values.is_empty() {
            return;
        }

        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;

        for &v in values {
            if !v.is_nan() && !v.is_infinite() {
                min = min.min(v);
                max = max.max(v);
            }
        }

        if min < max {
            // Add some padding (5%)
            let range = max - min;
            let padding = range * 0.05;
            self.price_min = min - padding;
            self.price_max = max + padding;

            // For volume/histogram, always start from 0
            if self.title == "Volume" {
                self.price_min = 0.0;
            }
        }
    }

    /// Convert price to Y coordinate within this pane
    pub fn price_to_y(&self, price: f64) -> f64 {
        if self.price_max <= self.price_min {
            return self.height / 2.0;
        }
        self.height * (1.0 - (price - self.price_min) / (self.price_max - self.price_min))
    }

    /// Convert Y coordinate to price within this pane
    pub fn y_to_price(&self, y: f64) -> f64 {
        self.price_max - (y / self.height) * (self.price_max - self.price_min)
    }

    /// Get price range
    pub fn price_range(&self) -> f64 {
        self.price_max - self.price_min
    }

    /// Save current price state before drag operation
    pub fn save_drag_start(&mut self) {
        self.drag_start_price_min = self.price_min;
        self.drag_start_price_max = self.price_max;
    }

    /// Zoom Y-axis around center from saved drag start
    /// Factor > 1.0 zooms out (expands range), factor < 1.0 zooms in
    pub fn zoom_from_drag(&mut self, factor: f64) {
        let center = (self.drag_start_price_min + self.drag_start_price_max) / 2.0;
        let half_range = (self.drag_start_price_max - self.drag_start_price_min) / 2.0 * factor;
        // Minimum range protection
        let min_range = center.abs().max(1.0) * 0.000001;
        let clamped_half = half_range.max(min_range / 2.0);
        self.price_min = center - clamped_half;
        self.price_max = center + clamped_half;
        self.auto_scale = false;
    }

    /// Pan Y-axis by price delta
    pub fn pan(&mut self, price_delta: f64) {
        self.price_min += price_delta;
        self.price_max += price_delta;
        self.auto_scale = false;
    }

    /// Update from auto-calculated range (only if auto_scale is true)
    pub fn update_auto_range(&mut self, min: f64, max: f64) {
        if self.auto_scale {
            self.price_min = min;
            self.price_max = max;
        }
    }

    /// Set price range directly (disables auto-scale)
    pub fn set_price_range(&mut self, min: f64, max: f64) {
        self.price_min = min;
        self.price_max = max;
        self.auto_scale = false;
    }

    /// Reset to auto-scale mode
    pub fn enable_auto_scale(&mut self) {
        self.auto_scale = true;
    }
}

/// Computed geometry for a rendered pane (for hit testing and rendering)
#[derive(Clone, Debug)]
pub struct PaneGeometry {
    /// Pane ID (for looking up in PaneManager)
    pub pane_id: PaneId,
    /// Associated indicator instance ID (None for main pane)
    pub instance_id: Option<u64>,
    /// Index in the panes list (for ordering)
    pub index: usize,
    /// Y offset from chart origin (top of this pane)
    pub y_offset: f64,
    /// Height of chart area
    pub height: f64,
    /// Chart width (same as main chart)
    pub chart_width: f64,
    /// Price range for this pane
    pub price_min: f64,
    pub price_max: f64,
}

impl PaneGeometry {
    /// Convert Y coordinate (relative to pane) to price
    pub fn y_to_price(&self, local_y: f64) -> f64 {
        if self.height <= 0.0 {
            return (self.price_min + self.price_max) / 2.0;
        }
        let ratio = local_y / self.height;
        self.price_max - ratio * (self.price_max - self.price_min)
    }

    /// Convert price to Y coordinate (relative to pane)
    pub fn price_to_y(&self, price: f64) -> f64 {
        if self.price_max <= self.price_min {
            return self.height / 2.0;
        }
        self.height * (1.0 - (price - self.price_min) / (self.price_max - self.price_min))
    }

    /// Check if a global Y coordinate is within this pane
    pub fn contains_y(&self, global_y: f64) -> bool {
        global_y >= self.y_offset && global_y < self.y_offset + self.height
    }

    /// Convert global Y to local Y (relative to pane top)
    pub fn global_to_local_y(&self, global_y: f64) -> f64 {
        global_y - self.y_offset
    }

    /// Clamp a local Y coordinate to stay within this pane's bounds
    pub fn clamp_local_y(&self, local_y: f64) -> f64 {
        if self.height <= 0.0 {
            return 0.0;
        }
        local_y.clamp(0.0, self.height)
    }

    /// Clamp a global Y coordinate to stay within this pane's bounds
    pub fn clamp_global_y(&self, global_y: f64) -> f64 {
        let max = self.y_offset + self.height;
        if self.y_offset > max {
            return self.y_offset;
        }
        global_y.clamp(self.y_offset, max)
    }

    /// Clamp price to stay within this pane's Y-axis range
    pub fn clamp_price(&self, price: f64) -> f64 {
        if self.price_min > self.price_max {
            // Inverted range (tiny/invalid pane), return midpoint
            return (self.price_min + self.price_max) / 2.0;
        }
        price.clamp(self.price_min, self.price_max)
    }

    /// Price range for this pane
    pub fn price_range(&self) -> f64 {
        self.price_max - self.price_min
    }
}

// =============================================================================
// SubPane - Unified sub-pane structure for indicator panes
// =============================================================================

/// Unified sub-pane structure combining geometry and Y-axis state.
/// This is the single source of truth for sub-pane data, eliminating
/// the need for separate SubPaneGeometry and SubPaneYState collections.
#[derive(Clone, Debug)]
pub struct SubPane {
    // Identity
    /// Indicator instance ID (unique identifier)
    pub instance_id: u64,
    /// Index in the sub-panes list (updated each frame)
    pub index: usize,

    // Geometry (updated each frame)
    /// Y offset from chart origin (top of this pane)
    pub y_offset: f32,
    /// Height of chart area (not including price scale)
    pub height: f32,
    /// Chart width (same as main chart)
    pub chart_width: f32,

    // Y-axis state (persistent)
    /// Current price min (Y-axis bottom)
    pub price_min: f64,
    /// Current price max (Y-axis top)
    pub price_max: f64,
    /// Whether auto-scale is enabled
    pub auto_scale: bool,
    /// Saved state for drag operations
    drag_start_price_min: f64,
    drag_start_price_max: f64,

    // Height state (persistent)
    /// User-set height ratio: 0.0 means use default (100px), otherwise fraction
    /// of available sub-pane space allocated to this pane.
    pub height_ratio: f32,
    /// Whether this sub-pane is currently hidden (indicator still exists).
    pub hidden: bool,
    /// When true, this pane renders ABOVE the main chart (main chart below).
    pub above_main: bool,
    /// When true, this pane is maximized (fills the entire leaf, hides all others).
    pub maximized: bool,
    /// Saved height_ratio before maximize, restored on restore.
    pub pre_maximize_height_ratio: f32,
}

impl SubPane {
    /// Create a new SubPane with the given instance ID
    pub fn new(instance_id: u64) -> Self {
        Self {
            instance_id,
            index: 0,
            y_offset: 0.0,
            height: 100.0,
            chart_width: 0.0,
            price_min: 0.0,
            price_max: 100.0,
            auto_scale: true,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
            height_ratio: 0.15,
            hidden: false,
            above_main: false,
            maximized: false,
            pre_maximize_height_ratio: 0.0,
        }
    }

    // =========================================================================
    // Coordinate conversion
    // =========================================================================

    /// Convert local Y coordinate (within pane) to price
    pub fn y_to_price(&self, local_y: f64) -> f64 {
        let ratio = local_y / self.height as f64;
        self.price_max - ratio * self.range()
    }

    /// Convert price to local Y coordinate (within pane)
    #[allow(dead_code)]
    pub fn price_to_y(&self, price: f64) -> f64 {
        let ratio = (self.price_max - price) / self.range();
        ratio * self.height as f64
    }

    /// Check if global Y coordinate is within this pane
    pub fn contains_y(&self, global_y: f64) -> bool {
        global_y >= self.y_offset as f64 && global_y < (self.y_offset + self.height) as f64
    }

    /// Convert global Y to local Y within this pane
    pub fn local_y(&self, global_y: f64) -> f64 {
        global_y - self.y_offset as f64
    }

    /// Get pane top Y (global coordinate)
    pub fn top(&self) -> f64 {
        self.y_offset as f64
    }

    /// Get pane bottom Y (global coordinate)
    #[allow(dead_code)]
    pub fn bottom(&self) -> f64 {
        self.y_offset as f64 + self.height as f64
    }

    // =========================================================================
    // Y-axis operations
    // =========================================================================

    /// Save state before drag
    pub fn save_drag_start(&mut self) {
        self.drag_start_price_min = self.price_min;
        self.drag_start_price_max = self.price_max;
    }

    /// Zoom Y-axis around center from saved drag start
    pub fn zoom_from_drag(&mut self, factor: f64) {
        let center = (self.drag_start_price_min + self.drag_start_price_max) / 2.0;
        let half_range = (self.drag_start_price_max - self.drag_start_price_min) / 2.0 * factor;
        let min_range = center.abs().max(1.0) * 0.000001;
        let clamped_half = half_range.max(min_range / 2.0);
        self.price_min = center - clamped_half;
        self.price_max = center + clamped_half;
        self.auto_scale = false;
    }

    /// Pan Y-axis by price delta
    pub fn pan(&mut self, price_delta: f64) {
        self.price_min += price_delta;
        self.price_max += price_delta;
        self.auto_scale = false;
    }

    /// Get price range
    pub fn range(&self) -> f64 {
        self.price_max - self.price_min
    }

    /// Update from auto-calculated range (only if auto_scale is true)
    pub fn update_auto(&mut self, min: f64, max: f64) {
        if self.auto_scale {
            self.price_min = min;
            self.price_max = max;
        }
    }

    /// Reset Y-axis to auto-scale mode
    pub fn reset_to_auto(&mut self) {
        self.auto_scale = true;
    }

    // =========================================================================
    // Geometry update
    // =========================================================================

    /// Update geometry for this frame
    pub fn update_geometry(&mut self, index: usize, y_offset: f32, height: f32, chart_width: f32) {
        self.index = index;
        self.y_offset = y_offset;
        self.height = height;
        self.chart_width = chart_width;
    }
}

/// Coordinate clamping utilities for unified boundary policy
/// These are standalone functions for coordinates not tied to a specific pane
pub mod coordinate_utils {
    /// Clamp X coordinate to chart width bounds
    pub fn clamp_x(x: f64, chart_width: f64) -> f64 {
        x.clamp(0.0, chart_width)
    }

    /// Clamp bar index to valid data range
    pub fn clamp_bar_index(index: i64, data_len: usize) -> usize {
        if index < 0 {
            0
        } else if index as usize >= data_len {
            data_len.saturating_sub(1)
        } else {
            index as usize
        }
    }

    /// Clamp view_start to valid range (prevents over-scrolling)
    pub fn clamp_view_start(view_start: f64, data_len: usize, visible_bars: usize) -> f64 {
        let max_start = (data_len as f64 - visible_bars as f64).max(0.0);
        view_start.clamp(0.0, max_start)
    }

    /// Clamp bar_spacing to reasonable zoom limits
    pub fn clamp_bar_spacing(spacing: f64, min: f64, max: f64) -> f64 {
        spacing.clamp(min, max)
    }
}

/// Manager for multiple chart panes
#[derive(Clone, Debug)]
pub struct PaneManager {
    /// All panes (indexed by PaneId)
    panes: HashMap<PaneId, Pane>,
    /// Pane order (top to bottom)
    order: Vec<PaneId>,
    /// Next pane ID to assign
    next_id: PaneId,
    /// Total available height
    total_height: f64,
    /// Separator height between panes
    separator_height: f64,
    /// Currently dragged separator (pane ID above separator)
    dragging_separator: Option<PaneId>,
}

impl Default for PaneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneManager {
    /// Create new pane manager with main pane
    pub fn new() -> Self {
        let mut manager = Self {
            panes: HashMap::new(),
            order: Vec::new(),
            next_id: 1, // 0 is reserved for main pane
            total_height: 600.0,
            separator_height: 4.0,
            dragging_separator: None,
        };

        // Always create main pane
        let main = Pane::main();
        manager.panes.insert(MAIN_PANE, main);
        manager.order.push(MAIN_PANE);

        manager
    }

    /// Add a new sub-pane
    pub fn add_pane(&mut self, title: &str) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;

        let pane = Pane::indicator(id, title);
        self.panes.insert(id, pane);
        self.order.push(id);

        self.recalculate_heights();
        id
    }

    /// Add volume pane
    pub fn add_volume_pane(&mut self) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;

        let pane = Pane::volume(id);
        self.panes.insert(id, pane);
        self.order.push(id);

        self.recalculate_heights();
        id
    }

    /// Remove a pane
    pub fn remove_pane(&mut self, id: PaneId) {
        if id == MAIN_PANE {
            return; // Can't remove main pane
        }

        self.panes.remove(&id);
        self.order.retain(|&i| i != id);
        self.recalculate_heights();
    }

    /// Get pane by ID
    pub fn get_pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// Get mutable pane by ID
    pub fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&id)
    }

    /// Get main pane
    pub fn main_pane(&self) -> &Pane {
        self.panes.get(&MAIN_PANE).expect("Main pane must exist")
    }

    /// Get mutable main pane
    pub fn main_pane_mut(&mut self) -> &mut Pane {
        self.panes.get_mut(&MAIN_PANE).expect("Main pane must exist")
    }

    /// Get all panes in order
    pub fn panes_in_order(&self) -> Vec<&Pane> {
        self.order.iter().filter_map(|id| self.panes.get(id)).collect()
    }

    /// Get pane count (including main)
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Check if has sub-panes
    pub fn has_sub_panes(&self) -> bool {
        self.panes.len() > 1
    }

    /// Set total available height
    pub fn set_total_height(&mut self, height: f64) {
        self.total_height = height;
        self.recalculate_heights();
    }

    /// Get total height
    pub fn total_height(&self) -> f64 {
        self.total_height
    }

    /// Get separator height
    pub fn separator_height(&self) -> f64 {
        self.separator_height
    }

    /// Recalculate pane heights based on ratios
    fn recalculate_heights(&mut self) {
        if self.order.is_empty() {
            return;
        }

        // Calculate total separator height
        let separator_space = self.separator_height * (self.order.len() - 1).max(0) as f64;
        let available_height = self.total_height - separator_space;

        // Calculate total ratio
        let total_ratio: f64 = self.order.iter()
            .filter_map(|id| self.panes.get(id))
            .filter(|p| !p.collapsed)
            .map(|p| p.height_ratio)
            .sum();

        if total_ratio <= 0.0 {
            return;
        }

        // Distribute height based on ratios
        for id in &self.order {
            if let Some(pane) = self.panes.get_mut(id) {
                if pane.collapsed {
                    pane.height = 0.0;
                } else {
                    pane.height = (available_height * pane.height_ratio / total_ratio)
                        .max(pane.min_height)
                        .min(if pane.max_height > 0.0 { pane.max_height } else { f64::MAX });
                }
            }
        }
    }

    /// Get Y offset for a pane (where it starts from top)
    pub fn pane_y_offset(&self, pane_id: PaneId) -> f64 {
        let mut offset = 0.0;

        for &id in &self.order {
            if id == pane_id {
                break;
            }
            if let Some(pane) = self.panes.get(&id) {
                if !pane.collapsed {
                    offset += pane.height + self.separator_height;
                }
            }
        }

        offset
    }

    /// Find which pane contains a Y coordinate
    pub fn pane_at_y(&self, y: f64) -> Option<PaneId> {
        let mut current_y = 0.0;

        for &id in &self.order {
            if let Some(pane) = self.panes.get(&id) {
                if pane.collapsed {
                    continue;
                }

                let pane_end = current_y + pane.height;

                if y >= current_y && y < pane_end {
                    return Some(id);
                }

                current_y = pane_end + self.separator_height;
            }
        }

        None
    }

    /// Find pane at Y coordinate and return (PaneId, local_y)
    /// where local_y is the Y coordinate relative to the pane's top
    pub fn find_pane_at_y(&self, y: f64) -> Option<(PaneId, f64)> {
        let mut current_y = 0.0;

        for &id in &self.order {
            if let Some(pane) = self.panes.get(&id) {
                if pane.collapsed {
                    continue;
                }

                let pane_end = current_y + pane.height;

                if y >= current_y && y < pane_end {
                    let local_y = y - current_y;
                    return Some((id, local_y));
                }

                current_y = pane_end + self.separator_height;
            }
        }

        None
    }

    /// Get geometry for a specific pane (for hit testing and rendering)
    pub fn get_pane_geometry(&self, pane_id: PaneId, chart_width: f64) -> Option<PaneGeometry> {
        let pane = self.panes.get(&pane_id)?;

        Some(PaneGeometry {
            pane_id,
            instance_id: None, // Will be set by caller if needed
            index: self.order.iter().position(|&id| id == pane_id).unwrap_or(0),
            y_offset: self.pane_y_offset(pane_id),
            height: pane.height,
            chart_width,
            price_min: pane.price_min,
            price_max: pane.price_max,
        })
    }

    /// Get all pane geometries (for rendering all panes)
    pub fn get_all_geometries(&self, chart_width: f64) -> Vec<PaneGeometry> {
        let mut geometries = Vec::new();
        let mut y_offset = 0.0;

        for (index, &id) in self.order.iter().enumerate() {
            if let Some(pane) = self.panes.get(&id) {
                if pane.collapsed {
                    continue;
                }

                geometries.push(PaneGeometry {
                    pane_id: id,
                    instance_id: None,
                    index,
                    y_offset,
                    height: pane.height,
                    chart_width,
                    price_min: pane.price_min,
                    price_max: pane.price_max,
                });

                y_offset += pane.height + self.separator_height;
            }
        }

        geometries
    }

    /// Check if Y coordinate is on a separator
    pub fn separator_at_y(&self, y: f64) -> Option<PaneId> {
        let mut current_y = 0.0;

        for (i, &id) in self.order.iter().enumerate() {
            if let Some(pane) = self.panes.get(&id) {
                if pane.collapsed {
                    continue;
                }

                current_y += pane.height;

                // Check if on separator (between this pane and next)
                if i < self.order.len() - 1 {
                    if y >= current_y && y < current_y + self.separator_height {
                        return Some(id);
                    }
                    current_y += self.separator_height;
                }
            }
        }

        None
    }

    /// Start dragging a separator
    pub fn start_separator_drag(&mut self, pane_id: PaneId) {
        self.dragging_separator = Some(pane_id);
    }

    /// Update separator drag
    pub fn update_separator_drag(&mut self, delta_y: f64) {
        let Some(pane_id) = self.dragging_separator else {
            return;
        };

        // Find the pane and the one below it
        let pane_idx = self.order.iter().position(|&id| id == pane_id);
        let Some(idx) = pane_idx else {
            return;
        };

        if idx >= self.order.len() - 1 {
            return;
        }

        let next_id = self.order[idx + 1];

        // Get current heights
        let (pane_height, pane_min, pane_max) = {
            let pane = self.panes.get(&pane_id).unwrap();
            (pane.height, pane.min_height, if pane.max_height > 0.0 { pane.max_height } else { f64::MAX })
        };

        let (next_height, next_min, next_max) = {
            let next = self.panes.get(&next_id).unwrap();
            (next.height, next.min_height, if next.max_height > 0.0 { next.max_height } else { f64::MAX })
        };

        // Calculate new heights
        let new_pane_height = (pane_height + delta_y).max(pane_min).min(pane_max);
        let mut new_next_height = (next_height - delta_y).max(next_min).min(next_max);

        // Adjust if constraints prevent full delta
        let actual_delta = new_pane_height - pane_height;
        new_next_height = (next_height - actual_delta).max(next_min);

        // Apply new heights
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.height = new_pane_height;
            // Update ratio based on new height
            let total = new_pane_height + new_next_height;
            if total > 0.0 {
                pane.height_ratio = new_pane_height / self.total_height;
            }
        }

        if let Some(next) = self.panes.get_mut(&next_id) {
            next.height = new_next_height;
            let total = new_pane_height + new_next_height;
            if total > 0.0 {
                next.height_ratio = new_next_height / self.total_height;
            }
        }
    }

    /// End separator drag
    pub fn end_separator_drag(&mut self) {
        self.dragging_separator = None;
    }

    /// Check if currently dragging separator
    pub fn is_dragging_separator(&self) -> bool {
        self.dragging_separator.is_some()
    }

    /// Toggle pane collapsed state
    pub fn toggle_collapse(&mut self, pane_id: PaneId) {
        if pane_id == MAIN_PANE {
            return; // Can't collapse main pane
        }

        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.collapsed = !pane.collapsed;
        }

        self.recalculate_heights();
    }

    /// Get pane IDs assigned to indicators (for cleanup when indicator is removed)
    pub fn find_pane_by_title(&self, title: &str) -> Option<PaneId> {
        self.panes.iter()
            .find(|(_, p)| p.title == title)
            .map(|(&id, _)| id)
    }

    /// Identify which region of the chart a coordinate falls into.
    ///
    /// # Arguments
    /// * `x` - X coordinate in chart space
    /// * `y` - Y coordinate in chart space
    /// * `chart_width` - Width of the chart area (excluding price scale)
    /// * `chart_height` - Height of the chart area (excluding time scale)
    /// * `price_scale_width` - Width of the price scale
    /// * `time_scale_height` - Height of the time scale
    pub fn identify_region(
        &self,
        x: f64,
        y: f64,
        chart_width: f64,
        chart_height: f64,
        price_scale_width: f64,
        time_scale_height: f64,
    ) -> InteractionRegion {
        // Outside bounds check
        if x < 0.0 || y < 0.0 {
            return InteractionRegion::Outside;
        }

        let total_width = chart_width + price_scale_width;
        let total_height = chart_height + time_scale_height;

        if x > total_width || y > total_height {
            return InteractionRegion::Outside;
        }

        // Time scale region (bottom)
        if y >= chart_height {
            return InteractionRegion::TimeScale;
        }

        // Price scale region (right side, check for each pane)
        let in_price_scale = x >= chart_width;

        // Check for separator hit first
        if let Some(above_pane_id) = self.separator_at_y(y) {
            return InteractionRegion::Separator { above_pane_id };
        }

        // Find which pane we're in
        if let Some((pane_id, local_y)) = self.find_pane_at_y(y) {
            if in_price_scale {
                if pane_id == MAIN_PANE {
                    InteractionRegion::MainPriceScale { local_y }
                } else {
                    InteractionRegion::SubPanePriceScale { pane_id, local_y }
                }
            } else {
                if pane_id == MAIN_PANE {
                    InteractionRegion::MainChart { local_y }
                } else {
                    InteractionRegion::SubPane { pane_id, local_y }
                }
            }
        } else {
            InteractionRegion::Outside
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_manager_new() {
        let manager = PaneManager::new();
        assert_eq!(manager.pane_count(), 1);
        assert!(!manager.has_sub_panes());
    }

    #[test]
    fn test_add_pane() {
        let mut manager = PaneManager::new();
        let id = manager.add_pane("RSI");
        assert_eq!(manager.pane_count(), 2);
        assert!(manager.has_sub_panes());
        assert_eq!(manager.get_pane(id).unwrap().title, "RSI");
    }

    #[test]
    fn test_remove_pane() {
        let mut manager = PaneManager::new();
        let id = manager.add_pane("RSI");
        manager.remove_pane(id);
        assert_eq!(manager.pane_count(), 1);
    }

    #[test]
    fn test_cannot_remove_main_pane() {
        let mut manager = PaneManager::new();
        manager.remove_pane(MAIN_PANE);
        assert_eq!(manager.pane_count(), 1); // Still 1
    }

    #[test]
    fn test_pane_y_offset() {
        let mut manager = PaneManager::new();
        manager.set_total_height(500.0);

        // Add sub-pane
        let rsi_id = manager.add_pane("RSI");

        // Main pane starts at 0
        assert_eq!(manager.pane_y_offset(MAIN_PANE), 0.0);

        // RSI pane starts after main pane + separator
        let main_height = manager.main_pane().height;
        let expected_offset = main_height + manager.separator_height();
        assert!((manager.pane_y_offset(rsi_id) - expected_offset).abs() < 0.1);
    }

    #[test]
    fn test_sub_pane_creation() {
        let pane = SubPane::new(123);
        assert_eq!(pane.instance_id, 123);
        assert!(pane.auto_scale);
        assert_eq!(pane.price_min, 0.0);
        assert_eq!(pane.price_max, 100.0);
    }

    #[test]
    fn test_sub_pane_coordinate_conversion() {
        let mut pane = SubPane::new(1);
        pane.height = 100.0;
        pane.price_min = 0.0;
        pane.price_max = 100.0;

        // Top of pane (y=0) should be price_max
        assert!((pane.y_to_price(0.0) - 100.0).abs() < 0.001);
        // Bottom of pane (y=height) should be price_min
        assert!((pane.y_to_price(100.0) - 0.0).abs() < 0.001);
        // Middle should be middle price
        assert!((pane.y_to_price(50.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_sub_pane_zoom() {
        let mut pane = SubPane::new(1);
        pane.price_min = 0.0;
        pane.price_max = 100.0;
        pane.save_drag_start();

        // Zoom out by factor of 2
        pane.zoom_from_drag(2.0);
        assert!(!pane.auto_scale);
        // Center should be 50, range should double
        assert!((pane.price_min - (-50.0)).abs() < 0.001);
        assert!((pane.price_max - 150.0).abs() < 0.001);
    }

    #[test]
    fn test_sub_pane_pan() {
        let mut pane = SubPane::new(1);
        pane.price_min = 0.0;
        pane.price_max = 100.0;

        pane.pan(10.0);
        assert!(!pane.auto_scale);
        assert!((pane.price_min - 10.0).abs() < 0.001);
        assert!((pane.price_max - 110.0).abs() < 0.001);
    }
}
