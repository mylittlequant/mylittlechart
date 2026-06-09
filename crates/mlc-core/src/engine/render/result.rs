//! Frame result types
//!
//! This module provides types returned from rendering functions
//! to communicate actions and cursor changes back to the platform.

use serde::{Deserialize, Serialize};

// =============================================================================
// FrameResult
// =============================================================================

/// Result returned from frame rendering
///
/// Contains information about what happened during rendering
/// that the platform may need to act on.
#[derive(Clone, Debug, Default)]
pub struct FrameResult {
    /// Cursor to display
    pub cursor: CursorIcon,

    /// Actions triggered during rendering
    pub actions: Vec<RenderAction>,

    /// Whether to request a repaint (for animations)
    pub needs_repaint: bool,

    /// Tooltip text to show (if any)
    pub tooltip: Option<String>,

    /// Whether input was consumed (prevent propagation)
    pub consumed: bool,
}

impl FrameResult {
    /// Create default result
    pub fn new() -> Self {
        Self::default()
    }

    /// Set cursor
    pub fn with_cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = cursor;
        self
    }

    /// Add an action
    pub fn with_action(mut self, action: RenderAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Mark as needing repaint
    pub fn with_repaint(mut self) -> Self {
        self.needs_repaint = true;
        self
    }

    /// Set tooltip
    pub fn with_tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Mark input as consumed
    pub fn with_consumed(mut self) -> Self {
        self.consumed = true;
        self
    }

    /// Merge another result into this one
    pub fn merge(&mut self, other: FrameResult) {
        // Take the more specific cursor
        if other.cursor != CursorIcon::Default {
            self.cursor = other.cursor;
        }
        // Merge actions
        self.actions.extend(other.actions);
        // Merge flags
        self.needs_repaint = self.needs_repaint || other.needs_repaint;
        self.consumed = self.consumed || other.consumed;
        // Take tooltip if we don't have one
        if self.tooltip.is_none() {
            self.tooltip = other.tooltip;
        }
    }
}

// =============================================================================
// CursorIcon
// =============================================================================

/// Cursor icon to display
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CursorIcon {
    /// Default arrow cursor
    #[default]
    Default,

    /// Pointer/hand cursor (for clickable elements)
    Pointer,

    /// Text cursor (I-beam)
    Text,

    /// Crosshair cursor
    Crosshair,

    /// Move cursor (4-way arrow)
    Move,

    /// Grab cursor (open hand)
    Grab,

    /// Grabbing cursor (closed hand)
    Grabbing,

    /// Not allowed cursor
    NotAllowed,

    /// Resize north-south
    ResizeNS,

    /// Resize east-west
    ResizeEW,

    /// Resize northeast-southwest
    ResizeNESW,

    /// Resize northwest-southeast
    ResizeNWSE,

    /// Resize column
    ColResize,

    /// Resize row
    RowResize,

    /// Zoom in
    ZoomIn,

    /// Zoom out
    ZoomOut,

    /// Wait/loading cursor
    Wait,

    /// Progress cursor
    Progress,

    /// Help cursor
    Help,

    /// No cursor (hidden)
    None,
}

impl CursorIcon {
    /// Convert to CSS cursor value
    pub fn as_css(&self) -> &'static str {
        match self {
            CursorIcon::Default => "default",
            CursorIcon::Pointer => "pointer",
            CursorIcon::Text => "text",
            CursorIcon::Crosshair => "crosshair",
            CursorIcon::Move => "move",
            CursorIcon::Grab => "grab",
            CursorIcon::Grabbing => "grabbing",
            CursorIcon::NotAllowed => "not-allowed",
            CursorIcon::ResizeNS => "ns-resize",
            CursorIcon::ResizeEW => "ew-resize",
            CursorIcon::ResizeNESW => "nesw-resize",
            CursorIcon::ResizeNWSE => "nwse-resize",
            CursorIcon::ColResize => "col-resize",
            CursorIcon::RowResize => "row-resize",
            CursorIcon::ZoomIn => "zoom-in",
            CursorIcon::ZoomOut => "zoom-out",
            CursorIcon::Wait => "wait",
            CursorIcon::Progress => "progress",
            CursorIcon::Help => "help",
            CursorIcon::None => "none",
        }
    }
}

// =============================================================================
// RenderAction
// =============================================================================

/// Action triggered during rendering
///
/// These are returned to the platform for handling.
/// Common actions are kept generic; specific actions use Custom variant.
#[derive(Clone, Debug)]
pub enum RenderAction {
    /// Open a popup/dropdown at position
    OpenPopup {
        id: String,
        x: f64,
        y: f64,
    },

    /// Close current popup
    ClosePopup,

    /// Show context menu at position
    ShowContextMenu {
        x: f64,
        y: f64,
        items: Vec<String>,
    },

    /// Trigger chart action by name
    ChartAction(String),

    /// Request focus on element
    Focus(String),

    /// Blur current focus
    Blur,

    /// Start text editing
    StartTextEdit {
        id: String,
        initial_text: String,
    },

    /// Copy text to clipboard
    Copy(String),

    /// Request paste from clipboard
    Paste,

    /// Custom action with payload (for extension)
    Custom {
        action_type: String,
        payload: String,
    },
}
