//! Unified input handling for zengeld-chart
//!
//! This module provides platform-agnostic input types, handlers, and object systems.
//!
//! # Module Structure
//!
//! - **events/** - Low-level input events (mouse, keyboard, touch)
//!   - `ChartInputAction` - Semantic actions (Pan, Zoom, Click, etc.)
//!   - `DragMode` - What is currently being dragged
//!   - `MouseButton`, `KeyCode`, `Modifiers` - Input primitives
//!
//! - **actions/** - High-level chart actions (commands)
//!   - `ChartAction` - All UI actions (ToggleGrid, ZoomIn, SelectTool, etc.)
//!   - `Shortcut` - Keyboard shortcut definitions
//!
//! - **handler/** - Input event handlers
//!   - `ChartInputHandler` - Trait for processing input
//!   - `DefaultChartInputHandler` - Default implementation
//!   - `ChartOutputAction` - Commands produced by handler
//!
//! - **objects/** - Chart objects and their behavior
//!   - `ChartObject` - Trait for interactive objects
//!   - `ObjectRegistry` - Registry of all chart objects
//!   - `Draggable`, `DragManager` - Drag-and-drop system
//!   - `StyleSet`, `Styleable` - Unified styling
//!   - `CoordinateHelper` - Price/pixel transformations
//!
//! # Architecture
//!
//! ```text
//! User Input (mouse, keyboard, touch)
//!        |
//!        v
//! Platform Adapter (application-specific)
//!        |
//!        v
//! ChartInputAction (semantic event)
//!        |
//!        v
//! DefaultChartInputHandler
//!        |
//!        v
//! ChartOutputAction (commands)
//!        |
//!        v
//! Chart State Update
//! ```

pub mod actions;
pub mod events;
pub mod handler;
pub mod objects;

// Re-export events
pub use events::{ChartInputAction, DragMode, KeyCode, Modifiers, MouseButton};

// Re-export actions (commands)
pub use actions::{ChartAction, Shortcut};

// Re-export handler
pub use handler::{
    ChartHitTester, ChartInputHandler, ChartInputState, ChartOutputAction,
    DefaultChartInputHandler, HitResult, InputHandlerConfig, UndoAction,
};

// Re-export objects
pub use objects::{
    ChartObject, Configurable, ConfigProperty, ConfigPropertyType, CoordinateHelper, CursorStyle,
    DefaultStyles, DragAxis, DragConstraints, DragManager, DragState, Draggable, DraggableObject,
    FontStyleType, FontWeight, HitTestResult, ObjectCapabilities, ObjectEntry, ObjectRegistry,
    ObjectState, ObjectType, StyleSet, Styleable, UnifiedLineStyle, ZOrder, DRAG_THRESHOLD,
};
