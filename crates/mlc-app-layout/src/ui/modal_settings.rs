//! Settings modal state types for chart-related settings modals
//!
//! These types were moved from `zengeld-terminal-core` to `zengeld-chart` so
//! that chart-level crates can use them without depending on core.
//!
//! Core re-exports these via `pub use mlc_core::ui::modal_settings::*`.

use crate::ui::color_picker_state::ColorPickerState;
use crate::ui::scroll_state::ScrollState;
use crate::ui::widgets::ResizeEdge;
use mlc_core::drawing::primitives_v2::config::TimeframeVisibilityConfig;
use mlc_core::drawing::primitives_v2::{ControlPointShape, ControlPointSize};
use mlc_core::state::ewa_engine::EwaEngineConfig;
use mlc_core::state::smc_engine::SmcEngineConfig;
use mlc_core::state::orderflow_engine::OrderFlowEngineConfig;
use mlc_core::state::candle_pattern_engine::CandlePatternEngineConfig;
use uzor::app_context::TextInputState;
use mlc_alert_delivery::{NotificationSecrets, NotificationSettings};

/// A resize drag in flight: which edge has hold, where the pointer was when it
/// took hold, and where and how big the frame was at that moment. Both are
/// derived from the ORIGINAL each move rather than accumulated, so a drag that
/// overshoots a limit and comes back lands where the pointer says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResizeDrag {
    pub edge: ResizeEdge,
    pub origin: (f64, f64),
    pub from_pos: (f64, f64),
    pub from_size: (f64, f64),
}

/// The geometry a resizable modal keeps and the three moves the input side
/// makes on it. Implemented by every settings modal that can be resized, so
/// the drag dispatcher has one shape to call instead of one per modal.
pub trait ResizableModal {
    fn dragged_size(&self) -> Option<(f64, f64)>;
    fn set_dragged_size(&mut self, size: Option<(f64, f64)>);
    fn dragged_position(&self) -> Option<(f64, f64)>;
    fn set_dragged_position(&mut self, pos: Option<(f64, f64)>);
    fn resize_drag(&self) -> Option<ResizeDrag>;
    fn set_resize_drag(&mut self, drag: Option<ResizeDrag>);
    /// The floor this modal may be dragged down to. Lives next to the state so
    /// the renderer and the drag handler cannot disagree about it.
    fn min_size(&self) -> (f64, f64);

    fn start_resize(
        &mut self,
        edge: ResizeEdge,
        mouse: (f64, f64),
        from_pos: (f64, f64),
        from_size: (f64, f64),
    ) {
        self.set_resize_drag(Some(ResizeDrag { edge, origin: mouse, from_pos, from_size }));
    }

    fn update_resize(&mut self, mouse: (f64, f64)) {
        let Some(drag) = self.resize_drag() else { return };
        let delta = (mouse.0 - drag.origin.0, mouse.1 - drag.origin.1);
        let (pos, size) = drag.edge.apply(drag.from_pos, drag.from_size, delta, self.min_size());
        self.set_dragged_size(Some(size));
        self.set_dragged_position(Some(pos));
    }

    /// Report whether a drag was in flight, so the caller knows to stop here
    /// rather than pass the release on to the chart.
    fn end_resize(&mut self) -> bool {
        let was = self.resize_drag().is_some();
        self.set_resize_drag(None);
        was
    }
}

macro_rules! impl_resizable_modal {
    ($($t:ty => ($w:expr, $h:expr)),+ $(,)?) => { $(
        impl ResizableModal for $t {
            fn dragged_size(&self) -> Option<(f64, f64)> { self.size }
            fn set_dragged_size(&mut self, size: Option<(f64, f64)>) { self.size = size; }
            fn dragged_position(&self) -> Option<(f64, f64)> { self.position }
            fn set_dragged_position(&mut self, pos: Option<(f64, f64)>) { self.position = pos; }
            fn resize_drag(&self) -> Option<ResizeDrag> { self.resize_drag }
            fn set_resize_drag(&mut self, drag: Option<ResizeDrag>) { self.resize_drag = drag; }
            fn min_size(&self) -> (f64, f64) { ($w, $h) }
        }
    )+ };
}

// Pure-data types relocated to mlc-core — re-exported here for layout-side widget code.
pub use mlc_core::state::modal_data::{
    SubPaneButton,
    SubPaneOverlayState,
    IndicatorDisplayInfo,
    IndicatorParamDef,
    IndicatorParamType,
    IndicatorOutputDef,
    IndicatorOutputType,
};

// =============================================================================
// TextFieldStore widget IDs for settings sub-field editing
//
// Each settings modal uses ONE WidgetId for all its inline sub-field edits.
// The `active_field_id: Option<String>` in each state struct carries the
// discriminant (which concrete sub-field is being edited); text/cursor/selection
// live in the TextFieldStore under these IDs.
// =============================================================================

/// Sub-field edit input in the Primitive Settings modal.
pub const PRIMITIVE_SETTINGS_FIELD: &str = "prim_settings_field";

/// Sub-field edit input in the Indicator Settings modal.
pub const INDICATOR_SETTINGS_FIELD: &str = "ind_settings_field";

/// Sub-field edit input in the Chart Settings modal (watermark text, etc.).
pub const CHART_SETTINGS_FIELD: &str = "chart_settings_field";

/// Sub-field edit input in the Compare Settings modal (tf min/max fields).
pub const COMPARE_SETTINGS_FIELD: &str = "cmp_settings_field";

// =============================================================================
// Render proxy for the active sub-field (populated by ChartApp before render)
// =============================================================================

/// Render-time snapshot of the currently focused sub-field.
///
/// ChartApp populates this from TextFieldStore before calling the layout render
/// function.  The layout layer reads `active_render` instead of the removed
/// `editing_text` / `editing_text_state` fields.
#[derive(Clone, Debug, Default)]
pub struct ActiveFieldRender {
    /// Text currently in the field (from TextFieldStore).
    pub text: String,
    /// Cursor character index.
    pub cursor: usize,
    /// Selection anchor (start of selection range, < cursor = forward selection).
    pub selection_start: Option<usize>,
    /// Whether the cursor blink phase is currently visible.
    pub cursor_visible: bool,
}

impl ActiveFieldRender {
    /// True if a selection exists (selection_start is Some and != cursor).
    pub fn has_selection(&self) -> bool {
        self.selection_start.map_or(false, |s| s != self.cursor)
    }

    /// Returns `(lo, hi)` selection byte-char bounds, or `None` if no selection.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_start.and_then(|s| {
            if s == self.cursor { None } else { Some((s.min(self.cursor), s.max(self.cursor))) }
        })
    }
}

// =============================================================================
// Preset Name Input State
// =============================================================================

/// Purpose of the preset name input modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetNameInputMode {
    /// Creating a new preset (Save As).
    SaveAs,
    /// Renaming an existing preset.
    Rename,
    /// Creating a brand-new blank chart (asks for a name before clearing).
    NewChart,
    /// Saving the current set of active indicators as a named indicator set.
    CreateIndicatorSet,
}

/// State for the preset name input modal dialog.
#[derive(Clone, Debug)]
pub struct PresetNameInputState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// What action this modal performs.
    pub mode: PresetNameInputMode,
    /// The preset ID being renamed (only used in Rename mode).
    pub rename_preset_id: Option<String>,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left (for smooth dragging).
    pub drag_offset: Option<(f64, f64)>,
}

impl Default for PresetNameInputState {
    fn default() -> Self {
        Self {
            is_open: false,
            mode: PresetNameInputMode::SaveAs,
            rename_preset_id: None,
            position: None,
            is_dragging: false,
            drag_offset: None,
        }
    }
}

impl PresetNameInputState {
    /// Open the modal for "Save As". Caller must seed TextFieldStore with the default name.
    pub fn open_save_as(&mut self, _default_name: &str, _current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::SaveAs;
        self.rename_preset_id = None;
    }

    /// Open the modal for "Rename". Caller must seed TextFieldStore with the current name.
    pub fn open_rename(&mut self, preset_id: &str, _current_name: &str, _current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::Rename;
        self.rename_preset_id = Some(preset_id.to_string());
    }

    /// Open the modal for "New Chart". Caller must seed TextFieldStore with the default name.
    pub fn open_new_chart(&mut self, _default_name: &str, _current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::NewChart;
        self.rename_preset_id = None;
    }

    /// Open the modal for "Create Indicator Set". Caller must seed TextFieldStore with the default name.
    pub fn open_create_indicator_set(&mut self, _default_name: &str, _current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::CreateIndicatorSet;
        self.rename_preset_id = None;
    }

    /// Close the modal and reset state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.rename_preset_id = None;
        self.is_dragging = false;
        self.drag_offset = None;
        // Don't reset position — preserve last drag position for reopening
    }

    /// Start dragging from the header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update drag position.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

// =============================================================================
// Share-Open Overlay State
// =============================================================================

/// State for the "Open shared chart…" overlay (native-share-viewer arc,
/// 2026-09-06) — reached from the `share_chart` (globe) toolbar button's
/// camera-style double-click dropdown by selecting
/// `mlc_core::toolbar::OPEN_SHARE_ACTION_ID`. One text field
/// (`mlc_app::text_input::SHARE_OPEN_INPUT`) plus Open/Cancel — no drag, no
/// tabs, no remembered position: the smallest shape a submit-a-token dialog
/// needs. Never gated by the `cloud` feature: reading a share link is
/// public, so a signed-out session may open one exactly like a signed-in
/// one (unlike `share_chart`'s OWN single-click action, which does gate on
/// sign-in).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShareOpenState {
    /// Whether the overlay is currently open.
    pub is_open: bool,
}

impl ShareOpenState {
    /// Open the overlay. The caller (the dropdown-select click handler)
    /// still owns seeding the `SHARE_OPEN_INPUT` field — this struct holds
    /// no text of its own, the same split every other modal field in this
    /// crate follows.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the overlay.
    pub fn close(&mut self) {
        self.is_open = false;
    }
}

// =============================================================================
// Slider Drag State
// =============================================================================

/// Which handle of a dual-handle slider is being dragged
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DualSliderHandle {
    Min,
    Max,
}

/// State for slider dragging in settings modal
#[derive(Clone, Debug)]
pub struct SliderDragState {
    /// Field being dragged (e.g., "stroke_width", "style_prop:label_font_size")
    pub field_id: String,
    /// Slider rect for calculating position
    pub slider_x: f64,
    pub slider_width: f64,
    /// Value range
    pub min_val: f64,
    pub max_val: f64,
    /// For dual-handle sliders (like tf_*_slider): which handle is being dragged
    pub dual_handle: Option<DualSliderHandle>,
    /// Floating (preview) value during drag — updated every mouse-move.
    /// The actual state is only written on drag-end.
    pub floating_value: Option<f64>,
    /// Second floating value for the opposite handle of dual sliders.
    /// Min when dragging Max, Max when dragging Min.
    pub floating_value2: Option<f64>,
}

// =============================================================================
// Primitive Settings Tab
// =============================================================================

/// Tab in the primitive settings modal — the vocabulary a primitive type
/// declares its settings surface INTO
/// (`mlc_core::drawing::primitives_v2::config::PropertyCategory`), not a
/// fixed set every primitive shows: `Subject` (the primitive's own FIRST
/// tab — the level ladder for a `Ladder`-archetype primitive, or the
/// former `Levels`/`Appearance` pair's line/fill content for every other
/// one, named via `Primitive::subject_tab_label()` — see
/// `crate::layout::modals::primitive_settings`'s own doc for the merge),
/// `Labels` (content/font/style/position), `Cells` (Table's own editable
/// cell grid, 2026-08-09 — the one primitive that has it), `Placement`
/// (points + timeframe visibility). [`all`](Self::all) lists every tab in
/// STRIP ORDER — the order this enum declares them in, fixed regardless of
/// which order a primitive happens to declare its own properties. Which of
/// these actually appear for a given primitive is `available_tabs`'s job
/// (`crate::layout::modals::primitive_settings`), not this type's: `Subject`
/// is unconditional (every primitive has one — a ladder, or a base-stroke
/// row), no primitive but `Table` has `Cells`
/// (`Primitive::table_rows().is_some()`), and `Placement` is unconditional
/// (the points table and timeframe chips are bespoke collections every
/// primitive has, not declared properties — see `PropertyCategory`'s own
/// doc).
///
/// Replaces the earlier five-tab set (Style/Text/Coordinates/Levels/
/// Visibility, retired 2026-08-08), that set's own three-tab successor
/// (Вид/Уровни/Размещение, same day), and the four-tab
/// Levels/Appearance/Labels/Placement set that followed it (Cells folded
/// in 2026-08-09; Levels and Appearance merged into `Subject` here, W1 of
/// the archetype-modals rollout,
/// `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`) — hard
/// cutover every time, no compatibility variants kept alongside these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PrimitiveSettingsTab {
    #[default]
    Subject,
    Labels,
    Cells,
    Placement,
}

impl PrimitiveSettingsTab {
    /// Get tab ID string
    pub fn id(&self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Labels => "labels",
            Self::Cells => "cells",
            Self::Placement => "placement",
        }
    }

    /// Get tab display label (localized via current language). `Subject`
    /// needs the live primitive — its own `Primitive::subject_tab_label()`
    /// — since the same tab position carries a different word per
    /// primitive (a ladder's own "Уровни", a wave's own "Волны", ...);
    /// every other tab's word is fixed regardless of which primitive is
    /// open.
    pub fn label(&self, prim: &dyn mlc_core::drawing::primitives_v2::Primitive) -> &'static str {
        use mlc_core::i18n::{ConfigKey, current_language};
        let lang = current_language();
        match self {
            Self::Subject => prim.subject_tab_label(),
            Self::Labels => ConfigKey::Labels.get(lang),
            Self::Cells => ConfigKey::TabCells.get(lang),
            Self::Placement => ConfigKey::TabPlacement.get(lang),
        }
    }

    /// Every tab this modal knows how to draw, in fixed STRIP order —
    /// `available_tabs` (`crate::layout::modals::primitive_settings`)
    /// filters this down to the ones a given primitive actually has
    /// anything for; this is never itself the per-primitive tab set.
    pub fn all() -> &'static [PrimitiveSettingsTab] {
        &[Self::Subject, Self::Labels, Self::Cells, Self::Placement]
    }
}

/// "время" (a formatted date/time, real) vs "индекс бара" (needs the
/// chart's own bar/timeframe data to convert a millisecond timestamp to a
/// bar index — see `crate::layout::modals::primitive_settings_placement`).
/// Kept as a real toggle either way so the inert side is visibly present,
/// not silently dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimeDisplayMode {
    #[default]
    Time,
    BarIndex,
}

// =============================================================================
// Primitive Settings State
// =============================================================================
//
// The standalone "Набор" row + its own dropdown were deleted 2026-08-08
// (owner review) — applying one of a primitive's own level-set presets
// moved into the footer Template menu as a real
// `mlc_core::templates::PrimitiveTemplate` system row
// (`PrimitiveTemplate::system_templates`, itself built off
// `Primitive::level_set_presets`). The closed, Fibonacci-shaped `LevelSet`
// enum that used to live here (`Standard`/`Extended`/`KeyOnly`/`All`/
// `Custom`) and `PrimitiveSettingsState::level_set` that cached one — a
// SECOND, hardcoded definition of "what a preset means" that could never
// answer correctly for a Gann/pitchfork/time-zones type — were deleted
// 2026-08-17 with nothing to replace them: no renderer ever read either
// one (the footer menu above answers this question already, per-type, via
// `Primitive::level_set_presets`). The generic, tested replacement for
// "which набор does this composition match" is
// `crate::layout::modals::primitive_settings_intent::level_set_options_and_current`
// — a pure function of `&dyn Primitive` alone, not a cached field, so it
// can never go stale the way the deleted field could.

/// State for primitive settings modal
#[derive(Clone, Debug, Default)]
pub struct PrimitiveSettingsState {
    /// Is modal open?
    pub is_open: bool,
    /// Stable id of the primitive being edited.
    pub primitive_id: Option<u64>,
    /// Current active tab. Not guaranteed to be one this primitive
    /// actually has (e.g. right after `open()`, which cannot see the
    /// primitive to check) — the render call
    /// (`crate::layout::modals::primitive_settings::render_primitive_settings_modal`,
    /// via its own `resolve_active_tab`) falls back to the first tab that
    /// IS available for the current primitive every time this is stale, so
    /// this field itself never needs to be kept in sync with which
    /// primitive is open.
    pub active_tab: PrimitiveSettingsTab,
    /// Modal position (for dragging)
    pub position: Option<(f64, f64)>,
    /// Size the user dragged this modal to. RAM-only, like `position`: a
    /// modal does not survive a reload, so neither does its geometry.
    pub size: Option<(f64, f64)>,
    /// Resize drag in flight, if the user has hold of an edge.
    pub resize_drag: Option<ResizeDrag>,
    /// Is header being dragged?
    pub is_dragging: bool,
    /// Drag offset from modal top-left corner
    pub drag_offset: Option<(f64, f64)>,
    /// Color picker state for inline toolbar and settings
    pub color_picker: ColorPickerState,
    /// Field being edited by color picker ("stroke_color", "text_color", etc.)
    pub color_picker_field: Option<String>,
    /// Currently hovered content item ID
    pub hovered_item_id: Option<String>,
    /// Which sub-field is currently being edited inside this modal.
    /// The field_id discriminant — text/cursor/selection live in TextFieldStore
    /// under `PRIMITIVE_SETTINGS_FIELD`.
    pub active_field_id: Option<String>,
    /// Render-time snapshot of the focused sub-field (populated by ChartApp before each render).
    pub active_render: ActiveFieldRender,
    /// Slider drag state for number sliders
    pub slider_drag: Option<SliderDragState>,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// Which `PropertyType::LineStyle`/`Select` row's split cycle+chevron
    /// control has its dropdown open (item 1, 2026-08-09 restore) — the
    /// row's own field id (`"line_style"`, `"style_prop:{id}"`, …), or
    /// `None` when none is. A single `Option`, not a per-row bool: setting
    /// it to a new row's id is what GUARANTEES only one of these dropdowns
    /// is ever open at once — the same shape `IndicatorSettingsState::
    /// open_param_dropdown` already uses for its own single-open-at-a-time
    /// dropdown. Cleared whenever the modal opens/closes or the click lands
    /// somewhere unrelated (`mlc-app`'s `input/settings.rs`, mirroring
    /// `template_dropdown_open`'s own close-on-unrelated-click rule).
    pub open_value_dropdown: Option<String>,
    // No `applied_template_id` here, deliberately (deleted 2026-08-09): which
    // Template-menu row reads as current is derived from the primitive's own
    // persisted `PrimitiveData::style_reference`, never from session state
    // that outlives what it describes. The other settings modals still carry
    // their own copy because they have no equivalent persisted reference yet.
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Template-menu drag-to-reorder: `(dragging_key, current_mouse_y)` —
    /// `key` is a row's own position key (`PrimitiveTemplate::id`, for a
    /// system row or a user template alike), the same mechanism
    /// `WatchlistModalState::drag_reorder` already uses for its own row
    /// list (item 4, 2026-08-08 review).
    pub template_drag_reorder: Option<(String, f64)>,
    /// Pending template-menu drag-to-reorder: `(key, start_x, start_y)` —
    /// promoted to `template_drag_reorder` only once the pointer has moved
    /// at least 5px from the start position, so a short click still reaches
    /// the row's own click handler (mirrors
    /// `WatchlistModalState::drag_reorder_pending`'s own doc for why).
    pub template_drag_reorder_pending: Option<(String, f64, f64)>,
    /// Drop target index during a template-menu drag-to-reorder.
    pub template_drop_index: Option<usize>,
    /// Scroll state for the tab content area. The label-plumbing pass (item 1
    /// of the primitives batch) grew the Style tab past a fixed-height body
    /// with nowhere for the overflow to go; this plus `ScrollableContainer`
    /// in the renderer is the fix.
    pub scroll: ScrollState,

    // =========================================================================
    // Уровни tab (`crate::layout::modals::primitive_settings_levels`) —
    // folded in from the ui-lab prototype's own `DesignUiState` (2026-08-08
    // port): the app owns this now, it is the real modal's state, not a lab
    // toy.
    // =========================================================================
    /// Level VALUES explicitly removed from the ladder view via "×" this
    /// session — independent of each entry's own `FibLevelConfig::visible`
    /// (the checkbox: dims a row, keeps it in the list; see
    /// `is_level_selected`'s sibling doc). The underlying
    /// `Primitive::level_configs()` is never touched by an exclusion — an
    /// excluded value's colour/width/style/fill survive untouched, so
    /// re-adding it via "+ добавить уровень" gets it back exactly as it
    /// was. Cleared on `open()`: reopening the modal always starts from
    /// everything the primitive actually has, never a stale hidden set from
    /// a previous session. Value-keyed rather than index-keyed because the
    /// ladder is a sorted, filtered VIEW over the primitive's own level
    /// list — a positional index would point at a different level the
    /// moment a row is added, removed, or resorted.
    pub levels_excluded: Vec<f64>,
    /// Level values marked "touched by hand" (colour/width/style/fill
    /// only — composition changes do not set this). Value-keyed for the
    /// same reason `levels_excluded` is.
    pub level_overridden: Vec<f64>,
    /// Level value whose inspector row is expanded, if any. Value-keyed for
    /// the same reason `levels_excluded` is.
    pub selected_level: Option<f64>,
    /// Index into the Levels tab's own palette table
    /// (`crate::layout::modals::primitive_settings_palettes::PALETTES`) for
    /// the chosen colour ramp.
    pub palette: usize,

    // =========================================================================
    // Размещение tab (`crate::layout::modals::primitive_settings_placement`)
    // =========================================================================
    pub time_display_mode: TimeDisplayMode,
}

/// Float-equality tolerance for level-value lookups (ratios like `0.618`
/// round-trip through `format!("{:.3}")` text fields, so an exact `==`
/// would reject a value it just displayed).
const LEVEL_VALUE_EPS: f64 = 1e-9;

impl PrimitiveSettingsState {
    /// Create new state (closed)
    pub fn new() -> Self {
        Self::default()
    }

    /// Open settings for a primitive
    ///
    /// Does not touch "Save as…" visibility: that reads
    /// `PrimitiveData::style_reference`, which lives on the primitive
    /// itself (persisted with the drawing) rather than in this
    /// session-only modal state — opening the modal is no longer a
    /// baseline-establishing moment (see `mlc_core::drawing::
    /// StyleReference`'s own doc for why a persisted reference replaced
    /// the open-time snapshot this method used to also reset here).
    pub fn open(&mut self, primitive_id: u64) {
        self.is_open = true;
        self.primitive_id = Some(primitive_id);
        // `Subject` is unconditional — every primitive has one
        // (`available_tabs`'s own doc) — so this is always a real tab for
        // whatever primitive is being opened, unlike the render call's own
        // `resolve_active_tab` fallback (`primitive_settings.rs`), which
        // exists for the OTHER tabs this value might carry over from a
        // previously open, differently-shaped primitive.
        self.active_tab = PrimitiveSettingsTab::Subject;
        self.scroll.reset();
        self.levels_excluded.clear();
        self.level_overridden.clear();
        self.selected_level = None;
        self.palette = 0;
        self.time_display_mode = TimeDisplayMode::Time;
        self.open_value_dropdown = None;
        // Don't reset position - keep last position for convenience
    }

    // =========================================================================
    // Levels tab — value-keyed bookkeeping (folded in from the ui-lab
    // prototype's `DesignUiState`, see the field docs above for why
    // value-keyed).
    // =========================================================================

    pub fn is_level_excluded(&self, level: f64) -> bool {
        self.levels_excluded.iter().any(|v| (*v - level).abs() < LEVEL_VALUE_EPS)
    }

    pub fn exclude_level(&mut self, level: f64) {
        if !self.is_level_excluded(level) {
            self.levels_excluded.push(level);
        }
    }

    /// Drop a value's exclusion — called when re-adding a level that was
    /// previously removed via "×", so it rejoins the ladder view.
    pub fn unexclude_level(&mut self, level: f64) {
        self.levels_excluded.retain(|v| (*v - level).abs() >= LEVEL_VALUE_EPS);
    }

    pub fn is_level_overridden(&self, level: f64) -> bool {
        self.level_overridden.iter().any(|v| (*v - level).abs() < LEVEL_VALUE_EPS)
    }

    pub fn mark_level_overridden(&mut self, level: f64) {
        if !self.is_level_overridden(level) {
            self.level_overridden.push(level);
        }
    }

    /// Drop a value's override flag — called when that level is deleted, so
    /// a re-added level at the same value starts clean rather than
    /// inheriting a stale "touched by hand" mark from a row that no longer
    /// exists.
    pub fn clear_level_overridden(&mut self, level: f64) {
        self.level_overridden.retain(|v| (*v - level).abs() >= LEVEL_VALUE_EPS);
    }

    pub fn is_level_selected(&self, level: f64) -> bool {
        self.selected_level.map(|s| (s - level).abs() < LEVEL_VALUE_EPS).unwrap_or(false)
    }

    /// Close the modal
    pub fn close(&mut self) {
        self.is_open = false;
        self.primitive_id = None;
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        // Clear editing state so stale active_field_id cannot intercept keypresses
        // in other modals after this one is closed.
        self.active_field_id = None;
        self.slider_drag = None;
        self.text_select_dragging = false;
        self.template_dropdown_open = false;
        self.open_value_dropdown = None;
        self.save_template_mode = false;
        self.template_drag_reorder = None;
        self.template_drag_reorder_pending = None;
        self.template_drop_index = None;
    }

    /// Check if open
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Set active tab
    pub fn set_tab(&mut self, tab: PrimitiveSettingsTab) {
        self.active_tab = tab;
        self.scroll.reset();
    }

    /// Set position (for dragging)
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.position = Some((x, y));
    }

    /// Start dragging the modal header
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Check if currently dragging (modal header)
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    // =========================================================================
    // Slider drag methods
    // =========================================================================

    /// Start dragging a slider
    pub fn start_slider_drag(&mut self, field_id: &str, slider_x: f64, slider_width: f64, min_val: f64, max_val: f64) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x,
            slider_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Start dragging a dual-handle slider (min/max range)
    pub fn start_dual_slider_drag(&mut self, field_id: &str, slider_x: f64, slider_width: f64, min_val: f64, max_val: f64, handle: DualSliderHandle) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x,
            slider_width,
            min_val,
            max_val,
            dual_handle: Some(handle),
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Update floating value during drag (does NOT write to permanent state).
    /// Returns `Some((field_id, value))` with the current pointer position mapped
    /// to a value so the caller can update `floating_value` and trigger a repaint.
    pub fn update_slider_drag_float(&mut self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref mut drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Update slider value during drag - returns (field_id, new_value) if dragging.
    /// Also updates floating_value so the renderer can show the preview.
    pub fn update_slider_drag(&self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Get the current floating (preview) value if a drag is active.
    /// Returns `(field_id, value, dual_handle)`.
    pub fn get_floating_drag(&self) -> Option<(&str, f64, Option<DualSliderHandle>)> {
        self.slider_drag.as_ref().and_then(|d| {
            d.floating_value.map(|v| (d.field_id.as_str(), v, d.dual_handle))
        })
    }

    /// Get dual slider handle being dragged (if any)
    pub fn dual_slider_handle(&self) -> Option<DualSliderHandle> {
        self.slider_drag.as_ref().and_then(|s| s.dual_handle)
    }

    /// End slider dragging — returns the final floating value (if any) for committing.
    /// Clears drag state.
    pub fn end_slider_drag(&mut self) {
        self.slider_drag = None;
    }

    /// Consume the final floating value at drag-end and clear drag state.
    /// Returns `(field_id, value, dual_handle)` if there was a floating value.
    pub fn take_slider_drag_value(&mut self) -> Option<(String, f64, Option<DualSliderHandle>)> {
        if let Some(drag) = self.slider_drag.take() {
            drag.floating_value.map(|v| (drag.field_id, v, drag.dual_handle))
        } else {
            None
        }
    }

    /// Check if currently dragging a slider
    pub fn is_slider_dragging(&self) -> bool {
        self.slider_drag.is_some()
    }

    /// Get slider drag field_id if dragging
    pub fn slider_drag_field(&self) -> Option<&str> {
        self.slider_drag.as_ref().map(|s| s.field_id.as_str())
    }

    /// Start slider drag from track info (used with SliderTrackInfo from render result).
    /// Sets an initial floating_value based on click_x so the handle jumps to pointer.
    pub fn start_slider_drag_from_track(&mut self, field_id: &str, track_x: f64, track_width: f64, min_val: f64, max_val: f64) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Start dual slider drag from track info, selecting the nearest handle.
    pub fn start_dual_slider_drag_from_track(
        &mut self,
        field_id: &str,
        track_x: f64,
        track_width: f64,
        min_val: f64,
        max_val: f64,
        handle: DualSliderHandle,
        initial_click_x: f64,
    ) {
        let t = ((initial_click_x - track_x) / track_width).clamp(0.0, 1.0);
        let initial_value = min_val + t * (max_val - min_val);
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: Some(handle),
            floating_value: Some(initial_value),
            floating_value2: None,
        });
    }

    // =========================================================================
    // Color Picker methods
    // =========================================================================

    /// Open color picker for a field with smart positioning
    pub fn open_color_picker_smart(
        &mut self,
        field: &str,
        anchor_x: f64,
        anchor_y: f64,
        anchor_w: f64,
        anchor_h: f64,
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.color_picker_field = Some(field.to_string());
        self.color_picker.open_l1_smart(
            anchor_x, anchor_y,
            anchor_w, anchor_h,
            window_w, window_h,
            current_color,
        );
    }

    /// Close color picker
    pub fn close_color_picker(&mut self) {
        self.color_picker.close();
        self.color_picker_field = None;
    }

    /// Close one level of color picker: L2→L1 or L1→Closed.
    pub fn close_color_picker_one_level(&mut self) {
        use crate::ui::widgets::ColorPickerLevel;
        match self.color_picker.level {
            ColorPickerLevel::L2 => self.color_picker.back_to_l1(),
            ColorPickerLevel::L1 => self.close_color_picker(),
            ColorPickerLevel::Closed => {}
        }
    }

    /// Check if color picker is open
    pub fn is_color_picker_open(&self) -> bool {
        self.color_picker.is_open()
    }

    /// Get selected color from picker (if any)
    pub fn get_color_picker_result(&self) -> Option<(&str, &str)> {
        if let Some(ref field) = self.color_picker_field {
            if !self.color_picker.current_color.is_empty() {
                return Some((field.as_str(), self.color_picker.current_color.as_str()));
            }
        }
        None
    }
}

// =============================================================================
// Chart Settings Tab
// =============================================================================

/// Tab in chart settings modal.
///
/// Trading, Alerts, and Events tabs have been removed — they do not belong
/// in the per-chart settings modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChartSettingsTab {
    /// Инструмент - Symbol/instrument settings (candle colors, precision, timezone)
    #[default]
    Instrument,
    /// Строка статуса - Status line options (legend, tooltip, watermark)
    StatusLine,
    /// Шкалы и линии - Scales and grid lines
    ScalesLines,
    /// Оформление - Appearance (colors, theme, UI style)
    Appearance,
}

impl ChartSettingsTab {
    /// Get tab ID string
    pub fn id(&self) -> &'static str {
        match self {
            Self::Instrument => "instrument",
            Self::StatusLine => "status_line",
            Self::ScalesLines => "scales_lines",
            Self::Appearance => "appearance",
        }
    }

    /// Get tab display label (localized via current language)
    pub fn label(&self) -> &'static str {
        use mlc_core::i18n::{SettingsKey, current_language};
        let key = match self {
            Self::Instrument => SettingsKey::TabInstrument,
            Self::StatusLine => SettingsKey::TabStatusLine,
            Self::ScalesLines => SettingsKey::TabScalesLines,
            Self::Appearance => SettingsKey::TabAppearance,
        };
        key.get(current_language())
    }

    /// Get tab icon ID
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Instrument => "candlestick",
            Self::StatusLine => "info",
            Self::ScalesLines => "grid",
            Self::Appearance => "palette",
        }
    }

    /// Parse tab from string ID
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "instrument" => Some(Self::Instrument),
            "status_line" => Some(Self::StatusLine),
            "scales_lines" => Some(Self::ScalesLines),
            "appearance" => Some(Self::Appearance),
            _ => None,
        }
    }

    /// All available tabs
    pub fn all() -> &'static [ChartSettingsTab] {
        &[
            Self::Instrument,
            Self::StatusLine,
            Self::ScalesLines,
            Self::Appearance,
        ]
    }

    /// Get tab from index
    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    /// Get index of this tab
    pub fn index(&self) -> usize {
        Self::all().iter().position(|t| t == self).unwrap_or(0)
    }
}

// =============================================================================
// Chart Settings State
// =============================================================================

/// State for chart settings modal
#[derive(Clone, Debug)]
pub struct ChartSettingsState {
    /// Whether the modal is currently open
    pub is_open: bool,
    /// Current active tab
    pub active_tab: ChartSettingsTab,
    /// Modal position (for dragging)
    pub position: Option<(f64, f64)>,
    /// Size the user dragged this modal to. RAM-only, like `position`: a
    /// modal does not survive a reload, so neither does its geometry.
    pub size: Option<(f64, f64)>,
    /// Resize drag in flight, if the user has hold of an edge.
    pub resize_drag: Option<ResizeDrag>,
    /// Is header being dragged?
    pub is_dragging: bool,
    /// Drag offset from modal top-left corner
    pub drag_offset: Option<(f64, f64)>,
    /// Color picker state
    pub color_picker: ColorPickerState,
    /// Field being edited by color picker
    pub color_picker_field: Option<String>,
    /// Currently hovered item ID
    pub hovered_item_id: Option<String>,
    /// Dropdown state (field_id, is_open)
    pub active_dropdown: Option<String>,
    /// Scroll state for scales tab content
    pub scroll: ScrollState,
    /// Slider drag state (for appearance style opacity sliders)
    pub slider_drag: Option<SliderDragState>,
    /// Text input editing state (for slider value editing)
    pub text_input: TextInputState,
    /// Which sub-field is currently being edited inside this modal.
    /// Text/cursor/selection live in TextFieldStore under `CHART_SETTINGS_FIELD`.
    pub active_field_id: Option<String>,
    /// Render-time snapshot of the focused sub-field (populated by ChartApp before each render).
    pub active_render: ActiveFieldRender,
    /// Instrument checkbox: use previous close color
    pub instrument_use_prev_close: bool,
    /// Instrument checkbox: body enabled
    pub instrument_body_enabled: bool,
    /// Instrument checkbox: border enabled
    pub instrument_border_enabled: bool,
    /// Instrument checkbox: wick enabled
    pub instrument_wick_enabled: bool,
    /// Footprint checkbox: draw ABS-marker (absorption tint + "ABS" label).
    pub footprint_show_abs_markers: bool,
    /// Footprint checkbox: draw per-cell buy/sell volume numbers.
    pub footprint_show_volume_text: bool,
    /// Tick-bar checkbox: modulate body opacity by trade-count intensity.
    /// Default `false` — plain solid bodies exactly like `draw_candles`.
    pub tick_intensity: bool,
    /// Bar-family "Wicks" checkbox: re-enable wicks for range_bar /
    /// volume_bar / dollar_bar (they render WITHOUT wicks by default).
    /// Default `false`.
    pub bar_family_wicks: bool,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Hovered footer button
    pub hovered_footer_button: Option<String>,
}

impl Default for ChartSettingsState {
    fn default() -> Self {
        Self {
            size: None,
            resize_drag: None,
            is_open: false,
            active_tab: ChartSettingsTab::default(),
            position: None,
            is_dragging: false,
            drag_offset: None,
            color_picker: ColorPickerState::default(),
            color_picker_field: None,
            hovered_item_id: None,
            active_dropdown: None,
            scroll: ScrollState::default(),
            slider_drag: None,
            text_input: TextInputState::default(),
            active_field_id: None,
            active_render: ActiveFieldRender::default(),
            instrument_use_prev_close: false,
            instrument_body_enabled: true,
            instrument_border_enabled: true,
            instrument_wick_enabled: true,
            footprint_show_abs_markers: true,
            footprint_show_volume_text: true,
            tick_intensity: false,
            bar_family_wicks: false,
            text_select_dragging: false,
            template_dropdown_open: false,
            applied_template_id: None,
            save_template_mode: false,
            hovered_footer_button: None,
        }
    }
}

impl ChartSettingsState {
    /// Create new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the chart settings modal
    pub fn open(&mut self) {
        self.is_open = true;
        self.reset();
    }

    /// Close the chart settings modal
    pub fn close(&mut self) {
        self.is_open = false;
        // Clear editing state to prevent stale active_field_id from intercepting
        // keypresses in other modals after chart settings is closed.
        self.active_field_id = None;
        self.slider_drag = None;
        self.text_select_dragging = false;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.hovered_footer_button = None;
    }

    /// Toggle open/close
    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Reset to default state (on modal open)
    pub fn reset(&mut self) {
        self.active_tab = ChartSettingsTab::default();
        self.color_picker.close();
        self.color_picker_field = None;
        self.hovered_item_id = None;
        self.active_dropdown = None;
        self.scroll.reset();
        self.text_input.clear();
        // Keep position for convenience
    }

    /// Set active tab
    pub fn set_tab(&mut self, tab: ChartSettingsTab) {
        if self.active_tab != tab {
            self.active_tab = tab;
            // Close any open dropdowns when switching tabs
            self.active_dropdown = None;
            // Reset scroll when switching tabs
            self.scroll.reset();
        }
    }

    /// Set position (for dragging)
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.position = Some((x, y));
    }

    /// Start dragging the modal header
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Toggle dropdown
    pub fn toggle_dropdown(&mut self, field_id: &str) {
        if self.active_dropdown.as_deref() == Some(field_id) {
            self.active_dropdown = None;
        } else {
            self.active_dropdown = Some(field_id.to_string());
        }
    }

    /// Close dropdown
    pub fn close_dropdown(&mut self) {
        self.active_dropdown = None;
    }

    /// Open color picker for a field
    pub fn open_color_picker_smart(
        &mut self,
        field: &str,
        anchor_x: f64,
        anchor_y: f64,
        anchor_w: f64,
        anchor_h: f64,
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.color_picker_field = Some(field.to_string());
        self.color_picker.open_l1_smart(
            anchor_x, anchor_y,
            anchor_w, anchor_h,
            window_w, window_h,
            current_color,
        );
    }

    /// Close color picker
    pub fn close_color_picker(&mut self) {
        self.color_picker.close();
        self.color_picker_field = None;
    }

    /// Close one level of color picker: L2→L1 or L1→Closed.
    pub fn close_color_picker_one_level(&mut self) {
        use crate::ui::widgets::ColorPickerLevel;
        match self.color_picker.level {
            ColorPickerLevel::L2 => self.color_picker.back_to_l1(),
            ColorPickerLevel::L1 => self.close_color_picker(),
            ColorPickerLevel::Closed => {}
        }
    }

    /// Check if color picker is open
    pub fn is_color_picker_open(&self) -> bool {
        self.color_picker.is_open()
    }

    /// Get selected color from picker (if any)
    pub fn get_color_picker_result(&self) -> Option<(&str, &str)> {
        if let Some(ref field) = self.color_picker_field {
            if !self.color_picker.current_color.is_empty() {
                return Some((field.as_str(), self.color_picker.current_color.as_str()));
            }
        }
        None
    }

    /// Start slider drag from track info (used with SliderTrackInfo from render result).
    /// Sets initial floating_value based on click_x.
    pub fn start_slider_drag_from_track(&mut self, field_id: &str, track_x: f64, track_width: f64, min_val: f64, max_val: f64) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Check if currently dragging a slider
    pub fn is_slider_dragging(&self) -> bool {
        self.slider_drag.is_some()
    }

    /// Update floating value during drag. Does NOT write to permanent state.
    /// Returns (field_id, value) for the caller to trigger a repaint.
    pub fn update_slider_drag_float(&mut self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref mut drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Update slider value during drag - returns (field_id, new_value) if dragging
    pub fn update_slider_drag(&self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Consume the final floating value at drag-end and clear drag state.
    /// Returns (field_id, value, dual_handle) if there was a floating value.
    pub fn take_slider_drag_value(&mut self) -> Option<(String, f64, Option<DualSliderHandle>)> {
        if let Some(drag) = self.slider_drag.take() {
            drag.floating_value.map(|v| (drag.field_id, v, drag.dual_handle))
        } else {
            None
        }
    }

    /// Get the current floating (preview) value if a drag is active.
    pub fn get_floating_drag(&self) -> Option<(&str, f64, Option<DualSliderHandle>)> {
        self.slider_drag.as_ref().and_then(|d| {
            d.floating_value.map(|v| (d.field_id.as_str(), v, d.dual_handle))
        })
    }

    /// End slider dragging (discards floating value without applying).
    pub fn end_slider_drag(&mut self) {
        self.slider_drag = None;
    }

    /// Start editing slider value via text input
    pub fn start_slider_value_edit(&mut self, field_id: &str, current_value: f64, current_time_ms: u64) {
        self.text_input.start_editing_with_time(field_id, &format!("{:.2}", current_value), current_time_ms);
    }

    /// Get edited slider value (parses text as f64, returns None if invalid)
    pub fn get_edited_slider_value(&self) -> Option<f64> {
        if !self.text_input.is_active() {
            return None;
        }
        self.text_input.get_text().parse::<f64>().ok()
    }
}

// =============================================================================
// Indicator Settings Tab
// =============================================================================

/// Tab in indicator settings modal
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IndicatorSettingsTab {
    /// Параметры - Input parameters (period, source, etc.)
    #[default]
    Inputs,
    /// Стиль - Output styling (colors, line widths)
    Style,
    /// Видимость - Timeframe visibility
    Visibility,
    /// Сигналы - Signal detection settings
    Signals,
    /// Инфо - Description, metadata
    Info,
}

impl IndicatorSettingsTab {
    /// Get tab ID string
    pub fn id(&self) -> &'static str {
        match self {
            Self::Inputs => "inputs",
            Self::Style => "style",
            Self::Visibility => "visibility",
            Self::Signals => "signals",
            Self::Info => "info",
        }
    }

    /// Get tab display label (localized via current language)
    pub fn label(&self) -> &'static str {
        use mlc_core::i18n::{ConfigKey, IndicatorKey, SettingsKey, ToolbarTooltipKey, current_language};
        let lang = current_language();
        match self {
            Self::Inputs => IndicatorKey::TabInputs.get(lang),
            Self::Style => ConfigKey::Style.get(lang),
            Self::Visibility => ConfigKey::Visibility.get(lang),
            Self::Signals => ToolbarTooltipKey::Signals.get(lang),
            Self::Info => SettingsKey::CompareTabInfo.get(lang),
        }
    }

    /// Get tab icon ID
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Inputs => "settings",
            Self::Style => "palette",
            Self::Visibility => "eye",
            Self::Signals => "alert",
            Self::Info => "info",
        }
    }

    /// All available tabs
    pub fn all() -> &'static [IndicatorSettingsTab] {
        &[
            Self::Inputs,
            Self::Style,
            Self::Visibility,
            Self::Signals,
            Self::Info,
        ]
    }

    /// Get tab from index
    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    /// Get index of this tab
    pub fn index(&self) -> usize {
        Self::all().iter().position(|t| t == self).unwrap_or(0)
    }
}

// =============================================================================
// Indicator Settings State
// =============================================================================

/// State for indicator settings modal
#[derive(Clone, Debug, Default)]
pub struct IndicatorSettingsState {
    /// Indicator instance ID being edited
    pub indicator_id: Option<u64>,
    /// Current active tab
    pub active_tab: IndicatorSettingsTab,
    /// Modal position (for dragging)
    pub position: Option<(f64, f64)>,
    /// Size the user dragged this modal to. RAM-only, like `position`: a
    /// modal does not survive a reload, so neither does its geometry.
    pub size: Option<(f64, f64)>,
    /// Resize drag in flight, if the user has hold of an edge.
    pub resize_drag: Option<ResizeDrag>,
    /// Is header being dragged?
    pub is_dragging: bool,
    /// Drag offset from modal top-left corner
    pub drag_offset: Option<(f64, f64)>,
    /// Color picker state
    pub color_picker: ColorPickerState,
    /// Field being edited by color picker
    pub color_picker_field: Option<String>,
    /// Currently hovered item ID
    pub hovered_item_id: Option<String>,
    /// Scroll state for content area
    pub scroll: ScrollState,
    /// Currently editing input field (param name) - LEGACY, use active_field_id
    pub editing_field: Option<String>,
    /// Text being edited - LEGACY (still used as renderer fallback)
    pub editing_text: String,
    /// Cursor position in editing text - LEGACY (still used as renderer fallback)
    pub editing_cursor: usize,
    /// Hovered footer button
    pub hovered_footer_button: Option<String>,
    /// Which sub-field is currently being edited inside this modal.
    /// Text/cursor/selection live in TextFieldStore under `INDICATOR_SETTINGS_FIELD`.
    pub active_field_id: Option<String>,
    /// Render-time snapshot of the focused sub-field (populated by ChartApp before each render).
    pub active_render: ActiveFieldRender,
    /// Slider drag state for dual-handle sliders
    pub slider_drag: Option<SliderDragState>,
    /// Currently open parameter dropdown menu (param_name)
    pub open_param_dropdown: Option<String>,
    /// Venues offered for a `Venues` parameter, in display order, each with
    /// whether it is currently chosen. Populated by `ChartApp` before every
    /// render while this modal is open — the list is HOST state (which
    /// connectors serve this layer's declared stream), so it cannot live on
    /// the indicator definition, which is static and shared.
    pub venue_options: Vec<(String, bool)>,
    /// True while the stored set is EMPTY, i.e. the layer follows every
    /// offered venue. Every row is ticked in that state, so the ticks alone
    /// cannot distinguish it from a set that happens to name all of them —
    /// and the difference is real: the empty set keeps following the offered
    /// list as connectors come and go.
    pub venue_all: bool,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
}

impl IndicatorSettingsState {
    /// Create new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Open settings for an indicator
    pub fn open(&mut self, indicator_id: u64) {
        self.indicator_id = Some(indicator_id);
        self.active_tab = IndicatorSettingsTab::default();
        self.color_picker.close();
        self.color_picker_field = None;
        self.hovered_item_id = None;
        self.scroll.reset();
        self.editing_field = None;
        self.editing_text.clear();
        self.editing_cursor = 0;
        self.hovered_footer_button = None;
        self.active_field_id = None;
        self.slider_drag = None;
        self.open_param_dropdown = None;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        // Keep position for convenience
    }

    /// Close the modal
    pub fn close(&mut self) {
        self.indicator_id = None;
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.color_picker.close();
        self.color_picker_field = None;
        self.hovered_item_id = None;
        self.scroll.reset();
        self.editing_field = None;
        self.editing_text.clear();
        self.editing_cursor = 0;
        self.hovered_footer_button = None;
        self.active_field_id = None;
        self.slider_drag = None;
        self.open_param_dropdown = None;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
    }

    /// Check if open
    pub fn is_open(&self) -> bool {
        self.indicator_id.is_some()
    }

    /// Set active tab
    pub fn set_tab(&mut self, tab: IndicatorSettingsTab) {
        if self.active_tab != tab {
            self.active_tab = tab;
            // Reset scroll when switching tabs
            self.scroll.reset();
            // Close any open dropdown when switching tabs
            self.open_param_dropdown = None;
        }
    }

    /// Set position (for dragging)
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.position = Some((x, y));
    }

    /// Start dragging the modal header
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Open color picker for a field
    pub fn open_color_picker_smart(
        &mut self,
        field: &str,
        anchor_x: f64,
        anchor_y: f64,
        anchor_w: f64,
        anchor_h: f64,
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.color_picker_field = Some(field.to_string());
        self.color_picker.open_l1_smart(
            anchor_x, anchor_y,
            anchor_w, anchor_h,
            window_w, window_h,
            current_color,
        );
    }

    /// Close color picker
    pub fn close_color_picker(&mut self) {
        self.color_picker.close();
        self.color_picker_field = None;
    }

    /// Close one level of color picker: L2→L1 or L1→Closed.
    pub fn close_color_picker_one_level(&mut self) {
        use crate::ui::widgets::ColorPickerLevel;
        match self.color_picker.level {
            ColorPickerLevel::L2 => self.color_picker.back_to_l1(),
            ColorPickerLevel::L1 => self.close_color_picker(),
            ColorPickerLevel::Closed => {}
        }
    }

    /// Check if color picker is open
    pub fn is_color_picker_open(&self) -> bool {
        self.color_picker.is_open()
    }

    /// Start editing a parameter field
    pub fn start_editing(&mut self, field: &str, current_value: &str) {
        self.editing_field = Some(field.to_string());
        self.editing_text = current_value.to_string();
        self.editing_cursor = current_value.chars().count();
    }

    /// Stop editing and return the final value
    pub fn stop_editing(&mut self) -> Option<(String, String)> {
        if let Some(field) = self.editing_field.take() {
            let value = std::mem::take(&mut self.editing_text);
            self.editing_cursor = 0;
            Some((field, value))
        } else {
            None
        }
    }

    /// Cancel editing without saving
    pub fn cancel_editing(&mut self) {
        self.editing_field = None;
        self.editing_text.clear();
        self.editing_cursor = 0;
    }

    /// Check if currently editing a field
    pub fn is_editing(&self) -> bool {
        self.editing_field.is_some()
    }

    /// Check if editing specific field
    pub fn is_editing_field(&self, field: &str) -> bool {
        self.editing_field.as_deref() == Some(field)
    }

    /// Handle character input while editing
    pub fn handle_char(&mut self, c: char) {
        if self.editing_field.is_some() {
            let byte_pos = self.editing_text
                .char_indices()
                .nth(self.editing_cursor)
                .map(|(b, _)| b)
                .unwrap_or(self.editing_text.len());
            self.editing_text.insert(byte_pos, c);
            self.editing_cursor += 1;
        }
    }

    /// Handle backspace while editing
    pub fn handle_backspace(&mut self) {
        if self.editing_field.is_some() && self.editing_cursor > 0 {
            self.editing_cursor -= 1;
            let byte_start = self.editing_text
                .char_indices()
                .nth(self.editing_cursor)
                .map(|(b, _)| b)
                .unwrap_or(self.editing_text.len());
            let byte_end = self.editing_text
                .char_indices()
                .nth(self.editing_cursor + 1)
                .map(|(b, _)| b)
                .unwrap_or(self.editing_text.len());
            self.editing_text.drain(byte_start..byte_end);
        }
    }

    /// Handle delete while editing
    pub fn handle_delete(&mut self) {
        if self.editing_field.is_some() && self.editing_cursor < self.editing_text.chars().count() {
            let byte_start = self.editing_text
                .char_indices()
                .nth(self.editing_cursor)
                .map(|(b, _)| b)
                .unwrap_or(self.editing_text.len());
            let byte_end = self.editing_text
                .char_indices()
                .nth(self.editing_cursor + 1)
                .map(|(b, _)| b)
                .unwrap_or(self.editing_text.len());
            self.editing_text.drain(byte_start..byte_end);
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.editing_cursor > 0 {
            self.editing_cursor -= 1;
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.editing_cursor < self.editing_text.chars().count() {
            self.editing_cursor += 1;
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        self.editing_cursor = 0;
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        self.editing_cursor = self.editing_text.chars().count();
    }

    /// Start slider drag from track info (used with SliderTrackInfo from render result).
    pub fn start_slider_drag_from_track(&mut self, field_id: &str, track_x: f64, track_width: f64, min_val: f64, max_val: f64) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Start dual slider drag from track info, selecting the nearest handle.
    pub fn start_dual_slider_drag_from_track(
        &mut self,
        field_id: &str,
        track_x: f64,
        track_width: f64,
        min_val: f64,
        max_val: f64,
        handle: DualSliderHandle,
        initial_click_x: f64,
    ) {
        let t = ((initial_click_x - track_x) / track_width).clamp(0.0, 1.0);
        let initial_value = min_val + t * (max_val - min_val);
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: Some(handle),
            floating_value: Some(initial_value),
            floating_value2: None,
        });
    }

    /// Check if currently dragging a slider
    pub fn is_slider_dragging(&self) -> bool {
        self.slider_drag.is_some()
    }

    /// Update floating value during drag. Does NOT write to permanent state.
    pub fn update_slider_drag_float(&mut self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref mut drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Update slider value during drag - returns (field_id, new_value) if dragging
    pub fn update_slider_drag(&self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Get dual slider handle being dragged (if any)
    pub fn dual_slider_handle(&self) -> Option<DualSliderHandle> {
        self.slider_drag.as_ref().and_then(|s| s.dual_handle)
    }

    /// Get the current floating (preview) value if a drag is active.
    pub fn get_floating_drag(&self) -> Option<(&str, f64, Option<DualSliderHandle>)> {
        self.slider_drag.as_ref().and_then(|d| {
            d.floating_value.map(|v| (d.field_id.as_str(), v, d.dual_handle))
        })
    }

    /// Consume the final floating value at drag-end and clear drag state.
    /// Returns (field_id, value, dual_handle) if there was a floating value.
    pub fn take_slider_drag_value(&mut self) -> Option<(String, f64, Option<DualSliderHandle>)> {
        if let Some(drag) = self.slider_drag.take() {
            drag.floating_value.map(|v| (drag.field_id, v, drag.dual_handle))
        } else {
            None
        }
    }

    /// End slider dragging (discards floating value without applying).
    pub fn end_slider_drag(&mut self) {
        self.slider_drag = None;
    }
}

// =============================================================================
// Indicator Overlay State
// =============================================================================

/// State for indicator overlay dropdown in chart area
#[derive(Clone, Debug)]
pub struct IndicatorOverlayState {
    /// Should the indicator overlay be visible?
    pub visible: bool,
    /// Is dropdown open?
    pub is_open: bool,
    /// Currently hovered indicator instance ID
    pub hovered_indicator_id: Option<u64>,
    /// Currently hovered action button (visibility, settings, delete, more)
    pub hovered_action: Option<String>,
    /// Is the main button hovered?
    pub button_hovered: bool,
}

impl Default for IndicatorOverlayState {
    fn default() -> Self {
        Self {
            visible: true,
            is_open: false,
            hovered_indicator_id: None,
            hovered_action: None,
            button_hovered: false,
        }
    }
}

impl IndicatorOverlayState {
    /// Create new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle dropdown open/closed
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if !self.is_open {
            self.clear_hover();
        }
    }

    /// Open dropdown
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close dropdown
    pub fn close(&mut self) {
        self.is_open = false;
        self.clear_hover();
    }

    /// Clear all hover states
    pub fn clear_hover(&mut self) {
        self.hovered_indicator_id = None;
        self.hovered_action = None;
    }

    /// Set hover state for an indicator row
    pub fn set_hover(&mut self, indicator_id: u64, action: Option<&str>) {
        self.hovered_indicator_id = Some(indicator_id);
        self.hovered_action = action.map(|s| s.to_string());
    }

    /// Check if a specific indicator is hovered
    pub fn is_indicator_hovered(&self, indicator_id: u64) -> bool {
        self.hovered_indicator_id == Some(indicator_id)
    }

    /// Check if a specific action is hovered for an indicator
    pub fn is_action_hovered(&self, indicator_id: u64, action: &str) -> bool {
        self.hovered_indicator_id == Some(indicator_id)
            && self.hovered_action.as_deref() == Some(action)
    }
}

// SubPaneButton and SubPaneOverlayState relocated to mlc-core::state::modal_data.
// Re-exported at the top of this file via `pub use mlc_core::state::modal_data::...`.

// =============================================================================
// Compare Settings Modal State
// =============================================================================

/// Which tab is active in the Compare Settings modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CompareSettingsTab {
    #[default]
    Style,
    Visibility,
    Info,
}

impl CompareSettingsTab {
    /// Get tab ID string
    pub fn id(&self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Visibility => "visibility",
            Self::Info => "info",
        }
    }

    /// Get tab display label (localized via current language)
    pub fn label(&self) -> &'static str {
        use mlc_core::i18n::{SettingsKey, current_language};
        let lang = current_language();
        match self {
            Self::Style => SettingsKey::CompareTabStyle.get(lang),
            Self::Visibility => SettingsKey::CompareTabVisibility.get(lang),
            Self::Info => SettingsKey::CompareTabInfo.get(lang),
        }
    }

    /// All tabs
    pub fn all() -> &'static [CompareSettingsTab] {
        &[Self::Style, Self::Visibility, Self::Info]
    }

    /// Parse from string ID
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "style" => Some(Self::Style),
            "visibility" => Some(Self::Visibility),
            "info" => Some(Self::Info),
            _ => None,
        }
    }
}

/// State for the Compare Settings modal.
///
/// Opened when clicking the settings button on a compare series row in the
/// chevron overlay.  Provides Style / Visibility / Info tabs.
///
/// Caches display data from the `CompareSeries` at open time so the renderer
/// does not need a live reference to the series during the render pass.
#[derive(Clone, Debug)]
pub struct CompareSettingsState {
    /// Whether the modal is open.
    pub is_open: bool,
    /// Index of the compare series being edited.
    pub series_index: usize,
    /// Active tab.
    pub active_tab: CompareSettingsTab,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Size the user dragged this modal to. RAM-only, like `position`: a
    /// modal does not survive a reload, so neither does its geometry.
    pub size: Option<(f64, f64)>,
    /// Resize drag in flight, if the user has hold of an edge.
    pub resize_drag: Option<ResizeDrag>,
    /// Whether the modal header is being dragged.
    pub is_dragging: bool,
    /// Drag offset from modal top-left corner.
    pub drag_offset: Option<(f64, f64)>,
    /// Color picker state (for the line color swatch in the Style tab).
    pub color_picker: ColorPickerState,
    /// Which field the color picker was opened for (e.g. "line_color").
    pub color_picker_field: Option<String>,
    /// Whether the line-style dropdown is open.
    pub line_style_dropdown_open: bool,
    /// Whether the chart-type dropdown is open.
    pub chart_type_dropdown_open: bool,
    /// Hovered item ID (for hover highlight).
    pub hovered_item_id: Option<String>,
    /// Slider drag state (line width slider).
    pub slider_drag: Option<SliderDragState>,
    // ---- Cached display data (copied from CompareSeries at open time) ----
    /// Cached symbol ticker (for display in Info tab and title).
    pub cached_symbol: String,
    /// Cached display name.
    pub cached_name: String,
    /// Cached line color (hex string, e.g. "#2196F3").
    pub cached_color: String,
    /// Cached line width in pixels.
    pub cached_line_width: f32,
    /// Cached line style ("solid", "dashed", "dotted").
    pub cached_line_style: String,
    /// Cached chart type ("line", "step_line", "area", "candles", "bars",
    /// "hollow_candles", "heikin_ashi").
    pub cached_chart_type: String,
    /// Cached visibility flag.
    pub cached_visible: bool,
    /// Cached bar count.
    pub cached_bar_count: usize,
    /// Cached base price.
    pub cached_base_price: f64,
    /// Cached timeframe visibility config (loaded from series at open time).
    /// `None` means visible on all timeframes.
    pub cached_timeframe_visibility: Option<TimeframeVisibilityConfig>,
    // The `original_*` snapshot that used to live here fed one thing: the
    // Cancel button's revert. Both are gone (owner ruling 2026-08-03) — this
    // modal edits a series that already exists, so its edits apply live and
    // there is one exit, not two outcomes.
    // ---- Inline text editing for tf min/max number fields ----
    /// Which sub-field is currently being edited inside this modal.
    /// Text/cursor/selection live in TextFieldStore under `COMPARE_SETTINGS_FIELD`.
    pub active_field_id: Option<String>,
    /// Render-time snapshot of the focused sub-field (populated by ChartApp before each render).
    pub active_render: ActiveFieldRender,
    /// Whether the user is drag-selecting text in the active input field.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
}

impl Default for CompareSettingsState {
    fn default() -> Self {
        Self {
            size: None,
            resize_drag: None,
            is_open: false,
            series_index: 0,
            active_tab: CompareSettingsTab::Style,
            position: None,
            is_dragging: false,
            drag_offset: None,
            color_picker: ColorPickerState::new(),
            color_picker_field: None,
            line_style_dropdown_open: false,
            chart_type_dropdown_open: false,
            hovered_item_id: None,
            slider_drag: None,
            cached_symbol: String::new(),
            cached_name: String::new(),
            cached_color: "#2196F3".to_string(),
            cached_line_width: 2.0,
            cached_line_style: "solid".to_string(),
            cached_chart_type: "line".to_string(),
            cached_visible: true,
            cached_bar_count: 0,
            cached_base_price: 0.0,
            cached_timeframe_visibility: None,
            active_field_id: None,
            active_render: ActiveFieldRender::default(),
            text_select_dragging: false,
            template_dropdown_open: false,
            applied_template_id: None,
            save_template_mode: false,
        }
    }
}

impl CompareSettingsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the modal for a specific compare series.
    ///
    /// Pass the series fields so the renderer can display them without needing
    /// a live borrow on the compare overlay.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &mut self,
        series_index: usize,
        symbol: &str,
        name: &str,
        color: &str,
        line_width: f32,
        line_style: &str,
        chart_type: &str,
        visible: bool,
        bar_count: usize,
        base_price: f64,
        timeframe_visibility: Option<TimeframeVisibilityConfig>,
    ) {
        self.is_open = true;
        self.series_index = series_index;
        self.active_tab = CompareSettingsTab::Style;
        self.line_style_dropdown_open = false;
        self.chart_type_dropdown_open = false;
        self.hovered_item_id = None;
        self.slider_drag = None;
        self.active_field_id = None;
        self.text_select_dragging = false;
        self.cached_symbol = symbol.to_string();
        self.cached_name = name.to_string();
        self.cached_color = color.to_string();
        self.cached_line_width = line_width;
        self.cached_line_style = line_style.to_string();
        self.cached_chart_type = chart_type.to_string();
        self.cached_visible = visible;
        self.cached_bar_count = bar_count;
        self.cached_base_price = base_price;
        self.cached_timeframe_visibility = timeframe_visibility.clone();
        // Don't reset position — keep last for convenience.
    }

    /// Refresh the cached display data from an updated series.
    /// Call each frame while the modal is open to keep displays current.
    #[allow(clippy::too_many_arguments)]
    pub fn refresh_cache(
        &mut self,
        color: &str,
        line_width: f32,
        line_style: &str,
        chart_type: &str,
        visible: bool,
        bar_count: usize,
        base_price: f64,
    ) {
        self.cached_color = color.to_string();
        self.cached_line_width = line_width;
        self.cached_line_style = line_style.to_string();
        self.cached_chart_type = chart_type.to_string();
        self.cached_visible = visible;
        self.cached_bar_count = bar_count;
        self.cached_base_price = base_price;
    }

    /// Returns true if the line-width slider is currently being dragged.
    pub fn is_slider_dragging(&self) -> bool {
        self.slider_drag.is_some()
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.color_picker.close();
        self.color_picker_field = None;
        self.line_style_dropdown_open = false;
        self.hovered_item_id = None;
        self.slider_drag = None;
        self.active_field_id = None;
        self.text_select_dragging = false;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
    }

    /// Is the modal open?
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Set active tab.
    pub fn set_tab(&mut self, tab: CompareSettingsTab) {
        self.active_tab = tab;
    }

    /// Start dragging the modal header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((dx, dy)) = self.drag_offset {
            self.position = Some((mouse_x - dx, mouse_y - dy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Open the color picker for a given field at the swatch position.
    pub fn open_color_picker(
        &mut self,
        field: &str,
        anchor_x: f64,
        anchor_y: f64,
        anchor_w: f64,
        anchor_h: f64,
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.color_picker_field = Some(field.to_string());
        self.color_picker.open_l1_smart(
            anchor_x, anchor_y,
            anchor_w, anchor_h,
            window_w, window_h,
            current_color,
        );
    }

    /// Close the color picker.
    pub fn close_color_picker(&mut self) {
        self.color_picker.close();
        self.color_picker_field = None;
    }

    /// Is the color picker open?
    pub fn is_color_picker_open(&self) -> bool {
        self.color_picker.is_open()
    }

    /// Start dragging the line-width slider.
    pub fn start_slider_drag(
        &mut self,
        field_id: &str,
        slider_x: f64,
        slider_width: f64,
        min_val: f64,
        max_val: f64,
    ) {
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x,
            slider_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Update line-width slider drag position.
    /// Returns `(field_id, value)` if dragging.
    pub fn update_slider_drag(&mut self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref mut drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// End slider drag, returning the committed `(field_id, value)` if any.
    pub fn end_slider_drag(&mut self) -> Option<(String, f64)> {
        if let Some(drag) = self.slider_drag.take() {
            drag.floating_value.map(|v| (drag.field_id, v))
        } else {
            None
        }
    }

    /// Start dragging a dual-handle (min/max) slider (for tf_*_slider fields).
    pub fn start_dual_slider_drag(
        &mut self,
        field_id: &str,
        track_x: f64,
        track_width: f64,
        min_val: f64,
        max_val: f64,
        handle: DualSliderHandle,
        initial_click_x: f64,
    ) {
        let t = ((initial_click_x - track_x) / track_width).clamp(0.0, 1.0);
        let initial_value = min_val + t * (max_val - min_val);
        self.slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: Some(handle),
            floating_value: Some(initial_value),
            floating_value2: None,
        });
    }

    /// Consume the final floating value at drag-end and clear drag state.
    /// Returns `(field_id, value, dual_handle)` if there was a floating value.
    pub fn take_slider_drag_value(&mut self) -> Option<(String, f64, Option<DualSliderHandle>)> {
        if let Some(drag) = self.slider_drag.take() {
            drag.floating_value.map(|v| (drag.field_id, v, drag.dual_handle))
        } else {
            None
        }
    }

    /// Get the current floating (preview) value if a drag is active.
    pub fn get_floating_drag(&self) -> Option<(&str, f64, Option<DualSliderHandle>)> {
        self.slider_drag.as_ref().and_then(|d| {
            d.floating_value.map(|v| (d.field_id.as_str(), v, d.dual_handle))
        })
    }

    /// Get dual slider handle being dragged (if any).
    pub fn dual_slider_handle(&self) -> Option<DualSliderHandle> {
        self.slider_drag.as_ref().and_then(|s| s.dual_handle)
    }

    /// Update dual-handle slider drag position, returning (field_id, value, handle).
    pub fn update_dual_slider_drag(&mut self, mouse_x: f64) -> Option<(&str, f64, Option<DualSliderHandle>)> {
        if let Some(ref mut drag) = self.slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value, drag.dual_handle))
        } else {
            None
        }
    }

    /// Set initial centered position.  Call once after `open()` with screen size.
    pub fn pin_initial_position(&mut self, screen_w: f64, screen_h: f64) {
        if self.position.is_none() {
            let modal_w = 400.0;
            let modal_h = 320.0;
            self.position = Some((
                (screen_w - modal_w) / 2.0,
                (screen_h - modal_h) / 2.0,
            ));
        }
    }
}

// =============================================================================
// Alert types — re-exported from the `alerts` crate
// =============================================================================

pub use mlc_core::alert::{AlertStatus, AlertCondition, AlertItem, AlertSource, AlertTriggerMode, AlertTransport, SignalBarState, SignalDirection};

// =============================================================================
// Alert Settings Modal State
// =============================================================================

/// Which tab is active in the Alert Settings modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlertSettingsTab {
    #[default]
    Settings,
    Notifications,
    AlertsList,
}

/// Filter for the AlertsList tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlertListFilter {
    #[default]
    All,
    Active,
    Triggered,
}

/// State for the Alert Settings modal.
///
/// Mirrors the PrimitiveSettingsState pattern — modal position, drag, inline edit.
/// Opened from ObjectTree sidebar alert buttons to configure alerts on drawings/indicators.
#[derive(Clone, Debug)]
pub struct AlertSettingsState {
    /// Whether the modal is open.
    pub is_open: bool,
    /// The alert being edited, if any (None = creating new alert).
    pub editing_alert_id: Option<u64>,
    /// Source object — what the alert monitors.
    pub source: AlertSource,
    /// Source object info — what triggered opening (for display).
    pub source_name: String,
    /// Current symbol.
    pub symbol: String,
    /// Active tab.
    pub active_tab: AlertSettingsTab,
    /// Current condition selection.
    pub condition: AlertCondition,
    /// Price level.
    pub price: f64,
    /// Second price (for range conditions).
    pub price2: f64,
    /// Percentage for MovingUp/Down conditions.
    pub percentage: f64,
    /// Alert name / message.
    pub name: String,
    /// Trigger mode (OneShot, EveryTime, OncePerBar, TimesN).
    pub trigger_mode: AlertTriggerMode,
    /// N value used when trigger_mode is TimesN.
    pub times_n: u32,
    /// Popup transport enabled.
    pub popup_enabled: bool,
    /// Sound transport enabled.
    pub sound_enabled: bool,
    /// Webhook transport enabled.
    pub webhook_enabled: bool,
    /// Webhook URL.
    pub webhook_url: String,
    /// Whether the trigger-mode dropdown is open.
    pub trigger_mode_dropdown_open: bool,
    /// All alerts for the AlertsList tab (populated externally).
    pub all_alerts: Vec<AlertItem>,
    /// Filter for the AlertsList tab.
    pub list_filter: AlertListFilter,
    /// Scroll state for the AlertsList tab.
    pub list_scroll: ScrollState,
    /// Modal position on screen (None = centered).
    pub position: Option<(f64, f64)>,
    /// Whether the modal header is being dragged.
    pub is_dragging: bool,
    /// Drag offset from modal top-left corner to mouse.
    pub drag_offset: Option<(f64, f64)>,
    /// Currently hovered item for highlight effects.
    pub hovered_item_id: Option<String>,
    /// Which condition dropdown is open (if any).
    pub condition_dropdown_open: bool,
    /// Notification PREFERENCES (mirrors the plaintext profile, edited in
    /// modal). Always available — vault-independent.
    pub notification_settings: NotificationSettings,
    /// Notification SECRETS (bot token, subscriber chat ids, webhook URL).
    /// Populated only when `secrets_available` is true; otherwise holds
    /// [`NotificationSecrets::default`] and the render layer must show the
    /// secret-bearing fields as unavailable rather than empty-and-editable.
    pub notification_secrets: NotificationSecrets,
    /// Whether `notification_secrets` holds live data this frame — mirrors
    /// whether the profile vault is unlocked this session. Set by
    /// `ChartApp::apply_profile_state`.
    pub secrets_available: bool,
    /// Whether the active account is signed in — native Telegram delivery
    /// is "own bot, signed-in, vault-backed" (alerts-account-wide-and-
    /// telegram-delivery plan §3): the token/subscriber/test-button surface
    /// gates on this FIRST, `secrets_available` second. Mirrors
    /// `UserSettingsState::account_signed_in`, set from the same
    /// `auth_rx`-derived flag every frame (`mlc-app-shell-vello`'s
    /// `about_to_wait`).
    pub account_signed_in: bool,
    /// Render-time snapshot of the Telegram bot token input (populated from TextFieldStore by ChartApp).
    pub tg_token_render: ActiveFieldRender,
    /// Detected users from getUpdates: (chat_id, display_name, username).
    pub tg_detected_users: Vec<(String, String, String)>,
    /// Result of last Telegram test/verify operation.
    pub tg_status_message: String,
    /// Whether Telegram test is pending (a flag for external HTTP dispatch).
    pub tg_test_pending: bool,
    /// Whether Telegram chat_id auto-detect is pending.
    pub tg_detect_pending: bool,
    /// Dirty flag: notification settings changed and need to be persisted to disk.
    pub notification_settings_dirty: bool,
    /// Direction filter for Signal alerts.
    pub signal_direction: SignalDirection,
    /// Bar state filter for Signal alerts.
    pub signal_bar_state: SignalBarState,
    /// Whether the signal direction dropdown is open.
    pub signal_direction_dropdown_open: bool,
    /// Whether the signal bar state dropdown is open.
    pub signal_bar_state_dropdown_open: bool,
    /// Filter by signal kind (None = any kind). Only relevant for Signal alerts.
    pub kind_filter: Option<String>,
    /// Available signal kinds for dropdown (populated when editing Signal alerts).
    pub available_signal_kinds: Vec<String>,
    /// Whether the kind_filter dropdown is open.
    pub kind_filter_dropdown_open: bool,
}

impl Default for AlertSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            editing_alert_id: None,
            source: AlertSource::Price { symbol: String::new() },
            source_name: String::new(),
            symbol: String::new(),
            active_tab: AlertSettingsTab::default(),
            condition: AlertCondition::Crossing,
            price: 0.0,
            price2: 0.0,
            percentage: 5.0,
            name: String::new(),
            trigger_mode: AlertTriggerMode::OneShot,
            times_n: 5,
            popup_enabled: true,
            sound_enabled: false,
            webhook_enabled: false,
            webhook_url: String::new(),
            trigger_mode_dropdown_open: false,
            all_alerts: Vec::new(),
            list_filter: AlertListFilter::default(),
            list_scroll: ScrollState::default(),
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            condition_dropdown_open: false,
            notification_settings: NotificationSettings::default(),
            notification_secrets: NotificationSecrets::default(),
            secrets_available: false,
            account_signed_in: false,
            tg_token_render: ActiveFieldRender::default(),
            tg_detected_users: Vec::new(),
            tg_status_message: String::new(),
            tg_test_pending: false,
            tg_detect_pending: false,
            notification_settings_dirty: false,
            signal_direction: SignalDirection::Any,
            signal_bar_state: SignalBarState::Forming,
            signal_direction_dropdown_open: false,
            signal_bar_state_dropdown_open: false,
            kind_filter: None,
            available_signal_kinds: Vec::new(),
            kind_filter_dropdown_open: false,
        }
    }
}

impl AlertSettingsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Open the modal for creating a new alert, binding it to `source`.
    pub fn open_new(&mut self, source: AlertSource, symbol: &str, price: f64) {
        self.is_open = true;
        self.editing_alert_id = None;
        self.source_name = source.display_name();
        // Initialize signal-specific fields from source if it's a Signal alert.
        if let AlertSource::Signal { direction_filter, bar_state, .. } = &source {
            self.signal_direction = *direction_filter;
            self.signal_bar_state = *bar_state;
        } else {
            self.signal_direction = SignalDirection::Any;
            self.signal_bar_state = SignalBarState::Forming;
        }
        self.source = source;
        self.symbol = symbol.to_string();
        self.active_tab = AlertSettingsTab::Settings;
        self.condition = AlertCondition::Crossing;
        self.price = price;
        self.price2 = 0.0;
        self.percentage = 5.0;
        self.name = format!("{} alert", self.source_name);
        self.trigger_mode = AlertTriggerMode::OneShot;
        self.times_n = 5;
        self.popup_enabled = true;
        self.sound_enabled = false;
        self.webhook_enabled = false;
        self.webhook_url = String::new();
        self.trigger_mode_dropdown_open = false;
        self.signal_direction_dropdown_open = false;
        self.signal_bar_state_dropdown_open = false;
        self.kind_filter = None;
        self.kind_filter_dropdown_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.hovered_item_id = None;
        self.condition_dropdown_open = false;
        // Keep position from last use
    }

    /// Open the modal for editing an existing alert.
    pub fn open_edit(&mut self, alert: &AlertItem) {
        self.is_open = true;
        self.editing_alert_id = Some(alert.id);
        self.source = alert.source.clone();
        self.source_name = alert.source_display();
        self.symbol = alert.symbol().to_string();
        self.active_tab = AlertSettingsTab::Settings;
        self.condition = alert.condition;
        self.price = alert.price;
        self.price2 = alert.price2;
        self.percentage = alert.percentage;
        self.name = alert.name.clone();
        self.trigger_mode = alert.trigger_mode;
        self.times_n = match alert.trigger_mode {
            AlertTriggerMode::TimesN(n) => n,
            _ => 5,
        };
        self.popup_enabled = alert.transports.iter().any(|t| matches!(t, AlertTransport::Popup));
        self.sound_enabled = alert.transports.iter().any(|t| matches!(t, AlertTransport::Sound));
        self.webhook_enabled = alert.transports.iter().any(|t| matches!(t, AlertTransport::Webhook { .. }));
        self.webhook_url = alert.transports.iter().find_map(|t| match t {
            AlertTransport::Webhook { url } => Some(url.clone()),
            _ => None,
        }).unwrap_or_default();
        self.trigger_mode_dropdown_open = false;
        // Initialize signal-specific fields from the alert source.
        if let AlertSource::Signal { direction_filter, bar_state, kind_filter, .. } = &alert.source {
            self.signal_direction = *direction_filter;
            self.signal_bar_state = *bar_state;
            self.kind_filter = kind_filter.clone();
        } else {
            self.signal_direction = SignalDirection::Any;
            self.signal_bar_state = SignalBarState::Forming;
            self.kind_filter = None;
        }
        self.signal_direction_dropdown_open = false;
        self.signal_bar_state_dropdown_open = false;
        self.kind_filter_dropdown_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.hovered_item_id = None;
        self.condition_dropdown_open = false;
    }

    /// Set the initial centered position so that tab switches resize from
    /// a fixed header rather than re-centering the modal every frame.
    /// Call once after `open_new` / `open_edit` with the current screen size.
    pub fn pin_initial_position(&mut self, screen_w: f64, screen_h: f64) {
        if self.position.is_none() {
            // Use the Settings tab height (largest) so the modal starts well-centered.
            let est_h = 36.0 + 32.0 + 260.0; // HEADER_H + TAB_BAR_H + ~settings content
            let modal_w = 480.0; // MODAL_WIDTH
            self.position = Some((
                (screen_w - modal_w) / 2.0,
                (screen_h - est_h) / 2.0,
            ));
        }
    }

    /// Build the transports vector from the current state.
    pub fn build_transports(&self) -> Vec<AlertTransport> {
        let mut transports = Vec::new();
        if self.popup_enabled {
            transports.push(AlertTransport::Popup);
        }
        if self.sound_enabled {
            transports.push(AlertTransport::Sound);
        }
        if self.webhook_enabled {
            transports.push(AlertTransport::Webhook { url: self.webhook_url.clone() });
        }
        // Ensure at least one transport is always set.
        if transports.is_empty() {
            transports.push(AlertTransport::Popup);
        }
        transports
    }

    /// Build the trigger mode from the current state, resolving the `times_n` value.
    pub fn build_trigger_mode(&self) -> AlertTriggerMode {
        match self.trigger_mode {
            AlertTriggerMode::TimesN(_) => AlertTriggerMode::TimesN(self.times_n),
            other => other,
        }
    }

    /// Build an updated `AlertSource::Signal` from the current signal-specific modal state.
    ///
    /// Clones the indicator_id and label from the existing source, replacing
    /// `direction_filter` and `bar_state` with the values currently selected in the modal.
    /// Returns the source unchanged if it is not `AlertSource::Signal`.
    pub fn build_signal_source(&self) -> AlertSource {
        if let AlertSource::Signal { indicator_id, ref label, .. } = self.source {
            AlertSource::Signal {
                indicator_id,
                label: label.clone(),
                direction_filter: self.signal_direction,
                bar_state: self.signal_bar_state,
                kind_filter: self.kind_filter.clone(),
            }
        } else {
            self.source.clone()
        }
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.is_open = false;
        self.editing_alert_id = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.condition_dropdown_open = false;
        self.trigger_mode_dropdown_open = false;
        self.signal_direction_dropdown_open = false;
        self.signal_bar_state_dropdown_open = false;
        self.kind_filter_dropdown_open = false;
    }

    /// Start dragging the modal.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update drag position.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((dx, dy)) = self.drag_offset {
            self.position = Some((mouse_x - dx, mouse_y - dy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}

// =============================================================================
// Vault tab state (exchange API keys + webhook URL)
// =============================================================================
//
// Used to be a standalone modal (`crate::layout::modals::credentials`,
// retired) — opening it closed the User Settings modal underneath it, with
// no way back. The vault surface is the `Vault` tab of `UserSettingsState`
// now; this type is unchanged except for the four fields (`is_open`,
// `position`, `is_dragging`, `drag_offset`) that only ever meant something
// for a modal's own chrome.

/// Which page of the Vault tab is showing.
///
/// The five `*Passphrase*`/`*RecoveryKey*` variants mirror
/// `ProfileManagerPage`'s own — they render via the SAME functions in
/// `layout::modals::profile_manager` (`render_page_unlock` and friends),
/// parameterized by `VaultPageHost::Tab` instead of `VaultPageHost::Modal`.
/// `ProfileManagerPage` still owns `ProfileList`/`CreateNew`, which have no
/// counterpart here: the profile list is not part of this tab, and will not
/// exist at all for accounts below the paid tier (owner's ruling
/// 2026-09-01) — see `route_credentials_to_unlock`'s module doc in
/// `mlc-app/src/input/credentials.rs` for the entry point that picks between
/// these two variants, and `panel_click.rs`'s `"profile_mgr:back"` arm for
/// how the shared pages tell which enum to drive (`show_profile_manager`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CredentialsView {
    /// The list of issued exchange keys + the webhook URL field. Default —
    /// sealed or unlocked, this is always what switching to the tab shows.
    #[default]
    List,
    /// The "add a key" form (exchange, label, api_key, api_secret,
    /// passphrase). Only reachable while unlocked — see
    /// `ChartApp::handle_credentials_click`'s "add_key" arm, which routes to
    /// unlock instead of switching here while sealed.
    AddKey,
    /// Enter passphrase to unlock this profile's `vault.enc`.
    UnlockPassphrase,
    /// Set a new passphrase for a profile that has no vault yet.
    CreatePassphrase,
    /// Display the recovery key once after vault creation. Shown immediately
    /// after `CreatePassphrase` succeeds; no back button in either host —
    /// the user must acknowledge it, not navigate away.
    ShowRecoveryKey,
    /// Enter a recovery key to restore access when the passphrase is
    /// forgotten.
    UseRecoveryKey,
    /// After a recovery-key unlock — a new passphrase is mandatory before
    /// the vault is considered settled. No back button, same reasoning as
    /// `ShowRecoveryKey`.
    SetNewPassphrase,
}

/// State for the Vault tab — the vault-gated surface for exchange API keys
/// and the alert webhook URL.
///
/// Deliberately does NOT own the secrets directly: `exchange_keys` here is a
/// cache refreshed by `ChartApp` (via `sync_credentials_from_core`) from
/// `BackendCore`'s `UserProfileManager` — the same cache-refreshed-by-the-
/// app-layer pattern `AlertSettingsState::notification_secrets` already
/// uses, and for the same reason: this struct lives in `mlc-app-layout`,
/// which the shared-BackendCore arc does not let touch `core` directly.
/// `webhook_url`/`webhook_enabled` are NOT duplicated here — the tab reads
/// them from `AlertSettingsState` at render time (single source, no second
/// copy to drift) since editing the webhook URL here writes into that same
/// `notification_secrets.webhook.url` and rides its existing persist path.
#[derive(Clone, Debug)]
pub struct CredentialsState {
    /// Which page is showing.
    pub view: CredentialsView,
    /// Mirrors whether the vault is unlocked this session — same meaning as
    /// `AlertSettingsState::secrets_available`, refreshed the same way.
    pub secrets_available: bool,
    /// Whether this profile has a `vault.enc` at all. Distinct from
    /// `secrets_available`: a profile that never had a vault is not "sealed",
    /// and offering it "Unlock" asks for a passphrase that protects nothing.
    /// Sourced from `profiles_with_vault_status`, the same list the profile
    /// manager reads.
    pub vault_exists: bool,
    /// Cached exchange keys. While sealed, entries still carry
    /// `exchange_id`/`label`/`api_key` (redacted `api_secret`/`passphrase`)
    /// — see `UserProfileManager::seal_vault`'s doc for why that is honest
    /// rather than a leak. While unlocked, entries are the real thing.
    pub exchange_keys: Vec<mlc_core::user_profile::profile::StoredExchangeKey>,
    /// Currently hovered item for highlight effects.
    pub hovered_item_id: Option<String>,
    /// Render-time snapshot of the webhook URL input (populated from
    /// TextFieldStore by `ChartApp`).
    pub webhook_url_render: ActiveFieldRender,
    /// Whether the webhook URL field is focused.
    pub webhook_url_focused: bool,
    /// Render-time snapshots of the "add key" form fields.
    pub add_exchange_render: ActiveFieldRender,
    pub add_label_render: ActiveFieldRender,
    pub add_api_key_render: ActiveFieldRender,
    pub add_api_secret_render: ActiveFieldRender,
    pub add_passphrase_render: ActiveFieldRender,
    pub add_exchange_focused: bool,
    pub add_label_focused: bool,
    pub add_api_key_focused: bool,
    pub add_api_secret_focused: bool,
    pub add_passphrase_focused: bool,
    /// Sandbox/testnet toggle for the entry being added.
    pub add_testnet: bool,
    /// Whether the "add key" form's API-secret and passphrase fields
    /// currently show plain text instead of bullets — one flag for both
    /// fields, reset whenever the form is cleared or left
    /// (`clear_add_form`, which every closing/leaving path already calls).
    pub secret_revealed: bool,
    /// Validation error from the last "add" attempt (e.g. missing
    /// exchange/api key), shown under the form. Cleared on next attempt.
    pub error_message: Option<String>,
    /// Set when "add" (or "remove") was clicked while sealed: the click
    /// routes to `ProfileManagerPage::UnlockPassphrase` instead of acting,
    /// and this remembers to switch back to the Vault tab page (`List` or
    /// `AddKey`) once the unlock succeeds — see the module doc on
    /// `ChartApp::handle_credentials_click`.
    pub pending_unlock_return: Option<CredentialsView>,
}

impl Default for CredentialsState {
    fn default() -> Self {
        Self {
            view: CredentialsView::default(),
            secrets_available: false,
            exchange_keys: Vec::new(),
            hovered_item_id: None,
            webhook_url_render: ActiveFieldRender::default(),
            webhook_url_focused: false,
            add_exchange_render: ActiveFieldRender::default(),
            add_label_render: ActiveFieldRender::default(),
            add_api_key_render: ActiveFieldRender::default(),
            add_api_secret_render: ActiveFieldRender::default(),
            add_passphrase_render: ActiveFieldRender::default(),
            add_exchange_focused: false,
            add_label_focused: false,
            add_api_key_focused: false,
            add_api_secret_focused: false,
            add_passphrase_focused: false,
            add_testnet: false,
            secret_revealed: false,
            error_message: None,
            vault_exists: false,
            pending_unlock_return: None,
        }
    }
}

impl CredentialsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset to the list view. Caller (`ChartApp`) is responsible for
    /// refreshing `secrets_available`/`exchange_keys` beforehand. Named
    /// `open` from the surface's former life as a standalone modal — kept
    /// as the entry point for "show the Vault tab from scratch" even though
    /// there is no `is_open` of its own to flip anymore (the tab's
    /// visibility is `UserSettingsState::is_open` + `active_tab`).
    pub fn open(&mut self) {
        self.view = CredentialsView::List;
        self.error_message = None;
    }

    /// Reset the "add key" form fields (values live in `TextFieldStore`,
    /// not here — this only clears the render-proxy cache and focus flags
    /// so the next open shows a fresh form rather than the last render).
    pub fn clear_add_form(&mut self) {
        self.add_exchange_render = ActiveFieldRender::default();
        self.add_label_render = ActiveFieldRender::default();
        self.add_api_key_render = ActiveFieldRender::default();
        self.add_api_secret_render = ActiveFieldRender::default();
        self.add_passphrase_render = ActiveFieldRender::default();
        self.add_exchange_focused = false;
        self.add_label_focused = false;
        self.add_api_key_focused = false;
        self.add_api_secret_focused = false;
        self.add_passphrase_focused = false;
        self.add_testnet = false;
        self.secret_revealed = false;
    }
}

// Indicator display types relocated to mlc-core::state::modal_data.
// Re-exported at the top of this file via `pub use mlc_core::state::modal_data::...`.

// =============================================================================
// Theme display data (for appearance tab in chart settings modal)
// =============================================================================

/// A single color field for display in the appearance settings panel
#[derive(Clone, Debug)]
pub struct ThemeColorFieldDisplay {
    pub id: String,
    pub label: String,
    pub color_value: String,
}

/// A section of color fields in the appearance settings panel
#[derive(Clone, Debug)]
pub struct ThemeColorSectionDisplay {
    pub title: String,
    pub fields: Vec<ThemeColorFieldDisplay>,
}

/// Style parameter values for the appearance tab sliders
#[derive(Clone, Debug, Default)]
pub struct StyleParamsDisplay {
    pub toolbar_bg_opacity: f32,
    pub modal_bg_opacity: f32,
    pub sidebar_bg_opacity: f32,
    pub menu_bg_opacity: f32,
    pub scale_bg_opacity: f32,
    pub hover_bg_opacity: f32,
    pub crosshair_label_bg_opacity: f32,
    pub blur_radius: f32,
    pub has_blur: bool,
}

/// All theme display data passed to the appearance settings renderer
#[derive(Clone, Debug, Default)]
pub struct ThemeDisplayData {
    pub current_theme_preset: String,
    pub current_ui_style_index: usize,
    pub ui_style_labels: Vec<String>,
    pub style_params: StyleParamsDisplay,
    pub color_sections: Vec<ThemeColorSectionDisplay>,
}

// =============================================================================
// Chart screen area for modal centering
// =============================================================================

/// Usable screen area for modal centering.
///
/// Chart's `FrameLayout` only has `total`/`chart_area`/`chart_panel` — the full
/// toolbar layout is a core concept.  Chart modal renderers receive a
/// `ChartScreenArea` built by core before calling into chart.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartScreenArea {
    /// X origin of the usable screen (left edge of chart panel)
    pub x: f64,
    /// Y origin of the usable screen (top edge of chart panel)
    pub y: f64,
    /// Width of the usable screen area
    pub width: f64,
    /// Height of the usable screen area
    pub height: f64,
}

impl ChartScreenArea {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

// =============================================================================
// Overlay Settings State
// =============================================================================

/// Tab in the overlay panel tree manager modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayPanelTreeTab {
    #[default]
    TreeView,
    Eliminate,
    Hidden,
    Minimap,
}

impl OverlayPanelTreeTab {
    /// Get tab ID string.
    pub fn id(&self) -> &'static str {
        match self {
            Self::TreeView  => "tree_view",
            Self::Eliminate => "eliminate",
            Self::Hidden    => "hidden",
            Self::Minimap   => "minimap",
        }
    }

    /// Get tab display label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::TreeView  => "Tree",
            Self::Eliminate => "Eliminate",
            Self::Hidden    => "Hidden",
            Self::Minimap   => "Minimap",
        }
    }

    /// All available tabs (Minimap moved to standalone MAP sidebar).
    pub fn all() -> &'static [OverlayPanelTreeTab] {
        &[Self::TreeView, Self::Hidden]
    }
}

/// State for the overlay (leaf) settings modal.
///
/// This modal opens when the user clicks the gear icon on an overlay tab header.
/// It is a full 4-tab panel tree manager showing the chart's internal split layout.
#[derive(Clone, Debug, Default)]
pub struct OverlaySettingsState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// Modal position (for dragging). `None` = centered on screen.
    pub position: Option<(f64, f64)>,
    /// Whether the title bar is currently being dragged.
    pub is_dragging: bool,
    /// Drag offset from the modal top-left corner.
    pub drag_offset: Option<(f64, f64)>,
    // --- new fields ---
    /// Active tab.
    pub active_tab: OverlayPanelTreeTab,
    /// The leaf that triggered the modal open (for highlight).
    pub target_leaf_id: Option<uzor::panels::LeafId>,
    /// Currently hovered item widget ID (for button hover feedback).
    pub hovered_item_id: Option<String>,
    /// Currently selected node ID in tree/minimap view.
    pub selected_node_id: Option<u64>,
}

impl OverlaySettingsState {
    /// Create new state (modal closed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Open for a specific leaf (highlights it in all tabs).
    pub fn open_for_leaf(&mut self, leaf_id: uzor::panels::LeafId) {
        self.is_open = true;
        self.active_tab = OverlayPanelTreeTab::default();
        self.target_leaf_id = Some(leaf_id);
        self.hovered_item_id = None;
        self.selected_node_id = Some(leaf_id.0);
    }

    /// Open the overlay settings modal.
    pub fn open(&mut self) {
        self.is_open = true;
        self.active_tab = OverlayPanelTreeTab::default();
        self.hovered_item_id = None;
        // Do NOT reset target_leaf_id or selected_node_id here — keep from previous open.
    }

    /// Close the overlay settings modal.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.target_leaf_id = None;
        self.hovered_item_id = None;
        self.selected_node_id = None;
        // Keep position so it reopens in the same spot.
    }

    /// Toggle open/close.
    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Set active tab and clear hover state.
    pub fn set_tab(&mut self, tab: OverlayPanelTreeTab) {
        self.active_tab = tab;
        self.hovered_item_id = None;
    }

    /// Start dragging the modal title bar.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

// =============================================================================
// Chart Browser State
// =============================================================================

/// State for the "Open Chart" browser modal.
#[derive(Clone, Debug)]
pub struct ChartBrowserState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// The text typed into the search box.
    pub search_query: String,
    /// Scroll state for the preset list.
    pub scroll: ScrollState,
    /// The preset ID currently under the mouse cursor (for hover icons).
    pub hovered_preset_id: Option<String>,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left when drag started.
    pub drag_offset: Option<(f64, f64)>,
    /// When true, selecting a preset opens it in a new tab instead of the active tab.
    pub open_in_new_tab: bool,
}

impl Default for ChartBrowserState {
    fn default() -> Self {
        Self {
            is_open: false,
            search_query: String::new(),
            scroll: ScrollState::default(),
            hovered_preset_id: None,
            position: None,
            is_dragging: false,
            drag_offset: None,
            open_in_new_tab: false,
        }
    }
}

impl ChartBrowserState {
    /// Open the modal, resetting search and scroll state. Caller must clear TextFieldStore.
    pub fn open(&mut self, _current_time_ms: u64) {
        self.is_open = true;
        self.search_query.clear();
        self.scroll.reset();
        self.hovered_preset_id = None;
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.open_in_new_tab = false;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.is_open = false;
        self.open_in_new_tab = false;
    }

    /// Start dragging from the header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update drag position.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}

// =============================================================================
// Tags & Tabs Modal State
// =============================================================================

/// Which sidebar item is active in the Tags & Tabs modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagsTabsSidebar {
    /// The panel tree / overlay-settings section.
    #[default]
    Tabs,
    /// The sync-group / tag-manager section.
    Tags,
    /// Unified minimap showing layouts, leaves and groups colored by tag.
    Map,
}

/// Sub-tab within the TAGS sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagsTabsTagsTab {
    /// List of all sync groups.
    #[default]
    Groups,
    /// Details and flags for the selected group.
    Details,
}

/// State for the Tags & Tabs modal.
#[derive(Clone, Debug, Default)]
pub struct TagsTabsState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// Which sidebar section is active.
    pub sidebar: TagsTabsSidebar,
    /// Active sub-tab in the TAGS sidebar.
    pub tags_tab: TagsTabsTagsTab,
    /// The sync group currently selected in the Groups tab.
    pub selected_group_id: Option<mlc_core::BubbleId>,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the title bar is being dragged.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left when drag started.
    pub drag_offset: Option<(f64, f64)>,
    /// Scroll state for TABS section (TreeView/Eliminate/Hidden sub-tabs).
    pub tabs_scroll: ScrollState,
    /// Scroll state for TAGS section — Groups sub-tab.
    pub tags_groups_scroll: ScrollState,
    /// Scroll state for TAGS section — Details sub-tab.
    pub tags_details_scroll: ScrollState,
}

impl TagsTabsState {
    /// Open the modal.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Switch the sidebar section.
    pub fn set_sidebar(&mut self, sidebar: TagsTabsSidebar) {
        self.sidebar = sidebar;
    }

    /// Switch the TAGS sub-tab.
    pub fn set_tags_tab(&mut self, tab: TagsTabsTagsTab) {
        self.tags_tab = tab;
    }

    /// Start dragging the modal header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update modal position while dragging.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}

// =============================================================================
// Watchlist Modal State
// =============================================================================

/// Tab in the expanded watchlist modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchlistModalTab {
    Overview,
    Groups,
    Settings,
}

/// State for the expanded watchlist overlay modal.
#[derive(Clone, Debug)]
pub struct WatchlistModalState {
    pub is_open: bool,
    pub scroll: ScrollState,
    pub search_query: String,
    pub active_tab: WatchlistModalTab,
    pub position: Option<(f64, f64)>,
    pub is_dragging: bool,
    pub drag_offset: Option<(f64, f64)>,
    pub hovered_item_id: Option<String>,
    /// Drag-to-reorder state: `(dragging_index, current_mouse_y)`.
    pub drag_reorder: Option<(usize, f64)>,
    /// Pending drag-to-reorder: `(index, start_x, start_y)`.
    ///
    /// Set on mouse-down; promoted to `drag_reorder` only once the pointer
    /// has moved at least 5 px from the start position.  This prevents
    /// short clicks from being mis-classified as drag operations.
    pub drag_reorder_pending: Option<(usize, f64, f64)>,
    /// Drop target index during a drag-reorder operation.
    pub drop_index: Option<usize>,
    /// ID of the widget currently hovered inside the watchlist modal.
    pub hovered_widget: Option<String>,
}

impl WatchlistModalState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            scroll: ScrollState::default(),
            search_query: String::new(),
            active_tab: WatchlistModalTab::Overview,
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            drag_reorder: None,
            drag_reorder_pending: None,
            drop_index: None,
            hovered_widget: None,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.scroll.reset();
        self.search_query.clear();
        self.active_tab = WatchlistModalTab::Overview;
        self.drag_reorder = None;
        self.drag_reorder_pending = None;
        self.drop_index = None;
        self.hovered_widget = None;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.drag_reorder = None;
        self.drag_reorder_pending = None;
        self.drop_index = None;
        self.hovered_widget = None;
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((dx, dy)) = self.drag_offset {
            self.position = Some((mouse_x - dx, mouse_y - dy));
        }
    }

    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

impl Default for WatchlistModalState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Watchlist Group Name Input State
// =============================================================================

/// Purpose of the watchlist group name input modal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchlistGroupNameMode {
    /// Creating a new watchlist group.
    CreateNew,
    /// Renaming an existing watchlist group (holds the list id).
    Rename(u64),
}

/// State for the watchlist group name input modal dialog.
#[derive(Clone, Debug)]
pub struct WatchlistGroupNameInputState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// What action this modal performs.
    pub mode: WatchlistGroupNameMode,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left (for smooth dragging).
    pub drag_offset: Option<(f64, f64)>,
}

impl WatchlistGroupNameInputState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: WatchlistGroupNameMode::CreateNew,
            position: None,
            is_dragging: false,
            drag_offset: None,
        }
    }

    /// Open the modal for creating a new watchlist group.
    pub fn open_create_new(&mut self) {
        self.open_create_new_with_name("New Watchlist");
    }

    /// Open the modal for creating a new watchlist group with a specific default name.
    /// Caller must seed TextFieldStore with `default_name`.
    pub fn open_create_new_with_name(&mut self, _default_name: &str) {
        self.is_open = true;
        self.mode = WatchlistGroupNameMode::CreateNew;
        self.position = None;
    }

    /// Open the modal for renaming an existing watchlist group.
    /// Caller must seed TextFieldStore with `current_name`.
    pub fn open_rename(&mut self, list_id: u64, _current_name: &str) {
        self.is_open = true;
        self.mode = WatchlistGroupNameMode::Rename(list_id);
        self.position = None;
    }

    /// Close the modal and reset state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Start dragging from the header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update drag position.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((dx, dy)) = self.drag_offset {
            self.position = Some((mouse_x - dx, mouse_y - dy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

impl Default for WatchlistGroupNameInputState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// User Settings Modal State
// =============================================================================

/// Tabs available in the User Settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum UserSettingsTab {
    #[default]
    General,
    Sync,
    Vault,
    Persistence,
    Performance,
    Server,
}

impl UserSettingsTab {
    pub fn id(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Sync => "sync",
            Self::Vault => "vault",
            Self::Persistence => "persistence",
            Self::Performance => "performance",
            Self::Server => "server",
        }
    }

    pub fn label(&self) -> &'static str {
        use mlc_core::i18n::{UserSettingsKey, current_language};
        let lang = current_language();
        match self {
            Self::General => UserSettingsKey::TabGeneral.get(lang),
            Self::Sync => "Sync",
            Self::Vault => "Vault",
            Self::Persistence => UserSettingsKey::TabPersistence.get(lang),
            Self::Performance => UserSettingsKey::TabPerformance.get(lang),
            Self::Server => UserSettingsKey::TabServer.get(lang),
        }
    }

    pub fn all() -> &'static [Self] {
        // `Sync` retained as a variant for serde back-compat but hidden
        // from the tab strip — cloud sync is a paid server feature, not
        // local persist (Persistence tab below).
        &[Self::General, Self::Vault, Self::Persistence, Self::Performance, Self::Server]
    }

    /// [`Self::all`], with `Server` dropped while signed out — the Agent
    /// API is gated on `account_signed_in && server_enabled()` (agent-api-
    /// auth-and-secret-store plan, piece 4), so a signed-out session has no
    /// reachable server for the tab to manage. Same `retain`-by-id idiom
    /// `panel_app.rs:850` already uses for `share_chart` on the same gate.
    pub fn visible(signed_in: bool) -> Vec<Self> {
        let mut tabs = Self::all().to_vec();
        if !signed_in {
            tabs.retain(|t| *t != Self::Server);
        }
        tabs
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "general" => Some(Self::General),
            "sync" => Some(Self::Sync),
            "vault" => Some(Self::Vault),
            "persistence" => Some(Self::Persistence),
            "performance" => Some(Self::Performance),
            "server" => Some(Self::Server),
            _ => None,
        }
    }
}


// =============================================================================
// Profile Manager
// =============================================================================

/// Navigation pages for the unified Profile Manager overlay.
///
/// Replaces the old vault_unlock overlay and profile picker with a single
/// ordinary dismissible modal (`crate::layout::modals::profile_manager`)
/// that handles all profile-related flows.
#[derive(Clone, Debug, PartialEq)]
#[derive(Default)]
pub enum ProfileManagerPage {
    /// Main page: list of all profiles with status indicators.
    #[default]
    ProfileList,
    /// Enter passphrase to unlock a profile that has vault.enc.
    UnlockPassphrase,
    /// Create a new passphrase for a profile that has no vault.
    CreatePassphrase,
    /// Creating a brand new profile (name input).
    CreateNew,
    /// Display the recovery key once after vault creation.
    ///
    /// Shown immediately after `CreatePassphrase` succeeds.  The user must
    /// acknowledge ("I have written it down") before proceeding.
    ShowRecoveryKey,
    /// Enter a recovery key to restore access when passphrase is forgotten.
    UseRecoveryKey,
    /// After recovery unlock — user must set a new passphrase before proceeding.
    ///
    /// Shown immediately after `UseRecoveryKey` succeeds for the active profile.
    /// The user cannot skip this step — it is mandatory to ensure a valid passphrase
    /// is set after vault access is restored via recovery key.
    SetNewPassphrase,
}


// =============================================================================
// Account nickname (account-level, `POST /api/account/nick`)
// =============================================================================

/// Outcome of the most recent account-nickname submission from the
/// single-profile row's inline edit — same mailbox idiom as
/// [`RedeemStatus`] (the shell owns the round trip and writes the result
/// back here), but NOT `cfg(feature = "cloud")`-gated: the row it drives
/// (`crate::layout::modals::user_settings::render_single_profile_row`)
/// compiles in every build, unlike the login overlay.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum NickStatus {
    /// Nothing submitted since the edit was last opened.
    #[default]
    Idle,
    /// A submission is in flight — the Save button is dead and says so.
    Working,
    /// Accepted; carries the label to show for this account from now on.
    Saved { effective_label: String },
    /// Refused (local validation or the server), with the sentence to show
    /// the person who typed it.
    Failed { message: String },
}

/// Client-side account-nickname format check — mirrors the server's own
/// `POST /api/account/nick` 400 rule exactly, so a bad nick is refused
/// before a round trip: 3-24 characters, ASCII letters/digits plus
/// `_`/`-`/`.`, and the first character must be a letter or digit.
///
/// The one client-side copy of the rule: the click and Enter handlers in
/// `mlc-app` call it before arming `nick_save_requested`, so neither
/// transport crate (`mlc-cloud` on web, `mlc-updater` on native) validates
/// again — the server is the second and final check.
pub fn validate_nick(nick: &str) -> Result<(), String> {
    let len = nick.chars().count();
    if !(3..=24).contains(&len) {
        return Err("Nickname must be 3-24 characters".to_string());
    }
    if !nick.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err("Nickname may only contain letters, digits, '_', '-', '.'".to_string());
    }
    if !nick.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        return Err("Nickname must start with a letter or digit".to_string());
    }
    Ok(())
}

/// Local mirror of the server's `POST /api/forum/threads` title check
/// (3–120 bytes) — same reasoning `validate_nick` gives: refuse obviously
/// bad input before it ever leaves the device, the server re-checks
/// regardless. Byte length, not char count, to match the server's own
/// `title.len()` bound exactly.
#[cfg(feature = "cloud")]
pub fn validate_forum_title(title: &str) -> Result<(), String> {
    let len = title.len();
    if !(3..=120).contains(&len) {
        return Err("Title must be 3-120 characters".to_string());
    }
    Ok(())
}

/// Local mirror of the server's `POST /api/forum/threads`/
/// `.../replies` body check (1–8192 bytes) — see [`validate_forum_title`]'s
/// own doc.
#[cfg(feature = "cloud")]
pub fn validate_forum_body(body: &str) -> Result<(), String> {
    let len = body.len();
    if len == 0 {
        return Err("Body cannot be empty".to_string());
    }
    if len > 8192 {
        return Err("Body is too long (max 8192 bytes)".to_string());
    }
    Ok(())
}

/// State for the User Settings modal.
#[derive(Clone, Debug)]
pub struct UserSettingsState {
    /// Whether the modal is currently open.
    pub is_open: bool,

    // ── Entitlements (server-provided, `GET /api/entitlements`) ──────────────
    // Two booleans, not a tier enum, because the PROFILE section only ever
    // asks two independent questions: "may this identity be edited" and "may
    // there be more than one of it". A closed tier enum would still need to
    // answer both from the same value every time a new gate is added — these
    // stay orthogonal instead, the way the wire answer itself is a feature
    // set, not a plan name. Both default `false`: that is the FREE set, and
    // it is also the honest state before the shell's first entitlements
    // fetch has answered — see each shell's `cloud_session`/boot wiring,
    // which fills these in and never leaves the UI reading a locked-paid or
    // empty surface while a fetch is in flight or has failed.
    /// A credential exists for this install (native: token store/keychain;
    /// web: stored token) — NOT derived from the entitlements answer, which
    /// only ever describes what an already-known caller is allowed. An
    /// anonymous caller and a signed-in free caller can both resolve to the
    /// same (empty) feature set; this flag is the only thing that tells the
    /// PROFILE section which of the two it is actually drawing (fixed name
    /// vs. an editable nickname).
    pub account_signed_in: bool,
    /// The resolved feature set contains `"multi_profile"`. Gates the whole
    /// profile-list surface (radio dot, Avatar/Delete buttons, "+ New
    /// Profile") — see `render_profile_section`
    /// (`crate::layout::modals::user_settings`) for what each combination
    /// with `account_signed_in` draws.
    pub multi_profile: bool,

    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left when drag started.
    pub drag_offset: Option<(f64, f64)>,
    /// ID of the item currently hovered (for hover highlight).
    pub hovered_item_id: Option<String>,
    /// Currently active tab.
    pub active_tab: UserSettingsTab,
    /// ID of the currently open dropdown (None = no dropdown open).
    pub active_dropdown: Option<String>,
    /// Current recalc mode label synced from indicator_manager (e.g. "Per Frame").
    pub recalc_mode_label: String,
    /// Current UI language code (e.g. "en", "ru").
    /// Synced from UserProfile.language before each render.
    pub language: String,
    /// Whether the periodic RecalcMode diagnostic log is enabled.
    /// Synced from ChartApp.diagnostics_enabled before each render.
    pub diagnostics_enabled: bool,
    /// Whether the Trading tab's container takes over the topbar composition
    /// while it is the current workspace focus (workspace-panel arc, Stage 4).
    /// Synced from `ProfileUiSettings::trading_panel_topbar_takeover` before
    /// each render.
    pub trading_panel_topbar_takeover: bool,
    /// Active control-point (handle) shape, synced from
    /// `ProfileUiSettings::control_point_shape` before each render — drives
    /// which radio option the General tab's "Control points" Shape picker
    /// shows as selected.
    pub control_point_shape: ControlPointShape,
    /// Active control-point (handle) size, synced from
    /// `ProfileUiSettings::control_point_size` before each render — same
    /// role as `control_point_shape` for the Size picker.
    pub control_point_size: ControlPointSize,
    /// Persistence-tab mirrors of `ProfileUiSettings::persist_toolbar_*`.
    pub persist_toolbar_last_tools: bool,
    pub persist_toolbar_toggles: bool,
    /// Whether the internal Agent API server is enabled.
    pub server_enabled: bool,
    /// Port the server listens on.
    pub server_port: u16,
    /// Current server status: "running", "stopped", "error".
    pub server_status: String,

    // ── Agent API key manager (Server tab, agent-api-auth-and-secret-store
    // plan piece 4) ────────────────────────────────────────────────────────
    // This crate cannot see `mlc_backend_shell_http::AgentState` (dependency
    // direction is one-way upward: types → engines → backend → app →
    // shells, and `AgentState` lives in a shell crate) — `main.rs` mirrors
    // `AgentState::list_keys()` into these plain fields every frame instead.
    /// Minted keys' public records: `(fingerprint_hex, label, role,
    /// created_at_ms)`. `role` is the `Debug` spelling of
    /// `mlc_backend_shell_http::auth::AgentKeyRole` (`"Control"` /
    /// `"ReadOnly"`) — the same cross-crate string-mirror idiom
    /// `UiEventEntry::kind` already uses for `SystemEventKind`.
    pub agent_keys: Vec<(String, String, String, u64)>,
    /// Whether the agent-key `SecretStore` is sealed — `true` also while no
    /// store is wired at all, matching that store's own always-sealed
    /// placeholder default. The Server tab shows "Unlock the vault to
    /// manage keys" instead of the list/create form while this is set.
    pub agent_keys_sealed: bool,
    /// The most recently minted raw agent key, drained once from
    /// `AgentState::take_last_minted_key()` by `main.rs` — the Server tab's
    /// one-time reveal box shows this until "Copy"/dismiss clears it.
    pub last_created_key: Option<String>,
    /// Render-time snapshot of the key-manager "label" input (populated
    /// from TextFieldStore by ChartApp), same idiom as `nick_edit_render`.
    pub key_label_render: ActiveFieldRender,
    /// Whether the key-manager "label" input field is focused for keyboard
    /// input.
    pub key_label_focused: bool,

    /// Scroll state for the General tab content.
    pub general_tab_scroll: ScrollState,
    /// Scroll state for the Sync tab content.
    pub sync_tab_scroll: ScrollState,
    /// Scroll state for the Vault tab content — modelled exactly on
    /// `general_tab_scroll`; the vault surface used to be its own modal
    /// (which had no scrolling) and is now a tab body long enough to need
    /// it (status row + key list + webhook section, or the add-key form).
    pub vault_tab_scroll: ScrollState,
    /// Scroll state for the Persistence tab content.
    pub persistence_tab_scroll: ScrollState,
    /// Scroll state for the Server tab content.
    pub server_tab_scroll: ScrollState,
    /// Scroll state for the Performance tab content.
    pub performance_tab_scroll: ScrollState,

    // ── WELCOME WIZARD ──────────────────────────────────────────────────
    /// True when the first-run welcome wizard should be shown (no profile.json on first launch).
    pub show_welcome_wizard: bool,
    /// True when the profile is encrypted (salt.hex exists) but no vault key has been derived
    /// yet this session — informational only (owner's ruling 2026-08-31: a locked vault
    /// does not gate the app). Read by the profile-list row click to decide whether
    /// selecting the current profile routes to `UnlockPassphrase` or just dismisses.
    pub needs_vault_unlock: bool,
    /// Error message shown on the vault unlock overlay when the passphrase is wrong.
    /// Cleared when the user starts typing a new passphrase.
    pub vault_unlock_error: Option<String>,
    /// Number of consecutive failed vault unlock attempts.
    /// After 3 failures the "Forgot passphrase? Create new profile" button is shown.
    pub vault_unlock_attempts: u32,
    /// True only while the profile manager is showing a step with no live
    /// chart behind it yet (migration: an existing plaintext profile setting
    /// its first passphrase) — unlike `needs_vault_unlock`, this genuinely
    /// blocks dismissal (close button, dimmer click, Esc) because there is
    /// nothing usable to fall back to underneath.
    pub profile_manager_mandatory: bool,
    // ── PROFILE MANAGER ───────────────────────────────────────────────────────
    /// True when the profile manager overlay is shown (replaces vault_unlock + profile picker).
    pub show_profile_manager: bool,
    /// Formatted recovery key to display on the `ShowRecoveryKey` page.
    ///
    /// Set by `main.rs` after a successful vault creation and cleared when the
    /// user confirms they have recorded it.
    pub recovery_key_display: Option<String>,
    /// Current page of the profile manager.
    pub profile_manager_page: ProfileManagerPage,
    /// Profile ID being operated on in the profile manager (unlock/create passphrase target).
    pub profile_manager_target_id: String,
    /// Display name of the target profile (for showing in headers).
    pub profile_manager_target_name: String,
    /// Whether each profile has vault encryption.
    /// Vec<(id, display_name, avatar, has_vault)>.
    pub profiles_with_vault_status: Vec<(String, String, String, bool)>,
    /// Wizard page: 0 = mode selection, 1 = link account, 2 = E2E setup.
    pub wizard_page: u8,
    /// True if the user selected the E2E option (so page 2 is shown after linking).
    pub wizard_e2e_chosen: bool,

    // ── Welcome wizard drag geometry ──────────────────────────────────────
    // A SEPARATE trio from `position`/`is_dragging`/`drag_offset` above,
    // deliberately: those back the User Settings modal itself, and the
    // wizard renders from this same `UserSettingsState` but is a different
    // on-screen surface (`welcome_wizard.rs`'s own `begin_modal_shell` call,
    // namespace `"welcome_wizard"`, never `"user_settings"`). Sharing the
    // trio would make dragging the wizard move the (currently closed)
    // Settings window's remembered position too, and vice versa — the two
    // are never open at once, but their geometry must not alias regardless.
    /// Wizard modal position (`None` = centered on screen — the wizard's
    /// resting state on open and after every dismiss, never inherited from
    /// wherever a previous run's drag left it; see `reset_wizard_position`).
    pub wizard_position: Option<(f64, f64)>,
    /// Whether the wizard is currently being dragged by its header.
    pub wizard_is_dragging: bool,
    /// Offset from mouse to the wizard's top-left when its drag started.
    pub wizard_drag_offset: Option<(f64, f64)>,

    // ── Profile ──────────────────────────────────────────────────────────
    /// Display name of the active profile.
    pub profile_display_name: String,
    /// Avatar key of the active profile (e.g. "chart", "rocket").
    pub profile_avatar: String,
    /// UUID of the active profile.
    pub profile_id: String,
    /// UUID of the profile that is ACTUALLY loaded and running in this session.
    /// Set once at window creation from `user_manager.profile.profile_id` and never
    /// updated mid-session.  `profile_id` may diverge from this after a
    /// `profile_switch` (pending restart), but `runtime_profile_id` always reflects
    /// the truly-active profile so that Rename/Avatar/Delete buttons appear on the
    /// correct row.
    pub runtime_profile_id: String,
    /// All available profiles as (id, display_name, avatar) tuples.
    pub available_profiles: Vec<(String, String, String)>,
    /// Whether the profile name is currently being edited inline.
    pub profile_rename_mode: bool,
    /// Render-time snapshot of the profile rename input (populated from TextFieldStore by ChartApp).
    pub profile_rename_render: ActiveFieldRender,
    /// Whether the profile rename input field is focused for keyboard input.
    pub profile_rename_focused: bool,
    /// ID of the profile row currently being renamed (None = not renaming).
    pub profile_rename_target_id: Option<String>,

    // ── Account nickname (server-side, distinct from the local profile's
    // `profile_meta` display name above — see `POST /api/account/nick`) ──
    /// Whether the account nickname is being edited inline in the
    /// single-profile row's "Nickname" control. A signed-in identity's
    /// account nick is an ACCOUNT attribute set via `POST
    /// /api/account/nick` (owner's identity ruling 2026-09-02: "a nickname
    /// is an ACCOUNT attribute ... never a profile name" —
    /// `welcome_wizard.rs`'s own doc) — this never touches
    /// `profile_rename_mode`/`profile_display_name` above, even though
    /// both live in the same row for a single-profile identity.
    pub nick_edit_mode: bool,
    /// Render-time snapshot of the nick edit input (populated from
    /// TextFieldStore by ChartApp), same idiom as `profile_rename_render`.
    pub nick_edit_render: ActiveFieldRender,
    /// Whether the nick edit input field is focused for keyboard input.
    pub nick_edit_focused: bool,
    /// One-shot flag set by the Save button (or Enter in the field, once
    /// local validation via [`validate_nick`] passes); the shell observes
    /// it, submits the text currently in the field via its own cloud
    /// transport (`mlc-cloud`'s `set_nick` on web, `mlc-updater`'s
    /// `nick::set_nick` on native), and clears it. Same mailbox idiom as
    /// `CloudLoginState::redeem_requested`.
    pub nick_save_requested: bool,
    /// Outcome of the most recent nick submission (local validation
    /// failure or the server's answer).
    pub nick_status: NickStatus,
    /// The signed-in account's current label (nick if set, else the OAuth
    /// display name) — mirrors the chrome's own account-label source
    /// (web: `CloudSession::account_label`; native:
    /// `AuthStatus::LoggedIn::display_name`), synced every frame, so the
    /// edit box can prefill with what the person already sees elsewhere
    /// rather than the unrelated local profile name. Empty when signed out.
    pub account_label: String,

    /// Whether the avatar picker popover is open.
    pub show_avatar_picker: bool,
    /// ID of the profile whose avatar picker is open (None = active profile).
    pub profile_avatar_target_id: Option<String>,
    /// Whether the "New Profile" inline dialog is open.
    pub show_new_profile_dialog: bool,
    /// Render-time snapshot of the new profile name input (populated from TextFieldStore by ChartApp).
    pub new_profile_name_render: ActiveFieldRender,
    /// Whether the new profile name input field is focused for keyboard input.
    pub new_profile_name_focused: bool,

    // ── Set New Passphrase (post-recovery re-key) ─────────────────────────────
    /// Render-time snapshot of the new passphrase input (populated from TextFieldStore by ChartApp).
    /// The TextFieldStore holds the real text; this proxy is read by the layout layer.
    pub new_passphrase_render: ActiveFieldRender,
    /// Whether the new passphrase input field has keyboard focus.
    pub new_passphrase_focused: bool,
    /// Render-time snapshot of the confirm passphrase input (populated from TextFieldStore by ChartApp).
    pub confirm_passphrase_render: ActiveFieldRender,
    /// Whether the confirm passphrase input field has keyboard focus.
    pub confirm_passphrase_focused: bool,
    /// Error message shown on the SetNewPassphrase page (e.g. "Passphrases do not match").
    pub set_passphrase_error: String,

    // ── Profile list scroll ───────────────────────────────────────────────────
    /// Scroll state for the profile list in the ProfileList page.
    pub profile_list_scroll: ScrollState,

    // ── Profile manager text selection drag ───────────────────────────────────
    /// Which profile manager input field is currently being drag-selected.
    /// The value is the widget ID (e.g. "e2e_passphrase_input", "profile_mgr:name_input").
    /// None = no drag in progress.
    pub profile_mgr_text_select_dragging: Option<String>,

    // ── Recovery key display editing state ────────────────────────────────────
    /// Render-time snapshot of the recovery key display box (populated from TextFieldStore by ChartApp).
    /// The user can select/copy the key text by clicking and dragging inside the display box.
    pub recovery_key_display_render: ActiveFieldRender,
    /// Whether the recovery key display box has logical focus (user clicked it).
    /// Does NOT allow typing — only Ctrl+A and Ctrl+C (via on_copy_selection) are active.
    pub recovery_key_display_focused: bool,

    /// Timestamp (ms) when recovery key was last copied. Used for visual feedback.
    pub recovery_key_copied_at: u64,

    // ── DATA & CACHE slider state ─────────────────────────────────────────────
    /// Cached value synced from `UserProfile.data_load.background_bar_count`.
    pub data_bg_bars: u32,
    /// Cached value synced from `UserProfile.data_load.max_loaded_bars`.
    pub data_max_bars: u32,
    /// Active slider drag for the DATA & CACHE sliders.
    pub data_slider_drag: Option<SliderDragState>,

    // ── WELCOME WIZARD — per-page selections ─────────────────────────────────
    /// Theme preset selected on wizard page 3 (e.g. "dark", "mascot").
    /// Empty string means no explicit selection yet — renderer falls back to "dark".
    pub wizard_selected_theme: String,
    /// Rendering backend selected on wizard page 4
    /// (e.g. "vello_gpu", "instanced_wgpu", "vello_cpu", "vello_hybrid", "tiny_skia").
    /// Empty string means no explicit selection yet — renderer falls back to "vello_gpu".
    pub wizard_selected_backend: String,

    /// Whether the profile-manager's passphrase fields currently show plain
    /// text instead of bullets — one flag for the unlock field, the create +
    /// confirm pair, and the set-new + confirm pair alike, since a pair is
    /// always revealed together (comparing a passphrase against its
    /// confirmation is the main reason to look). Reset whenever the surface
    /// is dismissed or its page changes away from a passphrase page (see
    /// `ChartApp::dismiss_profile_manager` and the `"profile_mgr:back"`
    /// click arm).
    pub passphrase_revealed: bool,

}

impl Default for UserSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            account_signed_in: false,
            multi_profile: false,
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            active_tab: UserSettingsTab::default(),
            active_dropdown: None,
            recalc_mode_label: "Per Frame".to_string(),
            language: "en".to_string(),
            diagnostics_enabled: false,
            trading_panel_topbar_takeover: false,
            control_point_shape: ControlPointShape::default(),
            control_point_size: ControlPointSize::default(),
            persist_toolbar_last_tools: true,
            persist_toolbar_toggles: true,
            server_enabled: true,
            server_port: 17420,
            server_status: "running".to_string(),
            agent_keys: Vec::new(),
            agent_keys_sealed: true,
            last_created_key: None,
            key_label_render: ActiveFieldRender::default(),
            key_label_focused: false,
            general_tab_scroll: ScrollState::default(),
            sync_tab_scroll: ScrollState::default(),
            vault_tab_scroll: ScrollState::default(),
            persistence_tab_scroll: ScrollState::default(),
            server_tab_scroll: ScrollState::default(),
            performance_tab_scroll: ScrollState::default(),
            show_welcome_wizard: false,
            needs_vault_unlock: false,
            vault_unlock_error: None,
            vault_unlock_attempts: 0,
            profile_manager_mandatory: false,
            show_profile_manager: false,
            recovery_key_display: None,
            profile_manager_page: ProfileManagerPage::ProfileList,
            profile_manager_target_id: String::new(),
            profile_manager_target_name: String::new(),
            profiles_with_vault_status: Vec::new(),
            wizard_page: 0,
            wizard_e2e_chosen: false,
            wizard_position: None,
            wizard_is_dragging: false,
            wizard_drag_offset: None,
            // Empty, not "Default" (owner's identity ruling 2026-09-02: an
            // anonymous profile has no user-facing name at all). This is a
            // pre-sync placeholder that a real profile load overwrites within
            // a frame — but it was the last place in the tree still spelling
            // the old default out, and a stale literal here would reappear on
            // any path that renders before that sync lands.
            profile_display_name: String::new(),
            profile_avatar: "chart".to_string(),
            profile_id: String::new(),
            runtime_profile_id: String::new(),
            available_profiles: Vec::new(),
            profile_rename_mode: false,
            profile_rename_render: ActiveFieldRender::default(),
            profile_rename_focused: false,
            profile_rename_target_id: None,
            nick_edit_mode: false,
            nick_edit_render: ActiveFieldRender::default(),
            nick_edit_focused: false,
            nick_save_requested: false,
            nick_status: NickStatus::default(),
            account_label: String::new(),
            show_avatar_picker: false,
            profile_avatar_target_id: None,
            show_new_profile_dialog: false,
            new_profile_name_render: ActiveFieldRender::default(),
            new_profile_name_focused: false,
            new_passphrase_render: ActiveFieldRender::default(),
            new_passphrase_focused: false,
            confirm_passphrase_render: ActiveFieldRender::default(),
            confirm_passphrase_focused: false,
            set_passphrase_error: String::new(),
            profile_list_scroll: ScrollState::new(),
            profile_mgr_text_select_dragging: None,
            recovery_key_display_render: ActiveFieldRender::default(),
            recovery_key_display_focused: false,
            recovery_key_copied_at: 0,
            data_bg_bars: 2000,
            data_max_bars: 10000,
            data_slider_drag: None,
            wizard_selected_theme: String::new(),
            wizard_selected_backend: String::new(),
            passphrase_revealed: false,
        }
    }
}

impl UserSettingsState {
    /// Open the modal.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the modal and reset drag state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
        self.active_dropdown = None;
    }

    /// Toggle open/closed.
    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Switch to a different tab and close any open dropdown.
    pub fn set_tab(&mut self, tab: UserSettingsTab) {
        self.active_tab = tab;
        self.active_dropdown = None;
    }

    /// Start dragging from the modal header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update modal position while dragging.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }

    // ── Welcome wizard drag helpers ───────────────────────────────────────────
    // Own trio, own methods — see the field docs on `wizard_position` for why
    // this cannot reuse `start_drag`/`update_drag`/`end_drag` above.

    /// Start dragging the wizard from its header.
    pub fn wizard_start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.wizard_is_dragging = true;
        self.wizard_drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update the wizard's position while dragging.
    pub fn wizard_update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.wizard_drag_offset {
            self.wizard_position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging the wizard. Keeps `wizard_position` — a released drag is
    /// still where the user put it, for the rest of THIS run.
    pub fn wizard_end_drag(&mut self) {
        self.wizard_is_dragging = false;
        self.wizard_drag_offset = None;
    }

    /// Full reset of the wizard's drag geometry — called at the wizard's own
    /// open/close boundaries (dismiss via ×/dimmer/Esc, "Show Wizard", "Run
    /// Wizard", and the wizard's own completion), so a position dragged to in
    /// a PREVIOUS showing never carries into the next: the wizard always
    /// opens centred.
    pub fn reset_wizard_position(&mut self) {
        self.wizard_position = None;
        self.wizard_is_dragging = false;
        self.wizard_drag_offset = None;
    }

    // ── DATA & CACHE slider helpers ───────────────────────────────────────────

    /// Start dragging a DATA & CACHE slider from a track hit.
    pub fn start_data_slider_drag(&mut self, field_id: &str, track_x: f64, track_width: f64, min_val: f64, max_val: f64) {
        self.data_slider_drag = Some(SliderDragState {
            field_id: field_id.to_string(),
            slider_x: track_x,
            slider_width: track_width,
            min_val,
            max_val,
            dual_handle: None,
            floating_value: None,
            floating_value2: None,
        });
    }

    /// Update floating value during drag.  Returns `Some((field_id, value))`.
    pub fn update_data_slider_drag(&mut self, mouse_x: f64) -> Option<(&str, f64)> {
        if let Some(ref mut drag) = self.data_slider_drag {
            let t = ((mouse_x - drag.slider_x) / drag.slider_width).clamp(0.0, 1.0);
            let value = drag.min_val + t * (drag.max_val - drag.min_val);
            drag.floating_value = Some(value);
            Some((&drag.field_id, value))
        } else {
            None
        }
    }

    /// Whether a DATA & CACHE slider is currently being dragged.
    pub fn is_data_slider_dragging(&self) -> bool {
        self.data_slider_drag.is_some()
    }

    /// Consume the final value on drag-end.  Clears drag state.
    pub fn take_data_slider_value(&mut self) -> Option<(String, f64)> {
        self.data_slider_drag.take().and_then(|d| d.floating_value.map(|v| (d.field_id, v)))
    }

    /// Cancel drag without committing.
    pub fn end_data_slider_drag(&mut self) {
        self.data_slider_drag = None;
    }

    /// Return the floating (preview) value if dragging.
    pub fn data_slider_floating(&self) -> Option<(&str, f64)> {
        self.data_slider_drag.as_ref().and_then(|d| d.floating_value.map(|v| (d.field_id.as_str(), v)))
    }
}

impl_resizable_modal!(
    PrimitiveSettingsState => (360.0, 260.0),
    ChartSettingsState => (480.0, 380.0),
    IndicatorSettingsState => (420.0, 320.0),
    CompareSettingsState => (380.0, 300.0),
);

// =============================================================================
// =============================================================================
// MarkupEngineSettingsState — the "Price Action Engine" modal, the
// non-Elliott sibling of `EwaEngineSettingsState` below.
//
// ONE modal for three engines, with a family segment at the top, rather
// than three near-identical files. The three configs differ in their rows,
// not in their shape, and the plan this comes from
// (`docs/mlc/plans/markup-engine-parity-2026-08-25.md`) says in its own §4
// that the parity being built is parity of MACHINERY, not of ceremony. The
// entrance sets `family` from the subject that opened it, so opening from
// an order block lands on Smart Money — the same place three separate
// modals would have landed, without three copies of the shell, the drag
// handling, the result plumbing and the click routing.
//
// Same apply-on-change, no-diff shape as the EWA modal: every edit writes
// through to the live global and the profile, and the footer's close is
// just a close.
// =============================================================================

/// State for the "Price Action Engine" modal — three engine configs and
/// which family's rows are currently showing.
#[derive(Clone, Debug, Default)]
pub struct MarkupEngineSettingsState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// Modal position (`None` = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left when drag started.
    pub drag_offset: Option<(f64, f64)>,
    /// ID of the item currently hovered (for hover highlight).
    pub hovered_item_id: Option<String>,
    /// Which family's rows are showing — set by whichever claim opened the
    /// modal, then switchable by the segment at the top.
    pub family: MarkupSettingsFamily,
    /// Live copies of the three engine configs; see this section's own doc
    /// for why there is no separate pending copy.
    pub smc: SmcEngineConfig,
    pub orderflow: OrderFlowEngineConfig,
    pub candle: CandlePatternEngineConfig,
}

/// Which family's rows the modal is showing. Mirrors
/// `mlc_core::drawing::primitives_v2::markup::MarkupFamily` but is a UI
/// concern of its own — it survives the modal being open while nothing is
/// selected, which a family read off a selection cannot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkupSettingsFamily {
    #[default]
    SmartMoney,
    OrderFlow,
    CandlePattern,
}

impl MarkupSettingsFamily {
    /// The wire token this family registers its own segment under.
    pub fn id(self) -> &'static str {
        match self {
            Self::SmartMoney => "smart_money",
            Self::OrderFlow => "order_flow",
            Self::CandlePattern => "candles",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "smart_money" => Some(Self::SmartMoney),
            "order_flow" => Some(Self::OrderFlow),
            "candles" => Some(Self::CandlePattern),
            _ => None,
        }
    }

    /// Which family a primitive's own `type_id` belongs to — the entrance's
    /// own lookup, so opening the modal from an order block lands on Smart
    /// Money rather than on whatever was showing last.
    pub fn from_type_id(type_id: &str) -> Option<Self> {
        match type_id {
            "order_block" | "fair_value_gap" | "liquidity_sweep" | "structure_break" | "dealing_range"
            | "amd_cycle" | "smc_scan" => Some(Self::SmartMoney),
            "imbalance_zone" | "absorption_mark" | "delta_divergence" | "order_flow_scan" => {
                Some(Self::OrderFlow)
            }
            "candle_pattern" | "candle_pattern_scan" => Some(Self::CandlePattern),
            _ => None,
        }
    }
}

impl MarkupEngineSettingsState {
    /// Open the modal on `family`'s own rows.
    pub fn open_on(&mut self, family: MarkupSettingsFamily) {
        self.family = family;
        self.is_open = true;
    }

    /// Close the modal and reset drag state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Start dragging from the modal header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update modal position while dragging.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// Finish dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

// EwaEngineSettingsState — the "EWA Engine Settings" modal (owner ask
// 2026-08-14: "для настройки ЕВА движка можно сделать отдельную настройку,
// ну вот прям модалку ЕВА движка, в которую можно попасть из каждого ЕВА
// дравлинга — из его модалки настроек или по какой-то ещё общей кнопке типа
// EWA settings в ПКМ или дабл-ЛКМ"). See
// `crate::layout::modals::ewa_engine_settings` for the renderer.
//
// `config` is the modal's own live copy of `mlc_core::state::ewa_engine::
// EwaEngineConfig` — mirrored from the profile at load
// (`ChartApp::apply_profile_state`) and written through on every edit (the
// SAME apply-on-change, no-diff shape `UserSettingsState::control_point_shape`/
// `control_point_size` already use for the same class of process-global,
// RAM+profile setting — see that struct's own field docs). There is
// deliberately no separate "pending" copy: this config is not per-drawing,
// so there is nothing to accept or discard.
// =============================================================================

/// State for the "EWA Engine Settings" modal — fixed size, no tabs, no
/// scrolling (its whole body is a handful of rows), draggable by its own
/// header like every other settings modal in this file.
#[derive(Clone, Debug)]
pub struct EwaEngineSettingsState {
    /// Whether the modal is currently open.
    pub is_open: bool,
    /// Modal position (`None` = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left when drag started.
    pub drag_offset: Option<(f64, f64)>,
    /// ID of the item currently hovered (for hover highlight).
    pub hovered_item_id: Option<String>,
    /// The modal's own live copy of the engine config — see this section's
    /// own doc for why there is no separate pending/dirty copy.
    pub config: EwaEngineConfig,
}

impl Default for EwaEngineSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            config: EwaEngineConfig::default(),
        }
    }
}

impl EwaEngineSettingsState {
    /// Open the modal.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Close the modal and reset drag state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Toggle open/closed.
    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Start dragging from the modal header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update modal position while dragging.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}

// =============================================================================
// CloudLoginState — sign-in / account overlay (cloud builds only)
// =============================================================================

/// Phase of the device-code login flow.
///
/// This struct is pure UI state plus a flag mailbox — the actual I/O lives in
/// the shell's cloud session driver (`mlc-app-shell-web`, which owns the
/// `mlc-cloud` transport). The driver rewrites `phase` as the flow advances.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub enum CloudLoginPhase {
    /// Offer the OAuth providers directly, with no device code in play.
    ///
    /// The native shell signs in through a loopback redirect
    /// (`mlc-updater`'s `oauth::start_oauth_flow`: browser → localhost
    /// callback → token), which needs nothing but a provider choice. The
    /// device-code phases below exist for a client that cannot receive a
    /// redirect; a desktop app can, so making it show a code and ask the
    /// user to type it somewhere is ceremony with no purpose.
    ChooseProvider,
    /// Just opened; the shell should request a device code.
    RequestCode,
    /// Waiting for the code-creation request to finish.
    CreatingCode,
    /// Code in hand — shown to the user, polling for confirmation.
    AwaitingConfirm { code: String },
    /// Login succeeded this session.
    Success { display_name: String },
    /// Already authenticated — account view (cabinet).
    SignedIn { display_name: String, provider: String },
    /// Flow failed (network, expired code, attestation rejected).
    Error { message: String },
}

/// Outcome of the most recent gift/trial code submission.
///
/// A mailbox, not a request: the modal draws whatever is here, the shell owns
/// the round trip and writes the result back. Both shells reach the same
/// endpoint by different roads — the web one through `mlc-cloud`'s transport,
/// the desktop one through `mlc-updater`'s watch channel — and this is where
/// the two meet.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq, Default)]
pub enum RedeemStatus {
    /// Nothing submitted since the overlay opened.
    #[default]
    Idle,
    /// A submission is in flight — the button is dead and says so.
    Working,
    /// Accepted; the account is now paid until this instant (RFC 3339).
    Redeemed { paid_until: String },
    /// Refused, with the sentence to show the person who typed it.
    Failed { message: String },
}

/// State for the cloud sign-in / account overlay.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug)]
pub struct CloudLoginState {
    /// Whether the overlay is currently open.
    pub is_open: bool,
    /// Current flow phase (owned by the shell's cloud driver).
    pub phase: CloudLoginPhase,
    /// A provider the user picked in [`CloudLoginPhase::ChooseProvider`],
    /// waiting for the shell to act on it.
    ///
    /// What "act on it" means is the shell's business and differs between
    /// them — native runs the loopback flow in-process, web navigates — so
    /// the layout crate records the choice and stays out of it.
    pub oauth_login_requested: Option<String>,
    /// One-shot mailbox: a URL the shell must open in a new tab (sign-in
    /// page, OAuth provider authorize URL). Set by modal buttons, cleared
    /// by the driver after the window.open call.
    pub open_url_requested: Option<String>,
    /// One-shot flag set by the "Sign out" button in the account view; the
    /// shell observes it, revokes the token, and clears it.
    pub sign_out_requested: bool,
    /// One-shot flag set by the "Redeem" button (or Enter in the code field)
    /// in the account view; the shell observes it, submits the code currently
    /// in `text_input::REDEEM_CODE`, and clears it.
    pub redeem_requested: bool,
    /// What came back from the last submission. Written by the shell.
    pub redeem_status: RedeemStatus,
    /// Whether this account's paid access is live, as last answered by
    /// `GET /api/entitlements`. Written by the shell every time that answer
    /// lands — never inferred here from a feature name.
    pub is_paid: bool,
    /// When that paid access ends (RFC 3339), when it is paid at all.
    pub paid_until: Option<String>,
    /// Overlay position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the overlay is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to overlay top-left (for smooth dragging).
    pub drag_offset: Option<(f64, f64)>,
}

#[cfg(feature = "cloud")]
impl Default for CloudLoginState {
    fn default() -> Self {
        Self {
            is_open: false,
            phase: CloudLoginPhase::RequestCode,
            oauth_login_requested: None,
            open_url_requested: None,
            sign_out_requested: false,
            redeem_requested: false,
            redeem_status: RedeemStatus::Idle,
            is_paid: false,
            paid_until: None,
            position: None,
            is_dragging: false,
            drag_offset: None,
        }
    }
}

#[cfg(feature = "cloud")]
impl CloudLoginState {
    /// Open the overlay. The shell's driver decides the initial phase
    /// (SignedIn when a token exists, RequestCode otherwise).
    pub fn open(&mut self) {
        self.is_open = true;
        self.open_url_requested = None;
        self.sign_out_requested = false;
        self.redeem_requested = false;
        // Last visit's "already used" is not this visit's news.
        self.redeem_status = RedeemStatus::Idle;
        self.phase = CloudLoginPhase::RequestCode;
    }

    /// Close the overlay. One-shot flags are cleared; the phase is left
    /// for the driver to reset on next open.
    pub fn close(&mut self) {
        self.is_open = false;
        self.open_url_requested = None;
        self.sign_out_requested = false;
        self.redeem_requested = false;
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Toggle open/closed.
    pub fn toggle(&mut self) {
        if self.is_open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Start dragging from the overlay header.
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update overlay position while dragging.
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    /// End dragging.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}

// =============================================================================
// CloudOverlayState — News / Download overlays (cloud builds only)
// =============================================================================

// =============================================================================
// Forum (News overlay, `NewsTab::Forum`, paid-only) — see the plan at
// `docs/mlc/plans/web-surface-cleanup-and-in-app-blog-2026-09-05.md` phase 3
// and the scope at `docs/mlc/plans/users-paid-features-scope-2026-09-05.md`
// §4. Server: `GET/POST/DELETE /api/forum/*`, auth + paid required, 401
// anonymous / 403 free.
// =============================================================================

/// One row of the Forum tab's thread list. A flat mirror of
/// `mlc_cloud::ForumThreadSummary`, redeclared here for the same reason
/// `CloudNewsPost` mirrors `mlc_cloud::BlogPost`: the layout crate must not
/// depend on the cloud transport crate.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloudForumThreadSummary {
    pub id: i64,
    pub title: String,
    pub author: String,
    /// Whether the SIGNED-IN caller authored this thread — drives the
    /// "Delete" affordance on the thread row/detail.
    pub author_is_me: bool,
    pub created_at: String,
    pub last_reply_at: Option<String>,
    pub reply_count: i64,
    pub excerpt: String,
}

/// One reply under an open thread. A flat mirror of `mlc_cloud::ForumReply`.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloudForumReply {
    pub id: i64,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
}

/// A thread with its replies, as shown in the Forum tab's reading view. A
/// flat merge of `mlc_cloud::ForumThread` + `ForumThreadResponse::replies`
/// — the wire shape keeps them as two sibling fields, this crate's shell
/// callers combine them before writing this.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloudForumThread {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
    pub replies: Vec<CloudForumReply>,
}

/// Fetch state of the Forum tab's thread list — independent of
/// [`ForumSubView`] so returning from a thread (or an abandoned compose)
/// never re-fetches page one.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ForumListState {
    #[default]
    Empty,
    Loading,
    Loaded {
        threads: Vec<CloudForumThreadSummary>,
        /// Pagination cursor for "Load more" — `None` once the list has
        /// run out.
        next_before: Option<String>,
        /// A "Load more" page fetch is in flight — drawn as a disabled
        /// button rather than removing the row, same reasoning
        /// `cloud_login.rs`'s redeem button gives for its own `working`
        /// flag: the row must not reflow under the cursor that just
        /// clicked it.
        loading_more: bool,
    },
    Error {
        message: String,
    },
}

/// Which Forum sub-view is showing: the list (default — reads
/// [`CloudOverlayState::forum_list`]), one open thread, or the "new
/// thread" compose form.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ForumSubView {
    #[default]
    List,
    Thread {
        /// `None` while the `LoadThread` request is in flight.
        thread: Option<CloudForumThread>,
        loading: bool,
        /// The thread load itself failed (wrong id, network, revoked
        /// entitlement mid-session).
        error: Option<String>,
        /// A reply submission is in flight.
        posting_reply: bool,
        /// The last reply submission's refusal, if any — separate from
        /// `error` because a reply can fail on an otherwise successfully
        /// loaded thread.
        reply_error: Option<String>,
    },
    Compose {
        /// A create-thread submission is in flight.
        posting: bool,
        /// The last submission's refusal (server or local validation), if
        /// any.
        error: Option<String>,
    },
}

/// Which cloud overlay is showing.
#[cfg(feature = "cloud")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudOverlayKind {
    /// Release news ("what's new") — data from GET /api/updates/latest.
    /// Also carries the LINKS section (social/project links) at the bottom
    /// of its list view; a standalone Links overlay used to hold those, but
    /// a bare list of links was thin content for a chrome button of its own
    /// (owner, 2026-09-04).
    News,
    /// The same release manifest, read by whichever surface is asking.
    ///
    /// On the web page it means "get the desktop app" and renders download
    /// links. In the desktop app it means "install this update": the shell
    /// already runs the updater against that manifest in-process, so the
    /// overlay shows update state and an Install button, never a link to a
    /// binary the user is expected to fetch by hand. One feed, two verbs —
    /// see [`CloudOverlayData::Update`].
    Download,
}

/// Fetched content for the News/Download overlays (filled by the shell driver).
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub enum CloudOverlayData {
    /// Nothing fetched yet — the driver should request it.
    Empty,
    /// Fetch in flight.
    Loading,
    /// Web's read of the release manifest: this visitor's own build (the
    /// collapsed view) plus the full release catalogue, fetched once
    /// alongside it, for the expanded "other platforms & versions" list.
    Ready {
        /// This visitor's platform id, resolved by the shell BEFORE the
        /// fetch — `win64` / `macos-arm64` / `macos-x64` / `linux64`
        /// (`mlc_updater::platform`'s own ids). The caption and the
        /// primary action row read this rather than re-deriving it.
        platform: String,
        version: String,
        notes: String,
        download_url: String,
        /// Every published release, every platform, newest first — read
        /// once alongside the fetch above; shown only once the overlay is
        /// expanded (`CloudOverlayState::expanded`).
        releases: Vec<CloudReleaseEntry>,
    },
    /// Blog feed for the News overlay (from /blog/posts.json).
    News { posts: Vec<CloudNewsPost> },
    /// Native's read of the release manifest: where the in-process updater
    /// currently stands. Never carries a download URL — see
    /// [`UpdateOverlayPhase`].
    Update { phase: UpdateOverlayPhase },
    /// Fetch failed.
    Error { message: String },
}

/// The updater's state as the Download overlay shows it on a desktop build.
///
/// A flat mirror of `mlc_updater::UpdateStatus`, redeclared here because the
/// layout crate must not depend on the updater — the shell maps one onto the
/// other. Every phase past `Available` is the app updating *itself*: it
/// fetches, checks the hash and replaces its own binary, which is the whole
/// reason the desktop surface does not hand out a link.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateOverlayPhase {
    /// Asking the manifest. Distinct from `UpToDate` on purpose: the updater
    /// sits idle both before and after a check, and reporting "you are up to
    /// date" while the question is still open would be a guess.
    Checking,
    /// The manifest answers with the version already running.
    UpToDate { version: String },
    /// A newer build exists and has not been started.
    Available { version: String, notes: String },
    /// Fetching the new binary.
    Downloading { percent: u8 },
    /// Checking the downloaded binary's hash against the manifest.
    Verifying,
    /// Writing the new binary over the running one.
    Installing,
    /// Installed — it takes effect on the next launch.
    RestartPending,
    /// The update run failed; the installed build is untouched.
    Failed { message: String },
}

/// One entry in the release catalogue, as shown in the Download overlay's
/// expanded list. A flat mirror of `mlc_cloud::ReleaseEntry`, redeclared
/// here for the same reason `CloudNewsPost` mirrors `mlc_cloud::BlogPost`:
/// the layout crate must not depend on the cloud transport crate.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloudReleaseEntry {
    pub platform: String,
    pub version: String,
    pub file_size: u64,
    pub download_url: String,
    /// Server timestamp (`created_at`), ISO-8601. Shown trimmed to a date
    /// — this crate does not depend on a datetime library to re-parse it.
    pub created_at: String,
}

/// One blog post as shown in the News overlay.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub struct CloudNewsPost {
    pub id: String,
    /// `"post"` (editorial) or `"patch"` (release note) — which of the News
    /// list view's two tabs ([`NewsTab`]) this entry shows under.
    pub kind: String,
    /// Release version this entry documents (patch kind only); when
    /// present, the Patch notes tab shows it in place of `title`.
    pub version: Option<String>,
    pub date: String,
    pub title: String,
    pub summary: String,
    /// Full text for the in-modal reading view (blank-line separated
    /// paragraphs); falls back to the summary when the feed lacks it.
    pub body: String,
    pub url: String,
}

/// Which slice of the blog feed the News overlay's list view shows.
///
/// Added 2026-09-04: the feed plus the LINKS section beneath it outgrew one
/// scroll, so the list view got a tab bar instead of a longer page.
/// Reshaped 2026-09-05 (server now tags every entry with `kind`): `Posts`
/// is the glance a visitor gets by default — up to a handful of editorial
/// entries (`kind != "patch"`) and the same LINKS section this overlay
/// always showed; `PatchNotes` trades the LINKS section for the full,
/// unbounded release-note history (`kind == "patch"`), newest first. The
/// variants were `Latest`/`All` before this reshape — `from_id` still
/// accepts those ids so a client mid-upgrade (or a stale click target from
/// the other tier) does not silently no-op.
#[cfg(feature = "cloud")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NewsTab {
    #[default]
    Posts,
    PatchNotes,
    /// Paid-only communication board (`GET/POST/DELETE /api/forum/*`).
    /// Drawn only when [`CloudOverlayState::forum_entitled`] is true — see
    /// `cloud_overlay.rs`'s `news_tab_items`. Content lives in
    /// [`CloudOverlayState::forum_list`]/`forum_view`, NOT `CloudOverlayState::
    /// data` — switching to/from this tab must not discard (or be discarded
    /// by) the blog feed the other two tabs share.
    Forum,
}

#[cfg(feature = "cloud")]
impl NewsTab {
    /// Tab id — also the widget-id suffix the shell's tab bar registers
    /// (`cloud_overlay:tab:{id}`), so this is the one place the two sides of
    /// that click can drift apart.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Posts => "posts",
            Self::PatchNotes => "patch_notes",
            Self::Forum => "forum",
        }
    }

    /// Tab label, in the app's short lower-case-free chrome voice.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Posts => "Posts",
            Self::PatchNotes => "Patch notes",
            Self::Forum => "Forum",
        }
    }

    /// Accepts both the current ids and the pre-2026-09-05 `latest`/`all`
    /// ids, mapped onto their closest surviving meaning (`latest` was
    /// always the default glance, `all` was always the unbounded list).
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "posts" | "latest" => Some(Self::Posts),
            "patch_notes" | "all" => Some(Self::PatchNotes),
            "forum" => Some(Self::Forum),
            _ => None,
        }
    }

    /// Index into the shell's own tab strip (`ModalTabs::Horizontal::active`).
    pub fn index(&self) -> usize {
        match self {
            Self::Posts => 0,
            Self::PatchNotes => 1,
            Self::Forum => 2,
        }
    }
}

/// Forum tab passive-refresh cadence, ms. Both the thread list (page one)
/// and the currently open thread view re-fetch on this cadence while the
/// Forum tab is visible, so another participant's reply or new thread shows
/// up without the caller closing and reopening the overlay. Read by both
/// shells' existing poll (`cloud_session.rs`, `main.rs`), next to the
/// `forum_list == Empty` check that already drives the very first fetch —
/// see [`CloudOverlayState::forum_list_due_for_refresh`]/
/// [`CloudOverlayState::forum_thread_due_for_refresh`].
#[cfg(feature = "cloud")]
pub const FORUM_REFRESH_MS: u64 = 30_000;

#[cfg(feature = "cloud")]
fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A user-triggered Forum action — one-shot mailbox
/// ([`CloudOverlayState::forum_request`]), same idiom as
/// `CloudLoginState::redeem_requested`. The passive "fetch page one when
/// the tab opens" case is NOT here: like News/Download, that is driven
/// directly by the shell polling `forum_list == Empty`, not a click.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug, PartialEq)]
pub enum ForumRequest {
    /// "Load more" on the thread list.
    LoadMoreThreads { before: String },
    /// A thread row was clicked.
    LoadThread { id: i64 },
    /// Compose view's Post button.
    CreateThread { title: String, body: String },
    /// Thread view's reply box submit.
    Reply { thread_id: i64, body: String },
    /// Thread view's Delete (own thread only).
    DeleteThread { id: i64 },
    /// A reply's Delete (own reply only).
    DeleteReply { id: i64, thread_id: i64 },
}

/// State for the News / Download overlays (one open at a time).
///
/// Same mailbox contract as [`CloudLoginState`]: pure UI state + flags,
/// effects in the shell driver.
#[cfg(feature = "cloud")]
#[derive(Clone, Debug)]
pub struct CloudOverlayState {
    pub is_open: bool,
    pub kind: CloudOverlayKind,
    pub data: CloudOverlayData,
    /// One-shot mailbox: URL to open in a new tab (link rows, downloads).
    pub open_url_requested: Option<String>,
    /// One-shot mailbox: the user pressed Install in the Download overlay of
    /// a desktop build. The shell turns it into `UpdaterCommand::InstallNow`;
    /// the web shell never sets it and never reads it.
    pub update_install_requested: bool,
    /// News reading view: index of the post being read in-modal, None = the
    /// post list. Reset whenever the overlay opens or closes.
    pub selected_post: Option<usize>,
    /// Download overlay: whether the expanded list (other platforms + older
    /// versions) is showing. Reset whenever the overlay opens or closes —
    /// reopening it must not inherit the last visit's expansion, same
    /// reason `selected_post` resets.
    pub expanded: bool,
    /// News overlay: which of the list view's two tabs is showing. Reset
    /// whenever the overlay opens or closes, same reason `selected_post` and
    /// `expanded` reset — a panel reopened later must not surface on the tab
    /// someone left it on.
    pub news_tab: NewsTab,
    /// Scroll offset for whichever of the two views draws a real scroll
    /// container (`crate::layout::modals::cloud_overlay`'s `draw_body`): the
    /// Download body, and the News list view. Reset on `open()`/`close()`
    /// and via `set_news_tab`, same reason `selected_post`/`expanded`
    /// reset — a reopened panel, or a panel switched to the OTHER News tab,
    /// must not resume mid-scroll from wherever the last view left it.
    pub scroll: ScrollState,
    pub position: Option<(f64, f64)>,
    pub is_dragging: bool,
    pub drag_offset: Option<(f64, f64)>,
    /// Whether `GET /api/entitlements`'s feature list carried `"forum"` for
    /// THIS caller — the sole visibility switch for `NewsTab::Forum` and
    /// every forum control under it. Written every frame by the shell
    /// driver's entitlements-apply block (`cloud_session.rs`'s wasm side,
    /// `main.rs`'s native side), the same cadence `UserSettingsState::
    /// multi_profile` already gets — never inferred from `is_paid` alone,
    /// since the feature list is what paid currently buys and moves
    /// independently of the tier bit.
    pub forum_entitled: bool,
    /// Forum tab: thread-list fetch state — independent of `forum_view` so
    /// returning from a thread or an abandoned compose never re-fetches it.
    pub forum_list: ForumListState,
    /// Wall-clock ms ([`now_ms`]) `forum_list` last landed a successful page
    /// one — `None` until the first page lands. Compared against
    /// [`FORUM_REFRESH_MS`] by [`Self::forum_list_due_for_refresh`],
    /// independent of `forum_list`'s own Empty/Loading/Loaded shape so a
    /// `Loaded` page does not sit unrefreshed for as long as the tab stays
    /// open.
    pub forum_list_fetched_at: Option<u64>,
    /// A background page-one refetch (due-for-refresh, NOT the first fetch
    /// or an explicit Load-more) is in flight — the shells' own in-flight
    /// gate (`forum_in_flight` on web, `ForumState::busy` on native) already
    /// stops a doubled request, but the due-check itself needs this to know
    /// not to re-fire every poll between "fired" and "landed".
    pub forum_list_refreshing: bool,
    /// Forum tab: which sub-view is showing.
    pub forum_view: ForumSubView,
    /// Same bookkeeping as `forum_list_fetched_at`, for the thread currently
    /// open in `ForumSubView::Thread` — reset to `None` whenever a different
    /// thread is opened (a fresh `LoadThread` always lands before this is
    /// due again) or the overlay closes.
    pub forum_thread_fetched_at: Option<u64>,
    /// Same as `forum_list_refreshing`, for the open thread's background
    /// refresh.
    pub forum_thread_refreshing: bool,
    /// The in-flight forum request targeting the open thread is specifically
    /// `ForumRequest::Reply` — set by the reply-submit click handler
    /// (`panel_click.rs`), consumed by the shells' successful-apply code to
    /// know whether to clear the `FORUM_REPLY` text field. Distinct from
    /// `ForumSubView::Thread::posting_reply`, which native's single-channel
    /// updater answer shares across Reply/DeleteReply/DeleteThread (it
    /// cannot otherwise tell which of the three just landed) — this flag is
    /// never set by the other two, and is reset to `false` wherever it is
    /// consumed, on success or failure alike.
    pub forum_reply_pending: bool,
    /// One-shot mailbox for a Forum action the shell should perform.
    pub forum_request: Option<ForumRequest>,
}

#[cfg(feature = "cloud")]
impl Default for CloudOverlayState {
    fn default() -> Self {
        Self {
            is_open: false,
            kind: CloudOverlayKind::News,
            data: CloudOverlayData::Empty,
            open_url_requested: None,
            update_install_requested: false,
            selected_post: None,
            expanded: false,
            news_tab: NewsTab::Posts,
            scroll: ScrollState::new(),
            position: None,
            is_dragging: false,
            drag_offset: None,
            forum_entitled: false,
            forum_list: ForumListState::Empty,
            forum_list_fetched_at: None,
            forum_list_refreshing: false,
            forum_view: ForumSubView::List,
            forum_thread_fetched_at: None,
            forum_thread_refreshing: false,
            forum_reply_pending: false,
            forum_request: None,
        }
    }
}

#[cfg(feature = "cloud")]
impl CloudOverlayState {
    pub fn open(&mut self, kind: CloudOverlayKind) {
        self.is_open = true;
        self.kind = kind;
        self.open_url_requested = None;
        self.selected_post = None;
        self.expanded = false;
        self.news_tab = NewsTab::default();
        self.scroll.reset();
        self.is_dragging = false;
        self.drag_offset = None;
        // Keep what is already fetched only when it is the finished form
        // THIS kind renders; anything else goes back to `Empty` so the
        // driver fetches again.
        //
        // Stated as "what to keep" rather than as a list of mismatched
        // pairs, because the list was incomplete the moment `Update` was
        // added and would be again: News opened over an `Update` payload
        // matched no pair, so it kept data it cannot render, and `Empty` is
        // the only value that makes the driver refetch — the overlay sat on
        // "Loading…" forever. `Loading` is deliberately not kept either: a
        // fetch whose result was dropped (the user switched kinds while it
        // was in flight) leaves exactly that value behind, and re-asking is
        // the only way out of it.
        let keep = matches!(
            (kind, &self.data),
            (CloudOverlayKind::News, CloudOverlayData::News { .. })
                | (CloudOverlayKind::Download, CloudOverlayData::Ready { .. })
                | (CloudOverlayKind::Download, CloudOverlayData::Update { .. })
        );
        if !keep {
            self.data = CloudOverlayData::Empty;
        }
        // Forum: always drop back to the list sub-view on reopen — a stale
        // open thread or an abandoned compose form must not resurface.
        // `forum_list` itself is kept when already `Loaded` (same "what to
        // keep" reasoning as `data` above); `Loading`/`Error` reset to
        // `Empty` so the driver fetches again rather than sitting on a
        // fetch that was interrupted by the overlay closing mid-flight.
        self.forum_view = ForumSubView::List;
        self.forum_thread_fetched_at = None;
        self.forum_thread_refreshing = false;
        self.forum_reply_pending = false;
        if !matches!(self.forum_list, ForumListState::Loaded { .. }) {
            self.forum_list = ForumListState::Empty;
            self.forum_list_fetched_at = None;
        }
        self.forum_list_refreshing = false;
        self.forum_request = None;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.open_url_requested = None;
        self.selected_post = None;
        self.expanded = false;
        self.news_tab = NewsTab::default();
        self.scroll.reset();
        self.is_dragging = false;
        self.drag_offset = None;
        // `forum_list`/`forum_view` deliberately survive a close — `open()`
        // above decides what to keep on the way back in, same split `data`
        // already has between the two methods. The in-flight background
        // bookkeeping below does NOT survive: a refresh whose result lands
        // after the overlay is closed must not leave a flag stuck `true`
        // that then blocks every future due-check.
        self.forum_list_refreshing = false;
        self.forum_thread_refreshing = false;
        self.forum_reply_pending = false;
        self.forum_request = None;
    }

    pub fn toggle(&mut self, kind: CloudOverlayKind) {
        if self.is_open && self.kind == kind {
            self.close();
        } else {
            self.open(kind);
        }
    }

    /// Switch the News list view's tab, resetting scroll the same reason
    /// `open()`/`close()` do — the tab switched TO must not inherit the
    /// scroll position of whichever tab was showing before it. The click
    /// handler (`mlc-app`'s `panel_click.rs`, `"tab:" =>` arm) should call
    /// this instead of writing `news_tab` directly for that reset to apply.
    pub fn set_news_tab(&mut self, tab: NewsTab) {
        self.news_tab = tab;
        self.scroll.reset();
    }

    /// Marks the cached thread list stale: resets it to `Empty` (and clears
    /// its fetch timestamp) so the next poll's "fetch page one when nothing
    /// is loaded" idiom fires again — WITHOUT touching `forum_view`, so a
    /// caller mid-thread or mid-compose is not yanked back to the list just
    /// because the list itself went stale. Call after any Forum action that
    /// can change what the list shows (`CreateThread`/`DeleteThread`
    /// succeeding, `Reply`/`DeleteReply` succeeding — a reply moves its
    /// thread's `reply_count`/`last_reply_at`), plus `back` and switching TO
    /// the Forum tab.
    pub fn invalidate_forum_list(&mut self) {
        self.forum_list = ForumListState::Empty;
        self.forum_list_fetched_at = None;
        self.forum_list_refreshing = false;
    }

    /// True once [`FORUM_REFRESH_MS`] has passed since the list last landed
    /// a successful page one, and no background refresh is already in
    /// flight. Read by both shells' passive-fetch poll alongside their
    /// existing `forum_list == Empty` check to decide "reissue page one
    /// quietly" for an already-`Loaded` page.
    pub fn forum_list_due_for_refresh(&self) -> bool {
        !self.forum_list_refreshing
            && matches!(self.forum_list_fetched_at, Some(t) if now_ms().saturating_sub(t) >= FORUM_REFRESH_MS)
    }

    /// Stamps `forum_list` as freshly fetched — call once a page-one fetch
    /// (passive, due-refresh, or a mutation's own follow-up read) lands.
    pub fn mark_forum_list_fetched(&mut self) {
        self.forum_list_fetched_at = Some(now_ms());
        self.forum_list_refreshing = false;
    }

    /// Same cadence as [`Self::forum_list_due_for_refresh`], for the thread
    /// currently open in `ForumSubView::Thread`.
    pub fn forum_thread_due_for_refresh(&self) -> bool {
        !self.forum_thread_refreshing
            && matches!(self.forum_thread_fetched_at, Some(t) if now_ms().saturating_sub(t) >= FORUM_REFRESH_MS)
    }

    /// Stamps the open thread as freshly fetched — call once a `LoadThread`
    /// (initial open, due-refresh, or a mutation's own follow-up read)
    /// lands.
    pub fn mark_forum_thread_fetched(&mut self) {
        self.forum_thread_fetched_at = Some(now_ms());
        self.forum_thread_refreshing = false;
    }

    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    pub fn end_drag(&mut self) {
        self.is_dragging = false;
    }
}
