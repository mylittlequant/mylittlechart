//! Platform-agnostic input state
//!
//! This module provides `InputState` - a snapshot of user input
//! that platforms populate and pass to rendering/widget code.
//!
//! # Example
//!
//! ```ignore
//! // Application creates InputState from platform events
//! let input = InputState {
//!     pointer: PointerState {
//!         pos: Some((100.0, 200.0)),
//!         button_down: MouseButton::Left,
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//!
//! // Widget/chart code uses InputState for hit testing
//! if input.is_hovered(button_rect) && input.is_clicked() {
//!     // Handle button click
//! }
//! ```

use serde::{Deserialize, Serialize};

// =============================================================================
// InputState - Main Input Snapshot
// =============================================================================

/// Platform-agnostic input state snapshot
///
/// This struct captures the current state of user input and is passed
/// to all rendering functions for interaction detection.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    /// Pointer (mouse/touch) state
    pub pointer: PointerState,

    /// Keyboard modifier keys
    pub modifiers: ModifierKeys,

    /// Scroll delta (wheel)
    pub scroll_delta: (f64, f64),

    /// Current drag state (if dragging)
    pub drag: Option<DragState>,

    /// Time since last frame (for animations)
    pub dt: f64,

    /// Frame timestamp in seconds
    pub time: f64,
}

impl InputState {
    /// Create new InputState with given pointer position
    pub fn new() -> Self {
        Self::default()
    }

    /// Set pointer position
    pub fn with_pointer_pos(mut self, x: f64, y: f64) -> Self {
        self.pointer.pos = Some((x, y));
        self
    }

    /// Check if pointer is hovering over a rectangle
    pub fn is_hovered(&self, x: f64, y: f64, w: f64, h: f64) -> bool {
        if let Some((px, py)) = self.pointer.pos {
            px >= x && px <= x + w && py >= y && py <= y + h
        } else {
            false
        }
    }

    /// Check if pointer is hovering over a rectangle (tuple form)
    pub fn is_hovered_rect(&self, rect: (f64, f64, f64, f64)) -> bool {
        self.is_hovered(rect.0, rect.1, rect.2, rect.3)
    }

    /// Check if left mouse button was clicked this frame
    pub fn is_clicked(&self) -> bool {
        self.pointer.clicked == Some(MouseButton::Left)
    }

    /// Check if right mouse button was clicked this frame
    pub fn is_right_clicked(&self) -> bool {
        self.pointer.clicked == Some(MouseButton::Right)
    }

    /// Check if left mouse button was double-clicked this frame
    pub fn is_double_clicked(&self) -> bool {
        self.pointer.double_clicked == Some(MouseButton::Left)
    }

    /// Check if left mouse button is currently pressed
    pub fn is_mouse_down(&self) -> bool {
        self.pointer.button_down == Some(MouseButton::Left)
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Get drag delta if dragging
    pub fn drag_delta(&self) -> Option<(f64, f64)> {
        self.drag.as_ref().map(|d| d.delta)
    }

    /// Check if shift key is held
    pub fn shift(&self) -> bool {
        self.modifiers.shift
    }

    /// Check if ctrl/cmd key is held
    pub fn ctrl(&self) -> bool {
        self.modifiers.ctrl
    }

    /// Check if alt key is held
    pub fn alt(&self) -> bool {
        self.modifiers.alt
    }

    /// Get pointer position
    pub fn pointer_pos(&self) -> Option<(f64, f64)> {
        self.pointer.pos
    }

    /// Consume click (mark as handled to prevent propagation)
    /// Returns true if there was a click to consume
    pub fn consume_click(&mut self) -> bool {
        if self.pointer.clicked.is_some() {
            self.pointer.clicked = None;
            true
        } else {
            false
        }
    }
}

// =============================================================================
// PointerState
// =============================================================================

/// Mouse/touch pointer state
#[derive(Clone, Debug, Default)]
pub struct PointerState {
    /// Current pointer position (None if not over canvas)
    pub pos: Option<(f64, f64)>,

    /// Which button is currently held down (if any)
    pub button_down: Option<MouseButton>,

    /// Which button was clicked this frame (single click)
    pub clicked: Option<MouseButton>,

    /// Which button was double-clicked this frame
    pub double_clicked: Option<MouseButton>,

    /// Previous pointer position (for calculating delta)
    pub prev_pos: Option<(f64, f64)>,
}

impl PointerState {
    /// Get pointer movement delta since last frame
    pub fn delta(&self) -> (f64, f64) {
        match (self.pos, self.prev_pos) {
            (Some((x, y)), Some((px, py))) => (x - px, y - py),
            _ => (0.0, 0.0),
        }
    }

    /// Check if pointer is over the canvas
    pub fn is_present(&self) -> bool {
        self.pos.is_some()
    }
}

// =============================================================================
// MouseButton
// =============================================================================

/// Mouse button identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

// =============================================================================
// ModifierKeys
// =============================================================================

/// Keyboard modifier keys state
#[derive(Clone, Copy, Debug, Default)]
pub struct ModifierKeys {
    /// Shift key is held
    pub shift: bool,

    /// Ctrl key (or Cmd on Mac) is held
    pub ctrl: bool,

    /// Alt key (or Option on Mac) is held
    pub alt: bool,

    /// Meta key (Cmd on Mac, Win on Windows) is held
    pub meta: bool,
}

impl ModifierKeys {
    /// No modifiers held
    pub fn none() -> Self {
        Self::default()
    }

    /// Create with shift modifier
    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Default::default()
        }
    }

    /// Create with ctrl modifier
    pub fn ctrl() -> Self {
        Self {
            ctrl: true,
            ..Default::default()
        }
    }

    /// Check if any modifier is held
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }
}

// =============================================================================
// DragState
// =============================================================================

/// Active drag operation state
#[derive(Clone, Debug)]
pub struct DragState {
    /// Starting position of the drag
    pub start: (f64, f64),

    /// Current position
    pub current: (f64, f64),

    /// Delta since last frame
    pub delta: (f64, f64),

    /// Total delta from start
    pub total_delta: (f64, f64),

    /// Which button is being used for drag
    pub button: MouseButton,
}

impl DragState {
    /// Create new drag state
    pub fn new(start: (f64, f64), current: (f64, f64), button: MouseButton) -> Self {
        let total_delta = (current.0 - start.0, current.1 - start.1);
        Self {
            start,
            current,
            delta: (0.0, 0.0),
            total_delta,
            button,
        }
    }

    /// Update drag with new position
    pub fn update(&mut self, x: f64, y: f64) {
        self.delta = (x - self.current.0, y - self.current.1);
        self.current = (x, y);
        self.total_delta = (self.current.0 - self.start.0, self.current.1 - self.start.1);
    }
}

// =============================================================================
// Rect Helper
// =============================================================================

/// Simple rectangle for hit testing
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_min_max(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        Self {
            x: min_x,
            y: min_y,
            w: max_x - min_x,
            h: max_y - min_y,
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn contains_point(&self, point: (f64, f64)) -> bool {
        self.contains(point.0, point.1)
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Expand rect by amount on all sides
    pub fn expand(&self, amount: f64) -> Self {
        Self {
            x: self.x - amount,
            y: self.y - amount,
            w: self.w + amount * 2.0,
            h: self.h + amount * 2.0,
        }
    }

    /// Shrink rect by amount on all sides
    pub fn shrink(&self, amount: f64) -> Self {
        self.expand(-amount)
    }
}
