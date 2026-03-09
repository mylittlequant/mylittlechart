//! Context menu state and builder utilities
//!
//! Provides types for managing right-click context menus on chart elements
//! such as primitives, indicators, and the chart background.

use crate::LeafId;

// =============================================================================
// Context Menu Target
// =============================================================================

/// What the context menu was opened on
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ContextMenuTarget {
    /// No specific target (empty area)
    #[default]
    None,
    /// A drawing primitive by index
    Primitive(usize),
    /// A signal by index
    Signal(usize),
    /// Chart background
    ChartBackground,
    /// An indicator by instance ID
    Indicator(u64),
    /// A color tag for a leaf panel
    ColorTag(LeafId),
}

// =============================================================================
// Context Menu Item State
// =============================================================================

/// A rendered context menu item
#[derive(Clone, Debug)]
pub struct ContextMenuItemState {
    /// Action ID (e.g., "delete", "clone", "settings")
    pub action: String,
    /// Display label
    pub label: String,
    /// Optional icon (emoji or icon id)
    pub icon: Option<String>,
    /// Is this a separator?
    pub is_separator: bool,
    /// Is this a danger/destructive action?
    pub is_danger: bool,
    /// Is this item enabled?
    pub enabled: bool,
}

impl ContextMenuItemState {
    /// Create an action item
    pub fn action(action: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            label: label.into(),
            icon: None,
            is_separator: false,
            is_danger: false,
            enabled: true,
        }
    }

    /// Create an action with icon
    pub fn action_with_icon(icon: impl Into<String>, action: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            label: label.into(),
            icon: Some(icon.into()),
            is_separator: false,
            is_danger: false,
            enabled: true,
        }
    }

    /// Create a separator
    pub fn separator() -> Self {
        Self {
            action: String::new(),
            label: String::new(),
            icon: None,
            is_separator: true,
            is_danger: false,
            enabled: true,
        }
    }

    /// Create a danger action
    pub fn danger(action: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            label: label.into(),
            icon: None,
            is_separator: false,
            is_danger: true,
            enabled: true,
        }
    }

    /// Create a danger action with icon
    pub fn danger_with_icon(icon: impl Into<String>, action: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            label: label.into(),
            icon: Some(icon.into()),
            is_separator: false,
            is_danger: true,
            enabled: true,
        }
    }

    /// Set enabled state
    pub fn set_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

// =============================================================================
// Context Menu State
// =============================================================================

/// State for an open context menu
#[derive(Clone, Debug, Default)]
pub struct ContextMenuState {
    /// Is a context menu currently open?
    pub is_open: bool,
    /// Position to display menu (screen coordinates)
    pub x: f64,
    pub y: f64,
    /// Target type for context menu actions
    pub target: ContextMenuTarget,
    /// Menu items to display
    pub items: Vec<ContextMenuItemState>,
}

impl ContextMenuState {
    /// Create new context menu state (closed)
    pub fn new() -> Self {
        Self::default()
    }

    /// Open context menu at position (no bounds checking)
    pub fn open(&mut self, x: f64, y: f64, target: ContextMenuTarget, items: Vec<ContextMenuItemState>) {
        self.is_open = true;
        self.x = x;
        self.y = y;
        self.target = target;
        self.items = items;
    }

    /// Open context menu with smart positioning (stays within window bounds)
    pub fn open_smart(
        &mut self,
        x: f64,
        y: f64,
        target: ContextMenuTarget,
        items: Vec<ContextMenuItemState>,
        screen_w: f64,
        screen_h: f64,
    ) {
        // Menu dimensions (must match render.rs constants - see render_context_menu)
        let item_height = 32.0;  // render.rs line 4740
        let padding_y = 8.0;     // render.rs line 4741
        let separator_height = 9.0;
        let menu_width = 180.0;  // render.rs line 4749
        let margin = 4.0;

        // Calculate menu height
        let item_count = items.iter().filter(|i| !i.is_separator).count();
        let separator_count = items.iter().filter(|i| i.is_separator).count();
        let menu_height = (item_count as f64 * item_height) + (separator_count as f64 * separator_height) + padding_y * 2.0;

        // Clamp position to stay within screen bounds
        let final_x = if x + menu_width > screen_w - margin {
            (screen_w - menu_width - margin).max(margin)
        } else {
            x.max(margin)
        };

        let final_y = if y + menu_height > screen_h - margin {
            (screen_h - menu_height - margin).max(margin)
        } else {
            y.max(margin)
        };

        self.is_open = true;
        self.x = final_x;
        self.y = final_y;
        self.target = target;
        self.items = items;
    }

    /// Close context menu
    pub fn close(&mut self) {
        self.is_open = false;
        self.items.clear();
        self.target = ContextMenuTarget::None;
    }

    /// Check if context menu is open
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Get primitive index if targeting a primitive
    pub fn primitive_idx(&self) -> Option<usize> {
        match self.target {
            ContextMenuTarget::Primitive(idx) => Some(idx),
            _ => None,
        }
    }

    /// Get indicator instance ID if targeting an indicator
    pub fn indicator_id(&self) -> Option<u64> {
        match self.target {
            ContextMenuTarget::Indicator(id) => Some(id),
            _ => None,
        }
    }
}

// =============================================================================
// Context Menu Builder
// =============================================================================

/// Build context menu items for a primitive
///
/// Note: icon field uses icon IDs (not emoji) that map to Icon enum in render code:
/// - "settings" -> Icon::Settings
/// - "copy" -> Icon::Copy
/// - "lock" -> Icon::Lock
/// - "unlock" -> Icon::Unlock
/// - "eye" -> Icon::Eye
/// - "eye_off" -> Icon::EyeOff
/// - "delete" -> Icon::Delete
/// - "arrow_up" -> Icon::ArrowUp
/// - "arrow_down" -> Icon::ArrowDown
/// Build context menu items for a color tag
pub fn build_color_tag_context_menu() -> Vec<ContextMenuItemState> {
    vec![
        ContextMenuItemState::danger_with_icon("unlink", "desync", "Desync"),
    ]
}

pub fn build_primitive_context_menu(
    _display_name: &str,
    is_locked: bool,
    is_visible: bool,
) -> Vec<ContextMenuItemState> {
    let lock_label = if is_locked { "Разблокировать" } else { "Заблокировать" };
    let lock_icon = if is_locked { "unlock" } else { "lock" };
    let visibility_label = if is_visible { "Скрыть" } else { "Показать" };
    let visibility_icon = if is_visible { "eye_off" } else { "eye" };

    vec![
        ContextMenuItemState::action_with_icon("settings", "settings", "Настройки"),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("copy", "clone", "Клонировать"),
        ContextMenuItemState::action_with_icon(lock_icon, "toggle_lock", lock_label),
        ContextMenuItemState::action_with_icon(visibility_icon, "toggle_visibility", visibility_label),
        ContextMenuItemState::separator(),
        ContextMenuItemState::danger_with_icon("delete", "delete", "Удалить"),
    ]
}
