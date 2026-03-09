//! State types for the unified Tags & Tabs modal.
//!
//! The modal has two sections selectable via a left sidebar:
//! - **TABS** — reuses [`OverlaySettingsState`] entirely for panel-tree management.
//! - **TAGS** — manages sync groups with sub-tabs: Groups, Map, Details.

use crate::ui::modal_settings::OverlaySettingsState;
use crate::LeafId;

// =============================================================================
// TagsTabsSidebar
// =============================================================================

/// Which section is active in the left sidebar of the Tags & Tabs modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagsTabsSidebar {
    #[default]
    Tabs,
    Tags,
}

// =============================================================================
// TagsSubTab
// =============================================================================

/// Sub-tabs available within the TAGS section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagsSubTab {
    #[default]
    Groups,
    Map,
    Details,
}

impl TagsSubTab {
    /// Stable string identifier for this sub-tab.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Groups  => "groups",
            Self::Map     => "map",
            Self::Details => "details",
        }
    }

    /// Human-readable display label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Groups  => "Groups",
            Self::Map     => "Map",
            Self::Details => "Details",
        }
    }

    /// All available sub-tabs in display order.
    pub fn all() -> &'static [TagsSubTab] {
        &[Self::Groups, Self::Map, Self::Details]
    }
}

// =============================================================================
// TagsTabsState
// =============================================================================

/// Top-level state for the unified Tags & Tabs modal.
///
/// The modal is draggable and contains two sidebar sections:
/// - **TABS** delegates entirely to [`OverlaySettingsState`].
/// - **TAGS** has its own sub-tabs, group selection, hover tracking, and scroll.
#[derive(Clone, Debug)]
pub struct TagsTabsState {
    /// Whether the modal is currently visible.
    pub is_open: bool,
    /// Modal position (pixels from top-left of the window).
    ///
    /// `None` means the modal should be centered on first paint.
    pub position: Option<(f64, f64)>,
    /// Whether the title bar is currently being dragged.
    pub is_dragging: bool,
    /// Offset from the mouse cursor to the modal top-left corner, captured
    /// when the drag begins. Used to compute smooth repositioning.
    pub drag_offset: Option<(f64, f64)>,

    /// Which sidebar section is currently selected.
    pub sidebar: TagsTabsSidebar,

    // -------------------------------------------------------------------------
    // TABS section
    // -------------------------------------------------------------------------

    /// Full overlay/panel-tree manager state — reused verbatim.
    pub tabs_section: OverlaySettingsState,

    // -------------------------------------------------------------------------
    // TAGS section
    // -------------------------------------------------------------------------

    /// Active sub-tab within the TAGS section.
    pub tags_sub_tab: TagsSubTab,
    /// The `SyncGroupId.0` of the currently selected group, if any.
    pub selected_group_id: Option<u64>,
    /// Widget ID of the currently hovered list item (for hover feedback).
    pub hovered_item_id: Option<String>,
    /// `LeafId.0` of the leaf currently hovered on the minimap view.
    pub map_hovered_leaf: Option<u64>,
    /// Vertical scroll offset in pixels for the scrollable content area.
    pub scroll_offset: f64,
}

impl Default for TagsTabsState {
    fn default() -> Self {
        Self {
            is_open: false,
            position: None,
            is_dragging: false,
            drag_offset: None,
            sidebar: TagsTabsSidebar::default(),
            tabs_section: OverlaySettingsState::default(),
            tags_sub_tab: TagsSubTab::default(),
            selected_group_id: None,
            hovered_item_id: None,
            map_hovered_leaf: None,
            scroll_offset: 0.0,
        }
    }
}

impl TagsTabsState {
    /// Open the modal with the TABS sidebar active (default view).
    pub fn open(&mut self) {
        self.is_open = true;
        self.sidebar = TagsTabsSidebar::Tabs;
        self.hovered_item_id = None;
    }

    /// Open the modal for a specific leaf, activating the TABS section and
    /// forwarding the leaf to [`OverlaySettingsState::open_for_leaf`].
    pub fn open_for_leaf(&mut self, leaf_id: LeafId) {
        self.is_open = true;
        self.sidebar = TagsTabsSidebar::Tabs;
        self.hovered_item_id = None;
        self.tabs_section.open_for_leaf(leaf_id);
    }

    /// Close the modal and reset transient drag state.
    ///
    /// Position is intentionally preserved so the modal reopens in the same spot.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.hovered_item_id = None;
    }

    /// Begin dragging the title bar.
    ///
    /// `mouse_x/mouse_y` — current mouse position in window coordinates.
    /// `modal_x/modal_y` — current top-left corner of the modal.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update the modal position while a drag is in progress.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End the current drag.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}
