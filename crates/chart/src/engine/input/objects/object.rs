//! Universal Chart Object System for zengeld-chart
//!
//! Provides a unified interface for all chart objects including:
//! - Series (candlestick, line, area, histogram, etc.)
//! - Overlays (legend, tooltip, watermark, crosshair, grid)
//! - Primitives (trend lines, rectangles, horizontal/vertical lines)
//! - Markers and Price Lines
//!
//! # Architecture
//!
//! ```text
//! ChartObject (trait)
//! ├── Series (configure only)
//! ├── Overlays (configure + position)
//! │   ├── Legend
//! │   ├── Tooltip
//! │   └── Watermark
//! ├── Primitives (configure + drag)
//! │   ├── TrendLine
//! │   ├── Rectangle
//! │   ├── HorizontalLine
//! │   └── VerticalLine
//! ├── Markers (configure + drag)
//! └── PriceLines (configure + drag)
//! ```

use super::draggable::{CursorStyle, DragAxis, DragConstraints, Draggable};
use super::style::{Styleable, ZOrder};
use serde::{Deserialize, Serialize};
use std::any::Any;

// =============================================================================
// Object Type Classification
// =============================================================================

/// Type classification for chart objects
///
/// Determines capabilities and behavior:
/// - Series: Data display, no drag
/// - Overlay: UI elements, position-only (no drag)
/// - Primitive: Drawing tools, can drag
/// - Marker: Data annotations, can drag
/// - PriceLine: Horizontal price levels, can drag
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectType {
    /// Series (candlestick, line, area, etc.) - configure only
    Series,
    /// Overlay (legend, tooltip, watermark) - configure + position
    Overlay,
    /// Primitive (trend line, rectangle, etc.) - configure + drag
    Primitive,
    /// Marker (data annotation) - configure + drag
    Marker,
    /// Price Line (horizontal level) - configure + drag
    PriceLine,
}

impl ObjectType {
    /// Check if this type supports drag-and-drop
    pub fn supports_drag(&self) -> bool {
        matches!(
            self,
            ObjectType::Primitive | ObjectType::Marker | ObjectType::PriceLine
        )
    }

    /// Check if this type supports positioning (without drag)
    pub fn supports_position(&self) -> bool {
        matches!(self, ObjectType::Overlay)
    }

    /// Check if this type is always visible (no toggle)
    pub fn always_visible(&self) -> bool {
        matches!(self, ObjectType::Series)
    }
}

// =============================================================================
// Object Capabilities
// =============================================================================

/// Capabilities flags for chart objects
#[derive(Debug, Clone, Copy, Default)]
pub struct ObjectCapabilities {
    /// Can be styled (color, font, etc.)
    pub styleable: bool,
    /// Can be dragged
    pub draggable: bool,
    /// Can be positioned (overlays)
    pub positionable: bool,
    /// Can be selected
    pub selectable: bool,
    /// Can be deleted by user
    pub deletable: bool,
    /// Can be resized
    pub resizable: bool,
}

impl ObjectCapabilities {
    /// Capabilities for series
    pub fn series() -> Self {
        Self {
            styleable: true,
            draggable: false,
            positionable: false,
            selectable: false,
            deletable: false,
            resizable: false,
        }
    }

    /// Capabilities for overlays
    pub fn overlay() -> Self {
        Self {
            styleable: true,
            draggable: false,
            positionable: true,
            selectable: false,
            deletable: false,
            resizable: false,
        }
    }

    /// Capabilities for primitives
    pub fn primitive() -> Self {
        Self {
            styleable: true,
            draggable: true,
            positionable: false,
            selectable: true,
            deletable: true,
            resizable: true,
        }
    }

    /// Capabilities for markers
    pub fn marker() -> Self {
        Self {
            styleable: true,
            draggable: true,
            positionable: false,
            selectable: true,
            deletable: true,
            resizable: false,
        }
    }

    /// Capabilities for price lines
    pub fn price_line() -> Self {
        Self {
            styleable: true,
            draggable: true,
            positionable: false,
            selectable: true,
            deletable: true,
            resizable: false,
        }
    }
}

impl From<ObjectType> for ObjectCapabilities {
    fn from(obj_type: ObjectType) -> Self {
        match obj_type {
            ObjectType::Series => ObjectCapabilities::series(),
            ObjectType::Overlay => ObjectCapabilities::overlay(),
            ObjectType::Primitive => ObjectCapabilities::primitive(),
            ObjectType::Marker => ObjectCapabilities::marker(),
            ObjectType::PriceLine => ObjectCapabilities::price_line(),
        }
    }
}

// =============================================================================
// Object State
// =============================================================================

/// Runtime state for chart objects
#[derive(Debug, Clone, Default)]
pub struct ObjectState {
    /// Is currently visible
    pub visible: bool,
    /// Is currently selected
    pub selected: bool,
    /// Is currently hovered
    pub hovered: bool,
    /// Is currently being dragged
    pub dragging: bool,
    /// Is locked (can't be modified)
    pub locked: bool,
}

impl ObjectState {
    /// Create default visible state
    pub fn visible() -> Self {
        Self {
            visible: true,
            ..Default::default()
        }
    }

    /// Create hidden state
    pub fn hidden() -> Self {
        Self {
            visible: false,
            ..Default::default()
        }
    }
}

// =============================================================================
// ChartObject Trait
// =============================================================================

/// Universal trait for all chart objects
///
/// Provides a common interface for:
/// - Identification (id, type)
/// - Styling (via Styleable)
/// - Drag-and-drop (via Draggable, for supported types)
/// - State management (visible, selected, etc.)
///
/// # Implementation Notes
///
/// Objects should implement this trait along with:
/// - `Styleable` for styling support
/// - `Draggable` for drag support (primitives, markers, price lines)
pub trait ChartObject: Send + Sync {
    /// Get object ID
    ///
    /// Returns None for anonymous objects.
    fn id(&self) -> Option<&str>;

    /// Set object ID
    fn set_id(&mut self, id: Option<String>);

    /// Get object type
    fn object_type(&self) -> ObjectType;

    /// Get object capabilities
    fn capabilities(&self) -> ObjectCapabilities {
        match self.object_type() {
            ObjectType::Series => ObjectCapabilities::series(),
            ObjectType::Overlay => ObjectCapabilities::overlay(),
            ObjectType::Primitive => ObjectCapabilities::primitive(),
            ObjectType::Marker => ObjectCapabilities::marker(),
            ObjectType::PriceLine => ObjectCapabilities::price_line(),
        }
    }

    /// Get current state
    fn state(&self) -> &ObjectState;

    /// Get mutable state
    fn state_mut(&mut self) -> &mut ObjectState;

    /// Get Z-order for layering
    fn z_order(&self) -> ZOrder {
        ZOrder::Normal
    }

    /// Set Z-order
    fn set_z_order(&mut self, _z_order: ZOrder) {
        // Default: no-op (override in implementations)
    }

    /// Check if object is visible
    fn is_visible(&self) -> bool {
        self.state().visible
    }

    /// Set visibility
    fn set_visible(&mut self, visible: bool) {
        self.state_mut().visible = visible;
    }

    /// Check if object is selected
    fn is_selected(&self) -> bool {
        self.state().selected
    }

    /// Set selected state
    fn set_selected(&mut self, selected: bool) {
        if self.capabilities().selectable {
            self.state_mut().selected = selected;
        }
    }

    /// Check if object is hovered
    fn is_hovered(&self) -> bool {
        self.state().hovered
    }

    /// Set hovered state
    fn set_hovered(&mut self, hovered: bool) {
        self.state_mut().hovered = hovered;
    }

    /// Check if object is locked
    fn is_locked(&self) -> bool {
        self.state().locked
    }

    /// Set locked state
    fn set_locked(&mut self, locked: bool) {
        self.state_mut().locked = locked;
    }

    /// Hit test at point
    ///
    /// Returns true if point (x, y) hits this object.
    fn hit_test(&self, x: f64, y: f64) -> bool;

    /// Get cursor for hover state
    fn hover_cursor(&self) -> CursorStyle {
        if self.capabilities().draggable && !self.is_locked() {
            CursorStyle::Grab
        } else if self.capabilities().selectable {
            CursorStyle::Pointer
        } else {
            CursorStyle::Default
        }
    }

    /// Downcast to Any for type dispatch
    fn as_any(&self) -> &dyn Any;

    /// Downcast to mutable Any
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// =============================================================================
// Configurable Trait (for runtime configuration)
// =============================================================================

/// Trait for objects that can be configured at runtime
///
/// Extends Styleable with additional configuration options.
pub trait Configurable: Styleable {
    /// Reset to default configuration
    fn reset_config(&mut self);

    /// Get list of configurable properties
    fn config_properties(&self) -> Vec<ConfigProperty>;
}

/// Description of a configurable property
#[derive(Debug, Clone)]
pub struct ConfigProperty {
    /// Property name (key)
    pub name: String,
    /// Display label
    pub label: String,
    /// Property type
    pub property_type: ConfigPropertyType,
    /// Current value as string
    pub value: String,
    /// Default value
    pub default_value: String,
    /// Is property read-only
    pub read_only: bool,
}

/// Type of configurable property
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPropertyType {
    /// String value
    String,
    /// Integer number
    Integer,
    /// Float number
    Float,
    /// Boolean flag
    Boolean,
    /// Color (CSS format)
    Color,
    /// Selection from options
    Enum(Vec<String>),
}

// =============================================================================
// DraggableObject Trait (combines ChartObject + Draggable)
// =============================================================================

/// Trait for objects that support both ChartObject and Draggable
///
/// Use this for primitives, markers, and price lines.
pub trait DraggableObject: ChartObject + Draggable {
    /// Get drag axis constraint
    fn get_drag_axis(&self) -> DragAxis {
        DragAxis::Both
    }

    /// Get drag constraints
    fn get_drag_constraints(&self) -> Option<DragConstraints> {
        None
    }
}

// =============================================================================
// Object Registry Entry
// =============================================================================

/// Entry in the object registry
pub struct ObjectEntry {
    /// Object instance
    pub object: Box<dyn ChartObject>,
    /// Creation timestamp
    pub created_at: i64,
    /// Last modified timestamp
    pub modified_at: i64,
}

// =============================================================================
// Object Registry
// =============================================================================

/// Registry for managing chart objects
///
/// Provides:
/// - Object storage by ID
/// - Type-based filtering
/// - Hit testing across all objects
/// - Z-order management
pub struct ObjectRegistry {
    /// Objects indexed by ID
    objects: std::collections::HashMap<String, ObjectEntry>,
    /// Auto-increment counter for anonymous objects
    next_id: u64,
}

impl ObjectRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            objects: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    /// Generate a unique ID
    pub fn generate_id(&mut self, prefix: &str) -> String {
        let id = format!("{}_{}", prefix, self.next_id);
        self.next_id += 1;
        id
    }

    /// Add object to registry
    pub fn add(&mut self, mut object: Box<dyn ChartObject>) -> String {
        let id = object
            .id()
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.generate_id("obj"));

        object.set_id(Some(id.clone()));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.objects.insert(
            id.clone(),
            ObjectEntry {
                object,
                created_at: now,
                modified_at: now,
            },
        );

        id
    }

    /// Remove object by ID
    pub fn remove(&mut self, id: &str) -> Option<Box<dyn ChartObject>> {
        self.objects.remove(id).map(|e| e.object)
    }

    /// Get object by ID
    pub fn get(&self, id: &str) -> Option<&dyn ChartObject> {
        self.objects.get(id).map(|e| e.object.as_ref())
    }

    /// Get mutable object entry by ID
    pub fn get_entry_mut(&mut self, id: &str) -> Option<&mut ObjectEntry> {
        self.objects.get_mut(id)
    }

    /// Get all objects of a type
    pub fn objects_of_type(&self, obj_type: ObjectType) -> Vec<&dyn ChartObject> {
        self.objects
            .values()
            .filter(|e| e.object.object_type() == obj_type)
            .map(|e| e.object.as_ref())
            .collect()
    }

    /// Get all visible objects sorted by Z-order
    pub fn visible_objects_sorted(&self) -> Vec<&dyn ChartObject> {
        let mut objects: Vec<_> = self
            .objects
            .values()
            .filter(|e| e.object.is_visible())
            .map(|e| e.object.as_ref())
            .collect();

        objects.sort_by_key(|o| o.z_order());
        objects
    }

    /// Hit test at point, returning topmost hit object
    pub fn hit_test(&self, x: f64, y: f64) -> Option<&str> {
        // Get visible objects sorted by Z-order (reverse for top-first)
        let mut hits: Vec<_> = self
            .objects
            .iter()
            .filter(|(_, e)| e.object.is_visible() && e.object.hit_test(x, y))
            .collect();

        // Sort by Z-order descending (top first)
        hits.sort_by(|a, b| b.1.object.z_order().cmp(&a.1.object.z_order()));

        hits.first().map(|(id, _)| id.as_str())
    }

    /// Get all draggable objects at point
    pub fn draggable_at(&self, x: f64, y: f64) -> Vec<&str> {
        self.objects
            .iter()
            .filter(|(_, e)| {
                e.object.is_visible()
                    && e.object.capabilities().draggable
                    && !e.object.is_locked()
                    && e.object.hit_test(x, y)
            })
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Get number of objects
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Clear all objects
    pub fn clear(&mut self) {
        self.objects.clear();
    }

    /// Clear objects of a specific type
    pub fn clear_type(&mut self, obj_type: ObjectType) {
        self.objects
            .retain(|_, e| e.object.object_type() != obj_type);
    }
}

impl Default for ObjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_type_capabilities() {
        assert!(!ObjectType::Series.supports_drag());
        assert!(!ObjectType::Overlay.supports_drag());
        assert!(ObjectType::Primitive.supports_drag());
        assert!(ObjectType::Marker.supports_drag());
        assert!(ObjectType::PriceLine.supports_drag());

        assert!(ObjectType::Overlay.supports_position());
        assert!(!ObjectType::Series.supports_position());
    }

    #[test]
    fn test_object_capabilities() {
        let series_caps = ObjectCapabilities::series();
        assert!(series_caps.styleable);
        assert!(!series_caps.draggable);

        let primitive_caps = ObjectCapabilities::primitive();
        assert!(primitive_caps.styleable);
        assert!(primitive_caps.draggable);
        assert!(primitive_caps.selectable);
    }

    #[test]
    fn test_object_state() {
        let mut state = ObjectState::visible();
        assert!(state.visible);
        assert!(!state.selected);

        state.selected = true;
        assert!(state.selected);
    }

    #[test]
    fn test_registry_id_generation() {
        let mut registry = ObjectRegistry::new();

        let id1 = registry.generate_id("line");
        let id2 = registry.generate_id("line");
        let id3 = registry.generate_id("rect");

        assert_eq!(id1, "line_1");
        assert_eq!(id2, "line_2");
        assert_eq!(id3, "rect_3");
    }
}
