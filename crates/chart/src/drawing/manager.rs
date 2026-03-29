//! Drawing manager - high-level API for the drawing system (v2)
//!
//! Works with PrimitiveRegistry and Box<dyn Primitive> instead of hardcoded types.

use crate::{PriceScale, Viewport, Bar, find_bar_for_timestamp};
use super::primitives_v2::{
    Primitive, PrimitiveRegistry, ClickBehavior,
    ControlPoint, ControlPointType, HitTestResult,
    PrimitiveExt,  // For is_locked, set_locked, is_visible, set_visible
};

/// Drawing state - tracks multi-click primitive creation
#[derive(Clone, Debug, Default)]
pub enum DrawingState {
    /// No drawing in progress
    #[default]
    Idle,
    /// First point placed, waiting for more points
    Creating {
        /// Tool type_id being created
        tool_id: String,
        /// Accumulated points so far
        points: Vec<(f64, f64)>,
    },
}

impl DrawingState {
    /// Check if we're in the middle of a drawing operation
    pub fn is_drawing(&self) -> bool {
        matches!(self, DrawingState::Creating { .. })
    }

    /// Get the first point if creating
    pub fn first_point(&self) -> Option<(f64, f64)> {
        match self {
            DrawingState::Creating { points, .. } => points.first().copied(),
            _ => None,
        }
    }

    /// Get the tool being used
    pub fn tool_id(&self) -> Option<&str> {
        match self {
            DrawingState::Creating { tool_id, .. } => Some(tool_id),
            _ => None,
        }
    }

    /// Get all points accumulated so far
    pub fn points(&self) -> &[(f64, f64)] {
        match self {
            DrawingState::Creating { points, .. } => points,
            _ => &[],
        }
    }

    /// Cancel current drawing operation
    pub fn cancel(&mut self) {
        *self = DrawingState::Idle;
    }
}

/// Type of drag operation in progress
#[derive(Clone, Debug, PartialEq)]
pub enum DragType {
    /// Dragging the whole primitive (translate)
    Move,
    /// Dragging a specific control point (resize/reshape)
    ControlPoint(ControlPointType),
}

impl Default for DragType {
    fn default() -> Self {
        DragType::Move
    }
}

/// Drawing manager - complete drawing system using PrimitiveRegistry
///
/// This manages all drawing operations including:
/// - Tool selection (by type_id string)
/// - Primitive creation via PrimitiveRegistry
/// - Primitive storage as Box<dyn Primitive>
/// - Hit testing
/// - Drag and drop (move + control points)
pub struct DrawingManager {
    /// Current selected tool type_id (None = cursor mode)
    current_tool: Option<String>,
    /// Tool variant (e.g., "target" for "emoji:target")
    tool_variant: Option<String>,
    /// Drawing state machine
    state: DrawingState,
    /// All created primitives
    primitives: Vec<Box<dyn Primitive>>,
    /// Default color for new primitives
    default_color: String,
    /// Currently selected primitive index
    selected: Option<usize>,
    /// Primitive being dragged
    dragging: Option<usize>,
    /// Type of drag operation
    drag_type: DragType,
    /// Drag start position (bar, price)
    drag_start: Option<(f64, f64)>,
    /// Lock drawings (prevent editing/dragging)
    locked: bool,
    /// Drawings visible
    visible: bool,
    /// Current pane ID for new primitives (None = main chart, Some = sub-pane indicator instance)
    current_pane_id: Option<u64>,
    /// Current window ID for new primitives (for multi-window support)
    current_window_id: Option<u64>,
}

impl Default for DrawingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DrawingManager {
    /// Clone the drawing manager, deep-cloning all primitives via `clone_box`.
    ///
    /// Used when seeding split sub-windows so existing drawings appear in
    /// the first leaf of a newly-created split.
    fn clone(&self) -> Self {
        Self {
            current_tool: self.current_tool.clone(),
            tool_variant: self.tool_variant.clone(),
            state: self.state.clone(),
            // Box<dyn Primitive> has Clone via clone_box — Vec clone works directly.
            primitives: self.primitives.clone(),
            default_color: self.default_color.clone(),
            selected: None,    // Reset selection state — not meaningful after clone
            dragging: None,    // Reset drag state
            drag_type: DragType::Move,
            drag_start: None,
            locked: self.locked,
            visible: self.visible,
            current_pane_id: self.current_pane_id,
            current_window_id: self.current_window_id,
        }
    }
}

impl DrawingManager {
    /// Create a new drawing manager
    pub fn new() -> Self {
        Self {
            current_tool: None,
            tool_variant: None,
            state: DrawingState::Idle,
            primitives: Vec::new(),
            default_color: "#2196F3".to_string(),
            selected: None,
            dragging: None,
            drag_type: DragType::Move,
            drag_start: None,
            locked: false,
            visible: true,
            current_pane_id: None,
            current_window_id: None,
        }
    }

    /// Get tool variant (e.g., "target" for "emoji:target")
    pub fn tool_variant(&self) -> Option<&str> {
        self.tool_variant.as_deref()
    }

    // =========================================================================
    // Tool Management
    // =========================================================================

    /// Get current tool type_id (None = cursor mode)
    pub fn current_tool(&self) -> Option<&str> {
        self.current_tool.as_deref()
    }

    /// Set current tool by type_id
    ///
    /// Use None, "cursor", or "none" to switch to cursor/selection mode.
    /// Any other string selects that primitive type from the registry.
    /// Supports "tool:variant" format (e.g., "emoji:target" parses to tool="emoji", variant="target")
    pub fn set_tool(&mut self, tool_id: Option<&str>) {
        // Cancel any drawing in progress
        self.state.cancel();
        self.tool_variant = None;

        self.current_tool = match tool_id {
            None | Some("cursor") | Some("none") | Some("crosshair") | Some("hand") => None,
            Some(id) => {
                // Parse "tool:variant" format (e.g., "emoji:target")
                let (base_id, variant) = if let Some(colon_pos) = id.find(':') {
                    let base = &id[..colon_pos];
                    let var = &id[colon_pos + 1..];
                    (base, Some(var.to_string()))
                } else {
                    (id, None)
                };

                // Verify tool exists in registry
                let registry = PrimitiveRegistry::global().read().unwrap();
                if let Some(meta) = registry.get(base_id) {
                    // Store variant if present
                    self.tool_variant = variant;

                    // For FreehandDrag tools, immediately enter Creating state
                    // so drag starts drawing without needing a "priming" click
                    if meta.click_behavior == ClickBehavior::FreehandDrag {
                        self.state = DrawingState::Creating {
                            tool_id: base_id.to_string(),
                            points: Vec::new(), // Empty - first point added on drag start
                        };
                    }
                    Some(base_id.to_string())
                } else {
                    // Unknown tool_id, fall back to cursor mode
                    None
                }
            }
        };
    }

    /// Set default color for new primitives
    pub fn set_default_color(&mut self, color: &str) {
        self.default_color = color.to_string();
    }

    /// Get default color (from manager)
    pub fn default_color(&self) -> &str {
        &self.default_color
    }

    /// Get the effective color for the current tool
    /// Returns the tool's metadata default_color if available, otherwise the manager's default_color
    pub fn effective_color(&self) -> String {
        if let Some(tool_id) = &self.current_tool {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                return meta.default_color.to_string();
            }
        }
        self.default_color.clone()
    }

    /// Check if drawings are locked
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Set lock state
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Toggle lock state
    pub fn toggle_lock(&mut self) {
        self.locked = !self.locked;
    }

    /// Check if drawings are visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set visibility state
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Toggle visibility state
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    // =========================================================================
    // Pane Context for Sub-pane Primitives
    // =========================================================================

    /// Set the current pane ID for new primitives
    ///
    /// Call this before on_click() when creating primitives on indicator sub-panes.
    /// None = main chart, Some(instance_id) = indicator sub-pane
    pub fn set_current_pane(&mut self, pane_id: Option<u64>) {
        self.current_pane_id = pane_id;
    }

    /// Get the current pane ID
    pub fn current_pane(&self) -> Option<u64> {
        self.current_pane_id
    }

    // =========================================================================
    // Window Context for Multi-Window Support
    // =========================================================================

    /// Set the current window ID for new primitives
    ///
    /// Call this when switching active windows to ensure new primitives
    /// are associated with the correct window.
    pub fn set_current_window(&mut self, window_id: Option<u64>) {
        self.current_window_id = window_id;
    }

    /// Get the current window ID
    pub fn current_window(&self) -> Option<u64> {
        self.current_window_id
    }

    /// Get primitives filtered by window ID
    ///
    /// Returns iterator over primitives that belong to the specified window.
    /// If window_id is None, returns primitives with no window assigned (legacy).
    pub fn primitives_for_window(&self, window_id: Option<u64>) -> impl Iterator<Item = &dyn Primitive> {
        self.primitives.iter()
            .filter(move |p| p.data().window_id == window_id)
            .map(|p| p.as_ref())
    }

    /// Get primitive indices for a specific window
    pub fn primitive_indices_for_window(&self, window_id: Option<u64>) -> Vec<usize> {
        self.primitives.iter()
            .enumerate()
            .filter(|(_, p)| p.data().window_id == window_id)
            .map(|(i, _)| i)
            .collect()
    }

    // =========================================================================
    // Drawing State
    // =========================================================================

    /// Check if currently drawing
    pub fn is_drawing(&self) -> bool {
        self.state.is_drawing()
    }

    /// Get drawing state for preview rendering
    pub fn drawing_state(&self) -> &DrawingState {
        &self.state
    }

    /// Get accumulated points during drawing (for anchor point display)
    pub fn drawing_points(&self) -> Option<&[(f64, f64)]> {
        match &self.state {
            DrawingState::Creating { points, .. } => Some(points.as_slice()),
            DrawingState::Idle => None,
        }
    }

    /// Cancel current drawing operation
    pub fn cancel_drawing(&mut self) {
        self.state.cancel();
    }

    /// Set drawing state from a sync peer (for preview sync).
    ///
    /// Called by the sync propagation logic to mirror the active leaf's
    /// in-progress creation onto peer leaves so `create_preview` renders
    /// the rubber-band line there too.
    ///
    /// `tool_id = None` means the peer finished or cancelled — clears our
    /// synced state, but only when we are not drawing ourselves (to avoid
    /// clobbering a locally-started drawing).
    pub fn set_synced_drawing_state(&mut self, tool_id: Option<String>, points: Vec<(f64, f64)>) {
        match tool_id {
            Some(id) => {
                self.state = DrawingState::Creating { tool_id: id, points };
            }
            None => {
                if self.state.is_drawing() {
                    self.state = DrawingState::Idle;
                }
            }
        }
    }

    /// Start freehand drawing on drag start
    /// Adds the first point to the already-active Creating state
    /// Returns true if point was added
    pub fn start_freehand(&mut self, bar: f64, price: f64) -> bool {
        // FreehandDrag tools are already in Creating state from set_tool()
        // Just add the first point when drag starts
        if let DrawingState::Creating { tool_id, points } = &mut self.state {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                if meta.click_behavior == ClickBehavior::FreehandDrag && points.is_empty() {
                    drop(registry);
                    points.push((bar, price));
                    return true;
                }
            }
        }
        false
    }

    /// Add a point during freehand drawing
    /// Returns true if point was added
    /// Uses minimum distance filtering to avoid too many close points
    pub fn add_freehand_point(&mut self, bar: f64, price: f64) -> bool {
        // Minimum distance threshold (in data coordinates) - points closer than this are skipped
        // This helps reduce jitter while still capturing the overall stroke shape
        const MIN_BAR_DIST: f64 = 0.15;  // ~15% of a bar width
        const MIN_PRICE_RATIO: f64 = 0.0002; // ~0.02% of price (works across different price scales)

        if let DrawingState::Creating { tool_id, points } = &mut self.state {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                if meta.click_behavior == ClickBehavior::FreehandDrag {
                    // Check distance from last point
                    if let Some(&(last_bar, last_price)) = points.last() {
                        let bar_dist = (bar - last_bar).abs();
                        let price_dist = (price - last_price).abs();
                        let avg_price = (price + last_price).abs() / 2.0;
                        let price_threshold = avg_price * MIN_PRICE_RATIO;

                        // Skip if too close to last point
                        if bar_dist < MIN_BAR_DIST && price_dist < price_threshold {
                            return false;
                        }
                    }

                    points.push((bar, price));
                    return true;
                }
            }
        }
        false
    }

    /// Check if current tool is a FreehandDrag tool
    pub fn is_freehand_tool(&self) -> bool {
        if let Some(ref tool_id) = self.current_tool {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                return meta.click_behavior == ClickBehavior::FreehandDrag;
            }
        }
        false
    }

    /// Complete freehand drawing and create the primitive
    /// Returns true if primitive was created
    /// NOTE: Unlike other primitives, FreehandDrag tools stay active after completion
    /// to allow continuous drawing without re-selecting the tool
    pub fn complete_freehand(&mut self) -> bool {
        if let DrawingState::Creating { tool_id, points } = &self.state {
            let point_count = points.len();

            if point_count >= 2 {
                let tool_id_clone = tool_id.clone();
                let registry = PrimitiveRegistry::global().read().unwrap();
                if let Some(meta) = registry.get(&tool_id_clone) {
                    if meta.click_behavior == ClickBehavior::FreehandDrag {
                        // Use tool's default color from metadata
                        let color = meta.default_color;
                        if let Some(mut prim) = registry.create(&tool_id_clone, points, Some(color)) {
                            drop(registry); // Release lock before mutable borrow
                            prim.data_mut().id = crate::drawing::alloc_primitive_id();
                            prim.data_mut().pane_id = self.current_pane_id;
                            prim.data_mut().window_id = self.current_window_id;
                            self.primitives.push(prim);
                            self.selected = Some(self.primitives.len() - 1);
                            // Reset to ready state for continuous drawing
                            // Tool stays active, just clear points for next stroke
                            self.state = DrawingState::Creating {
                                tool_id: tool_id_clone,
                                points: Vec::new(),
                            };
                            // Don't reset current_tool - stay in freehand mode
                            self.current_pane_id = None;
                            return true;
                        }
                    }
                }
            } else {
                // Not enough points - stay in Creating state for FreehandDrag tools
                // so user can try again without re-selecting the tool
                let tool_id_clone = tool_id.clone();
                // Clear points but stay in Creating state
                self.state = DrawingState::Creating {
                    tool_id: tool_id_clone,
                    points: Vec::new(),
                };
                return false;
            }
        }
        false
    }

    /// Create a preview primitive for the current drawing state
    ///
    /// This creates a temporary primitive using accumulated points + cursor position.
    /// Used for live preview while drawing multi-point primitives.
    /// Returns None if not currently drawing or if preview can't be created.
    pub fn create_preview(&self, cursor_bar: f64, cursor_price: f64) -> Option<Box<dyn Primitive>> {
        let DrawingState::Creating { tool_id, points } = &self.state else {
            return None;
        };

        if points.is_empty() {
            return None;
        }

        let registry = PrimitiveRegistry::global().read().unwrap();
        let meta = registry.get(tool_id)?;

        // Build preview points: existing points + cursor
        let mut preview_points = points.clone();
        preview_points.push((cursor_bar, cursor_price));

        // For primitives with fixed point count, pad with cursor position
        // This creates a "collapsed" shape that expands as user clicks
        // FreehandDrag doesn't use this preview - it has its own rendering
        let min_points = match meta.click_behavior {
            ClickBehavior::SingleClick => 1,
            ClickBehavior::TwoPoint | ClickBehavior::ClickDrag => 2,
            ClickBehavior::ThreePoint => 3,
            ClickBehavior::FourPoint => 4,
            ClickBehavior::MultiPoint(n) => n as usize,
            ClickBehavior::FreehandDrag => return None, // Uses custom preview rendering
        };

        while preview_points.len() < min_points {
            preview_points.push((cursor_bar, cursor_price));
        }

        // Create the primitive with preview points
        let mut prim = (meta.factory)(&preview_points, &self.default_color);

        // Apply tool variant (e.g., emoji type) to preview
        if let Some(variant) = &self.tool_variant {
            self.apply_variant_to_primitive(&mut prim, tool_id, variant);
        }

        Some(prim)
    }

    // =========================================================================
    // Click Handling
    // =========================================================================

    /// Handle a click at the given data coordinates
    ///
    /// Returns true if a primitive was created
    pub fn on_click(&mut self, bar: f64, price: f64) -> bool {
        let tool_id = match &self.current_tool {
            Some(id) => id.clone(),
            None => return false,
        };

        let registry = PrimitiveRegistry::global().read().unwrap();
        let click_behavior = match registry.click_behavior(&tool_id) {
            Some(cb) => cb,
            None => return false,
        };

        match click_behavior {
            ClickBehavior::SingleClick => {
                // Create immediately with single point
                if let Some(mut prim) = registry.create(&tool_id, &[(bar, price)], Some(&self.default_color)) {
                    prim.data_mut().id = crate::drawing::alloc_primitive_id();
                    prim.data_mut().pane_id = self.current_pane_id;
                    prim.data_mut().window_id = self.current_window_id;

                    // Apply tool variant (e.g., emoji type)
                    if let Some(variant) = &self.tool_variant {
                        self.apply_variant_to_primitive(&mut prim, &tool_id, variant);
                    }

                    self.primitives.push(prim);
                    self.selected = Some(self.primitives.len() - 1);
                    self.current_tool = None; // Reset tool after creation
                    self.tool_variant = None; // Reset variant
                    self.current_pane_id = None; // Reset pane context
                    return true;
                }
            }
            ClickBehavior::TwoPoint | ClickBehavior::ClickDrag => {
                match &mut self.state {
                    DrawingState::Idle => {
                        // First click - start creating
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                    DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == &tool_id => {
                        // Second click - create primitive
                        points.push((bar, price));
                        if let Some(mut prim) = registry.create(&tool_id, points, Some(&self.default_color)) {
                            prim.data_mut().id = crate::drawing::alloc_primitive_id();
                            prim.data_mut().pane_id = self.current_pane_id;
                            prim.data_mut().window_id = self.current_window_id;

                            // Apply tool variant (e.g., emoji type)
                            if let Some(variant) = &self.tool_variant {
                                self.apply_variant_to_primitive(&mut prim, &tool_id, variant);
                            }

                            self.primitives.push(prim);
                            self.selected = Some(self.primitives.len() - 1);
                            self.state = DrawingState::Idle;
                            self.current_tool = None; // Reset tool after creation
                            self.tool_variant = None; // Reset variant
                            self.current_pane_id = None; // Reset pane context
                            return true;
                        }
                        self.state = DrawingState::Idle;
                    }
                    _ => {
                        // Different tool - restart
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                }
            }
            ClickBehavior::ThreePoint => {
                match &mut self.state {
                    DrawingState::Idle => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                    DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == &tool_id => {
                        points.push((bar, price));
                        if points.len() >= 3 {
                            if let Some(mut prim) = registry.create(&tool_id, points, Some(&self.default_color)) {
                                prim.data_mut().id = crate::drawing::alloc_primitive_id();
                                prim.data_mut().pane_id = self.current_pane_id;
                                prim.data_mut().window_id = self.current_window_id;
                                self.primitives.push(prim);
                                self.selected = Some(self.primitives.len() - 1);
                                self.state = DrawingState::Idle;
                                self.current_tool = None;
                                self.current_pane_id = None; // Reset pane context
                                return true;
                            }
                            self.state = DrawingState::Idle;
                        }
                    }
                    _ => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                }
            }
            ClickBehavior::FourPoint => {
                match &mut self.state {
                    DrawingState::Idle => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                    DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == &tool_id => {
                        points.push((bar, price));
                        if points.len() >= 4 {
                            if let Some(mut prim) = registry.create(&tool_id, points, Some(&self.default_color)) {
                                prim.data_mut().id = crate::drawing::alloc_primitive_id();
                                prim.data_mut().pane_id = self.current_pane_id;
                                self.primitives.push(prim);
                                self.selected = Some(self.primitives.len() - 1);
                                self.state = DrawingState::Idle;
                                self.current_tool = None;
                                self.current_pane_id = None; // Reset pane context
                                return true;
                            }
                            self.state = DrawingState::Idle;
                        }
                    }
                    _ => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                }
            }
            ClickBehavior::MultiPoint(min_points) => {
                // For multi-point with exact count, auto-finish when reached
                // Single click adds points until min_points is reached
                match &mut self.state {
                    DrawingState::Idle => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                    DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == &tool_id => {
                        points.push((bar, price));
                        // Auto-finish when we have enough points
                        if points.len() >= min_points as usize {
                            if let Some(mut prim) = registry.create(&tool_id, points, Some(&self.default_color)) {
                                prim.data_mut().id = crate::drawing::alloc_primitive_id();
                                prim.data_mut().pane_id = self.current_pane_id;
                                self.primitives.push(prim);
                                self.selected = Some(self.primitives.len() - 1);
                                self.state = DrawingState::Idle;
                                self.current_tool = None;
                                self.current_pane_id = None; // Reset pane context
                                return true;
                            }
                            self.state = DrawingState::Idle;
                        }
                    }
                    _ => {
                        self.state = DrawingState::Creating {
                            tool_id: tool_id.clone(),
                            points: vec![(bar, price)],
                        };
                    }
                }
            }
            ClickBehavior::FreehandDrag => {
                // FreehandDrag ignores clicks - only works via drag
                // This prevents accidental tool reset when clicking
                return false;
            }
        }

        false
    }

    /// Finish a multi-point primitive (called on double-click or Enter)
    pub fn finish_multipoint(&mut self) -> bool {
        if let DrawingState::Creating { tool_id, points } = &self.state {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(ClickBehavior::MultiPoint(min)) = registry.click_behavior(tool_id) {
                if points.len() >= min as usize {
                    if let Some(mut prim) = registry.create(tool_id, points, Some(&self.default_color)) {
                        prim.data_mut().id = crate::drawing::alloc_primitive_id();
                        prim.data_mut().pane_id = self.current_pane_id;
                        self.primitives.push(prim);
                        self.selected = Some(self.primitives.len() - 1);
                        self.state = DrawingState::Idle;
                        self.current_tool = None;
                        self.current_pane_id = None; // Reset pane context
                        return true;
                    }
                }
            }
        }
        self.state = DrawingState::Idle;
        false
    }

    // =========================================================================
    // Primitive Access
    // =========================================================================

    /// Get all primitives as a slice.
    ///
    /// Alias for `primitives()` — used by grouped-window render fallback in the
    /// standalone (`group_id == None`) path.
    pub fn primitives_slice(&self) -> &[Box<dyn Primitive>] {
        &self.primitives
    }

    /// Get all primitives
    pub fn primitives(&self) -> &[Box<dyn Primitive>] {
        &self.primitives
    }

    /// Get mutable access to primitives
    pub fn primitives_mut(&mut self) -> &mut Vec<Box<dyn Primitive>> {
        &mut self.primitives
    }

    /// Replace the completed-primitive list with clones from a TagManager group.
    ///
    /// Called each frame for grouped windows so that the `DrawingManager` acts as
    /// a render cache, always reflecting the authoritative group state.
    /// In-progress drag state (`dragging`) is preserved across the replace because
    /// drag is an active operation. Selection is UI state and is always cleared.
    pub fn sync_from_group_primitives(&mut self, group_prims: &[Box<dyn Primitive>]) {
        // Preserve drag by primitive id across the full replace.
        // Selection is intentionally NOT preserved — it is UI state, not group data.
        let dragging_id = self.dragging.and_then(|idx| self.primitives.get(idx).map(|p| p.data().id));

        self.primitives = group_prims.iter().map(|p| p.clone_box()).collect();
        self.selected = None;

        // Restore drag index by matching primitive id.
        self.dragging = dragging_id.and_then(|id| self.primitives.iter().position(|p| p.data().id == id));
        if self.dragging.is_none() {
            self.drag_start = None;
        }
    }

    /// Add an externally-created primitive with proper ID assignment
    /// Use this when creating primitives outside the normal on_click flow
    pub fn add_external_primitive(&mut self, mut prim: Box<dyn Primitive>) {
        prim.data_mut().id = crate::drawing::alloc_primitive_id();
        prim.data_mut().pane_id = self.current_pane_id;
        prim.data_mut().window_id = self.current_window_id;
        self.primitives.push(prim);
        self.selected = Some(self.primitives.len() - 1);
    }

    /// Get primitives sorted by z_order for rendering (lowest z_order first)
    /// Returns indices sorted by z_order
    pub fn primitives_sorted_by_z_order(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.primitives.len()).collect();
        indices.sort_by_key(|&i| self.primitives[i].data().z_order);
        indices
    }

    /// Get primitive count
    pub fn count(&self) -> usize {
        self.primitives.len()
    }

    /// Remove a primitive by index
    pub fn remove(&mut self, index: usize) -> Option<Box<dyn Primitive>> {
        if index < self.primitives.len() {
            if self.selected == Some(index) {
                self.selected = None;
            } else if let Some(sel) = self.selected {
                if sel > index {
                    self.selected = Some(sel - 1);
                }
            }
            Some(self.primitives.remove(index))
        } else {
            None
        }
    }

    /// Clear all primitives
    pub fn clear(&mut self) {
        self.primitives.clear();
        self.selected = None;
        self.dragging = None;
    }

    /// Recalculate bar positions for all primitives from their stored timestamps.
    ///
    /// Should be called when timeframe changes to update primitive positions.
    /// Uses centralized point_timestamps in PrimitiveData.
    pub fn recalculate_all_bar_caches(&mut self, bars: &[Bar]) {
        for prim in &mut self.primitives {
            let timestamps = &prim.data().point_timestamps;
            if timestamps.is_empty() {
                // No timestamps stored yet - skip (primitive will use current bar positions)
                continue;
            }

            // Get current points to extract prices
            let current_points = prim.points();
            if current_points.len() != timestamps.len() {
                // Mismatch - skip to avoid corruption
                continue;
            }

            // Convert timestamps to new bar indices, keeping prices
            let new_points: Vec<(f64, f64)> = timestamps
                .iter()
                .zip(current_points.iter())
                .map(|(ts, (old_bar, price))| {
                    let bar = match find_bar_for_timestamp(bars, *ts) {
                        Some(idx) => idx as f64,
                        None => *old_bar,
                    };
                    (bar, *price)
                })
                .collect();

            prim.set_points(&new_points);
        }
    }

    /// Ensure all primitives have point_timestamps populated.
    ///
    /// For primitives with empty timestamps, compute them from current bar indices.
    /// This is a one-time migration for old presets that did not save timestamps.
    /// Must be called BEFORE recalculate_all_bar_caches so that method has
    /// timestamps to work with and will not skip these primitives.
    pub fn ensure_timestamps_populated(&mut self, bars: &[Bar]) {
        for prim in &mut self.primitives {
            if prim.data().point_timestamps.is_empty() && !bars.is_empty() {
                Self::sync_primitive_timestamps(prim.as_mut(), bars);
            }
        }
    }

    /// Update timestamps from current bar indices for all primitives.
    ///
    /// Should be called after primitive creation or drag operations to sync timestamps.
    /// Uses centralized point_timestamps in PrimitiveData.
    pub fn update_all_timestamps_from_bars(&mut self, bars: &[Bar]) {
        for prim in &mut self.primitives {
            Self::sync_primitive_timestamps(prim.as_mut(), bars);
        }
    }

    /// Sync timestamps for a single primitive from its current bar positions.
    pub fn sync_primitive_timestamps(prim: &mut dyn Primitive, bars: &[Bar]) {
        let points = prim.points();
        let timestamps: Vec<i64> = points
            .iter()
            .map(|(bar, _price)| Self::bar_idx_to_timestamp(*bar as usize, bars))
            .collect();
        prim.data_mut().point_timestamps = timestamps;
    }

    /// Convert bar index to timestamp, with extrapolation for future bars.
    fn bar_idx_to_timestamp(bar_idx: usize, bars: &[Bar]) -> i64 {
        if bars.is_empty() {
            return 0;
        }

        // Within bounds - direct lookup
        if bar_idx < bars.len() {
            return bars[bar_idx].timestamp;
        }

        // Out of bounds (future) - extrapolate based on bar interval
        let interval = if bars.len() >= 2 {
            bars[bars.len() - 1].timestamp - bars[bars.len() - 2].timestamp
        } else {
            3600 // Default to 1 hour
        };

        let last_timestamp = bars[bars.len() - 1].timestamp;
        let bars_beyond = bar_idx - (bars.len() - 1);
        last_timestamp + (bars_beyond as i64 * interval)
    }

    // =========================================================================
    // Selection
    // =========================================================================

    /// Get selected primitive index
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Set selected primitive
    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Delete selected primitive
    pub fn delete_selected(&mut self) -> bool {
        if let Some(idx) = self.selected {
            self.remove(idx);
            true
        } else {
            false
        }
    }

    /// Try to select a primitive at screen coordinates
    pub fn try_select_at(&mut self, x: f64, y: f64, viewport: &Viewport, price_scale: &PriceScale) -> bool {
        if let Some(idx) = self.hit_test(x, y, viewport, price_scale) {
            self.selected = Some(idx);
            true
        } else {
            self.selected = None;
            false
        }
    }

    /// Deselect any selected primitive
    pub fn deselect(&mut self) {
        self.selected = None;
    }

    /// Apply variant to a newly created primitive
    ///
    /// This handles tool:variant format (e.g., "emoji:target" sets emoji type)
    fn apply_variant_to_primitive(&self, prim: &mut Box<dyn Primitive>, tool_id: &str, variant: &str) {
        use super::primitives_v2::icons::emoji::{Emoji, EmojiType};

        if tool_id == "emoji" {
            // Parse emoji variant and apply
            if let Some(emoji_type) = EmojiType::from_id(variant) {
                // Deserialize, modify, re-box
                let json = prim.to_json();
                if let Ok(mut emoji) = serde_json::from_str::<Emoji>(&json) {
                    emoji.emoji_type = emoji_type;
                    *prim = Box::new(emoji);
                }
            }
        }
        // Future: handle other tool variants here
    }

    // =========================================================================
    // Primitive Operations (by index)
    // =========================================================================

    /// Clone a primitive by index, returns new primitive's index
    pub fn clone_primitive(&mut self, index: usize) -> Option<usize> {
        if index >= self.primitives.len() {
            return None;
        }

        let mut cloned = self.primitives[index].clone();
        // Assign a fresh unique ID so the clone is independent of the original.
        cloned.data_mut().id = crate::drawing::alloc_primitive_id();
        let new_idx = self.primitives.len();
        self.primitives.push(cloned);
        Some(new_idx)
    }

    /// Translate (move) a primitive by index in bar/price coordinates
    pub fn translate_at(&mut self, index: usize, bar_delta: f64, price_delta: f64) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.translate(bar_delta, price_delta);
        }
    }

    /// Toggle lock state for primitive at index
    pub fn toggle_lock_primitive(&mut self, index: usize) {
        if let Some(prim) = self.primitives.get_mut(index) {
            let new_locked = !prim.is_locked();
            prim.set_locked(new_locked);
        }
    }

    /// Toggle visibility for primitive at index
    pub fn toggle_visibility(&mut self, index: usize) {
        if let Some(prim) = self.primitives.get_mut(index) {
            let new_visible = !prim.is_visible();
            prim.set_visible(new_visible);
        }
    }

    /// Bring primitive to front (end of list = rendered last = on top)
    pub fn bring_to_front(&mut self, index: usize) {
        if index < self.primitives.len() && index != self.primitives.len() - 1 {
            let prim = self.primitives.remove(index);
            self.primitives.push(prim);
            // Update selection if needed
            if self.selected == Some(index) {
                self.selected = Some(self.primitives.len() - 1);
            } else if let Some(sel) = self.selected {
                if sel > index {
                    self.selected = Some(sel - 1);
                }
            }
        }
    }

    /// Send primitive to back (beginning of list = rendered first = behind)
    pub fn send_to_back(&mut self, index: usize) {
        if index > 0 && index < self.primitives.len() {
            let prim = self.primitives.remove(index);
            self.primitives.insert(0, prim);
            // Update selection if needed
            if self.selected == Some(index) {
                self.selected = Some(0);
            } else if let Some(sel) = self.selected {
                if sel < index {
                    self.selected = Some(sel + 1);
                }
            }
        }
    }

    /// Move primitive from old_index to new_index (for undo/redo of reorder)
    pub fn move_to_index(&mut self, old_index: usize, new_index: usize) {
        if old_index >= self.primitives.len() || new_index >= self.primitives.len() {
            return;
        }
        if old_index == new_index {
            return;
        }
        let prim = self.primitives.remove(old_index);
        self.primitives.insert(new_index, prim);
        // Update selection if needed
        if self.selected == Some(old_index) {
            self.selected = Some(new_index);
        } else if let Some(sel) = self.selected {
            // Adjust selection for items between old and new position
            if old_index < new_index {
                // Moved forward: items between shift back
                if sel > old_index && sel <= new_index {
                    self.selected = Some(sel - 1);
                }
            } else {
                // Moved backward: items between shift forward
                if sel >= new_index && sel < old_index {
                    self.selected = Some(sel + 1);
                }
            }
        }
    }

    // =========================================================================
    // Hit Testing
    // =========================================================================

    /// Hit test at screen coordinates (for main chart primitives only)
    ///
    /// Note: Locked primitives CAN be selected (to allow unlocking them),
    /// but drag/resize operations are blocked separately.
    pub fn hit_test(&self, x: f64, y: f64, viewport: &Viewport, price_scale: &PriceScale) -> Option<usize> {
        // If globally locked, no primitives are selectable
        if self.locked {
            return None;
        }

        // Test in reverse order (topmost first)
        // Only test primitives that belong to main chart (pane_id == None)
        for (idx, prim) in self.primitives.iter().enumerate().rev() {
            if prim.data().pane_id.is_some() {
                continue; // Skip sub-pane primitives
            }
            // Note: We DO allow selecting locked primitives so user can unlock them
            if !matches!(prim.hit_test(x, y, viewport, price_scale), HitTestResult::Miss) {
                return Some(idx);
            }
        }
        None
    }

    /// Hit test at screen coordinates within a specific pane
    ///
    /// The x, y coordinates should be relative to the pane's rect (not the whole chart).
    /// The viewport should have chart_height set to the pane's height.
    /// Note: Locked primitives CAN be selected (to allow unlocking them),
    /// but drag/resize operations are blocked separately.
    pub fn hit_test_in_pane(
        &self,
        x: f64,
        y: f64,
        pane_id: u64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<usize> {
        // If globally locked, no primitives are selectable
        if self.locked {
            return None;
        }

        // Test in reverse order (topmost first)
        // Only test primitives that belong to this pane
        for (idx, prim) in self.primitives.iter().enumerate().rev() {
            if prim.data().pane_id != Some(pane_id) {
                continue; // Skip primitives not in this pane
            }
            // Note: We DO allow selecting locked primitives so user can unlock them
            if !matches!(prim.hit_test(x, y, viewport, price_scale), HitTestResult::Miss) {
                return Some(idx);
            }
        }
        None
    }

    /// Hit test control points of selected primitive
    ///
    /// Returns None if globally locked or the selected primitive is locked.
    pub fn hit_test_control_point(
        &self,
        x: f64,
        y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        // If globally locked, control points are not interactable
        if self.locked {
            return None;
        }

        let idx = self.selected?;
        let prim = self.primitives.get(idx)?;

        // If the primitive itself is locked, control points are not interactable
        if prim.data().locked {
            return None;
        }

        match prim.hit_test(x, y, viewport, price_scale) {
            HitTestResult::ControlPoint(cp) => Some(cp),
            _ => None,
        }
    }

    /// Hit test control points of selected primitive in a specific pane
    ///
    /// The x, y coordinates should be relative to the pane's rect.
    /// Returns the control point type if hit, and verifies the selected primitive belongs to the pane.
    /// Returns None if globally locked or the selected primitive is locked.
    pub fn hit_test_control_point_in_pane(
        &self,
        x: f64,
        y: f64,
        pane_id: u64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        if self.locked {
            return None;
        }

        let idx = match self.selected {
            Some(i) => i,
            None => return None,
        };
        let prim = match self.primitives.get(idx) {
            Some(p) => p,
            None => return None,
        };

        if prim.data().pane_id != Some(pane_id) {
            return None;
        }

        if prim.data().locked {
            return None;
        }

        let result = prim.hit_test(x, y, viewport, price_scale);
        match result {
            HitTestResult::ControlPoint(cp) => Some(cp),
            _ => None,
        }
    }

    // =========================================================================
    // Drag and Drop
    // =========================================================================

    /// Start dragging a primitive (whole object move)
    ///
    /// Returns false if globally locked or the primitive is locked (drag should pass through to chart).
    /// Returns true if drag was started successfully.
    pub fn start_drag(&mut self, index: usize, bar: f64, price: f64) -> bool {
        // Don't allow dragging if globally locked
        if self.locked {
            return false;
        }

        if index < self.primitives.len() {
            // Don't allow dragging if the specific primitive is locked
            if self.primitives[index].data().locked {
                return false;
            }
            self.dragging = Some(index);
            self.selected = Some(index);
            self.drag_type = DragType::Move;
            self.drag_start = Some((bar, price));
            return true;
        }
        false
    }

    /// Start dragging a control point (resize/reshape)
    ///
    /// Does nothing if globally locked or the primitive is locked.
    pub fn start_control_point_drag(
        &mut self,
        index: usize,
        control_point: ControlPointType,
        bar: f64,
        price: f64,
    ) {
        // Don't allow dragging if globally locked
        if self.locked {
            return;
        }

        if index < self.primitives.len() {
            // Don't allow dragging if the specific primitive is locked
            if self.primitives[index].data().locked {
                return;
            }
            self.dragging = Some(index);
            self.selected = Some(index);
            self.drag_type = DragType::ControlPoint(control_point);
            self.drag_start = Some((bar, price));
        }
    }

    /// Update drag position (data coordinates)
    pub fn update_drag(&mut self, current_bar: f64, current_price: f64) {
        if let (Some(idx), Some((start_bar, start_price))) = (self.dragging, self.drag_start) {
            if idx < self.primitives.len() {
                match &self.drag_type {
                    DragType::Move => {
                        let bar_delta = current_bar - start_bar;
                        let price_delta = current_price - start_price;

                        if bar_delta.abs() > 0.001 || price_delta.abs() > 0.0001 {
                            self.primitives[idx].translate(bar_delta, price_delta);
                            self.drag_start = Some((current_bar, current_price));
                        }
                    }
                    DragType::ControlPoint(point_type) => {
                        self.primitives[idx].move_control_point(point_type.clone(), current_bar, current_price);
                        self.drag_start = Some((current_bar, current_price));
                    }
                }
            }
        }
    }

    /// Update drag position using screen coordinates
    /// This is needed for primitives like emoji/image that store size in pixels
    pub fn update_drag_screen(
        &mut self,
        screen_x: f64,
        screen_y: f64,
        current_bar: f64,
        current_price: f64,
        viewport: &crate::Viewport,
        price_scale: &crate::PriceScale,
    ) {
        if let (Some(idx), Some((start_bar, start_price))) = (self.dragging, self.drag_start) {
            if idx < self.primitives.len() {
                match &self.drag_type {
                    DragType::Move => {
                        let bar_delta = current_bar - start_bar;
                        let price_delta = current_price - start_price;

                        if bar_delta.abs() > 0.001 || price_delta.abs() > 0.0001 {
                            self.primitives[idx].translate(bar_delta, price_delta);
                            self.drag_start = Some((current_bar, current_price));
                        }
                    }
                    DragType::ControlPoint(point_type) => {
                        // Use screen coordinates for control point drag
                        self.primitives[idx].move_control_point_screen(
                            point_type.clone(),
                            screen_x,
                            screen_y,
                            viewport,
                            price_scale,
                        );
                        self.drag_start = Some((current_bar, current_price));
                    }
                }
            }
        }
    }

    /// End drag operation
    pub fn end_drag(&mut self) {
        self.dragging = None;
        self.drag_type = DragType::Move;
        self.drag_start = None;
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Get the primitive id of the currently dragged primitive, if any.
    pub fn dragging_primitive_id(&self) -> Option<u64> {
        self.dragging
            .and_then(|idx| self.primitives.get(idx))
            .map(|p| p.data().id)
    }

    /// Get dragging primitive index
    pub fn dragging(&self) -> Option<usize> {
        self.dragging
    }

    /// Get current drag type
    pub fn drag_type(&self) -> &DragType {
        &self.drag_type
    }

    // =========================================================================
    // Configuration API - Selected Primitive
    // =========================================================================

    /// Get reference to selected primitive
    pub fn selected_primitive(&self) -> Option<&Box<dyn Primitive>> {
        self.selected.and_then(|idx| self.primitives.get(idx))
    }

    /// Get mutable reference to selected primitive
    pub fn selected_primitive_mut(&mut self) -> Option<&mut Box<dyn Primitive>> {
        self.selected.and_then(|idx| self.primitives.get_mut(idx))
    }

    /// Set stroke color of selected primitive
    pub fn set_selected_color(&mut self, color: &str) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.data_mut().color.stroke = color.to_string();
        }
    }

    /// Set fill color of selected primitive
    pub fn set_selected_fill(&mut self, fill: Option<&str>) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.data_mut().color.fill = fill.map(String::from);
        }
    }

    /// Set line width of selected primitive
    pub fn set_selected_width(&mut self, width: f64) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.data_mut().width = width.clamp(1.0, 20.0);
        }
    }

    /// Increase line width of selected primitive by 1px
    pub fn increase_selected_width(&mut self) {
        if let Some(prim) = self.selected_primitive_mut() {
            let current = prim.data().width;
            prim.data_mut().width = (current + 1.0).clamp(1.0, 20.0);
        }
    }

    /// Decrease line width of selected primitive by 1px
    pub fn decrease_selected_width(&mut self) {
        if let Some(prim) = self.selected_primitive_mut() {
            let current = prim.data().width;
            prim.data_mut().width = (current - 1.0).clamp(1.0, 20.0);
        }
    }

    /// Get selected primitive index
    pub fn selected_idx(&self) -> Option<usize> {
        self.selected
    }

    /// Set line style of selected primitive
    pub fn set_selected_style(&mut self, style: super::primitives_v2::LineStyle) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.data_mut().style = style;
        }
    }

    /// Set text content of selected primitive (creates text if not present)
    pub fn set_selected_text_content(&mut self, content: &str) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.content = content.to_string();
            } else {
                // Create new text with default settings
                data.text = Some(super::primitives_v2::PrimitiveText::new(content));
            }
        }
    }

    /// Set text font size of selected primitive (creates text if not present)
    pub fn set_selected_text_font_size(&mut self, font_size: f64) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.font_size = font_size.clamp(8.0, 72.0);
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.font_size = font_size.clamp(8.0, 72.0);
                data.text = Some(new_text);
            }
        }
    }

    /// Set text bold of selected primitive (creates text if not present)
    pub fn set_selected_text_bold(&mut self, bold: bool) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.bold = bold;
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.bold = bold;
                data.text = Some(new_text);
            }
        }
    }

    /// Set text italic of selected primitive (creates text if not present)
    pub fn set_selected_text_italic(&mut self, italic: bool) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.italic = italic;
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.italic = italic;
                data.text = Some(new_text);
            }
        }
    }

    /// Set text color of selected primitive (creates text if not present)
    pub fn set_selected_text_color(&mut self, color: &str) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.color = Some(color.to_string());
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.color = Some(color.to_string());
                data.text = Some(new_text);
            }
        }
    }

    /// Apply color to selected primitive by field name
    ///
    /// Field names: "stroke_color", "text_color", "fill_color"
    pub fn apply_color_by_field(&mut self, field: &str, color: &str) {
        match field {
            "stroke_color" => self.set_selected_color(color),
            "text_color" => self.set_selected_text_color(color),
            "fill_color" => self.set_selected_fill(Some(color)),
            _ => eprintln!("[DrawingManager] Unknown color field: {}", field),
        }
    }

    /// Set text vertical alignment of selected primitive
    pub fn set_selected_text_v_align(&mut self, align: super::primitives_v2::TextAlign) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.v_align = align;
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.v_align = align;
                data.text = Some(new_text);
            }
        }
    }

    /// Set text horizontal alignment of selected primitive
    pub fn set_selected_text_h_align(&mut self, align: super::primitives_v2::TextAlign) {
        if let Some(prim) = self.selected_primitive_mut() {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.h_align = align;
            } else {
                let mut new_text = super::primitives_v2::PrimitiveText::new("");
                new_text.h_align = align;
                data.text = Some(new_text);
            }
        }
    }

    /// Set level configs of selected primitive (for Fibonacci, Gann, Pitchfork)
    pub fn set_selected_level_configs(&mut self, configs: Vec<super::primitives_v2::FibLevelConfig>) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.set_level_configs(configs);
        }
    }

    /// Set sync mode of selected primitive
    pub fn set_selected_sync_mode(&mut self, mode: super::primitives_v2::SyncMode) {
        if let Some(prim) = self.selected_primitive_mut() {
            prim.data_mut().sync_mode = mode;
        }
    }

    /// Get control points for selected primitive (in screen coordinates)
    pub fn selected_control_points(&self, viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        if let Some(prim) = self.selected_primitive() {
            prim.control_points(viewport, price_scale)
        } else {
            Vec::new()
        }
    }

    // =========================================================================
    // Configuration API - By Index
    // =========================================================================

    /// Get reference to primitive by index
    pub fn primitive(&self, index: usize) -> Option<&Box<dyn Primitive>> {
        self.primitives.get(index)
    }

    /// Get mutable reference to primitive by index
    pub fn primitive_mut(&mut self, index: usize) -> Option<&mut Box<dyn Primitive>> {
        self.primitives.get_mut(index)
    }

    /// Set color of primitive by index
    pub fn set_color_at(&mut self, index: usize, color: &str) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().color.stroke = color.to_string();
        }
    }

    /// Set line width of primitive by index
    pub fn set_width_at(&mut self, index: usize, width: f64) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().width = width.clamp(1.0, 20.0);
        }
    }

    /// Set line style of primitive by index
    pub fn set_style_at(&mut self, index: usize, style: super::primitives_v2::LineStyle) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().style = style;
        }
    }

    /// Apply a style property to primitive by index
    ///
    /// This method calls the primitive's apply_style_property method
    /// to set custom style properties defined by style_properties()
    pub fn apply_style_property(&mut self, index: usize, prop_id: &str, value: super::primitives_v2::config::PropertyValue) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.apply_style_property(prop_id, &value);
        }
    }

    /// Apply a level property to primitive by index
    ///
    /// This method calls the primitive's apply_level_property method
    /// to set custom level properties defined by level_properties()
    pub fn apply_level_property(&mut self, index: usize, prop_id: &str, value: super::primitives_v2::config::PropertyValue) {
        if let Some(prim) = self.primitives.get_mut(index) {
            if prim.apply_level_property(prop_id, &value) {
                eprintln!("[DrawingManager] Applied level property '{}' to primitive {}", prop_id, index);
            } else {
                eprintln!("[DrawingManager] Failed to apply level property '{}' to primitive {}", prop_id, index);
            }
        }
    }

    /// Apply a text property to primitive by index
    ///
    /// This method calls the primitive's apply_text_property method
    /// to set custom text properties defined by text_properties()
    pub fn apply_text_property(&mut self, index: usize, prop_id: &str, value: super::primitives_v2::config::PropertyValue) {
        if let Some(prim) = self.primitives.get_mut(index) {
            if prim.apply_text_property(prop_id, &value) {
                eprintln!("[DrawingManager] Applied text property '{}' to primitive {}", prop_id, index);
            } else {
                eprintln!("[DrawingManager] Failed to apply text property '{}' to primitive {}", prop_id, index);
            }
        }
    }

    // =========================================================================
    // Extended Configuration API
    // =========================================================================

    /// Toggle lock state of selected primitive
    pub fn toggle_selected_lock(&mut self) {
        if let Some(prim) = self.selected_primitive_mut() {
            let locked = prim.data().locked;
            prim.data_mut().locked = !locked;
        }
    }

    /// Toggle visibility of selected primitive
    pub fn toggle_selected_visibility(&mut self) {
        if let Some(prim) = self.selected_primitive_mut() {
            let visible = prim.data().visible;
            prim.data_mut().visible = !visible;
        }
    }

    /// Build a SelectedPrimitiveConfig snapshot for the currently selected primitive.
    ///
    /// Returns `None` when nothing is selected.
    pub fn get_selected_config(&self) -> Option<crate::state::selected_config::SelectedPrimitiveConfig> {
        let prim = self.selected_primitive()?;
        let data = prim.data();
        let type_id = prim.type_id();
        let supports_text = {
            let registry = PrimitiveRegistry::global().read().unwrap();
            registry.supports_text(type_id)
        };
        let text_color = if supports_text {
            data.text.as_ref().and_then(|t| t.color.clone())
        } else {
            None
        };
        Some(crate::state::selected_config::SelectedPrimitiveConfig {
            name: prim.display_name().to_string(),
            color: data.color.stroke.clone(),
            width: data.width,
            style: data.style.as_str().to_string(),
            locked: data.locked,
            text_color,
            supports_text,
        })
    }

    /// Set visibility of primitive by index
    pub fn set_visibility_at(&mut self, index: usize, visible: bool) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().visible = visible;
        }
    }

    /// Set lock state of primitive by index
    pub fn set_lock_at(&mut self, index: usize, locked: bool) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().locked = locked;
        }
    }

    /// Clone selected primitive
    pub fn clone_selected(&mut self) -> Option<usize> {
        if let Some(prim) = self.selected_primitive() {
            let mut cloned = prim.clone_box();
            // Assign new ID
            cloned.data_mut().id = crate::drawing::alloc_primitive_id();
            // Offset position slightly
            cloned.translate(5.0, 0.0);
            let idx = self.primitives.len();
            self.primitives.push(cloned);
            self.selected = Some(idx);
            Some(idx)
        } else {
            None
        }
    }

    /// Bring selected primitive to front (highest z-order)
    pub fn bring_selected_to_front(&mut self) {
        if let Some(idx) = self.selected {
            // Get max z_order
            let max_z = self.primitives.iter().map(|p| p.data().z_order).max().unwrap_or(0);
            if let Some(prim) = self.primitives.get_mut(idx) {
                prim.data_mut().z_order = max_z + 1;
            }
        }
    }

    /// Send selected primitive to back (lowest z-order)
    pub fn send_selected_to_back(&mut self) {
        if let Some(idx) = self.selected {
            // Get min z_order
            let min_z = self.primitives.iter().map(|p| p.data().z_order).min().unwrap_or(0);
            if let Some(prim) = self.primitives.get_mut(idx) {
                prim.data_mut().z_order = min_z - 1;
            }
        }
    }

    /// Move primitive at index up one position in z-order
    pub fn move_up(&mut self, index: usize) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().z_order += 1;
        }
    }

    /// Move primitive at index down one position in z-order
    pub fn move_down(&mut self, index: usize) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().z_order -= 1;
        }
    }

    /// Get list of all primitives for Object Tree
    pub fn primitive_list(&self) -> Vec<PrimitiveListItem> {
        self.primitives.iter().enumerate().map(|(idx, prim)| {
            let data = prim.data();
            PrimitiveListItem {
                index: idx,
                id: data.id,
                type_id: prim.type_id().to_string(),
                display_name: prim.display_name().to_string(),
                color: data.color.stroke.clone(),
                visible: data.visible,
                locked: data.locked,
                selected: self.selected == Some(idx),
            }
        }).collect()
    }

    /// Select primitive by index (for Object Tree)
    pub fn select_by_index(&mut self, index: usize) {
        if index < self.primitives.len() {
            self.selected = Some(index);
        }
    }

    /// Find primitive index by ID
    pub fn find_index_by_id(&self, id: u64) -> Option<usize> {
        self.primitives.iter().position(|p| p.data().id == id)
    }

    /// Delete primitive by index
    pub fn delete_at(&mut self, index: usize) -> bool {
        if index < self.primitives.len() {
            self.primitives.remove(index);
            // Adjust selection
            if let Some(sel) = self.selected {
                if sel == index {
                    self.selected = None;
                } else if sel > index {
                    self.selected = Some(sel - 1);
                }
            }
            true
        } else {
            false
        }
    }

    // =========================================================================
    // Undo/Redo Support API
    // =========================================================================

    /// Create primitive at specific index using registry
    /// Used for undo (recreating deleted primitive)
    pub fn create_at(
        &mut self,
        index: usize,
        type_id: &str,
        points: &[(f64, f64)],
        data: &super::primitives_v2::PrimitiveData,
    ) -> bool {
        let registry = PrimitiveRegistry::global().read().unwrap();
        if let Some(mut prim) = registry.create(type_id, points, Some(&data.color.stroke)) {
            // Restore full data
            *prim.data_mut() = data.clone();

            // Insert at specific position or append
            if index <= self.primitives.len() {
                self.primitives.insert(index, prim);
                // Adjust selection if needed
                if let Some(sel) = self.selected {
                    if sel >= index {
                        self.selected = Some(sel + 1);
                    }
                }
                true
            } else {
                self.primitives.push(prim);
                true
            }
        } else {
            false
        }
    }

    /// Get points of primitive at index
    pub fn get_points_at(&self, index: usize) -> Option<Vec<(f64, f64)>> {
        self.primitives.get(index).map(|p| p.points())
    }

    /// Set points of primitive at index
    pub fn set_points_at(&mut self, index: usize, points: &[(f64, f64)]) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.set_points(points);
        }
    }

    /// Get data of primitive at index (for undo snapshots)
    pub fn get_data_at(&self, index: usize) -> Option<super::primitives_v2::PrimitiveData> {
        self.primitives.get(index).map(|p| p.data().clone())
    }

    /// Set data of primitive at index (for undo/redo of config changes)
    pub fn set_data_at(&mut self, index: usize, data: &super::primitives_v2::PrimitiveData) {
        if let Some(prim) = self.primitives.get_mut(index) {
            // Apply all data fields from the snapshot
            let prim_data = prim.data_mut();
            prim_data.color = data.color.clone();
            prim_data.width = data.width;
            prim_data.style = data.style.clone();
            prim_data.visible = data.visible;
            prim_data.locked = data.locked;
            prim_data.display_name = data.display_name.clone();
            prim_data.text = data.text.clone();
            prim_data.timeframe_visibility = data.timeframe_visibility.clone();
            prim_data.point_timestamps = data.point_timestamps.clone();
            // Note: we don't change id, type_id, origin_id, symbol, or other immutable identity properties
        }
    }

    /// Get type_id of primitive at index
    pub fn get_type_id_at(&self, index: usize) -> Option<String> {
        self.primitives.get(index).map(|p| p.type_id().to_string())
    }

    /// Get last created primitive index (for recording creation in undo)
    pub fn last_index(&self) -> Option<usize> {
        if self.primitives.is_empty() {
            None
        } else {
            Some(self.primitives.len() - 1)
        }
    }

    /// Replace primitive at index from JSON (for undo/redo of complex changes like Fib levels)
    /// This deserializes the full primitive including type-specific data
    pub fn replace_primitive_from_json(&mut self, index: usize, type_id: &str, json: &str) -> bool {
        use super::primitives_v2::registry::PrimitiveRegistry;

        if index >= self.primitives.len() {
            return false;
        }

        // Use registry to create primitive from JSON
        let registry = PrimitiveRegistry::global().read().unwrap();
        if let Some(prim) = registry.from_json(type_id, json) {
            self.primitives[index] = prim;
            true
        } else {
            false
        }
    }

    /// Get full JSON of primitive at index (for undo snapshots)
    pub fn get_primitive_json(&self, index: usize) -> Option<String> {
        self.primitives.get(index).map(|p| p.to_json())
    }

    /// Snapshot primitive at index for undo/redo (type_id, points, data)
    pub fn snapshot_at(&self, index: usize) -> Option<(String, Vec<(f64, f64)>, super::primitives_v2::PrimitiveData)> {
        let prim = self.primitives.get(index)?;
        let type_id = prim.type_id().to_string();
        let points = prim.points().to_vec();
        let data = prim.data().clone();
        Some((type_id, points, data))
    }

    /// Snapshot all primitives for undo/redo
    pub fn snapshot_all(&self) -> Vec<(String, Vec<(f64, f64)>, super::primitives_v2::PrimitiveData)> {
        (0..self.primitives.len())
            .filter_map(|i| self.snapshot_at(i))
            .collect()
    }

    /// Insert primitive at specific index (for undo recreation).
    ///
    /// Delegates to `create_at` which uses the global primitive registry.
    pub fn insert_at(
        &mut self,
        index: usize,
        type_id: &str,
        points: &[(f64, f64)],
        data: &super::primitives_v2::PrimitiveData,
    ) -> bool {
        self.create_at(index, type_id, points, data)
    }

    // =========================================================================
    // Timeframe Visibility Management
    // =========================================================================

    /// Get timeframe visibility config for primitive at index
    pub fn get_timeframe_visibility(&self, index: usize) -> Option<&super::primitives_v2::config::TimeframeVisibilityConfig> {
        self.primitives.get(index)
            .and_then(|p| p.data().timeframe_visibility.as_ref())
    }

    /// Set timeframe visibility config for primitive at index
    pub fn set_timeframe_visibility(&mut self, index: usize, config: super::primitives_v2::config::TimeframeVisibilityConfig) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().timeframe_visibility = Some(config);
        }
    }

    /// Set primitive to show on all timeframes
    pub fn set_show_on_all_timeframes(&mut self, index: usize) {
        if let Some(prim) = self.primitives.get_mut(index) {
            prim.data_mut().timeframe_visibility = Some(super::primitives_v2::config::TimeframeVisibilityConfig::all());
        }
    }

    // =========================================================================
    // Template Application
    // =========================================================================

    /// Apply template style to primitive at index
    pub fn apply_template_style(&mut self, index: usize, template: &super::primitives_v2::config::SettingsTemplate) {
        if let Some(prim) = self.primitives.get_mut(index) {
            let data = prim.data_mut();

            // Apply style properties
            if let Some(ref color) = template.style.color {
                data.color.stroke = color.clone();
            }
            if let Some(width) = template.style.width {
                data.width = width;
            }
            if let Some(ref line_style) = template.style.line_style {
                data.style = match line_style.as_str() {
                    "dashed" => super::primitives_v2::LineStyle::Dashed,
                    "dotted" => super::primitives_v2::LineStyle::Dotted,
                    "large_dashed" => super::primitives_v2::LineStyle::LargeDashed,
                    "sparse_dotted" => super::primitives_v2::LineStyle::SparseDotted,
                    _ => super::primitives_v2::LineStyle::Solid,
                };
            }

            // Apply timeframe visibility if present
            if let Some(ref tfv) = template.timeframe_visibility {
                data.timeframe_visibility = Some(tfv.clone());
            }
        }
    }

    /// Get primitives of a specific type
    pub fn primitives_of_type(&self, type_id: &str) -> Vec<usize> {
        self.primitives
            .iter()
            .enumerate()
            .filter(|(_, p)| p.type_id() == type_id)
            .map(|(i, _)| i)
            .collect()
    }

    // =========================================================================
    // Sync Group Support
    // =========================================================================

    /// DEPRECATED: Legacy clone-based sync. TagManager uses group-owned primitives instead.
    /// Still used by `clone_for_split` in chart_window.rs — will be removed when split
    /// no longer pre-populates cloned primitives (they get cleared for grouped windows anyway).
    pub fn clone_primitives_for_sync(&self, new_window_id: u64) -> Vec<Box<dyn Primitive>> {
        self.primitives.iter().map(|p| {
            let mut cloned = p.clone_box();
            cloned.data_mut().origin_id = Some(p.data().id);
            cloned.data_mut().window_id = Some(new_window_id);
            cloned
        }).collect()
    }

    /// DEPRECATED: Legacy clone-based sync helper. Used by legacy propagation functions.
    pub fn add_synced_primitives(&mut self, prims: Vec<Box<dyn Primitive>>) {
        for p in prims {
            self.primitives.push(p);
        }
    }

    /// DEPRECATED: Legacy origin_id-based purge. TagManager uses `clear_all_primitives()`.
    /// Still used in `perform_desync` for non-grouped windows.
    pub fn purge_synced_primitives(&mut self) {
        self.primitives.retain(|p| p.data().origin_id.is_none());
        // Reset selection if it pointed to a purged primitive
        self.selected = None;
    }

    /// Clear ALL primitives from this manager.
    /// Used when a window disconnects from a TagManager group — the group
    /// owns the primitives, so the window's cached copies should be removed.
    pub fn clear_all_primitives(&mut self) {
        self.primitives.clear();
        self.selected = None;
        self.dragging = None;
        self.drag_start = None;
    }

    /// DEPRECATED: Legacy single-primitive clone. Used by `propagate_new_primitive_to_sync_group`.
    pub fn clone_primitive_for_sync(&self, prim_id: u64, new_window_id: u64) -> Option<Box<dyn Primitive>> {
        self.primitives.iter()
            .find(|p| p.data().id == prim_id)
            .map(|p| {
                let mut cloned = p.clone_box();
                cloned.data_mut().origin_id = Some(p.data().id);
                cloned.data_mut().window_id = Some(new_window_id);
                cloned
            })
    }

    /// DEPRECATED: Legacy origin_id-based removal. TagManager removes by primitive id directly.
    pub fn remove_synced_by_origin(&mut self, origin_id: u64) {
        self.primitives.retain(|p| p.data().origin_id != Some(origin_id));
        self.selected = None;
    }

    /// DEPRECATED: Used by legacy `propagate_new_primitive_to_sync_group`.
    pub fn last_original_id(&self) -> Option<u64> {
        self.primitives.iter().rev()
            .find(|p| p.data().origin_id.is_none())
            .map(|p| p.data().id)
    }

    /// DEPRECATED: Legacy origin_id-based point update. TagManager uses
    /// `update_group_primitive_after_drag` + pre-render sync instead.
    /// Still used as fallback in `propagate_primitive_update_to_sync_group`.
    pub fn update_synced_primitive_points(&mut self, origin_id: u64, new_points: &[(f64, f64)]) {
        if let Some(prim) = self.primitives.iter_mut()
            .find(|p| p.data().origin_id == Some(origin_id))
        {
            prim.set_points(new_points);
        }
    }

    /// Get the current points of the first original (non-synced) primitive with
    /// the given `id`.  Returns `None` if the primitive is not found.
    pub fn get_points_by_id(&self, id: u64) -> Option<Vec<(f64, f64)>> {
        self.primitives.iter()
            .find(|p| p.data().id == id && p.data().origin_id.is_none())
            .map(|p| p.points())
    }
}

/// Item for primitive list (Object Tree)
#[derive(Clone, Debug)]
pub struct PrimitiveListItem {
    pub index: usize,
    pub id: u64,
    pub type_id: String,
    pub display_name: String,
    pub color: String,
    pub visible: bool,
    pub locked: bool,
    pub selected: bool,
}
