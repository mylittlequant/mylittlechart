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
    /// A DM message bubble, by its own timeline block index — matches
    /// [`crate::ui::selectable_text::LineHit::block`], the messenger UI-2b
    /// right-click menu (Copy / Select all / Reply — cloud builds only,
    /// `messenger_overlay.rs`'s own right-click dispatch in `mlc-app`).
    #[cfg(feature = "cloud")]
    DmMessage(i64),
    /// A DM chat-list row, by its own room id string
    /// (`mlc_messenger_model::RoomId::as_str()`) — the messenger UI-2b
    /// right-click menu (Mark as read, when the provider's own
    /// `Capabilities::receipts` supports it).
    #[cfg(feature = "cloud")]
    DmChatRow(String),
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
    /// `Some(checked)` draws a checkbox glyph in the icon slot instead of
    /// `icon` — the messenger concept's `.cb`/`.cb.on` rows (e.g. an
    /// "Add to folder ▸" submenu's per-folder checklist). `None` draws no
    /// checkbox (harvest-ready menu/checkbox pass, community-messenger
    /// arc).
    pub checkbox: Option<bool>,
    /// A non-empty submenu — a right-chevron draws at the row's own right
    /// edge whenever this is non-empty, regardless of [`Self::
    /// submenu_open`] (harvest-ready menu/submenu pass, community-
    /// messenger arc).
    pub submenu: Vec<ContextMenuItemState>,
    /// Whether THIS item's own `submenu` is currently drawn open —
    /// [`render_context_menu`] only READS this (draws the nested panel
    /// when it is `true` and `submenu` is non-empty); setting it on
    /// hover/click is a later pass's own dispatch wiring, out of scope
    /// here.
    pub submenu_open: bool,
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
            checkbox: None,
            submenu: Vec::new(),
            submenu_open: false,
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
            checkbox: None,
            submenu: Vec::new(),
            submenu_open: false,
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
            checkbox: None,
            submenu: Vec::new(),
            submenu_open: false,
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
            checkbox: None,
            submenu: Vec::new(),
            submenu_open: false,
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
            checkbox: None,
            submenu: Vec::new(),
            submenu_open: false,
        }
    }

    /// Set enabled state
    pub fn set_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw a checkbox in the icon slot instead of `icon` — the concept's
    /// `.cb`/`.cb.on`.
    pub fn with_checkbox(mut self, checked: bool) -> Self {
        self.checkbox = Some(checked);
        self
    }

    /// Attach a submenu (a right-chevron draws at the row's own right
    /// edge) — the concept's `Mute ▸`/`Add to folder ▸` rows.
    pub fn with_submenu(mut self, items: Vec<ContextMenuItemState>) -> Self {
        self.submenu = items;
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
// Messenger UI-2b right-click menus (community-messenger arc, cloud builds
// only) — items are always plain rows (no icon, no submenu): the messenger
// concept's own row style for Copy/Select all/Reply/Mark as read draws no
// icon glyphs, unlike the chart's own primitive menu above.
// =============================================================================

/// A DM message bubble's own right-click menu — Copy and Select all are
/// generic client-side text operations DM always supports (they never
/// touch the wire), so they show unconditionally; Reply only when the
/// active provider's own `Capabilities::replies` says the wire protocol
/// supports it (today's DM adapter never sets it — see
/// `dm_messenger_provider::reads::capabilities`'s own doc — so this row is
/// dead on arrival until a Matrix-shaped provider lands, same "kept wired,
/// unreachable today" convention the wave-menu builders below already use
/// for their own provider-gated rows).
#[cfg(feature = "cloud")]
pub fn build_dm_message_context_menu(caps: mlc_messenger_model::Capabilities) -> Vec<ContextMenuItemState> {
    let mut items = vec![
        ContextMenuItemState::action("dm_msg:copy", "Copy"),
        ContextMenuItemState::action("dm_msg:select_all", "Select all"),
    ];
    if caps.replies {
        items.push(ContextMenuItemState::action("dm_msg:reply", "Reply"));
    }
    items
}

/// A DM chat-list row's own right-click menu (concept v1 screen "2 · Канал
/// + меню чата") — Pin/Mute/Archive/Leave show UNCONDITIONALLY (no
/// `Capabilities` flag gates them; today's DM adapter's own [`crate::ui::
/// dm_messenger_provider::DmMessengerProvider::enqueue`] refuses the
/// matching [`mlc_messenger_model::MessengerIntent`] with a `last_error`,
/// the SAME "kept wired, unreachable today" convention
/// [`build_dm_message_context_menu`]'s own `Reply` row already uses).
/// "Mark as read" keeps its pre-existing `caps.receipts` gate. "Add to
/// folder ▸" shows only when `caps.folders` is set, and is a plain
/// per-folder CHECKLIST for visual parity — ticking one is NOT wired to a
/// `MessengerIntent` this pass: the model carries no "tag this room" intent
/// at all yet (folders are tags, but nothing in [`mlc_messenger_model::
/// MessengerIntent`] adds/removes a single room's own tag), a gap for a
/// later pass to close, not invented here. `can_mark_read` is the same
/// per-call data-availability gate this function always had.
#[cfg(feature = "cloud")]
pub fn build_dm_chat_row_context_menu(
    caps: mlc_messenger_model::Capabilities,
    can_mark_read: bool,
    kind: mlc_messenger_model::RoomKind,
    pinned: bool,
    muted: bool,
    folders: &[mlc_messenger_model::Folder],
    room_tags: &[mlc_messenger_model::TagName],
) -> Vec<ContextMenuItemState> {
    use mlc_messenger_model::RoomKind;

    let mut items = vec![
        ContextMenuItemState::action("dm_chat_row:toggle_pin", if pinned { "Unpin" } else { "Pin to top" }),
        ContextMenuItemState::action("dm_chat_row:toggle_mute", if muted { "Unmute" } else { "Mute" }),
    ];
    if caps.receipts {
        items.push(ContextMenuItemState::action("dm_chat_row:mark_read", "Mark as read").set_enabled(can_mark_read));
    }
    if caps.folders {
        let mut rows: Vec<ContextMenuItemState> = folders
            .iter()
            .filter_map(|f| {
                let tag = f.id.as_ref()?;
                let on = room_tags.iter().any(|t| t == tag);
                Some(ContextMenuItemState::action(format!("dm_chat_row:add_folder:{}", tag.as_str()), f.name.clone()).with_checkbox(on))
            })
            .collect();
        rows.push(ContextMenuItemState::separator());
        rows.push(ContextMenuItemState::action("dm_chat_row:new_folder", "New folder…"));
        items.push(ContextMenuItemState::action("dm_chat_row:add_folder", "Add to folder").with_submenu(rows));
    }
    items.push(ContextMenuItemState::action("dm_chat_row:archive", "Archive"));
    items.push(ContextMenuItemState::separator());
    let leave_label = match kind {
        RoomKind::Channel => "Leave channel",
        RoomKind::Group => "Leave group",
        RoomKind::Direct => "Delete chat",
    };
    items.push(ContextMenuItemState::danger("dm_chat_row:leave", leave_label));
    items
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
        let item = ContextMenuItemState::action_with_icon(
            elliott_icon_id(kind_id),
            format!("wave_menu:convert:{kind_id}"),
            t_concept(*key),
        );
        items.push(if *is_current { item.set_enabled(false) } else { item });
    }
    items
}

/// The Elliott icon id (`toolbar.rs`'s own `pattern_section` spelling —
/// `"elliott_impulse"` / `"elliott_correction"` / `"elliott_triangle"` /
/// `"elliott_double_combo"` / `"elliott_triple_combo"`, registered in
/// `render_context_menu`'s own icon match) a "kind id" or "type id" names —
/// drops the `:subtype` suffix a conversion/subdivide kind id may carry
/// (see `elliott::count::split_kind_id`'s own doc), so a correction's four
/// subtypes (zigzag / regular flat / expanded flat / running flat) all
/// share the one `elliott_correction` icon by design, not an omission
/// (owner report 2026-08-14: wave-menu rows were text-only although every
/// one of these five structures already has a registered toolbar icon).
///
/// `pub(crate)` — also used by `panel_app.rs`'s own inline wave dropdown
/// (`inline:wave_kind:`/`inline:wave_next_option:` rows), the second
/// control surface reading the SAME `SelectedPrimitiveConfig` kind/type ids
/// this popup does, so both surfaces resolve an icon the same way rather
/// than keeping two copies of the split-on-`:` convention.
pub(crate) fn elliott_icon_id(kind_or_type_id: &str) -> &str {
    kind_or_type_id.split(':').next().unwrap_or(kind_or_type_id)
}

/// The trailing "EWA settings" row every wave-gesture popup ends with (owner
/// ask 2026-08-14: "по какой-то ещё общей кнопке типа EWA settings в ПКМ" —
/// the ENGINE's own settings, not this structure's own). Action
/// `"wave_menu:engine_settings"`, routed by `on_context_menu_action`
/// (`mlc-app`) next to the other `wave_menu:*` arms — opens
/// `layout::modals::ewa_engine_settings::render_ewa_engine_settings_modal`.
/// Shared by [`build_wave_menu`], [`build_successor_menu`] and
/// [`build_predecessor_menu`] so the three popups can never disagree about
/// where this row sits (always last) or what it says.
fn push_engine_settings_row(items: &mut Vec<ContextMenuItemState>) {
    use mlc_core::i18n::{t_concept, ConceptKey};
    items.push(ContextMenuItemState::separator());
    items.push(ContextMenuItemState::action_with_icon(
        "settings",
        "wave_menu:engine_settings",
        t_concept(ConceptKey::EwaEngineSettingsMenuLabel),
    ));
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
/// `"wave_menu:delete_child:{child_id}"`,
/// `"wave_menu:toggle_truncated_fifth"` (wave-5-only, see below) —
/// `on_context_menu_action` (`mlc-app`) parses these back; see that
/// function's own doc for why the parent's own id is never embedded (it is
/// already the context menu's `ContextMenuTarget::Primitive` id).
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

    // Wave-5-only "truncated fifth" checkable row (owner ruling 2026-08-15,
    // second entrance for `ElliottImpulse::truncated_fifth` — the first is
    // the structure's own settings modal, free via `style_properties()`).
    // `slot_id == "impulse_5"` is `WaveSlot::Impulse5`'s own fixed `id()`
    // spelling — the ONLY slot id any Elliott type produces with that
    // token — so this alone identifies "wave 5 of an impulse" without a
    // second `type_id` field on `SelectedPrimitiveConfig`. Renders the SAME
    // toggle idiom this crate's own `build_primitive_context_menu` already
    // uses for lock/visibility (`MenuKey::Lock`/`MenuKey::Unlock`): the
    // label names the ACTION a click performs, which reads back as the
    // row's own current state — this crate's context-menu icon table
    // (`layout::modals::context_menu::render_context_menu`) carries no
    // separate checkbox glyph (see [`build_conversion_block`]'s own doc for
    // why that table was never given one), so no icon is invented for it
    // either, matching this SAME builder's own icon-less rows just below
    // (`SelectSubdivisionLabel`/`RemoveSubdivisionLabel`).
    if slot_id == "impulse_5" {
        let label = if cfg.truncated_fifth {
            t_concept(ConceptKey::ClearTruncatedFifthLabel)
        } else {
            t_concept(ConceptKey::MarkTruncatedFifthLabel)
        };
        items.push(ContextMenuItemState::action("wave_menu:toggle_truncated_fifth", label));
        items.push(ContextMenuItemState::separator());
    }

    if let Some((_, kinds)) = cfg.wave_slot_kinds.iter().find(|(id, _)| id == slot_id) {
        for (kind_id, key) in kinds {
            items.push(ContextMenuItemState::action_with_icon(
                elliott_icon_id(kind_id),
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

    push_engine_settings_row(&mut items);
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
        items.push(ContextMenuItemState::action_with_icon(
            elliott_icon_id(type_id),
            format!("wave_menu:successor:{type_id}"),
            display_name.clone(),
        ));
    }

    if cfg.can_promote {
        items.push(ContextMenuItemState::separator());
        items.push(ContextMenuItemState::action("wave_menu:promote", t_concept(ConceptKey::PromoteChainLabel)));
    }

    push_engine_settings_row(&mut items);
    items
}

/// Build the "what precedes this structure" popup
/// (`ChartGesture::PredecessorMenu`) for the already-selected primitive —
/// [`build_successor_menu`] read backwards (owner ask 2026-08-14: "щас мы
/// чейн можем только с конца строить, а хорошо бы... делать достройку
/// перед началом чейна" — a count is read backwards as often as
/// forwards, so building must not be limited to appending after the
/// terminal pivot).
///
/// Action ids: `"wave_menu:predecessor:{type_id}"` — mirrors
/// `build_successor_menu`'s own `"wave_menu:successor:{type_id}"` rows.
/// No promote button here: E4 promotion collapses a chain FORWARD from
/// its own head, and [`SelectedPrimitiveConfig::can_promote`] already
/// answers that question from whichever member of the chain is selected
/// — nothing about building backwards changes what promotion means, so
/// this menu leaves it to whichever OTHER wave-gesture menu the analyst
/// opens on the chain's own head.
///
/// Opens with [`build_conversion_block`] (own doc has the full "what this
/// whole segment currently IS" rationale), same as [`build_wave_menu`]/
/// [`build_successor_menu`].
pub fn build_predecessor_menu(cfg: &SelectedPrimitiveConfig) -> Vec<ContextMenuItemState> {
    // No second name row, same reasoning `build_successor_menu`'s own doc
    // gives: the conversion block above already opens with this
    // structure's own name.
    let mut items = build_conversion_block(cfg);
    items.push(ContextMenuItemState::separator());

    for (type_id, display_name) in &cfg.predecessor_kinds {
        items.push(ContextMenuItemState::action_with_icon(
            elliott_icon_id(type_id),
            format!("wave_menu:predecessor:{type_id}"),
            display_name.clone(),
        ));
    }

    push_engine_settings_row(&mut items);
    items
}
