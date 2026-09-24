//! Core toolbar rendering — copied from zengeld-core for chart panel self-containment.
//!
//! This is an exact copy of `zengeld-core/src/ui/render/toolbar.rs` adapted to
//! work without core dependencies. Uses `uzor::render::RenderContext` directly and
//! resolves icons via the chart's own `icons::icon_svg()` registry.

use std::borrow::Cow;
use uzor::render::{RenderContext, TextAlign, TextBaseline, draw_svg_icon, draw_svg_multicolor};
use mlc_core::state::toolbar_state::{MascotAnim, MascotAnimKind};

/// The brand mark (owner ruling 2026-09-10: the bull is retired, the magic
/// hat replaces it everywhere the mascot is drawn).
const MAGIC_HAT_SVG: &str = include_str!("../../../../assets/mascot/magic_hat.svg");

// =============================================================================
// Local type definitions (replace core's WidgetRect and IconId)
// =============================================================================

// WidgetRect relocated to mlc-core::state::toolbar_geometry — re-exported here.
pub use mlc_core::state::toolbar_geometry::WidgetRect;

/// Icon identifier — a named reference to an SVG icon.
///
/// Mirrors `uzor::types::IconId` without depending on that crate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IconId(pub String);

impl IconId {
    pub fn new(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

impl From<&str> for IconId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for IconId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for IconId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// Toolbar types
// =============================================================================

/// Toolbar orientation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolbarOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Toolbar item type
#[derive(Clone, Debug)]
pub enum ToolbarItem {
    /// Button with icon and/or text
    Button {
        id: String,
        icon: Option<IconId>,
        text: Option<String>,
        active: bool,
        disabled: bool,
        /// Minimum width for buttons with text (0 = auto)
        min_width: f64,
        /// Draw a line through the label — "feature currently OFF" look
        /// for toggle chips whose label is the feature name (e.g. "Imb").
        strikethrough: bool,
    },
    /// Icon-only button
    IconButton {
        id: String,
        icon: IconId,
        active: bool,
        disabled: bool,
        /// Minimum width for icon buttons (0 = square item_size)
        min_width: f64,
    },
    /// Dropdown button (shows popup when clicked)
    Dropdown {
        id: String,
        icon: Option<IconId>,
        text: Option<String>,
        active: bool,
        /// Show chevron indicator
        show_chevron: bool,
        /// Minimum width for dropdowns with text (0 = auto)
        min_width: f64,
    },
    /// Visual separator
    Separator,
    /// Flexible spacer
    Spacer,
    /// Clock display (text only, right-aligned)
    Clock {
        id: String,
        time: String,
    },
    /// Color button with icon and color indicator bar
    ColorButton {
        id: String,
        icon: IconId,
        color: String,
        active: bool,
    },
    /// Line width button (shows line + number)
    LineWidthButton {
        id: String,
        width: u32,
        active: bool,
    },
    /// Split icon button: left part = main action, right part = chevron that opens dropdown.
    /// Registers two hit rects: `id` for the main area and `{id}_menu` for the chevron.
    SplitIconButton {
        id: String,
        icon: IconId,
        active: bool,
    },
    /// Split line-width button: left part = line+number action, right part = chevron.
    /// Registers two hit rects: `id` for the main area and `{id}_menu` for the chevron.
    SplitLineWidthButton {
        id: String,
        width: u32,
        active: bool,
    },
    /// Text label (non-interactive)
    Label {
        id: String,
        text: String,
    },
}

impl ToolbarItem {
    pub fn button(id: &str, icon: impl Into<IconId>, text: &str) -> Self {
        Self::Button {
            id: id.to_string(),
            icon: Some(icon.into()),
            text: Some(text.to_string()),
            active: false,
            disabled: false,
            min_width: 0.0,
            strikethrough: false,
        }
    }

    pub fn icon_button(id: &str, icon: impl Into<IconId>) -> Self {
        Self::IconButton {
            id: id.to_string(),
            icon: icon.into(),
            active: false,
            disabled: false,
            min_width: 0.0,
        }
    }

    pub fn dropdown(id: &str, icon: impl Into<IconId>, text: &str) -> Self {
        Self::Dropdown {
            id: id.to_string(),
            icon: Some(icon.into()),
            text: Some(text.to_string()),
            active: false,
            show_chevron: false,  // No chevrons by default - use with_chevron() if needed
            min_width: 0.0,
        }
    }

    /// Add chevron indicator to dropdown
    pub fn with_chevron(self) -> Self {
        match self {
            Self::Dropdown { id, icon, text, active, min_width, .. } => {
                Self::Dropdown { id, icon, text, active, show_chevron: true, min_width }
            }
            other => other,
        }
    }

    /// Set minimum width for buttons/dropdowns
    pub fn with_min_width(self, width: f64) -> Self {
        match self {
            Self::Button { id, icon, text, active, disabled, strikethrough, .. } => {
                Self::Button { id, icon, text, active, disabled, min_width: width, strikethrough }
            }
            Self::IconButton { id, icon, active, disabled, .. } => {
                Self::IconButton { id, icon, active, disabled, min_width: width }
            }
            Self::Dropdown { id, icon, text, active, show_chevron, .. } => {
                Self::Dropdown { id, icon, text, active, show_chevron, min_width: width }
            }
            other => other,
        }
    }

    pub fn separator() -> Self {
        Self::Separator
    }

    pub fn spacer() -> Self {
        Self::Spacer
    }

    pub fn clock(id: &str, time: &str) -> Self {
        Self::Clock {
            id: id.to_string(),
            time: time.to_string(),
        }
    }

    pub fn color_button(id: &str, icon: impl Into<IconId>, color: &str) -> Self {
        Self::ColorButton {
            id: id.to_string(),
            icon: icon.into(),
            color: color.to_string(),
            active: false,
        }
    }

    pub fn line_width_button(id: &str, width: u32) -> Self {
        Self::LineWidthButton {
            id: id.to_string(),
            width,
            active: false,
        }
    }

    /// Split icon button: main area has the icon, chevron on right opens a dropdown.
    pub fn split_icon_button(id: &str, icon: impl Into<IconId>) -> Self {
        Self::SplitIconButton {
            id: id.to_string(),
            icon: icon.into(),
            active: false,
        }
    }

    /// Split line-width button: main area shows line+number, chevron on right opens a dropdown.
    pub fn split_line_width_button(id: &str, width: u32) -> Self {
        Self::SplitLineWidthButton {
            id: id.to_string(),
            width,
            active: false,
        }
    }

    pub fn label(id: &str, text: &str) -> Self {
        Self::Label {
            id: id.to_string(),
            text: text.to_string(),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Button { id, .. } => Some(id),
            Self::IconButton { id, .. } => Some(id),
            Self::Dropdown { id, .. } => Some(id),
            Self::Clock { id, .. } => Some(id),
            Self::ColorButton { id, .. } => Some(id),
            Self::LineWidthButton { id, .. } => Some(id),
            Self::SplitIconButton { id, .. } => Some(id),
            Self::SplitLineWidthButton { id, .. } => Some(id),
            Self::Label { id, .. } => Some(id),
            Self::Separator | Self::Spacer => None,
        }
    }

    /// Set the active state of the item
    pub fn with_active(self, active: bool) -> Self {
        match self {
            Self::Button { id, icon, text, disabled, min_width, strikethrough, .. } => {
                Self::Button { id, icon, text, active, disabled, min_width, strikethrough }
            }
            Self::IconButton { id, icon, disabled, min_width, .. } => {
                Self::IconButton { id, icon, active, disabled, min_width }
            }
            Self::Dropdown { id, icon, text, show_chevron, min_width, .. } => {
                Self::Dropdown { id, icon, text, active, show_chevron, min_width }
            }
            Self::ColorButton { id, icon, color, .. } => {
                Self::ColorButton { id, icon, color, active }
            }
            Self::LineWidthButton { id, width, .. } => {
                Self::LineWidthButton { id, width, active }
            }
            Self::SplitIconButton { id, icon, .. } => {
                Self::SplitIconButton { id, icon, active }
            }
            Self::SplitLineWidthButton { id, width, .. } => {
                Self::SplitLineWidthButton { id, width, active }
            }
            other => other,
        }
    }

    /// Check if this item is active
    pub fn is_active(&self) -> bool {
        match self {
            Self::Button { active, .. } => *active,
            Self::IconButton { active, .. } => *active,
            Self::Dropdown { active, .. } => *active,
            Self::ColorButton { active, .. } => *active,
            Self::LineWidthButton { active, .. } => *active,
            Self::SplitIconButton { active, .. } => *active,
            Self::SplitLineWidthButton { active, .. } => *active,
            _ => false,
        }
    }

    /// Set the icon for IconButton or Dropdown
    pub fn with_icon(self, new_icon: IconId) -> Self {
        match self {
            Self::Button { id, text, active, disabled, min_width, strikethrough, .. } => {
                Self::Button { id, icon: Some(new_icon), text, active, disabled, min_width, strikethrough }
            }
            Self::IconButton { id, active, disabled, min_width, .. } => {
                Self::IconButton { id, icon: new_icon, active, disabled, min_width }
            }
            Self::Dropdown { id, text, active, show_chevron, min_width, .. } => {
                Self::Dropdown { id, icon: Some(new_icon), text, active, show_chevron, min_width }
            }
            other => other,
        }
    }

    /// Get the current icon (if any)
    pub fn icon(&self) -> Option<&IconId> {
        match self {
            Self::Button { icon, .. } => icon.as_ref(),
            Self::IconButton { icon, .. } => Some(icon),
            Self::Dropdown { icon, .. } => icon.as_ref(),
            _ => None,
        }
    }

    /// Set the text/label for Button or Dropdown
    pub fn with_text(self, new_text: String) -> Self {
        match self {
            Self::Button { id, icon, active, disabled, min_width, strikethrough, .. } => {
                Self::Button { id, icon, text: Some(new_text), active, disabled, min_width, strikethrough }
            }
            Self::Dropdown { id, icon, active, show_chevron, min_width, .. } => {
                Self::Dropdown { id, icon, text: Some(new_text), active, show_chevron, min_width }
            }
            other => other,
        }
    }
}

/// Section alignment
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SectionAlign {
    /// Align to start (left for horizontal, top for vertical)
    #[default]
    Start,
    /// Align to end (right for horizontal, bottom for vertical)
    End,
}

/// Toolbar section configuration
#[derive(Clone, Debug)]
pub struct ToolbarSection {
    /// Section items
    pub items: Vec<ToolbarItem>,
    /// Show separator after this section
    pub show_separator: bool,
    /// Section alignment
    pub align: SectionAlign,
}

impl ToolbarSection {
    pub fn new(items: Vec<ToolbarItem>) -> Self {
        Self {
            items,
            show_separator: false,
            align: SectionAlign::Start,
        }
    }

    pub fn with_separator(mut self) -> Self {
        self.show_separator = true;
        self
    }

    /// Set this section to be right-aligned (for horizontal toolbars)
    pub fn align_end(mut self) -> Self {
        self.align = SectionAlign::End;
        self
    }

    /// Apply active states to items based on a predicate
    ///
    /// This is used to update the visual active state of toolbar items
    /// at render time based on application state.
    ///
    /// # Arguments
    /// * `is_active` - Predicate that returns true if item ID should be active
    ///
    /// # Returns
    /// A new section with updated active states
    pub fn with_active_states<F>(self, is_active: F) -> Self
    where
        F: Fn(&str) -> bool,
    {
        let items = self.items.into_iter().map(|item| {
            // Get the id first (as owned String) to avoid borrow issues
            let should_be_active = item.id().map(&is_active).unwrap_or(false);
            if should_be_active != item.is_active() {
                item.with_active(should_be_active)
            } else {
                item
            }
        }).collect();

        Self {
            items,
            show_separator: self.show_separator,
            align: self.align,
        }
    }

    /// Create an inline config section for selected primitive
    ///
    /// Creates a section with primitive name, settings, color, line width,
    /// line style, [wave-slot picker (E2, only when `wave_slots` is
    /// non-empty)], [wave-succession picker (E3, only when
    /// `successor_kinds` is non-empty)], alert, lock, delete, and more
    /// buttons.
    ///
    /// `wave_slots`/`successor_kinds` are both `(item_id, label)` pairs —
    /// plain display-ready strings, not `WaveSlot`/a type id, matching
    /// every other param here (`style` is `"dashed"`, not a `LineStyle`):
    /// this file stays domain-agnostic toolbar chrome. `item_id` is used
    /// verbatim as the dropdown row's own id; the caller (`panel_app.rs`)
    /// is the one that knows it must already be prefixed
    /// `"inline:wave_option:"` / `"inline:wave_next_option:"` respectively.
    #[allow(clippy::too_many_arguments)]
    pub fn inline_config(
        name: &str,
        color: &str,
        text_color: Option<&str>,
        supports_text: bool,
        width: u32,
        style: &str,
        locked: bool,
        wave_slots: &[(String, String)],
        successor_kinds: &[(String, String)],
        can_promote: bool,
    ) -> Self {
        let mut items = vec![
            // Primitive name
            ToolbarItem::label("inline:name", name),
            // Settings button
            ToolbarItem::icon_button("inline:settings", IconId::new("Settings")),
            // Color fill button
            ToolbarItem::color_button("inline:color", IconId::new("ColorFill"), color),
        ];

        // Text color button (only if supports_text)
        if supports_text {
            let text_col = text_color.unwrap_or(color);
            items.push(ToolbarItem::color_button("inline:text_color", IconId::new("TextColor"), text_col));
        }

        // Separator space
        items.push(ToolbarItem::Separator);

        // Line width split button: left click increments, chevron opens dropdown
        items.push(ToolbarItem::split_line_width_button("inline:width", width));

        // Line style split button: left click cycles, chevron opens dropdown
        let style_icon = match style {
            "dashed" => IconId::new("LineDashed"),
            "dotted" => IconId::new("LineDotted"),
            "large_dashed" => IconId::new("LineDashed"),
            "sparse_dotted" => IconId::new("LineDotted"),
            _ => IconId::new("LineSolid"),
        };
        items.push(ToolbarItem::split_icon_button("inline:style", style_icon));

        // Wave-slot picker (E2) — only when the selected primitive actually
        // has slots to offer (Elliott family; empty for everything else).
        // Opens a plain popup (`inline:wave` toggles it, same shape as the
        // style/width chevrons) rather than a split button: there is no
        // sensible "cycle to the next slot" default action the way there is
        // a next width/style, only a set of slots to choose from.
        if !wave_slots.is_empty() {
            items.push(ToolbarItem::Separator);
            items.push(ToolbarItem::icon_button("inline:wave", IconId::new("layers")));
        }

        // Wave-SUCCESSION picker (E3) — the SECOND, separate wave control:
        // "what comes after this" rather than E2's "what subdivides this".
        // Only when the selected primitive actually has a valid successor
        // type to offer (`elliott::count::successor_kinds` non-empty — an
        // Elliott wave, since a non-Elliott primitive can have no
        // successor). `chevron_right` (not `layers`, E2's icon) so the two
        // buttons read as visually distinct actions rather than a doubled
        // control.
        if !successor_kinds.is_empty() {
            items.push(ToolbarItem::Separator);
            items.push(ToolbarItem::icon_button("inline:wave_next", IconId::new("chevron_right")));
        }

        // Promote-chain button (E4) — collapses the selected primitive's own
        // `follows`-linked same-degree chain into one coarser structure. A
        // single button, not a dropdown (there is nothing to choose, unlike
        // E2/E3's own row lists) — `can_promote` is the exact same
        // `promote_chain_shape` check the click itself runs
        // (`DrawingManager::selected_config`/`promote_wave_chain`), so this
        // button is never shown offering a click that would refuse.
        if can_promote {
            items.push(ToolbarItem::Separator);
            items.push(ToolbarItem::icon_button("inline:wave_up", IconId::new("chevron_up")));
        }

        // Separator space
        items.push(ToolbarItem::Separator);

        // Alert button
        items.push(ToolbarItem::icon_button("inline:alert", IconId::new("Alert")));

        // Lock button with active state if locked
        let lock_icon = if locked { IconId::new("Lock") } else { IconId::new("Unlock") };
        let lock_btn = ToolbarItem::icon_button("inline:lock", lock_icon)
            .with_active(locked);
        items.push(lock_btn);

        // Delete button
        items.push(ToolbarItem::icon_button("inline:delete", IconId::new("Delete")));

        Self::new(items)
    }

    /// Create a clock section for bottom toolbar (right-aligned)
    pub fn clock(time: &str) -> Self {
        Self::new(vec![
            ToolbarItem::clock("clock", time),
        ]).align_end()
    }
}

/// Apply active states to a list of sections based on a predicate
///
/// # Arguments
/// * `sections` - The toolbar sections to update
/// * `is_active` - Predicate that returns true if item ID should be active
///
/// # Returns
/// New sections with updated active states
pub fn apply_active_states<F>(sections: Vec<ToolbarSection>, is_active: F) -> Vec<ToolbarSection>
where
    F: Fn(&str) -> bool,
{
    sections.into_iter()
        .map(|section| section.with_active_states(&is_active))
        .collect()
}

/// Apply toggle icons to a list of sections based on toggle state
///
/// For buttons that have toggle icon pairs (like Lock/Unlock, Eye/EyeOff),
/// this function swaps the icon based on whether the button is toggled ON.
///
/// # Arguments
/// * `sections` - The toolbar sections to update
/// * `is_toggled` - Predicate that returns true if button ID is toggled ON
/// * `get_toggled_icon` - Returns the toggled icon name for a button ID, if it has one
///
/// # Returns
/// New sections with updated icons
pub fn apply_toggle_icons<F, G>(
    sections: Vec<ToolbarSection>,
    is_toggled: F,
    get_toggled_icon: G,
) -> Vec<ToolbarSection>
where
    F: Fn(&str) -> bool,
    G: Fn(&str) -> Option<&'static str>,
{
    sections.into_iter()
        .map(|section| {
            let items = section.items.into_iter().map(|item| {
                // Copy id to owned String to avoid borrow issues
                let id_owned = item.id().map(|s| s.to_string());

                // Check if should swap icon
                let maybe_new_icon = id_owned.as_ref().and_then(|id| {
                    if is_toggled(id) {
                        get_toggled_icon(id)
                    } else {
                        None
                    }
                });

                if let Some(toggled_icon) = maybe_new_icon {
                    item.with_icon(toggled_icon.into())
                } else {
                    item
                }
            }).collect();

            ToolbarSection {
                items,
                show_separator: section.show_separator,
                align: section.align,
            }
        })
        .collect()
}

/// Apply quick-select icons to dropdown buttons
///
/// For dropdown buttons with quick_select=true, this function replaces the default
/// icon with the last-selected tool's icon (stored in ToolbarState.quick_select_icons).
///
/// # Arguments
/// * `sections` - The toolbar sections to update
/// * `get_quick_select_icon` - Returns the quick-select icon for a button ID, if one is set
///
/// # Returns
/// New sections with updated icons for quick-select dropdowns
pub fn apply_quick_select_icons<F>(
    sections: Vec<ToolbarSection>,
    get_quick_select_icon: F,
) -> Vec<ToolbarSection>
where
    F: Fn(&str) -> Option<IconId>,
{
    sections.into_iter()
        .map(|section| {
            let items = section.items.into_iter().map(|item| {
                // Get item ID
                let id_owned = item.id().map(|s| s.to_string());

                // Check if there's a quick-select icon for this item
                let maybe_icon = id_owned.as_ref().and_then(|id| get_quick_select_icon(id));

                if let Some(icon) = maybe_icon {
                    item.with_icon(icon)
                } else {
                    item
                }
            }).collect();

            ToolbarSection {
                items,
                show_separator: section.show_separator,
                align: section.align,
            }
        })
        .collect()
}

/// Apply dynamic button labels to toolbar sections
///
/// For buttons with dynamic labels (like timeframe selector, symbol selector),
/// this function replaces the static label with the dynamic one.
///
/// # Arguments
/// * `sections` - The toolbar sections to update
/// * `get_button_label` - Returns the dynamic label for a button ID, if one is set
///
/// # Returns
/// New sections with updated labels for dynamic buttons
pub fn apply_button_labels<F>(
    sections: Vec<ToolbarSection>,
    get_button_label: F,
) -> Vec<ToolbarSection>
where
    F: Fn(&str) -> Option<String>,
{
    sections.into_iter()
        .map(|section| {
            let items = section.items.into_iter().map(|item| {
                // Get item ID
                let id_owned = item.id().map(|s| s.to_string());

                // Check if there's a dynamic label for this item
                let maybe_label = id_owned.as_ref().and_then(|id| get_button_label(id));

                if let Some(label) = maybe_label {
                    item.with_text(label)
                } else {
                    item
                }
            }).collect();

            ToolbarSection {
                items,
                show_separator: section.show_separator,
                align: section.align,
            }
        })
        .collect()
}

/// Toolbar configuration
#[derive(Clone, Debug)]
pub struct ToolbarConfig {
    /// Sections in the toolbar
    pub sections: Vec<ToolbarSection>,
    /// Toolbar orientation
    pub orientation: ToolbarOrientation,
    /// Item size (for icon buttons)
    pub item_size: f64,
    /// Icon size
    pub icon_size: f64,
    /// Item spacing
    pub spacing: f64,
    /// Padding around toolbar
    pub padding: f64,
    /// Separator size
    pub separator_size: f64,
    /// Current scroll offset in pixels for start-aligned content (0.0 = no scroll).
    /// Only start-aligned sections scroll; end-aligned sections stay pinned.
    pub scroll_offset: f64,
    /// Pointer position in the same window-coordinate space as the toolbar's
    /// own draw rect, when known. `None` when the caller building this
    /// config has no pointer to report (e.g. an embedded header toolbar that
    /// never hosts a `Clock` item) — items that would otherwise animate on
    /// hover fall back to their plain draw path in that case. Currently only
    /// `ToolbarItem::Clock` reads this (ascii cell-shader hover demo, owner
    /// spec 2026-09-02).
    pub cursor: Option<(f64, f64)>,
    /// Real, monotonically increasing elapsed time in seconds, threaded in
    /// by the caller from the same `current_time_ms` the render path already
    /// carries — never sourced from `SystemTime::now()` inside a painter.
    /// Only meaningful together with `cursor`; `0.0` when unused.
    pub anim_time_s: f64,
    /// Rendered-frame counter. The clock's glitch advances one step per
    /// frame, so it runs at the app's FPS cadence rather than on a clock of
    /// its own (owner's ruling 2026-09-02).
    pub anim_frame: u64,
    /// User toggle (clock popup's third checkbox, profile-persisted) for the
    /// hover glitch shader effect on `ToolbarItem::Clock`. `true` (default)
    /// keeps today's hovered-and-known-pointer behaviour; `false` always
    /// falls back to the plain `fill_text` draw path, same as an unknown
    /// pointer.
    pub clock_glitch_enabled: bool,
    /// Live one-shot magic-hat animation (owner ruling 2026-09-10) for the
    /// "Bot" agents-panel button — `None`/lapsed draws the hat at rest,
    /// exactly like every other icon. Callers with no mascot concept of
    /// their own (drawing/control/inline/favorites toolbars — none of
    /// which host a "Bot" item) simply pass `None`.
    pub mascot_anim: Option<MascotAnim>,
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        // Hardcoded values (mirrors StyleCatalog::toolbar() defaults from zengeld-core)
        Self {
            sections: Vec::new(),
            orientation: ToolbarOrientation::Horizontal,
            item_size: 28.0,
            icon_size: 16.0,
            spacing: 2.0,
            padding: 4.0,
            separator_size: 1.0,
            scroll_offset: 0.0,
            cursor: None,
            anim_time_s: 0.0,
            anim_frame: 0,
            clock_glitch_enabled: true,
            mascot_anim: None,
        }
    }
}

/// Toolbar rendering result
#[derive(Clone, Debug, Default)]
pub struct ToolbarResult {
    /// ID of clicked item (if any)
    pub clicked: Option<String>,
    /// ID of hovered item (if any)
    pub hovered: Option<String>,
    /// Item rectangles (for hit testing)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Whether the start-aligned content exceeds the available toolbar space.
    pub overflows: bool,
    /// Maximum valid scroll offset (0.0 when not overflowing).
    pub max_scroll: f64,
    /// Hit rect for the left/up scroll chevron (present only when overflowing and scroll_offset > 0).
    pub left_chevron_rect: Option<WidgetRect>,
    /// Hit rect for the right/down scroll chevron (present only when overflowing and scroll_offset < max_scroll).
    pub right_chevron_rect: Option<WidgetRect>,
}

/// Toolbar theme
#[derive(Clone, Debug)]
pub struct ToolbarTheme {
    pub background: String,
    pub separator: String,
    pub item_bg_hover: String,
    pub item_bg_active: String,
    pub item_text: String,
    pub item_text_muted: String,
    pub item_text_hover: String,
    pub item_text_active: String,
    /// Accent color for highlights, active indicators, focus states
    pub accent: String,
    /// Use sidebar-style buttons with accent indicator for vertical toolbars
    /// When true, vertical toolbar buttons have a 3px accent bar on the left
    pub sidebar_style: bool,
}

impl Default for ToolbarTheme {
    fn default() -> Self {
        Self {
            background: "#1e222d".to_string(),
            separator: "#2a2e39".to_string(),
            item_bg_hover: "#2a2e39".to_string(),
            item_bg_active: "#2196F3".to_string(),
            item_text: "#d1d4dc".to_string(),
            item_text_muted: "#787b86".to_string(),
            item_text_hover: "#ffffff".to_string(),
            item_text_active: "#ffffff".to_string(),
            accent: "#2962ff".to_string(),
            sidebar_style: false,
        }
    }
}

/// Draw `MAGIC_HAT_SVG` mirrored horizontally about the icon box's own
/// center — a draw-time transform (scale -1 on x), not an SVG edit, so a
/// future icon swap keeps the same orientation rule for free (owner ruling
/// 2026-09-10: "mirror the hat — the brim points the other way").
fn draw_hat_mirrored(ctx: &mut dyn RenderContext, mx: f64, my: f64, size: f64) {
    let cx = mx + size / 2.0;
    ctx.save();
    ctx.translate(cx, 0.0);
    ctx.scale(-1.0, 1.0);
    ctx.translate(-cx, 0.0);
    draw_svg_multicolor(ctx, MAGIC_HAT_SVG, mx, my, size, size);
    ctx.restore();
}

/// Render an icon by looking up its SVG via the chart's icon registry.
///
/// `mascot_anim` is the live one-shot magic-hat animation (owner ruling
/// 2026-09-10, tab open/close AND the "Bot" button's own sidebar open/close)
/// for the "Bot" agents-panel button — `None` or lapsed draws the
/// (mirrored) hat at rest, exactly as every other icon draws today.
/// Ignored for every other `icon_id`.
fn render_icon(
    ctx: &mut dyn RenderContext,
    icon_id: &IconId,
    rect: WidgetRect,
    color: &str,
    mascot_anim: Option<MascotAnim>,
) {
    if icon_id.name() == "Bot" {
        let mascot_size = 36.0;
        let mx = rect.x + (rect.width - mascot_size) / 2.0;
        let my = rect.y + (rect.height - mascot_size) / 2.0;
        match mascot_anim.filter(MascotAnim::is_live) {
            None => draw_hat_mirrored(ctx, mx, my, mascot_size),
            Some(anim) => {
                let t = anim.progress();
                ctx.save();
                match anim.kind {
                    MascotAnimKind::Open => {
                        // "Tip": rotate about the brim's bottom-center,
                        // ease-out-cubic dip then ease-out-back return. The
                        // pivot is mirrored the same way the hat itself is
                        // (reflected about the icon box's horizontal
                        // center) — it must land on the MIRRORED brim, not
                        // the unmirrored one `hat_brim_pivot` maps to.
                        let (raw_px, raw_py) =
                            super::mascot_anim::hat_brim_pivot(mx, my, mascot_size);
                        let flip_cx = mx + mascot_size / 2.0;
                        let pivot_x = 2.0 * flip_cx - raw_px;
                        ctx.translate(pivot_x, raw_py);
                        ctx.rotate(super::mascot_anim::tip_angle_rad(t));
                        ctx.translate(-pivot_x, -raw_py);
                    }
                    MascotAnimKind::Close => {
                        // "Squash": vertical scale anchored at the icon
                        // box's own bottom edge (owner spec: "anchor
                        // bottom" — the whole button box, not the brim).
                        // This pivot sits ON the mirror axis (box
                        // horizontal center), so mirroring never moves it.
                        let pivot_x = mx + mascot_size / 2.0;
                        let pivot_y = my + mascot_size;
                        ctx.translate(pivot_x, pivot_y);
                        ctx.scale(1.0, super::mascot_anim::squash_scale_y(t));
                        ctx.translate(-pivot_x, -pivot_y);
                    }
                }
                draw_hat_mirrored(ctx, mx, my, mascot_size);
                ctx.restore();
            }
        }
        return;
    }
    if let Some(svg) = super::icons::icon_svg(icon_id.name()) {
        let svg = stroke_compensated_icon(svg, rect.width);
        draw_svg_icon(ctx, &svg, rect.x, rect.y, rect.width, rect.height, color);
    }
}

/// Reference draw size (px) for icon stroke weight — the left/right side
/// rail's own `icon_size` (`panel_app.rs`'s `drawing_config`/`right_config`,
/// both 30.0). `draw_svg_icon` scales an icon's root `stroke-width` by
/// `draw_size / viewbox_width` (`uzor::render::svg`, viewBox always 24x24
/// here), so the SAME glyph drawn smaller — the top/bottom control strips,
/// `icon_size: 24.0` — renders a visibly thinner on-screen line than the
/// identical icon on a side rail (owner defect 2026-09-24: "top toolbar
/// icons look too thin"). [`stroke_compensated_icon`] is the fix, applied
/// once here since every toolbar (top/bottom/left/right) funnels through
/// this one `render_icon` — the chrome row (tab strip + globe/cloud/avatar,
/// `mlc-app-input-shell::chrome::render`) calls `draw_svg_icon` directly
/// and never passes through here, so it is untouched by construction, not
/// by a special case.
const REFERENCE_ICON_STROKE_SIZE: f64 = 30.0;

/// Rewrite ONLY the root `<svg ...>` tag's own `stroke-width` attribute so
/// that drawing this icon at `draw_size` px renders the SAME on-screen
/// stroke thickness as drawing the unmodified icon at
/// [`REFERENCE_ICON_STROKE_SIZE`] px: `draw_svg_icon`'s own
/// `stroke = root_stroke_width * (draw_size / 24)`, so scaling
/// `root_stroke_width` by `REFERENCE_ICON_STROKE_SIZE / draw_size` cancels
/// `draw_size` out of that product entirely — the compensated stroke is
/// `base_stroke * REFERENCE_ICON_STROKE_SIZE / 24`, independent of
/// `draw_size`, for any `draw_size` below the reference.
///
/// A no-op (returns the input `svg` reference, unmodified) at
/// `draw_size >= REFERENCE_ICON_STROKE_SIZE` — this is what keeps the
/// left/right rails themselves (already drawn at exactly the reference
/// size) byte-identical to before this fix, and is also why this can
/// never "thicken" an icon shared with the side rails there: at the
/// reference size the compensation factor is exactly 1.0, a no-op.
fn stroke_compensated_icon(svg: &'static str, draw_size: f64) -> Cow<'static, str> {
    if draw_size <= 0.0 || draw_size >= REFERENCE_ICON_STROKE_SIZE {
        return Cow::Borrowed(svg);
    }
    let Some(tag_start) = svg.find("<svg") else { return Cow::Borrowed(svg) };
    let Some(tag_end_rel) = svg[tag_start..].find('>') else { return Cow::Borrowed(svg) };
    let root_tag = &svg[tag_start..tag_start + tag_end_rel + 1];
    let Some(sw_rel) = root_tag.find("stroke-width=\"") else { return Cow::Borrowed(svg) };
    let value_start = tag_start + sw_rel + "stroke-width=\"".len();
    let Some(value_len) = svg[value_start..].find('"') else { return Cow::Borrowed(svg) };
    let value_end = value_start + value_len;
    let Ok(base_stroke) = svg[value_start..value_end].trim().parse::<f64>() else { return Cow::Borrowed(svg) };

    let compensated = base_stroke * REFERENCE_ICON_STROKE_SIZE / draw_size;
    let mut out = String::with_capacity(svg.len() + 8);
    out.push_str(&svg[..value_start]);
    out.push_str(&format!("{compensated}"));
    out.push_str(&svg[value_end..]);
    Cow::Owned(out)
}

/// Width of a clock item: exact `measure_text` of the string in the same
/// font it renders with, plus 8px padding per side. The text draws
/// right-aligned, so any width overestimate would surface as dead space
/// on the LEFT — between the expand button and the `[UTC…]` label.
fn clock_width(ctx: &mut dyn RenderContext, time: &str) -> f64 {
    ctx.set_font("13px monospace");
    (ctx.measure_text(time) + 16.0).max(40.0)
}

/// Measured natural width of a text-bearing Button/Dropdown item.
///
/// The box HUGS the label (owner defect 2026-07-29: the `item_size * 2.5`
/// hardcode left dead padding around "Chart"/"DOM"). `left_inset` /
/// `right_pad` are supplied by the call site so they mirror the EXACT
/// offsets its draw code uses (icon slot / chevron clearance / bare 8px).
/// Font is pinned to the same `13px sans-serif` every Tc label draws with —
/// measuring under an ambient font would size the box for the wrong glyphs.
fn text_natural_width(
    ctx: &mut dyn RenderContext,
    text: &str,
    left_inset: f64,
    right_pad: f64,
) -> f64 {
    ctx.set_font("13px sans-serif");
    left_inset + ctx.measure_text(text) + right_pad
}

/// Layout width of a Button item: `min_width` fixed when set, measured
/// natural otherwise (icon slot `icon_size + 4` mirrors the Button draw
/// arms; a no-icon label centers inside `8 + text + 8`).
fn button_layout_width(
    ctx: &mut dyn RenderContext,
    config: &ToolbarConfig,
    text: Option<&str>,
    has_icon: bool,
    min_width: f64,
) -> f64 {
    if min_width > 0.0 {
        return min_width;
    }
    match text {
        Some(t) => {
            let left = if has_icon { config.icon_size + 4.0 } else { 8.0 };
            text_natural_width(ctx, t, left, 8.0)
        }
        None => config.item_size,
    }
}

/// Layout width of a Dropdown item: `min_width` fixed when set, measured
/// natural otherwise (icon slot `icon_size + 8` mirrors the Dropdown draw
/// arms' `text_x`; chevron clearance only when the chevron is shown).
fn dropdown_layout_width(
    ctx: &mut dyn RenderContext,
    config: &ToolbarConfig,
    text: Option<&str>,
    has_icon: bool,
    show_chevron: bool,
    min_width: f64,
) -> f64 {
    if min_width > 0.0 {
        return min_width;
    }
    match text {
        Some(t) => {
            let left = if has_icon { config.icon_size + 8.0 } else { 8.0 };
            let right = if show_chevron { 14.0 } else { 8.0 };
            text_natural_width(ctx, t, left, right)
        }
        None => config.item_size,
    }
}

/// Calculate width of a section's items
pub fn calculate_section_width(ctx: &mut dyn RenderContext, section: &ToolbarSection, config: &ToolbarConfig) -> f64 {
    let mut width = 0.0;
    for item in &section.items {
        match item {
            ToolbarItem::Separator => {
                width += config.separator_size + config.spacing;
            }
            ToolbarItem::Spacer => {
                width += config.item_size;
            }
            ToolbarItem::Button { icon, text, min_width, .. } => {
                width += button_layout_width(
                    ctx, config, text.as_deref(), icon.is_some(), *min_width,
                ) + config.spacing;
            }
            ToolbarItem::IconButton { min_width, .. } => {
                let item_width = if *min_width > 0.0 {
                    *min_width
                } else {
                    config.item_size
                };
                width += item_width + config.spacing;
            }
            ToolbarItem::Dropdown { icon, text, show_chevron, min_width, .. } => {
                width += dropdown_layout_width(
                    ctx, config, text.as_deref(), icon.is_some(), *show_chevron, *min_width,
                ) + config.spacing;
            }
            ToolbarItem::Clock { time, .. } => {
                width += clock_width(ctx, time) + config.spacing;
            }
            ToolbarItem::ColorButton { .. } => {
                width += config.item_size + config.spacing;
            }
            ToolbarItem::LineWidthButton { .. } => {
                width += 36.0 + config.spacing; // Line + number width
            }
            ToolbarItem::SplitIconButton { .. } => {
                // Main icon area (item_size) + chevron area (10px)
                width += config.item_size + 10.0 + config.spacing;
            }
            ToolbarItem::SplitLineWidthButton { .. } => {
                // Same as LineWidthButton (36px) + chevron area (10px)
                width += 36.0 + 10.0 + config.spacing;
            }
            ToolbarItem::Label { text, .. } => {
                // Approximate - actual width computed during render
                width += (text.len() as f64 * 7.0) + 8.0 + config.spacing;
            }
        }
    }
    width
}

/// Draw a toolbar with built-in icon rendering
///
/// This version renders icons internally using the chart's icon registry,
/// no callback needed. Supports left-aligned and right-aligned sections.
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Toolbar configuration
/// - `rect` - Toolbar rectangle
/// - `theme` - Toolbar theme
/// - `hovered_id` - Currently hovered item ID
///
/// # Returns
/// Toolbar result with item rectangles
pub fn draw_toolbar_with_icons(
    ctx: &mut dyn RenderContext,
    config: &ToolbarConfig,
    rect: WidgetRect,
    theme: &ToolbarTheme,
    hovered_id: Option<&str>,
) -> ToolbarResult {
    let mut result = ToolbarResult::default();

    // Blur background (FrostedGlass/LiquidGlass) - draws before solid background
    ctx.draw_blur_background(rect.x, rect.y, rect.width, rect.height);

    // Draw background (semi-transparent when blur style is active)
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    let is_horizontal = matches!(config.orientation, ToolbarOrientation::Horizontal);

    // Separate sections by alignment
    let start_sections: Vec<_> = config.sections.iter()
        .enumerate()
        .filter(|(_, s)| s.align == SectionAlign::Start)
        .collect();
    let end_sections: Vec<_> = config.sections.iter()
        .enumerate()
        .filter(|(_, s)| s.align == SectionAlign::End)
        .collect();

    // Calculate total width/height of end-aligned sections
    let end_width: f64 = end_sections.iter()
        .map(|(_, s)| calculate_section_width(&mut *ctx, s, config))
        .sum();

    // Calculate total width/height of start-aligned content (to detect overflow).
    // Includes inter-section separators that are drawn between sections during rendering.
    let start_content_size: f64 = {
        let items_size: f64 = start_sections.iter()
            .map(|(_, s)| calculate_section_width(&mut *ctx, s, config))
            .sum();
        // Count separators between sections (sections with show_separator that aren't the last)
        let sep_count = start_sections.iter()
            .filter(|(idx, s)| s.show_separator && *idx < config.sections.len() - 1)
            .count() as f64;
        items_size + sep_count * (config.separator_size + config.spacing * 2.0)
    };

    // Available space for start-aligned content.
    // When end-aligned sections exist they consume some space, so the scrollable
    // region is the remaining portion.
    let chevron_size = 16.0_f64; // Compact chevron button — fits triangle with minimal wasted space
    let toolbar_main_size = if is_horizontal { rect.width } else { rect.height };
    let end_reserved = if end_width > 0.0 { end_width + config.padding } else { 0.0 };
    // Non-overflow available width accounts for padding on both sides.
    // This is the threshold at which overflow kicks in.
    let start_available = (toolbar_main_size - config.padding * 2.0 - end_reserved).max(0.0);

    // Determine if start-aligned content overflows
    let overflows = start_content_size > start_available;
    result.overflows = overflows;

    // Clamp the current scroll offset to a preliminary range so we can determine
    // which chevrons will be visible before computing the final content area.
    // We use a conservative max_scroll (both chevrons reserved) for the clamp.
    // Conservative max_scroll with both chevrons shown and tight packing (no padding).
    let max_scroll_both = if overflows {
        let content_with_both_chevs = (toolbar_main_size - end_reserved - chevron_size * 2.0).max(0.0);
        (start_content_size - content_with_both_chevs).max(0.0)
    } else {
        0.0
    };
    let scroll_offset_prelim = config.scroll_offset.clamp(0.0, max_scroll_both);

    // Determine which chevrons will actually be visible.
    // Left/up: only when already scrolled past 0.
    // Right/down: only when there is more content to the right/down.
    let show_left = overflows && scroll_offset_prelim > 0.0;
    // For show_right we need the true max_scroll, but that depends on show_left.
    // Resolve: left reserve is now known, compute right reserve from that.
    // In overflow mode use 0 (not padding) when no left chevron — tight packing.
    let left_reserve_prelim = if show_left { chevron_size } else if overflows { 0.0 } else { config.padding };

    // Recalculate max_scroll with the actual left reserve.
    // Right reserve is chevron_size when there is more content, 0 otherwise (tight packing).
    // We iterate once: assume right chevron shown when overflowing, then verify.
    let max_scroll = if overflows {
        // Content visible width = toolbar_main_size - end_reserved - left_reserve - right_reserve
        // With right chevron: right_reserve = chevron_size
        // max_scroll = start_content_size - content_size_with_right_chev
        let content_size_with_right = (toolbar_main_size - end_reserved - left_reserve_prelim - chevron_size).max(0.0);
        (start_content_size - content_size_with_right).max(0.0)
    } else {
        0.0
    };
    result.max_scroll = max_scroll;

    // Clamp the current scroll offset to valid range
    let scroll_offset = config.scroll_offset.clamp(0.0, max_scroll);

    // Recompute show_left/show_right with the final scroll_offset.
    let show_left = overflows && scroll_offset > 0.0;
    let show_right = overflows && scroll_offset < max_scroll;

    // In overflow mode, tightly pack content against chevrons with no padding gap.
    // When a chevron is not shown on a side, use 0 (not padding) so content fills
    // all the way to the edge without wasted space.
    let left_reserve = if show_left { chevron_size } else if overflows { 0.0 } else { config.padding };
    let right_reserve = if show_right { chevron_size } else if overflows { 0.0 } else { config.padding };

    // Reserve space for chevron buttons when overflowing.
    // Only reserve space for a chevron if it will actually be visible.
    let (content_start, content_end) = if overflows {
        if is_horizontal {
            (rect.x + left_reserve, rect.x + toolbar_main_size - end_reserved - right_reserve)
        } else {
            (rect.y + left_reserve, rect.y + toolbar_main_size - end_reserved - right_reserve)
        }
    } else if is_horizontal {
        (rect.x + config.padding, rect.x + toolbar_main_size - end_reserved - config.padding)
    } else {
        (rect.y + config.padding, rect.y + toolbar_main_size - end_reserved - config.padding)
    };
    let content_size = (content_end - content_start).max(0.0);

    // Helper closure for rendering a single item.
    // `pos` is the absolute position in the main axis.
    // `clip_start` / `clip_end` define the visible content region — items outside are skipped.
    let render_item = |ctx: &mut dyn RenderContext, item: &ToolbarItem, pos: f64, clip_start: f64, clip_end: f64, result: &mut ToolbarResult| -> f64 {
        match item {
            ToolbarItem::Separator => {
                let advance = config.separator_size + config.spacing;
                // Check visibility: fully outside clip area — skip drawing but still advance
                let item_end = pos + config.separator_size;
                if item_end > clip_start && pos < clip_end {
                    ctx.set_fill_color(&theme.separator);
                    if is_horizontal {
                        let sep_height = rect.height * 0.5;
                        let sep_y = rect.y + (rect.height - sep_height) / 2.0;
                        ctx.fill_rect(pos, sep_y, config.separator_size, sep_height);
                    } else {
                        let sep_width = rect.width * 0.7;
                        let sep_x = rect.x + (rect.width - sep_width) / 2.0;
                        ctx.fill_rect(sep_x, pos, sep_width, config.separator_size);
                    }
                }
                advance
            }
            ToolbarItem::Spacer => {
                config.item_size
            }
            ToolbarItem::Button { id, icon, text, active, disabled, min_width, strikethrough } => {
                let icon_opt = icon.as_ref();
                let text_opt = text.as_ref();
                let is_hovered = hovered_id == Some(id.as_str());

                let item_width = button_layout_width(
                    ctx, config, text.as_deref(), icon_opt.is_some(), *min_width,
                );

                let advance = (if is_horizontal { item_width } else { config.item_size }) + config.spacing;
                let item_end = pos + if is_horizontal { item_width } else { config.item_size };

                // Skip items entirely outside the visible content area
                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y + (rect.height - config.item_size) / 2.0, item_width, config.item_size)
                } else {
                    WidgetRect::new(rect.x + (rect.width - config.item_size) / 2.0, pos, config.item_size, config.item_size)
                };

                let icon_color = if *disabled {
                    &theme.item_text
                } else if *active {
                    ctx.draw_active_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_hovered {
                    ctx.draw_hover_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                if let Some(icon) = icon_opt {
                    let icon_rect = WidgetRect::new(
                        item_rect.center_x() - config.icon_size / 2.0,
                        item_rect.center_y() - config.icon_size / 2.0,
                        config.icon_size,
                        config.icon_size,
                    );
                    render_icon(ctx, icon, icon_rect, icon_color, config.mascot_anim);
                }

                if let Some(text) = text_opt {
                    let text_x = if icon_opt.is_some() {
                        item_rect.x + config.icon_size + 4.0
                    } else {
                        item_rect.center_x()
                    };
                    ctx.set_font("13px sans-serif");
                    ctx.set_fill_color(icon_color);
                    ctx.set_text_align(if icon_opt.is_some() { TextAlign::Left } else { TextAlign::Center });
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(text, text_x, item_rect.center_y());
                    if *strikethrough {
                        // "Feature OFF" presentation: 1px line through the
                        // measured label, slightly overhanging each side.
                        let tw = ctx.measure_text(text);
                        let line_x = if icon_opt.is_some() { text_x } else { text_x - tw / 2.0 };
                        ctx.set_fill_color(icon_color);
                        ctx.fill_rect(line_x - 1.0, item_rect.center_y() - 0.5, tw + 2.0, 1.0);
                    }
                }

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                advance
            }
            ToolbarItem::IconButton { id, icon, active, disabled, min_width } => {
                let is_hovered = hovered_id == Some(id.as_str());

                let item_width = if *min_width > 0.0 { *min_width } else { config.item_size };

                let advance = item_width + config.spacing;
                let item_end = pos + item_width;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let use_sidebar_style = !is_horizontal && theme.sidebar_style;
                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y + (rect.height - config.item_size) / 2.0, item_width, config.item_size)
                } else if use_sidebar_style && *active {
                    WidgetRect::new(rect.x, pos, rect.width, config.item_size)
                } else {
                    WidgetRect::new(rect.x + (rect.width - config.item_size) / 2.0, pos, config.item_size, config.item_size)
                };

                let icon_color = if *disabled {
                    &theme.item_text
                } else if *active {
                    if use_sidebar_style {
                        ctx.draw_sidebar_active_item(
                            item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                            &theme.accent, &theme.item_bg_active, 3.0,
                        );
                    } else {
                        ctx.draw_active_rounded_rect(
                            item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                            4.0, &theme.item_bg_active,
                        );
                    }
                    &theme.item_text_active
                } else if is_hovered {
                    ctx.draw_hover_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                // Order-flow slot text chips draw at 20px (owner order
                // 2026-08-01 «16х16 увеличь до 20х20») — every other icon
                // keeps the theme size.
                let icon_size = if matches!(
                    id.as_str(),
                    "slot_dom" | "slot_tape" | "slot_l2" | "slot_big_trades" | "slot_liq"
                        | "screener"
                ) {
                    (config.icon_size + 4.0).max(20.0)
                } else {
                    config.icon_size
                };
                let icon_rect = WidgetRect::new(
                    item_rect.center_x() - icon_size / 2.0,
                    item_rect.center_y() - icon_size / 2.0,
                    icon_size,
                    icon_size,
                );
                render_icon(ctx, icon, icon_rect, icon_color, config.mascot_anim);

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                advance
            }
            ToolbarItem::Dropdown { id, icon, text, active, show_chevron, min_width } => {
                let is_hovered = hovered_id == Some(id.as_str());

                let item_width = dropdown_layout_width(
                    ctx, config, text.as_deref(), icon.is_some(), *show_chevron, *min_width,
                );

                let advance = (if is_horizontal { item_width } else { config.item_size }) + config.spacing;
                let item_end = pos + if is_horizontal { item_width } else { config.item_size };

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y + (rect.height - config.item_size) / 2.0, item_width, config.item_size)
                } else {
                    WidgetRect::new(rect.x + (rect.width - config.item_size) / 2.0, pos, config.item_size, config.item_size)
                };

                // Adaptive hover/active highlight width: a long label (e.g.
                // "500R", "3811.79B" on the resolution/`timeframe_selector`
                // button) can render wider than the button's fixed LAYOUT
                // width (`item_width`, unchanged above — hit-test rect and
                // toolbar flow stay exactly as before). When that happens,
                // the highlight rect the owner sees on hover/active must
                // widen to cover the full rendered label instead of clipping
                // it. Formula: `highlight_w = max(item_width, icon_reserve +
                // text_w + right_pad)`, where `icon_reserve` mirrors the
                // exact `icon_x + icon_size` / `text_x` offsets used below
                // (4px icon inset when no text, `icon_size + 8` gap when
                // there's a label) and `right_pad` is the same 10px chevron
                // clearance `show_chevron` already reserves — grown
                // RIGHTWARD only (left edge/position pinned), matching how
                // the label itself overflows past `item_rect.right()`.
                // Measure under the label's own font — an ambient font from a
                // previous draw must not size the highlight.
                ctx.set_font("13px sans-serif");
                let text_w = text.as_deref().map(|t| ctx.measure_text(t)).unwrap_or(0.0);
                let icon_reserve = if text.is_some() {
                    if icon.is_some() { config.icon_size + 8.0 } else { 8.0 }
                } else {
                    4.0 + config.icon_size
                };
                let right_pad = if *show_chevron { 14.0 } else { 8.0 };
                let content_w = icon_reserve + text_w + right_pad;
                let highlight_w = item_width.max(content_w);
                let highlight_rect = if highlight_w > item_rect.width {
                    WidgetRect::new(item_rect.x, item_rect.y, highlight_w, item_rect.height)
                } else {
                    item_rect
                };

                let icon_color = if *active {
                    ctx.draw_active_rounded_rect(
                        highlight_rect.x, highlight_rect.y, highlight_rect.width, highlight_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_hovered {
                    ctx.draw_hover_rounded_rect(
                        highlight_rect.x, highlight_rect.y, highlight_rect.width, highlight_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                if let Some(icon) = icon {
                    let icon_rect = WidgetRect::new(
                        item_rect.x + 4.0,
                        item_rect.center_y() - config.icon_size / 2.0,
                        config.icon_size,
                        config.icon_size,
                    );
                    render_icon(ctx, icon, icon_rect, icon_color, config.mascot_anim);
                }

                if let Some(text) = text {
                    // Icon slot only when an icon actually renders — a bare
                    // text dropdown (tc_center "Chart"/"DOM") hugs at 8px.
                    let text_x = item_rect.x
                        + if icon.is_some() { config.icon_size + 8.0 } else { 8.0 };
                    ctx.set_font("13px sans-serif");
                    ctx.set_fill_color(icon_color);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(text, text_x, item_rect.center_y());
                }

                if *show_chevron {
                    let chevron_x = item_rect.right() - 10.0;
                    let chevron_y = item_rect.center_y();
                    let chevron_size = 4.0;

                    ctx.set_fill_color(icon_color);
                    ctx.begin_path();
                    ctx.move_to(chevron_x - chevron_size, chevron_y - chevron_size / 2.0);
                    ctx.line_to(chevron_x, chevron_y + chevron_size / 2.0);
                    ctx.line_to(chevron_x + chevron_size, chevron_y - chevron_size / 2.0);
                    ctx.close_path();
                    ctx.fill();
                }

                // Hit-test rect stays the LAYOUT rect (owner constraint) —
                // only the drawn highlight above widened.
                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                advance
            }
            ToolbarItem::Clock { id, time } => {
                let clock_width = clock_width(ctx, time);
                let advance = clock_width + config.spacing;
                let item_end = pos + clock_width;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y, clock_width, rect.height)
                } else {
                    WidgetRect::new(rect.x, pos, rect.width, config.item_size)
                };

                let is_hovered = hovered_id == Some(id.as_str());
                if is_hovered {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rounded_rect(item_rect.x, item_rect.y + 2.0, item_rect.width, item_rect.height - 4.0, 4.0);
                }

                // Hovered + a known pointer: run the clock through uzor-text's
                // ASCII cell-shader engine (owner spec 2026-09-02 — a genuine
                // demo of the shared engine on this one element, not a
                // hand-rolled lookalike). Every other case is exactly today's
                // single `fill_text` call, unchanged.
                match (is_hovered, config.cursor, config.clock_glitch_enabled) {
                    (true, Some(pointer), true) => {
                        super::clock_glitch::draw_clock_glitch(
                            ctx,
                            time,
                            item_rect,
                            &theme.item_text,
                            pointer,
                            config.anim_time_s,
                            config.anim_frame,
                        );
                    }
                    _ => {
                        ctx.set_font("13px monospace");
                        ctx.set_fill_color(&theme.item_text);
                        ctx.set_text_align(TextAlign::Right);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.fill_text(time, item_rect.right() - 8.0, item_rect.center_y());
                    }
                }

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }
                advance
            }
            ToolbarItem::ColorButton { id, icon, color, active } => {
                let is_hovered = hovered_id == Some(id.as_str());
                let item_width = config.item_size;
                let advance = item_width + config.spacing;
                let item_end = pos + item_width;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y + (rect.height - config.item_size) / 2.0, item_width, config.item_size)
                } else {
                    WidgetRect::new(rect.x + (rect.width - config.item_size) / 2.0, pos, config.item_size, config.item_size)
                };

                let icon_color = if *active {
                    ctx.draw_active_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_hovered {
                    ctx.draw_hover_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                let icon_rect = WidgetRect::new(
                    item_rect.center_x() - config.icon_size / 2.0,
                    item_rect.y + 2.0,
                    config.icon_size,
                    config.icon_size,
                );
                render_icon(ctx, icon, icon_rect, icon_color, config.mascot_anim);

                ctx.set_fill_color(color);
                ctx.fill_rect(item_rect.x + 4.0, item_rect.bottom() - 6.0, item_width - 8.0, 3.0);

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                advance
            }
            ToolbarItem::LineWidthButton { id, width, active } => {
                let is_hovered = hovered_id == Some(id.as_str());
                let item_width = 36.0;
                let advance = item_width + config.spacing;
                let item_end = pos + item_width;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y + (rect.height - config.item_size) / 2.0, item_width, config.item_size)
                } else {
                    WidgetRect::new(rect.x + (rect.width - item_width) / 2.0, pos, item_width, config.item_size)
                };

                let text_color = if *active {
                    ctx.draw_active_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_hovered {
                    ctx.draw_hover_rounded_rect(
                        item_rect.x, item_rect.y, item_rect.width, item_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                let line_thickness = (*width as f64).clamp(1.0, 4.0);
                ctx.set_stroke_color(text_color);
                ctx.set_stroke_width(line_thickness);
                ctx.begin_path();
                ctx.move_to(item_rect.x + 4.0, item_rect.center_y());
                ctx.line_to(item_rect.x + 16.0, item_rect.center_y());
                ctx.stroke();

                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&format!("{}", width), item_rect.x + 20.0, item_rect.center_y());

                result.item_rects.push((id.clone(), item_rect));
                if is_hovered {
                    result.hovered = Some(id.clone());
                }

                advance
            }
            ToolbarItem::SplitIconButton { id, icon, active } => {
                // Layout: [icon_area (item_size wide)][chevron_area (10px wide)]
                const CHEVRON_W: f64 = 10.0;
                let main_w = config.item_size;
                let total_w = main_w + CHEVRON_W;
                let advance = total_w + config.spacing;
                let item_end = pos + total_w;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let top = rect.y + (rect.height - config.item_size) / 2.0;
                let full_rect = WidgetRect::new(pos, top, total_w, config.item_size);
                let main_rect = WidgetRect::new(pos, top, main_w, config.item_size);
                let chev_rect = WidgetRect::new(pos + main_w, top, CHEVRON_W, config.item_size);

                let menu_id = format!("{}_menu", id);
                let is_main_hovered = hovered_id == Some(id.as_str());
                let is_chev_hovered = hovered_id == Some(menu_id.as_str());
                let is_any_hovered = is_main_hovered || is_chev_hovered;

                // Draw background covering both halves
                let icon_color = if *active {
                    ctx.draw_active_rounded_rect(
                        full_rect.x, full_rect.y, full_rect.width, full_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_any_hovered {
                    ctx.draw_hover_rounded_rect(
                        full_rect.x, full_rect.y, full_rect.width, full_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                // Draw icon in main area
                let icon_size = config.icon_size.min(main_w - 2.0);
                let icon_rect = WidgetRect::new(
                    main_rect.center_x() - icon_size / 2.0,
                    main_rect.center_y() - icon_size / 2.0,
                    icon_size,
                    icon_size,
                );
                render_icon(ctx, icon, icon_rect, icon_color, config.mascot_anim);

                // Draw tiny downward chevron in the chevron area
                {
                    let cx = chev_rect.center_x();
                    let cy = chev_rect.center_y();
                    let cs = 2.5_f64;
                    ctx.set_stroke_color(icon_color);
                    ctx.set_stroke_width(1.2);
                    ctx.begin_path();
                    ctx.move_to(cx - cs, cy - cs / 2.0);
                    ctx.line_to(cx, cy + cs / 2.0);
                    ctx.line_to(cx + cs, cy - cs / 2.0);
                    ctx.stroke();
                }

                // Register two separate hit rects
                result.item_rects.push((id.clone(), main_rect));
                result.item_rects.push((menu_id, chev_rect));
                if is_main_hovered {
                    result.hovered = Some(id.clone());
                } else if is_chev_hovered {
                    result.hovered = Some(format!("{}_menu", id));
                }

                advance
            }
            ToolbarItem::SplitLineWidthButton { id, width, active } => {
                // Layout: [line+number area (36px wide)][chevron_area (10px wide)]
                const CHEVRON_W: f64 = 10.0;
                let main_w = 36.0_f64;
                let total_w = main_w + CHEVRON_W;
                let advance = total_w + config.spacing;
                let item_end = pos + total_w;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let top = rect.y + (rect.height - config.item_size) / 2.0;
                let full_rect = WidgetRect::new(pos, top, total_w, config.item_size);
                let main_rect = WidgetRect::new(pos, top, main_w, config.item_size);
                let chev_rect = WidgetRect::new(pos + main_w, top, CHEVRON_W, config.item_size);

                let menu_id = format!("{}_menu", id);
                let is_main_hovered = hovered_id == Some(id.as_str());
                let is_chev_hovered = hovered_id == Some(menu_id.as_str());
                let is_any_hovered = is_main_hovered || is_chev_hovered;

                // Draw background covering both halves
                let text_color = if *active {
                    ctx.draw_active_rounded_rect(
                        full_rect.x, full_rect.y, full_rect.width, full_rect.height,
                        4.0, &theme.item_bg_active,
                    );
                    &theme.item_text_active
                } else if is_any_hovered {
                    ctx.draw_hover_rounded_rect(
                        full_rect.x, full_rect.y, full_rect.width, full_rect.height,
                        4.0, &theme.item_bg_hover,
                    );
                    &theme.item_text_hover
                } else {
                    &theme.item_text
                };

                // Draw line + number in the main area
                let line_thickness = (*width as f64).clamp(1.0, 4.0);
                ctx.set_stroke_color(text_color);
                ctx.set_stroke_width(line_thickness);
                ctx.begin_path();
                ctx.move_to(main_rect.x + 4.0, main_rect.center_y());
                ctx.line_to(main_rect.x + 16.0, main_rect.center_y());
                ctx.stroke();

                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&format!("{}", width), main_rect.x + 20.0, main_rect.center_y());

                // Draw tiny downward chevron in the chevron area
                {
                    let cx = chev_rect.center_x();
                    let cy = chev_rect.center_y();
                    let cs = 2.5_f64;
                    ctx.set_stroke_color(text_color);
                    ctx.set_stroke_width(1.2);
                    ctx.begin_path();
                    ctx.move_to(cx - cs, cy - cs / 2.0);
                    ctx.line_to(cx, cy + cs / 2.0);
                    ctx.line_to(cx + cs, cy - cs / 2.0);
                    ctx.stroke();
                }

                // Register two separate hit rects
                result.item_rects.push((id.clone(), main_rect));
                result.item_rects.push((menu_id, chev_rect));
                if is_main_hovered {
                    result.hovered = Some(id.clone());
                } else if is_chev_hovered {
                    result.hovered = Some(format!("{}_menu", id));
                }

                advance
            }
            ToolbarItem::Label { id, text } => {
                ctx.set_font("13px sans-serif");
                let text_width = ctx.measure_text(text);
                let item_width = text_width + 8.0;
                let advance = item_width + config.spacing;
                let item_end = pos + item_width;

                if item_end <= clip_start || pos >= clip_end {
                    return advance;
                }

                let item_rect = if is_horizontal {
                    WidgetRect::new(pos, rect.y, item_width, rect.height)
                } else {
                    WidgetRect::new(rect.x, pos, rect.width, config.item_size)
                };

                ctx.set_fill_color(&theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(text, item_rect.x + 4.0, item_rect.center_y());

                result.item_rects.push((id.clone(), item_rect));
                advance
            }
        }
    };

    // Render start-aligned sections with scroll offset applied.
    // Items are offset by -scroll_offset so content scrolls toward the start.
    // The clip region is [content_start, content_start + content_size].
    {
        let clip_start = content_start;
        let clip_end = content_start + content_size;

        // Initial position offset by scroll
        let mut pos = if is_horizontal {
            content_start - scroll_offset
        } else {
            content_start - scroll_offset
        };

        for (section_idx, section) in &start_sections {
            for item in &section.items {
                let advance = render_item(ctx, item, pos, clip_start, clip_end, &mut result);
                pos += advance;
            }

            // Draw section separator if needed
            if section.show_separator && *section_idx < config.sections.len() - 1 {
                let sep_end = pos + config.separator_size;
                if sep_end > clip_start && pos < clip_end {
                    ctx.set_fill_color(&theme.separator);
                    if is_horizontal {
                        let sep_height = rect.height * 0.5;
                        let sep_y = rect.y + (rect.height - sep_height) / 2.0;
                        ctx.fill_rect(pos, sep_y, config.separator_size, sep_height);
                    } else {
                        let sep_width = rect.width * 0.7;
                        let sep_x = rect.x + (rect.width - sep_width) / 2.0;
                        ctx.fill_rect(sep_x, pos, sep_width, config.separator_size);
                    }
                }
                pos += config.separator_size + config.spacing * 2.0;
            }
        }
    }

    // Render end-aligned sections — these are NOT scrolled, they stay pinned.
    if !end_sections.is_empty() {
        // End sections start after the right/bottom chevron (if overflowing) or padding.
        let end_start = if is_horizontal {
            rect.x + toolbar_main_size - end_reserved
        } else {
            rect.y + toolbar_main_size - end_reserved
        };

        let mut pos = end_start;

        for (section_idx, section) in &end_sections {
            for item in &section.items {
                // End-aligned items are always fully visible; clip_start = -inf, clip_end = +inf
                let advance = render_item(ctx, item, pos, f64::NEG_INFINITY, f64::INFINITY, &mut result);
                pos += advance;
            }

            if section.show_separator && *section_idx < config.sections.len() - 1 {
                ctx.set_fill_color(&theme.separator);
                if is_horizontal {
                    let sep_height = rect.height * 0.5;
                    let sep_y = rect.y + (rect.height - sep_height) / 2.0;
                    ctx.fill_rect(pos, sep_y, config.separator_size, sep_height);
                    pos += config.separator_size + config.spacing * 2.0;
                } else {
                    let sep_width = rect.width * 0.7;
                    let sep_x = rect.x + (rect.width - sep_width) / 2.0;
                    ctx.fill_rect(sep_x, pos, sep_width, config.separator_size);
                    pos += config.separator_size + config.spacing * 2.0;
                }
            }
        }
    }

    // Draw chevron buttons AFTER content so they render on top with opaque backgrounds.
    if overflows {
        let left_chev_rect = if is_horizontal {
            WidgetRect::new(rect.x, rect.y, chevron_size, rect.height)
        } else {
            WidgetRect::new(rect.x, rect.y, rect.width, chevron_size)
        };
        let right_chev_rect = if is_horizontal {
            WidgetRect::new(rect.x + toolbar_main_size - end_reserved - chevron_size, rect.y, chevron_size, rect.height)
        } else {
            WidgetRect::new(rect.x, rect.y + toolbar_main_size - end_reserved - chevron_size, rect.width, chevron_size)
        };

        let left_hovered = hovered_id == Some("__chevron_left");
        let right_hovered = hovered_id == Some("__chevron_right");
        let left_color = if left_hovered { &theme.item_text_hover } else { &theme.item_text };
        let right_color = if right_hovered { &theme.item_text_hover } else { &theme.item_text };
        let tri_half = 5.0;

        if show_left {
            ctx.set_fill_color(&theme.background);
            ctx.fill_rect(left_chev_rect.x, left_chev_rect.y, left_chev_rect.width, left_chev_rect.height);
            if left_hovered {
                ctx.draw_hover_rounded_rect(left_chev_rect.x, left_chev_rect.y, left_chev_rect.width, left_chev_rect.height, 4.0, &theme.item_bg_hover);
            }
            ctx.set_fill_color(left_color);
            ctx.begin_path();
            if is_horizontal {
                let cx = left_chev_rect.center_x();
                let cy = left_chev_rect.center_y();
                ctx.move_to(cx + tri_half, cy - tri_half);
                ctx.line_to(cx - tri_half, cy);
                ctx.line_to(cx + tri_half, cy + tri_half);
            } else {
                let cx = left_chev_rect.center_x();
                let cy = left_chev_rect.center_y();
                ctx.move_to(cx - tri_half, cy + tri_half);
                ctx.line_to(cx, cy - tri_half);
                ctx.line_to(cx + tri_half, cy + tri_half);
            }
            ctx.close_path();
            ctx.fill();
            result.left_chevron_rect = Some(left_chev_rect);
        }

        if show_right {
            ctx.set_fill_color(&theme.background);
            ctx.fill_rect(right_chev_rect.x, right_chev_rect.y, right_chev_rect.width, right_chev_rect.height);
            if right_hovered {
                ctx.draw_hover_rounded_rect(right_chev_rect.x, right_chev_rect.y, right_chev_rect.width, right_chev_rect.height, 4.0, &theme.item_bg_hover);
            }
            ctx.set_fill_color(right_color);
            ctx.begin_path();
            if is_horizontal {
                let cx = right_chev_rect.center_x();
                let cy = right_chev_rect.center_y();
                ctx.move_to(cx - tri_half, cy - tri_half);
                ctx.line_to(cx + tri_half, cy);
                ctx.line_to(cx - tri_half, cy + tri_half);
            } else {
                let cx = right_chev_rect.center_x();
                let cy = right_chev_rect.center_y();
                ctx.move_to(cx - tri_half, cy - tri_half);
                ctx.line_to(cx, cy + tri_half);
                ctx.line_to(cx + tri_half, cy - tri_half);
            }
            ctx.close_path();
            ctx.fill();
            result.right_chevron_rect = Some(right_chev_rect);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolbar_item() {
        let item = ToolbarItem::icon_button("test", "icon");
        assert_eq!(item.id(), Some("test"));

        let sep = ToolbarItem::separator();
        assert_eq!(sep.id(), None);
    }

    #[test]
    fn test_toolbar_section() {
        let section = ToolbarSection::new(vec![
            ToolbarItem::icon_button("a", "icon_a"),
            ToolbarItem::separator(),
            ToolbarItem::icon_button("b", "icon_b"),
        ]).with_separator();

        assert_eq!(section.items.len(), 3);
        assert!(section.show_separator);
    }

    /// A canonical house-standard icon: root `stroke-width="1.25"`, 24x24
    /// viewBox, matching every constant in `mlc-core::state::icons`.
    const SAMPLE_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round">
  <path d="M3 3h18v18H3z"/>
</svg>"##;

    /// At the reference size (left/right rail's own `icon_size`, 30.0) this
    /// must be a byte-for-byte no-op — the side rails must never change.
    #[test]
    fn stroke_compensation_is_a_no_op_at_the_reference_size() {
        let out = stroke_compensated_icon(SAMPLE_ICON, REFERENCE_ICON_STROKE_SIZE);
        assert!(matches!(out, Cow::Borrowed(_)), "must borrow, not allocate, at the reference size");
        assert_eq!(out.as_ref(), SAMPLE_ICON);
    }

    /// Above the reference size, also a no-op (an icon drawn bigger than
    /// the rail's own reference keeps scaling its stroke up naturally).
    #[test]
    fn stroke_compensation_is_a_no_op_above_the_reference_size() {
        let out = stroke_compensated_icon(SAMPLE_ICON, 42.0);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    /// The exact defect this fixes: the top/bottom control strips draw
    /// icons at 24.0 px (`panel_app.rs`'s `control_config`/`bottom_config`).
    /// Compensated root stroke-width must equal `1.25 * 30.0 / 24.0`, and —
    /// the actual on-screen assertion — `draw_svg_icon`'s own product
    /// `root_stroke_width * (draw_size / 24)` must come out IDENTICAL to
    /// the unmodified icon's product at the reference size, so the two
    /// toolbars render pixel-identical stroke weight before quantization.
    #[test]
    fn stroke_compensation_equalizes_rendered_stroke_at_24px() {
        let compensated = stroke_compensated_icon(SAMPLE_ICON, 24.0);
        assert!(matches!(compensated, Cow::Owned(_)), "must allocate a rewritten string below the reference size");
        assert!(compensated.contains("stroke-width=\"1.5625\""), "compensated svg: {compensated}");

        let side_rail_product = 1.25_f64 * (REFERENCE_ICON_STROKE_SIZE / 24.0); // unmodified icon at 30px
        let top_toolbar_product = 1.5625_f64 * (24.0 / 24.0); // compensated icon at 24px
        assert!((side_rail_product - top_toolbar_product).abs() < 1e-9);
    }

    /// The compensation factor cancels `draw_size` out algebraically, so
    /// ANY draw size below the reference lands on the exact same
    /// compensated product — not just the one 24px case above (covers the
    /// `icon_size + 4`-style quick-select variant some draw arms use).
    #[test]
    fn stroke_compensation_is_draw_size_independent_below_the_reference() {
        let reference_product = 1.25_f64 * (REFERENCE_ICON_STROKE_SIZE / 24.0);
        for draw_size in [16.0, 20.0, 24.0, 28.0, 29.9] {
            let svg = stroke_compensated_icon(SAMPLE_ICON, draw_size);
            let sw_start = svg.find("stroke-width=\"").unwrap() + "stroke-width=\"".len();
            let sw_end = svg[sw_start..].find('"').unwrap() + sw_start;
            let compensated_stroke: f64 = svg[sw_start..sw_end].parse().unwrap();
            let product = compensated_stroke * (draw_size / 24.0);
            assert!(
                (product - reference_product).abs() < 1e-9,
                "draw_size={draw_size}: product {product} must equal the reference product {reference_product}",
            );
        }
    }
}
