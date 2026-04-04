//! Chart input actions for zengeld-chart
//!
//! This module defines the semantic actions that can be performed on a chart
//! in response to user input. Actions are platform-agnostic representations
//! of user intent.

use super::drag_mode::DragMode;

/// Semantic input action for the chart.
///
/// These actions represent what the user wants to do (semantic intent),
/// not what they physically did (raw input). This abstraction allows
/// the same action to be triggered by different input methods:
/// - Mouse drag or touch pan for `Pan`
/// - Scroll wheel, pinch, or button click for `Zoom`
/// - etc.
///
/// # Example
///
/// ```ignore
/// use zengeld_chart::input::{ChartInputAction, DragMode};
///
/// let action = ChartInputAction::Pan { delta_x: 10.0, delta_y: 0.0 };
///
/// match action {
///     ChartInputAction::Pan { delta_x, delta_y } => {
///         viewport.pan(delta_x, delta_y);
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ChartInputAction {
    /// Pan the chart by a delta amount.
    ///
    /// Positive `delta_x` moves the chart content to the right (shows earlier data).
    /// Positive `delta_y` moves the chart content down (shows higher prices).
    Pan {
        /// Horizontal pan amount in pixels.
        delta_x: f64,
        /// Vertical pan amount in pixels.
        delta_y: f64,
    },

    /// Zoom the chart around a center point.
    ///
    /// Factor values:
    /// - `> 1.0` = zoom in (show less data, larger candles)
    /// - `< 1.0` = zoom out (show more data, smaller candles)
    /// - `= 1.0` = no change
    Zoom {
        /// X coordinate of zoom center in screen pixels.
        center_x: f64,
        /// Y coordinate of zoom center in screen pixels.
        center_y: f64,
        /// Horizontal zoom factor.
        factor_x: f64,
        /// Vertical zoom factor.
        factor_y: f64,
    },

    /// Start a drag operation.
    ///
    /// Sent when the user presses down and begins dragging.
    DragStart {
        /// What is being dragged.
        mode: DragMode,
        /// Starting X position in screen pixels.
        x: f64,
        /// Starting Y position in screen pixels.
        y: f64,
    },

    /// Continue a drag operation.
    ///
    /// Sent on each movement while dragging.
    DragMove {
        /// What is being dragged.
        mode: DragMode,
        /// Current X position in screen pixels.
        x: f64,
        /// Current Y position in screen pixels.
        y: f64,
        /// X movement since last event.
        delta_x: f64,
        /// Y movement since last event.
        delta_y: f64,
    },

    /// End a drag operation.
    ///
    /// Sent when the user releases after dragging.
    DragEnd {
        /// What was being dragged.
        mode: DragMode,
        /// Final X position in screen pixels.
        x: f64,
        /// Final Y position in screen pixels.
        y: f64,
    },

    /// Mouse/touch click at a position.
    Click {
        /// X position in screen pixels.
        x: f64,
        /// Y position in screen pixels.
        y: f64,
        /// Which button was clicked.
        button: MouseButton,
    },

    /// Double-click at a position.
    ///
    /// Often used to reset view or open settings.
    DoubleClick {
        /// X position in screen pixels.
        x: f64,
        /// Y position in screen pixels.
        y: f64,
    },

    /// Context menu requested (typically right-click).
    ContextMenu {
        /// X position in screen pixels.
        x: f64,
        /// Y position in screen pixels.
        y: f64,
    },

    /// Move the crosshair to a position.
    ///
    /// Sent on hover/move when crosshair should track cursor.
    CrosshairMove {
        /// X position in screen pixels.
        x: f64,
        /// Y position in screen pixels.
        y: f64,
    },

    /// Hide the crosshair.
    ///
    /// Sent when cursor leaves the chart area.
    CrosshairHide,

    /// Key press event.
    KeyPress {
        /// Which key was pressed.
        key: KeyCode,
        /// Modifier keys held during press.
        modifiers: Modifiers,
    },

    /// Scroll event (separate from zoom).
    ///
    /// Used for scrolling through data without zooming,
    /// typically when Shift is held.
    Scroll {
        /// X position of scroll.
        x: f64,
        /// Y position of scroll.
        y: f64,
        /// Horizontal scroll delta.
        delta_x: f64,
        /// Vertical scroll delta.
        delta_y: f64,
    },

    /// No action needed.
    ///
    /// Used as a placeholder when processing events that
    /// don't result in any chart action.
    #[default]
    None,
}

impl ChartInputAction {
    /// Check if this action modifies the viewport.
    ///
    /// Returns `true` for actions that change what data is visible.
    #[inline]
    pub fn modifies_viewport(&self) -> bool {
        matches!(
            self,
            ChartInputAction::Pan { .. }
                | ChartInputAction::Zoom { .. }
                | ChartInputAction::Scroll { .. }
        )
    }

    /// Check if this is a drag-related action.
    #[inline]
    pub fn is_drag_action(&self) -> bool {
        matches!(
            self,
            ChartInputAction::DragStart { .. }
                | ChartInputAction::DragMove { .. }
                | ChartInputAction::DragEnd { .. }
        )
    }

    /// Check if this is a no-op action.
    #[inline]
    pub fn is_none(&self) -> bool {
        matches!(self, ChartInputAction::None)
    }
}

/// Mouse button identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseButton {
    /// Left mouse button (primary).
    #[default]
    Left,
    /// Right mouse button (secondary/context).
    Right,
    /// Middle mouse button (wheel click).
    Middle,
}

/// Keyboard key codes.
///
/// This enum covers the common keys needed for chart shortcuts.
/// Applications should map their platform key codes to these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyCode {
    // Letters
    /// A key
    A,
    /// B key
    B,
    /// C key
    C,
    /// D key
    D,
    /// E key
    E,
    /// F key
    F,
    /// G key
    G,
    /// H key
    H,
    /// I key
    I,
    /// J key
    J,
    /// K key
    K,
    /// L key
    L,
    /// M key
    M,
    /// N key
    N,
    /// O key
    O,
    /// P key
    P,
    /// Q key
    Q,
    /// R key
    R,
    /// S key
    S,
    /// T key
    T,
    /// U key
    U,
    /// V key
    V,
    /// W key
    W,
    /// X key
    X,
    /// Y key
    Y,
    /// Z key
    Z,

    // Numbers
    /// 0 key
    Num0,
    /// 1 key
    Num1,
    /// 2 key
    Num2,
    /// 3 key
    Num3,
    /// 4 key
    Num4,
    /// 5 key
    Num5,
    /// 6 key
    Num6,
    /// 7 key
    Num7,
    /// 8 key
    Num8,
    /// 9 key
    Num9,

    // Function keys
    /// F1 key
    F1,
    /// F2 key
    F2,
    /// F3 key
    F3,
    /// F4 key
    F4,
    /// F5 key
    F5,
    /// F6 key
    F6,
    /// F7 key
    F7,
    /// F8 key
    F8,
    /// F9 key
    F9,
    /// F10 key
    F10,
    /// F11 key
    F11,
    /// F12 key
    F12,

    // Navigation
    /// Arrow Up key
    ArrowUp,
    /// Arrow Down key
    ArrowDown,
    /// Arrow Left key
    ArrowLeft,
    /// Arrow Right key
    ArrowRight,
    /// Home key
    Home,
    /// End key
    End,
    /// Page Up key
    PageUp,
    /// Page Down key
    PageDown,

    // Editing
    /// Backspace key
    Backspace,
    /// Delete key
    Delete,
    /// Insert key
    Insert,
    /// Enter/Return key
    Enter,
    /// Tab key
    Tab,
    /// Space key
    Space,

    // Modifiers (when pressed alone)
    /// Escape key
    Escape,

    // Symbols
    /// Plus/Equal key
    Plus,
    /// Minus key
    Minus,
    /// Open bracket key
    BracketLeft,
    /// Close bracket key
    BracketRight,

    /// Unknown or unmapped key
    #[default]
    Unknown,
}

/// Modifier key state.
///
/// Tracks which modifier keys were held during an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift key is pressed.
    pub shift: bool,
    /// Ctrl key is pressed (Command on macOS for some shortcuts).
    pub ctrl: bool,
    /// Alt key is pressed (Option on macOS).
    pub alt: bool,
    /// Meta key is pressed (Command on macOS, Windows key on Windows).
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers pressed.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// Shift modifier only.
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// Ctrl modifier only.
    pub const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    };

    /// Alt modifier only.
    pub const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
        meta: false,
    };

    /// Meta modifier only.
    pub const META: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: true,
    };

    /// Create a new Modifiers instance.
    pub const fn new(shift: bool, ctrl: bool, alt: bool, meta: bool) -> Self {
        Self {
            shift,
            ctrl,
            alt,
            meta,
        }
    }

    /// Check if any modifier is pressed.
    #[inline]
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }

    /// Check if no modifier is pressed.
    #[inline]
    pub fn none(&self) -> bool {
        !self.any()
    }

    /// Check if this matches the "command" key for the platform.
    ///
    /// On macOS, this is Meta (Command). On other platforms, this is Ctrl.
    #[inline]
    pub fn command(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.meta
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.ctrl
        }
    }

    /// Check for Ctrl+Shift combination.
    #[inline]
    pub fn ctrl_shift(&self) -> bool {
        self.ctrl && self.shift && !self.alt && !self.meta
    }

    /// Check for Ctrl+Alt combination.
    #[inline]
    pub fn ctrl_alt(&self) -> bool {
        self.ctrl && self.alt && !self.shift && !self.meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_modifies_viewport() {
        assert!(ChartInputAction::Pan {
            delta_x: 1.0,
            delta_y: 0.0
        }
        .modifies_viewport());
        assert!(ChartInputAction::Zoom {
            center_x: 0.0,
            center_y: 0.0,
            factor_x: 1.1,
            factor_y: 1.0
        }
        .modifies_viewport());
        assert!(ChartInputAction::Scroll {
            x: 0.0,
            y: 0.0,
            delta_x: 0.0,
            delta_y: 10.0
        }
        .modifies_viewport());

        assert!(!ChartInputAction::Click {
            x: 0.0,
            y: 0.0,
            button: MouseButton::Left
        }
        .modifies_viewport());
        assert!(!ChartInputAction::CrosshairMove { x: 0.0, y: 0.0 }.modifies_viewport());
    }

    #[test]
    fn test_action_is_drag() {
        assert!(ChartInputAction::DragStart {
            mode: DragMode::Chart,
            x: 0.0,
            y: 0.0
        }
        .is_drag_action());
        assert!(ChartInputAction::DragMove {
            mode: DragMode::Chart,
            x: 0.0,
            y: 0.0,
            delta_x: 1.0,
            delta_y: 0.0
        }
        .is_drag_action());
        assert!(ChartInputAction::DragEnd {
            mode: DragMode::Chart,
            x: 0.0,
            y: 0.0
        }
        .is_drag_action());

        assert!(!ChartInputAction::Pan {
            delta_x: 1.0,
            delta_y: 0.0
        }
        .is_drag_action());
    }

    #[test]
    fn test_modifiers() {
        assert!(!Modifiers::NONE.any());
        assert!(Modifiers::NONE.none());

        assert!(Modifiers::SHIFT.any());
        assert!(Modifiers::SHIFT.shift);
        assert!(!Modifiers::SHIFT.ctrl);

        let ctrl_shift = Modifiers::new(true, true, false, false);
        assert!(ctrl_shift.ctrl_shift());
        assert!(!ctrl_shift.ctrl_alt());
    }

    #[test]
    fn test_default_action() {
        let action: ChartInputAction = Default::default();
        assert!(action.is_none());
    }
}
