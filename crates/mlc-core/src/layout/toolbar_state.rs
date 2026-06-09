//! Toolbar state types.
//!
//! Moved here from `zengeld-terminal-core::layout::render_frame` so that the
//! `ToolbarState` type (which stores `Icon` values) can live in the same crate
//! that defines `Icon`, avoiding a circular dependency.
//!
//! Note: `RenderThemes` and `build_render_themes` stay in `zengeld-terminal-core`
//! because they use core-specific types (`ThemeManager`, `ToolbarTheme`).

use std::collections::HashMap;

use crate::ui::icons::Icon;

// =============================================================================
// Toolbar State
// =============================================================================

/// State for toolbar rendering (hover states, active tool, etc.)
///
/// This struct tracks both visual state and logical state for toolbar buttons:
///
/// ## Visual States (background highlighting)
/// - `hovered_id` - Button with mouse over it (shows `button_bg_hover` - gray)
/// - `primed_id` - Button with open dropdown (shows `button_bg_active` - blue, sticky)
/// - `active_tool_id` - Currently selected drawing tool (shows active for that tool)
/// - `active_panels` - Open sidebar panels (shows active for those toggles)
///
/// ## Icon States (which SVG to display)
/// - `toggled_states` - For toggle buttons (Lock/Unlock, Eye/EyeOff) - determines icon
/// - `quick_select_icons` - For dropdown buttons that remember last selection
#[derive(Clone, Debug, Default)]
pub struct ToolbarState {
    /// Currently hovered item ID (shows gray background)
    pub hovered_id: Option<String>,

    /// Currently "primed" item ID - dropdown button with menu open
    /// Shows blue active background until user clicks elsewhere.
    /// This is the "sticky" state for quick-select dropdowns.
    pub primed_id: Option<String>,

    /// Currently active tool ID (e.g., "trend_line", "crosshair")
    /// The selected drawing tool shows as active (blue background).
    pub active_tool_id: Option<String>,

    /// Active sidebar panels (for right toolbar toggles)
    /// Panel toggle buttons show as active when their panel is open.
    pub active_panels: Vec<String>,

    /// Toggle button states - maps button ID to ON/OFF state
    /// Used for Lock, Eye, Magnet buttons that switch between two icons.
    /// Key = button id (e.g., "lock", "eye", "magnet")
    /// Value = true means toggled ON (show toggled_icon), false = OFF (show default icon)
    pub toggled_states: HashMap<String, bool>,

    /// Quick-select dropdown remembered icons
    /// Maps dropdown button ID to the last selected icon from its catalog.
    /// When rendering, this icon is shown instead of the default icon.
    pub quick_select_icons: HashMap<String, Icon>,

    /// Quick-select dropdown remembered tool IDs
    /// Maps dropdown button ID to the last selected tool ID from its catalog.
    /// Used when activating the tool on first click (primed pattern).
    pub quick_select_tool_ids: HashMap<String, String>,

    /// Dynamic button labels override
    /// Maps button ID to a label that overrides the static definition.
    /// Used for timeframe selector, symbol selector, etc.
    pub button_labels: HashMap<String, String>,

    /// Currently open dropdown ID (if any)
    /// Only one dropdown can be open at a time.
    pub open_dropdown_id: Option<String>,

    /// Currently hovered item in open dropdown (for hover highlight)
    pub hovered_dropdown_item_id: Option<String>,

    /// Currently open submenu ID (if hovering over a submenu item)
    pub open_submenu_id: Option<String>,

    /// Currently hovered item in open submenu (for hover highlight)
    pub hovered_submenu_item_id: Option<String>,

    /// Currently hovered item in context menu (for hover highlight)
    pub hovered_context_menu_item_id: Option<String>,

    /// Currently hovered item in right sidebar (for hover highlight in theme settings, etc.)
    pub hovered_sidebar_item_id: Option<String>,
}

impl ToolbarState {
    /// Create a new toolbar state with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a button should show as visually active (blue background)
    ///
    /// A button is active if:
    /// - It's the primed dropdown (menu open)
    /// - It has an open dropdown menu
    /// - It's the active drawing tool
    /// - It's an active sidebar panel toggle
    /// - It's a toggle button that is ON (lock, eye, magnet)
    pub fn is_active(&self, id: &str) -> bool {
        // Primed dropdown
        if self.primed_id.as_deref() == Some(id) {
            return true;
        }
        // Open dropdown
        if self.open_dropdown_id.as_deref() == Some(id) {
            return true;
        }
        // Active drawing tool (direct match for individual tool buttons)
        if self.active_tool_id.as_deref() == Some(id) {
            return true;
        }
        // Check if this dropdown owns the currently active tool via quick_select_tool_ids
        // (e.g., "line_tools" dropdown is active if user selected "trend_line" from it)
        if let Some(ref active_tool) = self.active_tool_id {
            if let Some(dropdown_tool) = self.quick_select_tool_ids.get(id) {
                if dropdown_tool == active_tool {
                    return true;
                }
            }
        }
        // Active panel toggle
        if self.active_panels.iter().any(|p| p == id) {
            return true;
        }
        // Toggle button that is ON
        if self.toggled_states.get(id).copied().unwrap_or(false) {
            return true;
        }
        false
    }

    /// Check if a toggle button is in the ON state
    pub fn is_toggled(&self, id: &str) -> bool {
        self.toggled_states.get(id).copied().unwrap_or(false)
    }

    /// Set toggle state for a button
    pub fn set_toggled(&mut self, id: &str, toggled: bool) {
        self.toggled_states.insert(id.to_string(), toggled);
    }

    /// Toggle a button's state and return the new state
    pub fn toggle(&mut self, id: &str) -> bool {
        let new_state = !self.is_toggled(id);
        self.set_toggled(id, new_state);
        new_state
    }

    /// Get the quick-select icon for a dropdown, if any
    pub fn quick_select_icon(&self, id: &str) -> Option<&Icon> {
        self.quick_select_icons.get(id)
    }

    /// Set the quick-select icon for a dropdown
    pub fn set_quick_select_icon(&mut self, id: &str, icon: Icon) {
        self.quick_select_icons.insert(id.to_string(), icon);
    }

    /// Get the quick-select tool ID for a dropdown, if any
    pub fn quick_select_tool_id(&self, id: &str) -> Option<&str> {
        self.quick_select_tool_ids.get(id).map(|s| s.as_str())
    }

    /// Set both the quick-select icon and tool ID for a dropdown
    pub fn set_quick_select(&mut self, dropdown_id: &str, tool_id: &str, icon: Icon) {
        self.quick_select_icons.insert(dropdown_id.to_string(), icon);
        self.quick_select_tool_ids.insert(dropdown_id.to_string(), tool_id.to_string());
    }

    /// Get the dynamic label for a button, if any
    pub fn button_label(&self, id: &str) -> Option<&str> {
        self.button_labels.get(id).map(|s| s.as_str())
    }

    /// Set a dynamic label for a button (overrides static definition)
    pub fn set_button_label(&mut self, id: &str, label: &str) {
        self.button_labels.insert(id.to_string(), label.to_string());
    }

    /// Prime a dropdown (show it as active until dismissed)
    pub fn prime(&mut self, id: &str) {
        self.primed_id = Some(id.to_string());
    }

    /// Clear the primed state
    pub fn clear_primed(&mut self) {
        self.primed_id = None;
    }

    /// Handle a click on a toolbar button and return the action to take.
    ///
    /// This implements the "primed" pattern for quick_select dropdowns:
    /// - First click on quick_select dropdown: prime it + return ActivateTool
    /// - Second click on already-primed button: return OpenDropdown
    /// - Click on other button when one is primed: clear primed + handle new button
    /// - Non-quick_select dropdowns: always return OpenDropdown
    /// - Regular buttons: return their action directly
    ///
    /// The app should call this method and act on the returned `ToolbarClickResult`.
    pub fn handle_click(&mut self, button_id: &str, is_quick_select: bool) -> ToolbarClickResult {
        // Check if this button is already primed
        let is_primed = self.primed_id.as_deref() == Some(button_id);

        if is_quick_select {
            if is_primed {
                // Second click on primed button -> open dropdown
                // Keep it primed (dropdown is opening, will close on selection or click elsewhere)
                ToolbarClickResult::OpenDropdown
            } else {
                // First click on quick_select -> prime it + activate tool
                // Clear any other primed button first
                self.primed_id = Some(button_id.to_string());
                ToolbarClickResult::ActivateTool
            }
        } else {
            // Non-quick_select button or regular button
            // Clear primed state when clicking other buttons
            self.primed_id = None;
            // For non-quick_select dropdowns, just open them
            ToolbarClickResult::OpenDropdown
        }
    }

    /// Handle a click outside any toolbar button.
    /// Clears the primed state and closes any open dropdown.
    pub fn handle_click_outside(&mut self) {
        self.primed_id = None;
        self.open_dropdown_id = None;
    }

    /// Open a dropdown menu
    pub fn open_dropdown(&mut self, id: &str) {
        self.open_dropdown_id = Some(id.to_string());
    }

    /// Close the currently open dropdown
    pub fn close_dropdown(&mut self) {
        self.open_dropdown_id = None;
    }

    /// Toggle a dropdown - close if open, open if closed
    pub fn toggle_dropdown(&mut self, id: &str) {
        if self.open_dropdown_id.as_deref() == Some(id) {
            self.open_dropdown_id = None;
        } else {
            self.open_dropdown_id = Some(id.to_string());
        }
    }

    /// Check if a dropdown is open
    pub fn is_dropdown_open(&self, id: &str) -> bool {
        self.open_dropdown_id.as_deref() == Some(id)
    }

    /// Check if any dropdown is open
    pub fn has_open_dropdown(&self) -> bool {
        self.open_dropdown_id.is_some()
    }

    /// Close all dismissible UI elements (dropdowns only - modals/sidebars handled separately)
    /// Returns true if something was closed
    pub fn dismiss_dropdowns(&mut self) -> bool {
        let had_dropdown = self.open_dropdown_id.is_some();
        self.open_dropdown_id = None;
        self.primed_id = None;
        had_dropdown
    }
}

// =============================================================================
// Toolbar Click Result
// =============================================================================

/// Result of handling a toolbar button click
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarClickResult {
    /// Activate the tool (for quick_select first click)
    ActivateTool,
    /// Open the dropdown menu (for quick_select second click or regular dropdown)
    OpenDropdown,
}

// =============================================================================
// Toggle Icon Pair
// =============================================================================

/// Toggle icon pair - default and toggled icons
#[derive(Clone, Debug)]
pub struct ToggleIconPair {
    /// Icon shown when toggle is OFF
    pub default_icon: &'static str,
    /// Icon shown when toggle is ON
    pub toggled_icon: &'static str,
}
