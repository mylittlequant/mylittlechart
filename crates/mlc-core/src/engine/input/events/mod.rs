//! Low-level input events
//!
//! Platform-agnostic representations of mouse, keyboard, and touch events.
//!
//! - `ChartInputAction` - Semantic input actions (pan, zoom, click, etc.)
//! - `DragMode` - What is currently being dragged
//! - `MouseButton`, `KeyCode`, `Modifiers` - Input primitives

mod action;
mod drag_mode;

pub use action::{ChartInputAction, KeyCode, Modifiers, MouseButton};
pub use drag_mode::DragMode;
