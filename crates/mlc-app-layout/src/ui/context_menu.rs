//! Context menu state and builder utilities
//!
//! Provides types for managing right-click context menus on chart elements
//! such as primitives, indicators, and the chart background.

use mlc_core::LeafId;
use mlc_core::drawing::primitives_v2::elliott::WaveSlot;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;

// =============================================================================
// Context Menu Target
// =============================================================================

/// What the context menu was opened on
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ContextMenuTarget {
    /// No specific target (empty area)
    #[default]
    None,
    /// A drawing primitive by stable id
    Primitive(u64),
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

    /// Get primitive id if targeting a primitive
    pub fn primitive_id(&self) -> Option<u64> {
        match self.target {
            ContextMenuTarget::Primitive(id) => Some(id),
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
    use mlc_core::i18n::{MenuKey, current_language};
    let lang = current_language();
    let lock_label = if is_locked { MenuKey::Unlock.get(lang) } else { MenuKey::Lock.get(lang) };
    let lock_icon = if is_locked { "unlock" } else { "lock" };
    let visibility_label = if is_visible { MenuKey::HideItem.get(lang) } else { MenuKey::Show.get(lang) };
    let visibility_icon = if is_visible { "eye_off" } else { "eye" };

    vec![
        ContextMenuItemState::action_with_icon("settings", "settings", MenuKey::OpenSettings.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("copy", "clone", MenuKey::Clone.get(lang)),
        ContextMenuItemState::action_with_icon(lock_icon, "toggle_lock", lock_label),
        ContextMenuItemState::action_with_icon(visibility_icon, "toggle_visibility", visibility_label),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("arrow_up", "bring_to_front", MenuKey::BringToFront.get(lang)),
        ContextMenuItemState::action_with_icon("arrow_down", "send_to_back", MenuKey::SendToBack.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::danger_with_icon("delete", "delete", MenuKey::Delete.get(lang)),
    ]
}

// =============================================================================
// Wave-gesture menus (E5, second control surface — owner ask 2026-08-13)
// =============================================================================
//
// `ChartApp::try_wave_gesture` opens one of these two popups instead of the
// generic primitive menu above, on a hit against an Elliott primitive's own
// wave segment or terminal point (`mlc_core::state::gestures`'s own doc has
// the full "trigger deliberately not hardcoded" rationale). Both are built
// from `SelectedPrimitiveConfig` alone — the same E2/E3 read the inline
// config toolbar already uses (`selected_config`) — never a second source
// of truth for which kinds/successors are legal.

/// Action id for the wave menu's own disabled header row — see
/// [`build_wave_menu`]. Never dispatched: `on_context_menu_action` only
/// ever sees an ENABLED item's action id (the click dispatcher refuses a
/// disabled item before it reaches the handler), so this string exists only
/// to give the row a stable, harmless id.
const WAVE_MENU_LABEL_ACTION: &str = "wave_menu:label";

/// The "what this whole segment currently IS, and what it may become"
/// header both [`build_wave_menu`] and [`build_successor_menu`] open with
/// (owner report 2026-08-14: "не могу прямо в процессе разметки переменить
/// глобальное состояние отрезка" — the structure's own type must be
/// changeable in place, not just what nests inside one of its waves or what
/// follows it). An inert row naming the structure (`cfg.name`, same
/// disabled-row convention [`WAVE_MENU_LABEL_ACTION`] already uses), then
/// one row per [`SelectedPrimitiveConfig::conversion_kinds`] entry —
/// action `"wave_menu:convert:{kind_id}"`, parsed back by
/// `on_context_menu_action` (`mlc-app`). `DrawingManager::
/// convert_primitive_kind` re-checks the grammar itself before acting, so a
/// stale action id is refused there, not just left off this list.
///
/// The row naming the structure's OWN current kind is DISABLED rather than
/// given a marker icon. Neither icon table this crate's own context-menu
/// renderer draws from (`layout::modals::context_menu::render_context_menu`'s
/// own narrow string→[`crate::ui::Icon`] match, nor `mlc-core`'s own
/// larger `state::icons` id table) carries a "this is the current
/// selection" glyph — no checkmark, no dot, nothing in that vocabulary
/// means "current" rather than "a category of thing". An icon id absent
/// from the renderer's own match silently draws no icon at all, which
/// would leave the current row looking like a broken lookup rather than a
/// marked one; disabling it is the same fail-visible convention
/// [`ContextMenuItemState::set_enabled`] already offers everywhere else in
/// this file, not a new mechanism invented for this one row.
fn build_conversion_block(cfg: &SelectedPrimitiveConfig) -> Vec<ContextMenuItemState> {
    use mlc_core::i18n::t_concept;

    let mut items = Vec::new();
    items.push(ContextMenuItemState::action(WAVE_MENU_LABEL_ACTION, cfg.name.clone()).set_enabled(false));
    for (kind_id, key, is_current) in &cfg.conversion_kinds {
        let item = ContextMenuItemState::action(format!("wave_menu:convert:{kind_id}"), t_concept(*key));
        items.push(if *is_current { item.set_enabled(false) } else { item });
    }
    items
}

/// Build the "subdivide this wave" popup (`ChartGesture::WaveMenu`) for the
/// wave `slot` the gesture resolved to on the already-selected primitive.
///
/// `child_id` is the caller's answer to "does `slot` already hold a
/// subdivision" — `mlc-backend`'s `Drawings::wave_child_id`. `None` renders
/// only the kind-picker rows (mirrors the
/// inline E2 dropdown's own step-1/step-2 rows, one flat list here since a
/// popup has no dropdown-within-dropdown step); `Some` additionally offers
/// to select or remove the existing child, since offering a SECOND
/// subdivision into an already-occupied slot would silently orphan the
/// first one.
///
/// Action ids: `"wave_menu:subdivide:{slot_id}:{kind_id}"`,
/// `"wave_menu:select_child:{child_id}"`,
/// `"wave_menu:delete_child:{child_id}"` — `on_context_menu_action`
/// (`mlc-app`) parses these back; see that function's own doc for why the
/// parent's own id is never embedded (it is already the context menu's
/// `ContextMenuTarget::Primitive` id).
///
/// Opens with [`build_conversion_block`] (own doc has the full "what this
/// whole segment currently IS" rationale) — the reading order is "what this
/// whole segment is" -> "what goes in the wave you clicked".
pub fn build_wave_menu(cfg: &SelectedPrimitiveConfig, slot: WaveSlot, child_id: Option<u64>) -> Vec<ContextMenuItemState> {
    use mlc_core::i18n::{t_concept, ConceptKey};

    let slot_id = slot.id();
    let mut items = build_conversion_block(cfg);
    items.push(ContextMenuItemState::separator());

    let wave_label = cfg.wave_slots.iter()
        .find(|(id, _)| id == slot_id)
        .map(|(_, label)| label.as_str())
        .unwrap_or(slot_id);
    items.push(ContextMenuItemState::action(WAVE_MENU_LABEL_ACTION, wave_label).set_enabled(false));

    if let Some((_, kinds)) = cfg.wave_slot_kinds.iter().find(|(id, _)| id == slot_id) {
        for (kind_id, key) in kinds {
            items.push(ContextMenuItemState::action(
                format!("wave_menu:subdivide:{slot_id}:{kind_id}"),
                t_concept(*key),
            ));
        }
    }

    if let Some(child_id) = child_id {
        items.push(ContextMenuItemState::separator());
        items.push(ContextMenuItemState::action(
            format!("wave_menu:select_child:{child_id}"),
            t_concept(ConceptKey::SelectSubdivisionLabel),
        ));
        items.push(ContextMenuItemState::danger(
            format!("wave_menu:delete_child:{child_id}"),
            t_concept(ConceptKey::RemoveSubdivisionLabel),
        ));
    }

    items
}

/// Build the "what continues this structure" popup
/// (`ChartGesture::SuccessorMenu`) for the already-selected primitive.
///
/// Action ids: `"wave_menu:successor:{type_id}"`, `"wave_menu:promote"` —
/// mirrors the inline E3 dropdown's own rows plus the E4 promote button,
/// one flat list instead of a dropdown-plus-separate-button.
///
/// Opens with [`build_conversion_block`] (own doc has the full "what this
/// whole segment currently IS" rationale), same as [`build_wave_menu`].
pub fn build_successor_menu(cfg: &SelectedPrimitiveConfig) -> Vec<ContextMenuItemState> {
    use mlc_core::i18n::{t_concept, ConceptKey};

    // No second name row: the conversion block above already opens with this
    // structure's own name, and printing it twice in one popup reads as a
    // rendering bug rather than as two sections.
    let mut items = build_conversion_block(cfg);
    items.push(ContextMenuItemState::separator());

    for (type_id, display_name) in &cfg.successor_kinds {
        items.push(ContextMenuItemState::action(
            format!("wave_menu:successor:{type_id}"),
            display_name.clone(),
        ));
    }

    if cfg.can_promote {
        items.push(ContextMenuItemState::separator());
        items.push(ContextMenuItemState::action("wave_menu:promote", t_concept(ConceptKey::PromoteChainLabel)));
    }

    items
}
