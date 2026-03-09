//! Chart objects and their behavior
//!
//! Systems for interactive chart objects (primitives, indicators, etc.)
//!
//! - `ChartObject` - Trait for objects that can be displayed and interacted with
//! - `ObjectRegistry` - Registry of all chart objects
//! - `Draggable` - Drag-and-drop system
//! - `StyleSet`, `Styleable` - Styling system
//! - `CoordinateHelper` - Price/pixel coordinate transformations

mod coordinates;
mod draggable;
mod object;
mod style;

pub use coordinates::CoordinateHelper;
pub use draggable::{
    CursorStyle, DragAxis, DragConstraints, DragManager, DragState, Draggable, HitTestResult,
    DRAG_THRESHOLD,
};
pub use object::{
    ChartObject, Configurable, ConfigProperty, ConfigPropertyType, DraggableObject,
    ObjectCapabilities, ObjectEntry, ObjectRegistry, ObjectState, ObjectType,
};
pub use style::{DefaultStyles, FontStyleType, FontWeight, StyleSet, Styleable, UnifiedLineStyle, ZOrder};
