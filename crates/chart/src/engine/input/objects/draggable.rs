//! Drag-and-Drop System for zengeld-chart
//!
//! Provides drag-and-drop functionality for chart primitives, markers, and price lines.
//! Uses Manhattan distance threshold for detecting drag vs click.
//!
//! # Key Concepts
//!
//! - **Draggable trait**: Implement on objects that can be dragged
//! - **DragState**: Tracks active drag operation
//! - **DragConstraints**: Limits drag movement
//! - **CursorStyle**: Visual feedback during drag
//!
//! # Usage
//!
//! ```ignore
//! // In your primitive implementation:
//! impl Draggable for PriceLine {
//!     fn can_drag(&self) -> bool { true }
//!     fn drag_axis(&self) -> DragAxis { DragAxis::Vertical }
//!     fn on_drag_start(&mut self, x: f64, y: f64) { ... }
//!     fn on_drag(&mut self, dx: f64, dy: f64, constraints: Option<&DragConstraints>) { ... }
//!     fn on_drag_end(&mut self) { ... }
//! }
//! ```

use serde::{Deserialize, Serialize};

// =============================================================================
// Manhattan Distance Threshold
// =============================================================================

/// Minimum pixel distance to distinguish drag from click (standard: 5px)
pub const DRAG_THRESHOLD: f64 = 5.0;

// =============================================================================
// Cursor Style
// =============================================================================

/// CSS cursor styles for drag operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CursorStyle {
    /// Default cursor (no drag possible)
    #[default]
    Default,
    /// Pointer (clickable)
    Pointer,
    /// Grab hand (draggable, not dragging)
    Grab,
    /// Grabbing hand (actively dragging)
    Grabbing,
    /// Move cursor (4-way arrows)
    Move,
    /// Vertical resize (↕)
    NsResize,
    /// Horizontal resize (↔)
    EwResize,
    /// Diagonal resize (↗↙)
    NeswResize,
    /// Diagonal resize (↖↘)
    NwseResize,
    /// Crosshair
    Crosshair,
    /// Not allowed
    NotAllowed,
    /// Hidden cursor (no visible cursor)
    None,
}

impl CursorStyle {
    /// Get CSS cursor value
    pub fn css_value(&self) -> &'static str {
        match self {
            CursorStyle::Default => "default",
            CursorStyle::Pointer => "pointer",
            CursorStyle::Grab => "grab",
            CursorStyle::Grabbing => "grabbing",
            CursorStyle::Move => "move",
            CursorStyle::NsResize => "ns-resize",
            CursorStyle::EwResize => "ew-resize",
            CursorStyle::NeswResize => "nesw-resize",
            CursorStyle::NwseResize => "nwse-resize",
            CursorStyle::Crosshair => "crosshair",
            CursorStyle::NotAllowed => "not-allowed",
            CursorStyle::None => "none",
        }
    }
}

// =============================================================================
// Drag Axis
// =============================================================================

/// Constraint axis for drag movement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DragAxis {
    /// Free movement in both directions
    #[default]
    Both,
    /// Horizontal movement only (X axis)
    Horizontal,
    /// Vertical movement only (Y axis)
    Vertical,
}

// =============================================================================
// Drag Constraints
// =============================================================================

/// Constraints for drag movement
///
/// Limits how far an object can be dragged and enables snap-to-grid behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DragConstraints {
    /// Minimum X coordinate (pixels or data)
    pub min_x: Option<f64>,
    /// Maximum X coordinate (pixels or data)
    pub max_x: Option<f64>,
    /// Minimum Y coordinate (pixels or data)
    pub min_y: Option<f64>,
    /// Maximum Y coordinate (pixels or data)
    pub max_y: Option<f64>,
    /// Snap X to grid (grid spacing in pixels)
    pub snap_x: Option<f64>,
    /// Snap Y to grid (grid spacing in pixels)
    pub snap_y: Option<f64>,
    /// Keep within chart bounds
    pub bound_to_chart: bool,
}

impl DragConstraints {
    /// Create constraints for vertical-only movement (e.g., price lines)
    pub fn vertical_only() -> Self {
        Self {
            min_x: Some(0.0),
            max_x: Some(0.0), // Prevents horizontal movement
            ..Default::default()
        }
    }

    /// Create constraints for horizontal-only movement (e.g., time markers)
    pub fn horizontal_only() -> Self {
        Self {
            min_y: Some(0.0),
            max_y: Some(0.0), // Prevents vertical movement
            ..Default::default()
        }
    }

    /// Create constraints bounded to chart area
    pub fn bounded(width: f64, height: f64) -> Self {
        Self {
            min_x: Some(0.0),
            max_x: Some(width),
            min_y: Some(0.0),
            max_y: Some(height),
            bound_to_chart: true,
            ..Default::default()
        }
    }

    /// Create constraints with grid snapping
    pub fn with_snap(snap_x: f64, snap_y: f64) -> Self {
        Self {
            snap_x: Some(snap_x),
            snap_y: Some(snap_y),
            ..Default::default()
        }
    }

    /// Apply constraints to coordinates
    ///
    /// Returns constrained (x, y) values.
    pub fn apply(&self, mut x: f64, mut y: f64) -> (f64, f64) {
        // Apply min/max bounds
        if let Some(min) = self.min_x {
            x = x.max(min);
        }
        if let Some(max) = self.max_x {
            x = x.min(max);
        }
        if let Some(min) = self.min_y {
            y = y.max(min);
        }
        if let Some(max) = self.max_y {
            y = y.min(max);
        }

        // Apply grid snapping
        if let Some(snap) = self.snap_x {
            if snap > 0.0 {
                x = (x / snap).round() * snap;
            }
        }
        if let Some(snap) = self.snap_y {
            if snap > 0.0 {
                y = (y / snap).round() * snap;
            }
        }

        (x, y)
    }

    /// Apply constraints to delta movement
    pub fn apply_delta(&self, dx: f64, dy: f64, current_x: f64, current_y: f64) -> (f64, f64) {
        let (new_x, new_y) = self.apply(current_x + dx, current_y + dy);
        (new_x - current_x, new_y - current_y)
    }
}

// =============================================================================
// Drag State
// =============================================================================

/// State of an active drag operation
///
/// Tracks the drag from start to finish, including coordinates
/// in both screen space and data space.
#[derive(Debug, Clone, Default)]
pub struct DragState {
    /// Is drag currently active?
    pub active: bool,

    /// Starting X coordinate (screen pixels)
    pub start_x: f64,
    /// Starting Y coordinate (screen pixels)
    pub start_y: f64,

    /// Current X coordinate (screen pixels)
    pub current_x: f64,
    /// Current Y coordinate (screen pixels)
    pub current_y: f64,

    /// Starting X in data space (bar index or time)
    pub start_data_x: f64,
    /// Starting Y in data space (price)
    pub start_data_y: f64,

    /// Current X in data space
    pub current_data_x: f64,
    /// Current Y in data space
    pub current_data_y: f64,

    /// ID of the object being dragged
    pub object_id: Option<String>,

    /// Whether Manhattan distance threshold has been exceeded
    pub threshold_exceeded: bool,
}

impl DragState {
    /// Create a new inactive drag state
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a drag operation
    pub fn start(&mut self, x: f64, y: f64, data_x: f64, data_y: f64, object_id: Option<String>) {
        self.active = true;
        self.start_x = x;
        self.start_y = y;
        self.current_x = x;
        self.current_y = y;
        self.start_data_x = data_x;
        self.start_data_y = data_y;
        self.current_data_x = data_x;
        self.current_data_y = data_y;
        self.object_id = object_id;
        self.threshold_exceeded = false;
    }

    /// Update drag position
    pub fn update(&mut self, x: f64, y: f64, data_x: f64, data_y: f64) {
        self.current_x = x;
        self.current_y = y;
        self.current_data_x = data_x;
        self.current_data_y = data_y;

        // Check Manhattan distance threshold
        if !self.threshold_exceeded {
            let manhattan = (x - self.start_x).abs() + (y - self.start_y).abs();
            self.threshold_exceeded = manhattan >= DRAG_THRESHOLD;
        }
    }

    /// End drag operation
    pub fn end(&mut self) {
        self.active = false;
        self.object_id = None;
    }

    /// Cancel drag operation (reset to start)
    pub fn cancel(&mut self) {
        self.active = false;
        self.current_x = self.start_x;
        self.current_y = self.start_y;
        self.current_data_x = self.start_data_x;
        self.current_data_y = self.start_data_y;
        self.object_id = None;
    }

    /// Get screen delta from start
    pub fn delta(&self) -> (f64, f64) {
        (self.current_x - self.start_x, self.current_y - self.start_y)
    }

    /// Get data delta from start
    pub fn data_delta(&self) -> (f64, f64) {
        (
            self.current_data_x - self.start_data_x,
            self.current_data_y - self.start_data_y,
        )
    }

    /// Check if this is a valid drag (threshold exceeded)
    pub fn is_valid_drag(&self) -> bool {
        self.active && self.threshold_exceeded
    }

    /// Check if this is just a click (threshold not exceeded)
    pub fn is_click(&self) -> bool {
        !self.threshold_exceeded
    }
}

// =============================================================================
// Draggable Trait
// =============================================================================

/// Trait for objects that can be dragged
///
/// Implement this trait to enable drag-and-drop for primitives, markers, etc.
/// Series and overlays should NOT implement this trait.
pub trait Draggable {
    /// Check if this object can be dragged
    fn can_drag(&self) -> bool;

    /// Get cursor style for hover state
    fn hover_cursor(&self) -> CursorStyle {
        if self.can_drag() {
            CursorStyle::Grab
        } else {
            CursorStyle::Default
        }
    }

    /// Get cursor style for drag state
    fn drag_cursor(&self) -> CursorStyle {
        CursorStyle::Grabbing
    }

    /// Get allowed drag axis
    fn drag_axis(&self) -> DragAxis {
        DragAxis::Both
    }

    /// Get drag constraints for this object
    fn drag_constraints(&self) -> Option<DragConstraints> {
        None
    }

    /// Called when drag starts
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate
    /// * `y` - Screen Y coordinate
    fn on_drag_start(&mut self, x: f64, y: f64);

    /// Called during drag movement
    ///
    /// # Arguments
    /// * `dx` - Delta X from start (screen pixels)
    /// * `dy` - Delta Y from start (screen pixels)
    /// * `constraints` - Optional drag constraints
    fn on_drag(&mut self, dx: f64, dy: f64, constraints: Option<&DragConstraints>);

    /// Called when drag ends
    fn on_drag_end(&mut self);

    /// Called when drag is cancelled (e.g., Escape key)
    fn on_drag_cancel(&mut self) {
        self.on_drag_end();
    }
}

// =============================================================================
// Hit Test Result with Drag Info
// =============================================================================

/// Result of hit testing with drag capability info
#[derive(Debug, Clone)]
pub struct HitTestResult {
    /// ID of the hit object
    pub object_id: Option<String>,
    /// Whether the object can be dragged
    pub draggable: bool,
    /// Cursor to display
    pub cursor: CursorStyle,
    /// Priority (higher = on top)
    pub priority: i32,
}

impl HitTestResult {
    /// Create a new hit test result
    pub fn new(object_id: Option<String>, draggable: bool, cursor: CursorStyle) -> Self {
        Self {
            object_id,
            draggable,
            cursor,
            priority: 0,
        }
    }

    /// Create result for a non-draggable hit
    pub fn non_draggable(object_id: Option<String>) -> Self {
        Self {
            object_id,
            draggable: false,
            cursor: CursorStyle::Default,
            priority: 0,
        }
    }

    /// Create result for a draggable hit
    pub fn draggable(object_id: Option<String>) -> Self {
        Self {
            object_id,
            draggable: true,
            cursor: CursorStyle::Grab,
            priority: 0,
        }
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

// =============================================================================
// Drag Manager
// =============================================================================

/// Manages drag operations for the chart
///
/// Coordinates drag state and hit testing across all draggable objects.
#[derive(Debug, Default)]
pub struct DragManager {
    /// Current drag state
    state: DragState,
    /// Last hit test result
    last_hit: Option<HitTestResult>,
}

impl DragManager {
    /// Create a new drag manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current drag state
    pub fn state(&self) -> &DragState {
        &self.state
    }

    /// Get mutable drag state
    pub fn state_mut(&mut self) -> &mut DragState {
        &mut self.state
    }

    /// Check if a drag is active
    pub fn is_dragging(&self) -> bool {
        self.state.active
    }

    /// Check if drag threshold has been exceeded
    pub fn is_valid_drag(&self) -> bool {
        self.state.is_valid_drag()
    }

    /// Get the ID of the object being dragged
    pub fn dragged_object_id(&self) -> Option<&str> {
        if self.state.active {
            self.state.object_id.as_deref()
        } else {
            None
        }
    }

    /// Start a drag operation
    pub fn start_drag(
        &mut self,
        x: f64,
        y: f64,
        data_x: f64,
        data_y: f64,
        object_id: Option<String>,
    ) {
        self.state.start(x, y, data_x, data_y, object_id);
    }

    /// Update drag position
    pub fn update_drag(&mut self, x: f64, y: f64, data_x: f64, data_y: f64) {
        self.state.update(x, y, data_x, data_y);
    }

    /// End drag operation
    pub fn end_drag(&mut self) {
        self.state.end();
    }

    /// Cancel drag operation
    pub fn cancel_drag(&mut self) {
        self.state.cancel();
    }

    /// Update last hit test result
    pub fn set_hit(&mut self, hit: Option<HitTestResult>) {
        self.last_hit = hit;
    }

    /// Get last hit test result
    pub fn last_hit(&self) -> Option<&HitTestResult> {
        self.last_hit.as_ref()
    }

    /// Get current cursor based on state
    pub fn current_cursor(&self) -> CursorStyle {
        if self.state.active {
            CursorStyle::Grabbing
        } else if let Some(hit) = &self.last_hit {
            hit.cursor
        } else {
            CursorStyle::Default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drag_state_lifecycle() {
        let mut state = DragState::new();
        assert!(!state.active);

        // Start drag
        state.start(100.0, 200.0, 10.0, 50000.0, Some("test".to_string()));
        assert!(state.active);
        assert!(!state.threshold_exceeded);

        // Small movement (below threshold)
        state.update(102.0, 201.0, 10.1, 50001.0);
        assert!(!state.threshold_exceeded);
        assert!(state.is_click());

        // Large movement (exceeds threshold)
        state.update(110.0, 210.0, 11.0, 50100.0);
        assert!(state.threshold_exceeded);
        assert!(state.is_valid_drag());

        // Check deltas
        let (dx, dy) = state.delta();
        assert_eq!(dx, 10.0);
        assert_eq!(dy, 10.0);

        // End drag
        state.end();
        assert!(!state.active);
    }

    #[test]
    fn test_drag_state_cancel() {
        let mut state = DragState::new();
        state.start(100.0, 200.0, 10.0, 50000.0, None);
        state.update(150.0, 250.0, 15.0, 50500.0);

        state.cancel();

        assert!(!state.active);
        assert_eq!(state.current_x, 100.0);
        assert_eq!(state.current_y, 200.0);
    }

    #[test]
    fn test_drag_constraints_apply() {
        let constraints = DragConstraints {
            min_x: Some(0.0),
            max_x: Some(100.0),
            min_y: Some(0.0),
            max_y: Some(50.0),
            snap_x: Some(10.0),
            snap_y: None,
            bound_to_chart: true,
        };

        // Within bounds
        let (x, y) = constraints.apply(55.0, 25.0);
        assert_eq!(x, 60.0); // Snapped to 10
        assert_eq!(y, 25.0);

        // Outside bounds
        let (x, y) = constraints.apply(-10.0, 100.0);
        assert_eq!(x, 0.0); // Clamped to min
        assert_eq!(y, 50.0); // Clamped to max

        // Edge snapping
        let (x, y) = constraints.apply(94.0, 25.0);
        assert_eq!(x, 90.0); // Snapped to 90
    }

    #[test]
    fn test_drag_constraints_vertical_only() {
        let constraints = DragConstraints::vertical_only();

        let (x, y) = constraints.apply(100.0, 50.0);
        assert_eq!(x, 0.0); // Constrained to 0
        assert_eq!(y, 50.0); // Free
    }

    #[test]
    fn test_cursor_style_css() {
        assert_eq!(CursorStyle::Grab.css_value(), "grab");
        assert_eq!(CursorStyle::Grabbing.css_value(), "grabbing");
        assert_eq!(CursorStyle::NsResize.css_value(), "ns-resize");
    }

    #[test]
    fn test_drag_manager() {
        let mut manager = DragManager::new();
        assert!(!manager.is_dragging());

        manager.start_drag(100.0, 200.0, 10.0, 50000.0, Some("line1".to_string()));
        assert!(manager.is_dragging());
        assert_eq!(manager.dragged_object_id(), Some("line1"));

        manager.update_drag(150.0, 250.0, 15.0, 50500.0);
        assert!(manager.is_valid_drag());

        manager.end_drag();
        assert!(!manager.is_dragging());
    }

    #[test]
    fn test_hit_test_result() {
        let hit = HitTestResult::draggable(Some("marker1".to_string())).with_priority(10);

        assert!(hit.draggable);
        assert_eq!(hit.cursor, CursorStyle::Grab);
        assert_eq!(hit.priority, 10);
    }
}
