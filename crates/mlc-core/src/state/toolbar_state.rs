//! Toolbar interaction state (active/primed tool, toggle icon pairs) and chart-toolbar
//! interaction state (`ChartToolbarState`, `ToolbarConfig`, `ChartToolbarRenderResult`).
//! Pure data — no render logic (render methods stay in `panel_app`).
//!
//! Moved from `layout/toolbar_state.rs` (Phase X.3 back-edge removal).
//! The `layout` module keeps a re-export shim so existing call sites are unaffected.
//!
//! `ChartToolbarState`, `ToolbarConfig`, `ChartToolbarRenderResult`, and supporting types
//! moved from `panel_app.rs` (Phase X.4 back-edge removal) so that `state/chart_container.rs`
//! can hold these fields without importing from `panel_app`.
//!
//! Step 5 (UI-proxies, `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) note:
//! `ChartToolbarState` (below, `active_tool_id`/`primed_id`/`open_dropdown_id`)
//! IS the `DrawingToolState` proxy the Step 5 spec text describes — under
//! its pre-existing name. Step 5's original field list ("active tool, drag
//! state, hover target, current pane") pre-dates doctrine §1A (2026-07-01,
//! "supersedes any earlier reading"), which explicitly assigns
//! active-tool/drag/hover/current-pane to the backend `DrawingManager`
//! (`core.drawings_mut().set_tool/start_drag/set_current_pane`, all live and
//! working). `handle_toolbar_click_with_chart`/`handle_dropdown_select_with_chart`
//! below already implement the proxy pattern exactly: local UI-flow draft
//! fields (which button is primed/active/hovered) + dispatch through
//! `drawing_manager: &mut impl DrawingSession` in the same call. Creating a
//! second `DrawingToolState` struct duplicating those fields would
//! re-introduce the state-duplication bug §1A closed. Do not create one.

use std::collections::HashMap;

use crate::state::icons::Icon;

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

// =============================================================================
// Chart-toolbar types (moved from panel_app — Phase X.4 back-edge removal)
// =============================================================================

use web_time::Instant;

use crate::state::geometry::LayoutRect;
use crate::state::toolbar_geometry::ToolbarRenderResult;
use crate::state::toolbar_geometry::WidgetRect;
use crate::chart::types::crosshair::Crosshair;
use crate::state::drawing_session::DrawingSession;
use crate::events::ChartOutEvent;

// =============================================================================
// ToolbarConfig
// =============================================================================

/// Per-toolbar configuration for a chart panel.
///
/// Each field controls one toolbar position. `None` means the toolbar is
/// hidden and no space is carved for it. `Some(def)` means the toolbar is
/// visible and rendered with the given definition.
#[derive(Clone, Debug)]
pub struct ToolbarConfig {
    /// Top control strip (horizontal, default 40px).
    pub top: Option<uzor::panel_api::PanelToolbarDef>,
    /// Left drawing toolbar (vertical, default 50px).
    pub left: Option<uzor::panel_api::PanelToolbarDef>,
    /// Right sidebar toolbar (vertical, default 48px).
    pub right: Option<uzor::panel_api::PanelToolbarDef>,
    /// Bottom toolbar (horizontal, default 32px).
    pub bottom: Option<uzor::panel_api::PanelToolbarDef>,
}

impl ToolbarConfig {
    /// Terminal mode: top + left only.
    pub fn terminal() -> Self {
        Self {
            top: Some(crate::toolbar::top_toolbar()),
            left: Some(crate::toolbar::left_toolbar()),
            right: None,
            bottom: None,
        }
    }

    /// Standalone / Bloomberg mode: all 4 toolbars.
    ///
    /// Uses standalone-specific toolbar variants that omit terminal-only buttons
    /// (hamburger menu, watchlist, alerts) which require terminal infrastructure.
    pub fn standalone() -> Self {
        Self {
            top: Some(crate::toolbar::standalone_top_toolbar()),
            left: Some(crate::toolbar::left_toolbar()),
            right: Some(crate::toolbar::standalone_right_toolbar()),
            bottom: Some(crate::toolbar::bottom_toolbar()),
        }
    }

    /// Minimal mode: no toolbars.
    pub fn minimal() -> Self {
        Self {
            top: None,
            left: None,
            right: None,
            bottom: None,
        }
    }

    /// Custom builder starting from no toolbars.
    pub fn custom() -> Self {
        Self::minimal()
    }

    pub fn with_top(mut self, def: Option<uzor::panel_api::PanelToolbarDef>) -> Self {
        self.top = def;
        self
    }

    pub fn with_left(mut self, def: Option<uzor::panel_api::PanelToolbarDef>) -> Self {
        self.left = def;
        self
    }

    pub fn with_right(mut self, def: Option<uzor::panel_api::PanelToolbarDef>) -> Self {
        self.right = def;
        self
    }

    pub fn with_bottom(mut self, def: Option<uzor::panel_api::PanelToolbarDef>) -> Self {
        self.bottom = def;
        self
    }

    pub fn top_height(&self) -> f64 {
        self.top.as_ref().map_or(0.0, |d| d.size)
    }

    pub fn left_width(&self) -> f64 {
        self.left.as_ref().map_or(0.0, |d| d.size)
    }

    pub fn right_width(&self) -> f64 {
        self.right.as_ref().map_or(0.0, |d| d.size)
    }

    pub fn bottom_height(&self) -> f64 {
        self.bottom.as_ref().map_or(0.0, |d| d.size)
    }
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        Self::standalone()
    }
}

// =============================================================================
// Floating inline bar supporting types
// =============================================================================

/// Free-space region within a toolbar where the inline bar can slide when docked.
#[derive(Clone, Copy, Debug, Default)]
pub struct InlineSlideContainer {
    /// Left edge of the free space (absolute x).
    pub x: f64,
    /// Width of the free space.
    pub width: f64,
}

/// Edge to which the floating inline bar is docked/snapped.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum InlineDockEdge {
    /// Snapped to bottom toolbar (default)
    Bottom,
    /// Snapped to top toolbar
    Top,
    /// Free-floating at custom position
    Free,
}

/// Lightweight floating toolbar for inline primitive configuration.
///
/// When a drawing primitive is selected, this toolbar appears as a small
/// overlay bar. It can be dragged and snaps ("magnetizes") to the top or
/// bottom toolbar edges.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FloatingInlineBar {
    /// Horizontal offset from left edge of content_rect (content-local).
    pub x: f64,
    /// Vertical offset from top of content_rect (content-local).
    pub y: f64,
    /// Current dock state.
    pub dock_edge: InlineDockEdge,
    /// Whether the bar is currently being dragged.
    pub dragging: bool,
    /// Cursor offset from bar origin when drag started.
    pub drag_offset_x: f64,
    pub drag_offset_y: f64,
}

impl Default for FloatingInlineBar {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            dock_edge: InlineDockEdge::Bottom,
            dragging: false,
            drag_offset_x: 0.0,
            drag_offset_y: 0.0,
        }
    }
}

impl FloatingInlineBar {
    /// Snap threshold in pixels — if within this distance of a toolbar edge,
    /// the bar magnetizes to it.
    pub const SNAP_THRESHOLD: f64 = 20.0;

    /// Bar height — matches TOP_TOOLBAR_HEIGHT / BOTTOM_TOOLBAR_HEIGHT for seamless docking.
    pub const BAR_HEIGHT: f64 = 40.0;

    /// Start drag.
    pub fn start_drag(&mut self, cursor_x: f64, cursor_y: f64, bar_rect: &LayoutRect) {
        self.dragging = true;
        self.drag_offset_x = cursor_x - bar_rect.x;
        self.drag_offset_y = cursor_y - bar_rect.y;
        self.dock_edge = InlineDockEdge::Free;
    }

    /// End drag.
    pub fn end_drag(&mut self) {
        self.dragging = false;
    }
}

// =============================================================================
// Toolbar render result types
// =============================================================================

/// Info about a rendered dropdown popup
#[derive(Clone, Debug)]
pub struct DropdownRenderInfo {
    /// The dropdown_id that was rendered (e.g. "line_tools")
    pub dropdown_id: String,
    /// Item rects for hit testing: (item_id, rect)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// The total menu rect (for click-outside detection)
    pub menu_rect: WidgetRect,
    /// The submenu item currently being hovered (if any), set by draw_dropdown
    /// when the pointer is over a submenu-trigger item.  Callers should use this
    /// to open the submenu on hover without waiting for a click.
    pub open_submenu: Option<String>,
    /// The hovered item id from draw_dropdown (if any pointer is over an item).
    pub hovered: Option<String>,
    /// `(text_rect, char_x_positions)` of the `timeframe_selector` dropdown's
    /// visible inline granularity editor widget, when one is rendered this
    /// frame (mouse-support arc, item 1). `None` whenever no editor is
    /// active or the dropdown rendered is not `timeframe_selector`. Fed into
    /// `TextFieldStore::update_field` (mirrors every other modal text field —
    /// see `ChartApp::update_text_field_geometry`) so `on_drag_start`/
    /// `on_drag_move` can hit-test click-to-cursor and drag-select against
    /// the SAME metrics `draw_input` just rendered with.
    pub granularity_edit_input: Option<(WidgetRect, Vec<f64>)>,
}

/// Hit-test information for an inline popup dropdown (style or width picker).
#[derive(Clone, Debug)]
pub struct InlineDropdownResult {
    /// Which dropdown is open: "inline:style" or "inline:width"
    pub dropdown_id: String,
    /// Item rects for hit testing: (item_id, rect)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Total popup rect (for click-outside detection)
    pub menu_rect: WidgetRect,
}

/// Hit-test information for the inline config toolbar that appears as a
/// floating overlay bar when a drawing primitive is selected.
#[derive(Clone, Debug, Default)]
pub struct InlineConfigResult {
    /// Item rectangles for hit testing: (item_id, rect)
    pub item_rects: Vec<(String, crate::state::toolbar_geometry::ToolbarRect)>,
    /// The bar's bounding rect (for drag hit-testing).
    pub bar_rect: LayoutRect,
}

/// Result of rendering all four local chart-panel toolbars in a single pass.
#[derive(Clone, Debug, Default)]
pub struct ChartToolbarRenderResult {
    /// Hit zones from the left drawing toolbar.
    pub left_toolbar: ToolbarRenderResult,
    /// Hit zones from the top control strip.
    pub top_toolbar: ToolbarRenderResult,
    /// Hit zones from the right sidebar toolbar.
    pub right_toolbar: ToolbarRenderResult,
    /// Hit zones from the bottom toolbar.
    pub bottom_toolbar: ToolbarRenderResult,
    /// The content rect where the chart was drawn (for core to know where to
    /// render overlays on top).
    pub content_rect: LayoutRect,
    /// Hit zones from an open dropdown (if any)
    pub dropdown_result: Option<DropdownRenderInfo>,
    /// Hit zones for the inline config toolbar (when a primitive is selected)
    pub inline_config: Option<InlineConfigResult>,
    /// Hit zones from an open inline dropdown (style or width) if any
    pub inline_dropdown_result: Option<InlineDropdownResult>,
    /// Overflow scroll chevron rects for each toolbar (left/up and right/down).
    /// Present only when that toolbar is overflowing and the chevron is visible.
    pub top_left_chevron: Option<WidgetRect>,
    pub top_right_chevron: Option<WidgetRect>,
    pub bottom_left_chevron: Option<WidgetRect>,
    pub bottom_right_chevron: Option<WidgetRect>,
    pub left_up_chevron: Option<WidgetRect>,
    pub left_down_chevron: Option<WidgetRect>,
    pub right_up_chevron: Option<WidgetRect>,
    pub right_down_chevron: Option<WidgetRect>,
    /// Max scroll values for clamping
    pub top_max_scroll: f64,
    pub bottom_max_scroll: f64,
    pub left_max_scroll: f64,
    pub right_max_scroll: f64,
    /// `(text_rect, char_x_positions)` of the `symbol_selector` toolbar
    /// BUTTON's own widget-overlay text input, when the quick-search
    /// editor is live this frame (2026-07-17 rework: the button-hosted
    /// editor is the ONLY quick-search input — the former dropdown-hosted
    /// variant is deleted). Fed into `TextFieldStore::update_field` by
    /// `ChartApp::update_text_field_geometry`.
    pub symbol_inline_input: Option<(WidgetRect, Vec<f64>)>,
}

// =============================================================================
// ChartToolbarState
// =============================================================================

/// Toolbar state for the chart panel's local toolbars.
///
/// This tracks which drawing tool is active, which dropdowns are open,
/// toggle states (magnet, lock, visibility), and quick-select memory.
/// Each chart leaf gets its own instance of this state.
#[derive(Clone, Debug)]
pub struct ChartToolbarState {
    /// Currently active drawing tool (None = cursor mode)
    pub active_tool_id: Option<String>,
    /// Hovered drawing toolbar item (left vertical strip)
    pub hovered_left_toolbar_id: Option<String>,
    /// Hovered control strip item (top horizontal strip)
    pub hovered_top_toolbar_id: Option<String>,
    /// Hovered right sidebar toolbar item
    pub hovered_right_toolbar_id: Option<String>,
    /// Right-rail button id of the currently OPEN sidebar panel — rendered
    /// accent-active so the open tab is visible at a glance (owner order
    /// 2026-07-31). Set per frame by the app from `sidebar_state`.
    pub active_right_toolbar_id: Option<String>,
    /// Hovered bottom toolbar item
    pub hovered_bottom_toolbar_id: Option<String>,
    /// Currently open dropdown
    pub open_dropdown_id: Option<String>,
    /// Hovered item within an open dropdown
    pub hovered_dropdown_item: Option<String>,
    /// "Primed" dropdown (blue background, remembers last open)
    pub primed_id: Option<String>,
    /// Toggle states: magnet, lock, eye
    pub magnet_enabled: bool,
    /// Tracks whether the strong (body-only) magnet mode is active.
    /// Used to show the ICON_MAGNET_STRONG variant in the toolbar.
    pub magnet_strong: bool,
    pub drawings_locked: bool,
    pub drawings_visible: bool,
    /// Quick-select memory: dropdown_id -> (last_tool_id, icon_id)
    pub quick_select_memory: std::collections::HashMap<String, (String, String)>,
    /// Currently open submenu within a dropdown
    pub open_submenu_id: Option<String>,
    /// Hovered item within an open submenu
    pub hovered_submenu_item: Option<String>,
    /// Timestamp of the last magnet button click — used for double-click detection.
    /// Double-click (within 300ms) opens the magnet dropdown.
    /// Single-click toggles magnet ON/OFF.
    pub last_magnet_click_time: Option<Instant>,
    /// Floating inline config toolbar state (position, dock edge, drag state).
    pub floating_inline_bar: FloatingInlineBar,
    /// Hovered inline toolbar item id.
    pub hovered_inline_id: Option<String>,
    /// Whether the inline style dropdown is open.
    pub open_inline_style_dropdown: bool,
    /// Whether the inline width dropdown is open.
    pub open_inline_width_dropdown: bool,
    /// Whether the inline wave-slot picker dropdown is open (E2 — see
    /// `mlc-app-layout`'s `ToolbarSection::inline_config`'s `"inline:wave"`
    /// button). Same shape as the style/width dropdown flags above.
    pub open_inline_wave_dropdown: bool,
    /// The slot chosen in the E2 picker's own FIRST step, while
    /// `open_inline_wave_dropdown` is true (owner ask 2026-08-13: slot
    /// first, then which structure goes in it). `None` shows the slot list
    /// (`SelectedPrimitiveConfig::wave_slots`); `Some(slot_id)` shows that
    /// slot's own legal kinds (`SelectedPrimitiveConfig::wave_slot_kinds`)
    /// instead, preceded by an `"inline:wave_back"` row that steps back to
    /// `None`. Cleared on every path that closes this dropdown (the
    /// background absorber, the wave button's own toggle, selecting a
    /// kind, selecting a successor) — a stale pending slot must never
    /// survive into the next opening, since the primitive selected next
    /// may not even have this slot id.
    pub pending_wave_slot: Option<String>,
    /// Whether the inline wave-SUCCESSION picker dropdown is open (E3 — see
    /// `mlc-app-layout`'s `ToolbarSection::inline_config`'s
    /// `"inline:wave_next"` button, the SECOND wave control: "what comes
    /// after this" rather than E2's "what subdivides this"). Same shape as
    /// `open_inline_wave_dropdown` above — a separate flag because the two
    /// pickers can each be open independently of the other (never both at
    /// once in practice, since opening one closes the other, but they are
    /// still two distinct buttons with two distinct row lists).
    pub open_inline_wave_next_dropdown: bool,
    /// Hovered item inside the inline dropdown (for hover highlight).
    pub hovered_inline_dropdown_item: Option<String>,
    /// Scroll offsets for each toolbar (pixels; 0.0 = no scroll).
    pub top_scroll_offset: f64,
    pub bottom_scroll_offset: f64,
    pub left_scroll_offset: f64,
    pub right_scroll_offset: f64,
    /// Custom position override for the dropdown origin.
    ///
    /// Used by external callers (e.g. chrome + button) that want to open a dropdown
    /// at an arbitrary screen position rather than anchored to a toolbar button rect.
    /// When `Some`, `render_dropdown()` uses this position instead of looking up
    /// the button rect from toolbar results.  Cleared when the dropdown closes.
    pub open_dropdown_position: Option<(f64, f64)>,
    /// While the user is editing a chart-type param inline-input (range_bar /
    /// volume_bar / footprint), this holds the chart_type whose row owns the
    /// focused TextField. Cleared when the field blurs / commits.
    ///
    /// Used by the Commit handler to route the value to the right
    /// `ChartOutEvent::UpdateChartTypeParam` / `UpdateChartTypeInterval`.
    pub active_param_chart_type: Option<String>,
}

impl Default for ChartToolbarState {
    fn default() -> Self {
        Self {
            active_tool_id: None,
            hovered_left_toolbar_id: None,
            hovered_top_toolbar_id: None,
            hovered_right_toolbar_id: None,
            active_right_toolbar_id: None,
            hovered_bottom_toolbar_id: None,
            open_dropdown_id: None,
            hovered_dropdown_item: None,
            primed_id: Some("cursor_tools".to_string()),
            magnet_enabled: false,
            magnet_strong: false,
            drawings_locked: false,
            drawings_visible: true, // Drawings visible by default
            quick_select_memory: {
                let mut m = std::collections::HashMap::new();
                m.insert("cursor_tools".to_string(), ("crosshair".to_string(), "crosshair".to_string()));
                m.insert("line_tools".to_string(), ("trend_line".to_string(), "trend_line".to_string()));
                m.insert("fib_tools".to_string(), ("fib_retracement".to_string(), "fib_retracement".to_string()));
                m.insert("pattern_tools".to_string(), ("xabcd_pattern".to_string(), "xabcd_pattern".to_string()));
                m.insert("smart_money_tools".to_string(), ("order_block".to_string(), "order_block".to_string()));
                m.insert("order_flow_tools".to_string(), ("imbalance_zone".to_string(), "imbalance_zone".to_string()));
                m.insert("brush_tools".to_string(), ("brush".to_string(), "brush".to_string()));
                m.insert("annotation_tools".to_string(), ("text".to_string(), "text".to_string()));
                m.insert("projection_tools".to_string(), ("long_position".to_string(), "long_position".to_string()));
                m
            },
            open_submenu_id: None,
            hovered_submenu_item: None,
            last_magnet_click_time: None,
            floating_inline_bar: FloatingInlineBar::default(),
            hovered_inline_id: None,
            open_inline_style_dropdown: false,
            open_inline_width_dropdown: false,
            open_inline_wave_dropdown: false,
            pending_wave_slot: None,
            open_inline_wave_next_dropdown: false,
            hovered_inline_dropdown_item: None,
            top_scroll_offset: 0.0,
            bottom_scroll_offset: 0.0,
            left_scroll_offset: 0.0,
            right_scroll_offset: 0.0,
            open_dropdown_position: None,
            active_param_chart_type: None,
        }
    }
}

impl ChartToolbarState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a toolbar item is the active tool
    pub fn is_active(&self, id: &str) -> bool {
        if self.primed_id.as_deref() == Some(id) { return true; }
        if self.open_dropdown_id.as_deref() == Some(id) { return true; }
        if self.active_tool_id.as_deref() == Some(id) { return true; }
        if let Some(ref active_tool) = self.active_tool_id {
            if let Some((tool_id, _icon)) = self.quick_select_memory.get(id) {
                if tool_id == active_tool { return true; }
            }
        }
        false
    }

    /// Check if a toggle is on
    pub fn is_toggled(&self, id: &str) -> bool {
        match id {
            // Magnet dropdown: show as active when any magnet mode is enabled
            "magnet" => self.magnet_enabled,
            "lock" => self.drawings_locked,
            "eye" => !self.drawings_visible, // eye toggled = drawings hidden
            _ => false,
        }
    }

    /// Toggle a boolean state
    pub fn toggle(&mut self, id: &str) {
        match id {
            "lock" => self.drawings_locked = !self.drawings_locked,
            "eye" => self.drawings_visible = !self.drawings_visible,
            _ => {}
        }
    }

    /// Toggle magnet ON/OFF and update the crosshair mode.
    ///
    /// Toggle magnet ON/OFF, preserving the last selected mode (Weak/Strong).
    ///
    /// - If magnet is ON: turns OFF (Normal).
    /// - If magnet is OFF: restores the last selected mode (Strong if magnet_strong, else Weak OHLC).
    pub fn toggle_magnet(&mut self, crosshair: &mut Crosshair) {
        if crosshair.is_magnet() {
            // Turn OFF
            crosshair.set_magnet_mode(crate::chart::types::crosshair::CrosshairMode::Normal);
            self.magnet_enabled = false;
        } else {
            // Turn ON — restore last selected mode
            let mode = if self.magnet_strong {
                crate::chart::types::crosshair::CrosshairMode::Magnet
            } else {
                crate::chart::types::crosshair::CrosshairMode::MagnetOHLC
            };
            crosshair.set_magnet_mode(mode);
            self.magnet_enabled = true;
        }
        eprintln!("[ChartToolbar] magnet toggled: enabled={}, strong={}", self.magnet_enabled, self.magnet_strong);
    }

    /// Get the quick-select icon override for a dropdown
    pub fn quick_select_icon(&self, dropdown_id: &str) -> Option<&str> {
        self.quick_select_memory.get(dropdown_id).map(|(_, icon)| icon.as_str())
    }

    /// Set quick-select memory for a dropdown
    pub fn set_quick_select(&mut self, dropdown_id: &str, tool_id: &str, icon: &str) {
        self.quick_select_memory.insert(
            dropdown_id.to_string(),
            (tool_id.to_string(), icon.to_string()),
        );
    }

    /// Handle a click on a quick-select button.
    /// Returns true if tool was activated (first click), false if dropdown should open (second click / no memory).
    pub fn handle_quick_select_click(&mut self, dropdown_id: &str) -> bool {
        if self.primed_id.as_deref() == Some(dropdown_id) {
            // Second click on already-primed dropdown -> open it
            self.primed_id = None;
            false
        } else if let Some((tool_id, _)) = self.quick_select_memory.get(dropdown_id).cloned() {
            // First click with memory -> activate last tool
            self.active_tool_id = Some(tool_id);
            self.primed_id = Some(dropdown_id.to_string());
            true
        } else {
            // No memory -> open dropdown
            false
        }
    }

    /// Select a tool from dropdown and update quick-select memory
    pub fn select_tool(&mut self, dropdown_id: &str, tool_id: &str, icon: &str) {
        self.active_tool_id = Some(tool_id.to_string());
        self.set_quick_select(dropdown_id, tool_id, icon);
        self.open_dropdown_id = None;
        self.primed_id = Some(dropdown_id.to_string());
    }

    /// Deselect current tool (back to cursor)
    pub fn deselect_tool(&mut self) {
        self.active_tool_id = None;
        self.primed_id = None;
    }

    /// Handle a click on a toolbar item, updating internal state.
    ///
    /// For quick-select dropdowns, activates the remembered tool (or defers to dropdown).
    /// For toggles (magnet, lock, eye), flips the boolean state.
    /// Returns the activated tool id if a tool was selected, otherwise `None`.
    pub fn handle_click(&mut self, item_id: &str) -> Option<String> {
        match item_id {
            "lock" | "eye" => {
                self.toggle(item_id);
                None
            }
            // Magnet is an icon_button: single-click toggles ON/OFF.
            // Double-click opens dropdown — handled in handle_toolbar_click_with_chart.
            "magnet" => None,
            "cursor_tools" | "line_tools" | "fib_tools" | "pattern_tools"
            | "smart_money_tools" | "order_flow_tools"
            | "brush_tools" | "annotation_tools" | "projection_tools"
            | "delete_tools" => {
                // Toggle: if this dropdown is already open, close it on re-click.
                if self.open_dropdown_id.as_deref() == Some(item_id) {
                    self.open_dropdown_id = None;
                    None
                } else if self.handle_quick_select_click(item_id) {
                    self.active_tool_id.clone()
                } else {
                    // Open dropdown — handled by caller
                    self.open_dropdown_id = Some(item_id.to_string());
                    None
                }
            }
            _ => {
                // Control strip items or other buttons — no state change here
                None
            }
        }
    }

    /// Get icon for a toggle button based on state
    pub fn toggle_icon(&self, id: &str) -> &str {
        match id {
            "lock" => if self.drawings_locked { "Lock" } else { "Unlock" },
            "eye" => if self.drawings_visible { "Eye" } else { "EyeOff" },
            _ => "",
        }
    }

    /// Handle a toolbar click, executing chart-internal actions and returning app-level events.
    ///
    /// Returns `vec![ChartOutEvent::Consumed]` when the action was fully handled internally,
    /// or specific events for actions that need app-level handling (sidebar toggles, modals, etc.).
    ///
    /// # Parameters
    /// - `item_id`: the toolbar item that was clicked
    /// - `crosshair`: mutable reference to the chart's crosshair (for magnet toggle)
    /// - `drawing_manager`: the active bubble's drawing session (for lock/visible/tool)
    pub fn handle_toolbar_click_with_chart(
        &mut self,
        item_id: &str,
        crosshair: &mut Crosshair,
        drawing_manager: &mut impl DrawingSession,
    ) -> Vec<ChartOutEvent> {
        // Close any open dropdown when clicking a different toolbar button.
        if self.open_dropdown_id.as_deref() != Some(item_id) && self.open_dropdown_id.is_some() {
            self.open_dropdown_id = None;
        }

        match item_id {
            // === Magnet — icon_button with double-click dropdown ===
            // Single-click: toggle magnet ON/OFF.
            // Double-click (within 300ms): open dropdown to select magnet strength,
            // WITHOUT toggling (undo the first-click toggle so state stays consistent).
            "magnet" => {
                let now = Instant::now();
                let is_double_click = self.last_magnet_click_time
                    .map(|t| now.duration_since(t).as_millis() < 300)
                    .unwrap_or(false);

                if is_double_click {
                    // Double-click: undo the toggle from the first click, then open dropdown.
                    // This ensures the magnet state is not changed when the dropdown opens.
                    self.toggle_magnet(crosshair); // undo first-click toggle
                    self.last_magnet_click_time = None; // reset so next single-click is fresh
                    eprintln!("[ChartToolbar] magnet double-click — open dropdown");
                    self.open_dropdown_id = Some("magnet".to_string());
                } else {
                    // First click: record time AND toggle — if no second click arrives
                    // within 300ms this acts as the single-click toggle.
                    self.last_magnet_click_time = Some(now);
                    self.toggle_magnet(crosshair);
                }
                vec![ChartOutEvent::Consumed]
            }
            "lock" => {
                drawing_manager.toggle_lock();
                self.toggle("lock");
                eprintln!("[ChartToolbar] lock toggled: {}", self.drawings_locked);
                vec![ChartOutEvent::Consumed]
            }
            "eye" => {
                drawing_manager.toggle_visible();
                self.toggle("eye");
                eprintln!("[ChartToolbar] visibility toggled: {}", self.drawings_visible);
                vec![ChartOutEvent::Consumed]
            }

            // === Undo / Redo ===
            "undo" => {
                eprintln!("[ChartToolbar] undo");
                vec![ChartOutEvent::Undo]
            }
            "redo" => {
                eprintln!("[ChartToolbar] redo");
                vec![ChartOutEvent::Redo]
            }

            // === Screenshot ===
            "screenshot" => {
                eprintln!("[ChartToolbar] screenshot");
                vec![ChartOutEvent::Screenshot]
            }

            // === Chart settings — app-level modal ===
            "chart_settings" => {
                eprintln!("[ChartToolbar] open chart_settings");
                vec![ChartOutEvent::Consumed]
            }

            // === Quick-select drawing tool dropdowns ===
            "cursor_tools" | "line_tools" | "fib_tools" | "pattern_tools"
            | "smart_money_tools" | "order_flow_tools"
            | "brush_tools" | "annotation_tools" | "projection_tools"
            | "delete_tools" => {
                // Toggle: if this dropdown is already open, close it on re-click.
                if self.open_dropdown_id.as_deref() == Some(item_id) {
                    eprintln!("[ChartToolbar] close dropdown (toggle): {}", item_id);
                    self.open_dropdown_id = None;
                } else if self.handle_quick_select_click(item_id) {
                    // Tool activated via quick-select — apply to drawing manager
                    if let Some(ref tool_id) = self.active_tool_id.clone() {
                        eprintln!("[ChartToolbar] tool activated (primed): {} -> {}", item_id, tool_id);
                        drawing_manager.set_tool(Some(tool_id.as_str()));
                    }
                } else {
                    // Open dropdown — clear previous tool/primed state so only
                    // the dropdown button shows as active (via open_dropdown_id).
                    eprintln!("[ChartToolbar] open dropdown: {}", item_id);
                    self.active_tool_id = None;
                    self.primed_id = None;
                    self.open_dropdown_id = Some(item_id.to_string());
                    drawing_manager.set_tool(None);
                }
                vec![ChartOutEvent::Consumed]
            }

            // === Dropdown toggles (open/close the dropdown menu) ===
            "timeframe_selector" | "chart_type_selector" | "icon_tools"
            | "settings_menu" | "layout_menu" | "workspace_menu" | "presets_menu" => {
                if self.open_dropdown_id.as_deref() == Some(item_id) {
                    eprintln!("[ChartToolbar] close dropdown: {}", item_id);
                    self.open_dropdown_id = None;
                } else {
                    eprintln!("[ChartToolbar] open dropdown: {}", item_id);
                    self.open_dropdown_id = Some(item_id.to_string());
                }
                vec![ChartOutEvent::Consumed]
            }

            // === App-level: symbol / compare search ===
            // Whole button (text + magnifier icon, no separate hit zone —
            // symbol-search-compare-inner arc, Wave 4 rework, owner spec
            // 2026-07-16: any click on `symbol_selector` opens the fast-
            // search with the active symbol already seeded and editable in
            // place) routes through the same handler.
            "symbol_selector" => vec![ChartOutEvent::OpenSymbolSearch],
            "compare" => vec![ChartOutEvent::OpenCompareSearch],

            // === App-level: expand/suppress panel ===
            "expand" | "expand_chart" => vec![ChartOutEvent::ExpandPanel],
            "watchlist" => vec![ChartOutEvent::ToggleWatchlist],
            "screener" => vec![ChartOutEvent::ToggleScreener],
            "alerts" => vec![ChartOutEvent::ToggleAlerts],
            "layers" | "object_tree" => vec![ChartOutEvent::ToggleObjectTree],
            "signals" => vec![ChartOutEvent::ToggleSignals],
            "monitoring" => vec![ChartOutEvent::ToggleMonitoring],
            "agents" => vec![ChartOutEvent::ToggleAgents],
            "slot_trading"    => vec![ChartOutEvent::ToggleSlot(0)],
            "slot_dom"        => vec![ChartOutEvent::ToggleSlot(1)],
            "slot_tape"       => vec![ChartOutEvent::ToggleSlot(2)],
            "slot_l2"         => vec![ChartOutEvent::ToggleSlot(3)],
            "slot_big_trades" => vec![ChartOutEvent::ToggleSlot(4)],
            "slot_liq"        => vec![ChartOutEvent::ToggleSlot(5)],
            "trading" => vec![ChartOutEvent::ToggleTradingPanel],
            "positions" => vec![ChartOutEvent::TogglePositions],
            "theme_settings" => vec![ChartOutEvent::ToggleThemeSettings],
            "main_menu" => vec![ChartOutEvent::ToggleLeftPanel],

            // === Indicators modal ===
            "indicators" | "indicators_panel" => vec![ChartOutEvent::ToggleIndicators],

            // === Unknown — no-op ===
            _ => vec![],
        }
    }

    /// Handle a dropdown item selection, executing chart-internal actions and returning app-level events.
    ///
    /// # Parameters
    /// - `dropdown_id`: the dropdown that the item belongs to
    /// - `item_id`: the selected item
    /// - `crosshair`: mutable reference to the chart's crosshair
    /// - `drawing_manager`: the active bubble's drawing session
    pub fn handle_dropdown_select_with_chart(
        &mut self,
        dropdown_id: &str,
        item_id: &str,
        crosshair: &mut Crosshair,
        drawing_manager: &mut impl DrawingSession,
        autosave_enabled: bool,
    ) -> Vec<ChartOutEvent> {
        // Close any open dropdown regardless of which path we take
        self.open_dropdown_id = None;
        self.open_dropdown_position = None;

        match dropdown_id {
            // === Magnet mode selector (opened via double-click on magnet button) ===
            // "No Magnet" is not in this dropdown — use single-click toggle to turn OFF.
            "magnet" => {
                eprintln!("[ChartToolbar] magnet mode: {}", item_id);
                match item_id {
                    "magnet_ohlc" => {
                        crosshair.mode = crate::chart::types::crosshair::CrosshairMode::MagnetOHLC;
                        self.magnet_enabled = true;
                        self.magnet_strong = false;
                    }
                    "magnet_strong" => {
                        crosshair.mode = crate::chart::types::crosshair::CrosshairMode::Magnet;
                        self.magnet_enabled = true;
                        self.magnet_strong = true;
                    }
                    _ => {}
                }
                vec![ChartOutEvent::Consumed]
            }

            // === Drawing tool dropdowns — select and apply tool ===
            "cursor_tools" | "line_tools" | "fib_tools" | "pattern_tools"
            | "smart_money_tools" | "order_flow_tools"
            | "brush_tools" | "annotation_tools" | "projection_tools" => {
                eprintln!("[ChartToolbar] tool selected: {} -> {}", dropdown_id, item_id);
                self.select_tool(dropdown_id, item_id, item_id);
                drawing_manager.set_tool(Some(item_id));
                vec![ChartOutEvent::Consumed]
            }

            // === Delete actions — handled by drawing manager ===
            "delete_tools" => {
                eprintln!("[ChartToolbar] delete action: {}", item_id);
                match item_id {
                    "delete_selected" => {
                        drawing_manager.delete_selected();
                        vec![ChartOutEvent::Consumed]
                    }
                    "delete_all" => {
                        drawing_manager.delete_all();
                        vec![ChartOutEvent::Consumed]
                    }
                    _ => vec![],
                }
            }

            // === Icon / emoji tools ===
            "icon_tools" => {
                // Check if the clicked item is a submenu trigger
                if item_id == "emoji_submenu" {
                    // Toggle the submenu open/close — keep the dropdown open
                    if self.open_submenu_id.as_deref() == Some("emoji_submenu") {
                        self.open_submenu_id = None;
                    } else {
                        self.open_submenu_id = Some("emoji_submenu".to_string());
                    }
                    // Re-open the dropdown (it was closed at the top of this function)
                    self.open_dropdown_id = Some("icon_tools".to_string());
                    eprintln!("[ChartToolbar] submenu toggled: emoji_submenu -> {:?}", self.open_submenu_id);
                    vec![ChartOutEvent::Consumed]
                } else {
                    // Regular item or emoji grid item — select it as the active tool
                    eprintln!("[ChartToolbar] icon tool: {} -> {}", dropdown_id, item_id);
                    self.open_submenu_id = None;
                    self.active_tool_id = Some(item_id.to_string());
                    drawing_manager.set_tool(Some(item_id));
                    vec![ChartOutEvent::Consumed]
                }
            }

            // === Settings menu — submenu triggers and action items ===
            "settings_menu" if matches!(
                item_id,
                "grid_submenu" | "crosshair_submenu" | "legend_submenu"
                | "tooltip_submenu" | "watermark_submenu" | "theme_submenu"
                | "ui_style_submenu"
            ) => {
                // Toggle the submenu flyout open/close — keep the parent dropdown open
                if self.open_submenu_id.as_deref() == Some(item_id) {
                    self.open_submenu_id = None;
                } else {
                    self.open_submenu_id = Some(item_id.to_string());
                }
                self.open_dropdown_id = Some("settings_menu".to_string());
                eprintln!("[ChartToolbar] settings submenu toggled: {} -> {:?}", item_id, self.open_submenu_id);
                vec![ChartOutEvent::Consumed]
            }

            // === Chart type selector ===
            // Every item (incl. range_bar/volume_bar/footprint transforms) is a
            // chart type. Tick lives in the TF selector, not here.
            "chart_type_selector" => {
                eprintln!("[ChartToolbar] chart type selector: {}", item_id);
                vec![ChartOutEvent::ChangeChartType { chart_type: item_id.to_string() }]
            }

            // === Timeframe selector — emit event for app to reload data ===
            //
            // `ResolutionKind::Param` dropdown items carry the
            // `RESOLUTION_PARAM_ITEM_PREFIX` id prefix (owner doctrine
            // 2026-07-08: the TF button is the universal resolution picker)
            // — the id itself encodes the numeric value, so this dispatches
            // straight to `UpdateChartTypeParam` without needing to know the
            // active chart_type. Plain `tf_*` ids (Time/Interval) keep the
            // existing `ChangeTimeframe` flow unchanged.
            "timeframe_selector" => {
                if item_id == crate::chart_type::RESOLUTION_PARAM_ATR14_ITEM_ID {
                    eprintln!("[ChartToolbar] resolution param: ATR(14)");
                    vec![ChartOutEvent::SetChartTypeParamAtr14]
                } else if let Some(value_str) = item_id.strip_prefix(crate::chart_type::RESOLUTION_PARAM_ITEM_PREFIX) {
                    eprintln!("[ChartToolbar] resolution param: {}", value_str);
                    match value_str.parse::<f64>() {
                        Ok(value) => vec![ChartOutEvent::UpdateChartTypeParam { value }],
                        Err(_) => vec![ChartOutEvent::Consumed],
                    }
                } else if let Some(name) = item_id.strip_prefix(crate::chart_type::GRANULARITY_INTERVAL_ID_PREFIX) {
                    // Footprint bucket-interval CUSTOM/FAVORITE row
                    // (per-family scoping arc 2026-07-16) — normalize to the
                    // `tf_{name}` convention `ChangeTimeframe`'s handler
                    // expects (same `Timeframe::parse`-shaped name, just a
                    // distinct dropdown-row id prefix so it never collides
                    // with a kline `Time` custom of the same literal name).
                    eprintln!("[ChartToolbar] footprint interval: {}", name);
                    vec![ChartOutEvent::ChangeTimeframe { timeframe_id: format!("tf_{}", name) }]
                } else {
                    eprintln!("[ChartToolbar] timeframe: {}", item_id);
                    vec![ChartOutEvent::ChangeTimeframe { timeframe_id: item_id.to_string() }]
                }
            }

            // === Layout menu — internal split / expand ===
            "layout_menu" => {
                eprintln!("[ChartToolbar] layout action: {}", item_id);
                // Toggle items: keep the dropdown open so the user can see the toggle state change
                if item_id == "sync_symbol" || item_id == "sync_timeframe" || item_id == "sync_crosshair" || item_id == "sync_viewport" || item_id == "sync_drawings" || item_id == "sync_indicators" || item_id == "split_untagged" {
                    self.open_dropdown_id = Some("layout_menu".to_string());
                }
                match item_id {
                    "layout_single"       => vec![ChartOutEvent::InternalSetLayoutSingle],
                    "layout_split_h"      => vec![ChartOutEvent::InternalSplitHorizontal],
                    "layout_split_v"      => vec![ChartOutEvent::InternalSplitVertical],
                    "layout_2left_1right" => vec![ChartOutEvent::InternalSplit2Left1Right],
                    "layout_1left_2right" => vec![ChartOutEvent::InternalSplit1Left2Right],
                    "layout_2top_1bottom" => vec![ChartOutEvent::InternalSplit2Top1Bottom],
                    "layout_1top_2bottom" => vec![ChartOutEvent::InternalSplit1Top2Bottom],
                    "layout_3columns"     => vec![ChartOutEvent::InternalSplit3Columns],
                    "layout_3rows"        => vec![ChartOutEvent::InternalSplit3Rows],
                    "layout_grid_2x2"     => vec![ChartOutEvent::InternalSplitGrid2x2],
                    "layout_1big_3small"  => vec![ChartOutEvent::InternalSplit1Big3Small],
                    "panel_close"         => vec![ChartOutEvent::InternalClosePanel],
                    "panel_reset_sizes"   => vec![ChartOutEvent::InternalResetSizes],
                    "split_untagged"      => vec![ChartOutEvent::InternalToggleSplitUntagged],
                    "sync_symbol"         => vec![ChartOutEvent::InternalToggleSyncSymbol],
                    "sync_timeframe"      => vec![ChartOutEvent::InternalToggleSyncTimeframe],
                    "sync_crosshair"      => vec![ChartOutEvent::InternalToggleSyncCrosshair],
                    "sync_viewport"       => vec![ChartOutEvent::InternalToggleSyncViewport],
                    "sync_drawings"       => vec![ChartOutEvent::InternalToggleSyncDrawings],
                    "sync_indicators"     => vec![ChartOutEvent::InternalToggleSyncIndicators],
                    _ => vec![ChartOutEvent::Consumed],
                }
            }

            // === Presets menu dropdown ===
            "presets_menu" => {
                eprintln!("[ChartToolbar] preset action: {}", item_id);
                if item_id == "preset_save_as" {
                    vec![ChartOutEvent::OpenPresetSaveAs]
                } else if item_id.starts_with("preset_load:") {
                    let id = item_id.strip_prefix("preset_load:").unwrap_or("").to_string();
                    // Top-toolbar dropdown = "act on the active chart slot"
                    // per owner UX rule.  Use LoadPresetInActiveSlot so the
                    // active tab's id in `open_tabs` is replaced with the
                    // loaded preset, NOT appended next to it.
                    vec![ChartOutEvent::LoadPresetInActiveSlot { id }]
                } else if item_id == "preset_rename" {
                    vec![ChartOutEvent::OpenPresetRename]
                } else if item_id == "preset_new_chart" {
                    vec![ChartOutEvent::NewChart]
                } else if item_id == "preset_open_chart" {
                    vec![ChartOutEvent::OpenChartBrowser]
                } else if item_id == "preset_save" {
                    // Safety net: autosave on means Save is disabled — do nothing
                    if autosave_enabled {
                        self.open_dropdown_id = Some("presets_menu".to_string());
                        vec![ChartOutEvent::Consumed]
                    } else {
                        vec![ChartOutEvent::SaveCurrentPreset]
                    }
                } else if item_id == "preset_autosave" {
                    // Keep the dropdown open so the user can see the toggle state change
                    self.open_dropdown_id = Some("presets_menu".to_string());
                    vec![ChartOutEvent::ToggleAutosave]
                } else {
                    vec![ChartOutEvent::Consumed]
                }
            }

            // === New-tab menu dropdown (opened by chrome "+" button) ===
            "new_tab_menu" => {
                eprintln!("[ChartToolbar] new_tab action: {}", item_id);
                if item_id == "new_tab:new_chart" {
                    vec![ChartOutEvent::NewChart]
                } else if item_id == "new_tab:browser" {
                    vec![ChartOutEvent::OpenChartBrowserInNewTab]
                } else if let Some(preset_id) = item_id.strip_prefix("new_tab:open:") {
                    vec![ChartOutEvent::OpenTab { id: preset_id.to_string() }]
                } else {
                    vec![ChartOutEvent::Consumed]
                }
            }

            // === Settings menu dropdown — full content matching terminal ===
            "settings_menu" => {
                eprintln!("[ChartToolbar] settings action: {}", item_id);
                match item_id {
                    // Chart settings modal
                    "chart_settings" => vec![ChartOutEvent::OpenChartSettings],

                    // Grid
                    "grid_toggle"   => vec![ChartOutEvent::ToggleGrid],
                    "grid_vert"     => vec![ChartOutEvent::ToggleGridVertical],
                    "grid_horz"     => vec![ChartOutEvent::ToggleGridHorizontal],

                    // Volume Profile overlay (C10)

                    // OI / Funding (C15)

                    // Crosshair
                    "crosshair_toggle" => vec![ChartOutEvent::ToggleCrosshair],
                    "ch_normal" => {
                        crosshair.mode = crate::chart::types::crosshair::CrosshairMode::Normal;
                        self.magnet_enabled = false;
                        self.magnet_strong = false;
                        vec![ChartOutEvent::Consumed]
                    }
                    "ch_magnet" => {
                        crosshair.mode = crate::chart::types::crosshair::CrosshairMode::Magnet;
                        self.magnet_enabled = true;
                        self.magnet_strong = true;
                        vec![ChartOutEvent::Consumed]
                    }
                    "ch_magnet_ohlc" => {
                        crosshair.mode = crate::chart::types::crosshair::CrosshairMode::MagnetOHLC;
                        self.magnet_enabled = true;
                        self.magnet_strong = false;
                        vec![ChartOutEvent::Consumed]
                    }

                    // Legend
                    "legend_toggle"   => vec![ChartOutEvent::ToggleLegend],
                    "legend_ohlc"     => vec![ChartOutEvent::ToggleLegendOHLC],
                    "legend_change"   => vec![ChartOutEvent::ToggleLegendChange],
                    "legend_percent"  => vec![ChartOutEvent::ToggleLegendPercent],

                    // Tooltip
                    "tooltip_toggle" => vec![ChartOutEvent::ToggleTooltip],
                    "tooltip_follow" => vec![ChartOutEvent::ToggleTooltipFollow],

                    // Theme
                    "theme_dark"           => vec![ChartOutEvent::SetTheme("dark")],
                    "theme_light"          => vec![ChartOutEvent::SetTheme("light")],
                    "theme_high_contrast"       => vec![ChartOutEvent::SetTheme("high_contrast")],
                    "theme_high_contrast_mono"  => vec![ChartOutEvent::SetTheme("high_contrast_mono")],
                    "theme_mascot"              => vec![ChartOutEvent::SetTheme("mascot")],

                    // UI Style
                    "style_solid"              => vec![ChartOutEvent::SetStyle("solid")],
                    "style_glass"              => vec![ChartOutEvent::SetStyle("glass")],
                    "style_frosted_glass_flat" => vec![ChartOutEvent::SetStyle("frosted_glass_flat")],
                    "style_frosted_glass_3d"   => vec![ChartOutEvent::SetStyle("frosted_glass_3d")],
                    "style_liquid_glass_flat"  => vec![ChartOutEvent::SetStyle("liquid_glass_flat")],
                    "style_liquid_glass_3d"    => vec![ChartOutEvent::SetStyle("liquid_glass_3d")],

                    // Watermark
                    "watermark_toggle"       => vec![ChartOutEvent::ToggleWatermark],
                    "watermark_text_seeyou"  => vec![ChartOutEvent::SetWatermarkText("SEE YOU...")],
                    "watermark_text_demo"    => vec![ChartOutEvent::SetWatermarkText("DEMO")],
                    "watermark_text_paper"   => vec![ChartOutEvent::SetWatermarkText("PAPER TRADING")],
                    "watermark_text_live"    => vec![ChartOutEvent::SetWatermarkText("LIVE")],
                    "watermark_pos_center"   => vec![ChartOutEvent::SetWatermarkPosition("center")],
                    "watermark_pos_bl"       => vec![ChartOutEvent::SetWatermarkPosition("bottom_left")],
                    "watermark_pos_br"       => vec![ChartOutEvent::SetWatermarkPosition("bottom_right")],

                    _ => vec![ChartOutEvent::Consumed],
                }
            }

            // === Unknown ===
            _ => vec![],
        }
    }

    /// Handle a chevron click for a toolbar, adjusting its scroll offset.
    ///
    /// `toolbar` identifies which toolbar ("top", "bottom", "left", "right").
    /// `forward` is true when scrolling forward (right/down chevron clicked),
    /// false when scrolling backward (left/up chevron clicked).
    /// `max_scroll` is the maximum valid scroll offset returned from the last render.
    ///
    /// The scroll step is approximately one `item_size` worth of items (~100px).
    pub fn handle_chevron_click(&mut self, toolbar: &str, forward: bool, max_scroll: f64) {
        const SCROLL_STEP: f64 = 100.0;
        // Snap threshold: if remaining distance is less than this, jump to the edge
        // so we never end up showing half-buttons at the boundaries.
        const SNAP_EDGE: f64 = 40.0;
        let offset = match toolbar {
            "top"    => &mut self.top_scroll_offset,
            "bottom" => &mut self.bottom_scroll_offset,
            "left"   => &mut self.left_scroll_offset,
            "right"  => &mut self.right_scroll_offset,
            _ => return,
        };
        if forward {
            let new_val = *offset + SCROLL_STEP;
            // If close to max, snap to max so last items are fully visible
            *offset = if max_scroll - new_val < SNAP_EDGE { max_scroll } else { new_val.min(max_scroll) };
        } else {
            let new_val = *offset - SCROLL_STEP;
            // If close to 0, snap to 0 so first items are fully visible
            *offset = if new_val < SNAP_EDGE { 0.0 } else { new_val.max(0.0) };
        }
    }
}
