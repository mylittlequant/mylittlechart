//! Settings modal state types for chart-related settings modals
//!
//! These types were moved from `zengeld-terminal-core` to `zengeld-chart` so
//! that chart-level crates can use them without depending on core.
//!
//! Core re-exports these via `pub use zengeld_chart::ui::modal_settings::*`.

use crate::ui::color_picker_state::ColorPickerState;
use crate::ui::scroll_state::ScrollState;
use crate::drawing::primitives_v2::config::TimeframeVisibilityConfig;
use uzor::app_context::TextInputState;
use alert_delivery::NotificationSettings;

// =============================================================================
// Text Editing State
// =============================================================================

/// State for inline text editing in settings modals
#[derive(Clone, Debug)]
pub struct TextEditingState {
    /// Field being edited (e.g., "text_content")
    pub field_id: String,
    /// Current text value
    pub text: String,
    /// Cursor position (character index)
    pub cursor: usize,
    /// Selection start (if Some, text is selected)
    pub selection_start: Option<usize>,
    /// Timestamp for cursor blink (milliseconds)
    pub blink_time: u64,
}

impl TextEditingState {
    /// Reset all editing state (text, cursor, selection).
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_start = None;
    }

    /// Convert a character index to a byte offset in `self.text`.
    ///
    /// This is necessary because `String::insert`, `String::remove`, and
    /// `String::drain` all expect **byte offsets**, while the `cursor` field
    /// stores **character indices** (number of Unicode scalar values).  For
    /// pure-ASCII text the two coincide, but any multi-byte character (e.g.
    /// Cyrillic, CJK, emoji) causes them to diverge and passing a char index
    /// as a byte offset triggers a `char_boundary` assertion panic.
    ///
    /// Mirrors the identical helper in `uzor::TextInputState`.
    pub fn char_to_byte_pos(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(byte_pos, _)| byte_pos)
            .unwrap_or(self.text.len())
    }

    /// Check if cursor should be visible based on blink timing
    pub fn is_cursor_visible(&self, current_time_ms: u64) -> bool {
        // Blink every 500ms
        let elapsed = current_time_ms.wrapping_sub(self.blink_time);
        (elapsed / 500).is_multiple_of(2)
    }

    /// Reset blink timer (call when cursor moves or text changes)
    pub fn reset_blink(&mut self, current_time_ms: u64) {
        self.blink_time = current_time_ms;
    }

    /// Return the selection range as an ordered `(min, max)` pair.
    ///
    /// When `selection_start` is `Some(anchor)`, the selection spans
    /// `anchor..cursor` (or `cursor..anchor` when the cursor is to the left).
    /// Returns `None` when there is no active selection or anchor == cursor.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        if let Some(anchor) = self.selection_start {
            let lo = anchor.min(self.cursor);
            let hi = anchor.max(self.cursor);
            if lo < hi {
                return Some((lo, hi));
            }
        }
        None
    }

    /// Whether there is an active, non-empty selection.
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// Delete the currently selected text and place the cursor at the
    /// beginning of the deleted range.  Does nothing if there is no selection.
    pub fn delete_selection(&mut self) {
        if let Some((lo, hi)) = self.selection_range() {
            let lo_byte = self.char_to_byte_pos(lo);
            let hi_byte = self.char_to_byte_pos(hi);
            self.text.drain(lo_byte..hi_byte);
            self.cursor = lo;
            self.selection_start = None;
        }
    }

    /// Select all text: anchor at 0, cursor at end.
    pub fn select_all(&mut self) {
        let len = self.text.chars().count();
        self.selection_start = Some(0);
        self.cursor = len;
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
    /// Text editing state (field_id, text, cursor, selection, blink).
    pub editing: TextEditingState,
    /// The preset ID being renamed (only used in Rename mode).
    pub rename_preset_id: Option<String>,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left (for smooth dragging).
    pub drag_offset: Option<(f64, f64)>,
    /// Whether the user is drag-selecting text in the input field.
    pub text_select_dragging: bool,
}

impl Default for PresetNameInputState {
    fn default() -> Self {
        Self {
            is_open: false,
            mode: PresetNameInputMode::SaveAs,
            editing: TextEditingState {
                field_id: "preset_name".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            rename_preset_id: None,
            position: None,
            is_dragging: false,
            drag_offset: None,
            text_select_dragging: false,
        }
    }
}

impl PresetNameInputState {
    /// Open the modal for "Save As" with a default preset name.
    pub fn open_save_as(&mut self, default_name: &str, current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::SaveAs;
        self.rename_preset_id = None;
        self.editing = TextEditingState {
            field_id: "preset_name".to_string(),
            text: default_name.to_string(),
            cursor: default_name.chars().count(),
            selection_start: None,
            blink_time: current_time_ms,
        };
    }

    /// Open the modal for "Rename" with the current preset name pre-filled.
    pub fn open_rename(&mut self, preset_id: &str, current_name: &str, current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::Rename;
        self.rename_preset_id = Some(preset_id.to_string());
        self.editing = TextEditingState {
            field_id: "preset_name".to_string(),
            text: current_name.to_string(),
            cursor: current_name.chars().count(),
            selection_start: None,
            blink_time: current_time_ms,
        };
    }

    /// Open the modal for "New Chart" with a default "Untitled N" name pre-filled.
    pub fn open_new_chart(&mut self, default_name: &str, current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::NewChart;
        self.rename_preset_id = None;
        self.editing = TextEditingState {
            field_id: "preset_name".to_string(),
            text: default_name.to_string(),
            cursor: default_name.chars().count(),
            selection_start: None,
            blink_time: current_time_ms,
        };
    }

    /// Open the modal for "Create Indicator Set" with a default name.
    pub fn open_create_indicator_set(&mut self, default_name: &str, current_time_ms: u64) {
        self.is_open = true;
        self.mode = PresetNameInputMode::CreateIndicatorSet;
        self.rename_preset_id = None;
        self.editing = TextEditingState {
            field_id: "preset_name".to_string(),
            text: default_name.to_string(),
            cursor: default_name.chars().count(),
            selection_start: None,
            blink_time: current_time_ms,
        };
    }

    /// Close the modal and reset state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.editing.text.clear();
        self.editing.cursor = 0;
        self.editing.selection_start = None;
        self.rename_preset_id = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.text_select_dragging = false;
        // Don't reset position — preserve last drag position for reopening
    }

    /// Get the entered name text.
    pub fn name(&self) -> &str {
        &self.editing.text
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

/// Tab in primitive settings modal
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PrimitiveSettingsTab {
    #[default]
    Style,
    Text,
    Coordinates,
    Levels,
    Visibility,
}

impl PrimitiveSettingsTab {
    /// Get tab ID string
    pub fn id(&self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Text => "text",
            Self::Coordinates => "coordinates",
            Self::Levels => "levels",
            Self::Visibility => "visibility",
        }
    }

    /// Get tab display label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Style => "Стиль",
            Self::Text => "Текст",
            Self::Coordinates => "Координаты",
            Self::Levels => "Уровни",
            Self::Visibility => "Видимость",
        }
    }

    /// All available tabs
    pub fn all() -> &'static [PrimitiveSettingsTab] {
        &[
            Self::Style,
            Self::Text,
            Self::Coordinates,
            Self::Levels,
            Self::Visibility,
        ]
    }
}

// =============================================================================
// Primitive Settings State
// =============================================================================

/// State for primitive settings modal
#[derive(Clone, Debug, Default)]
pub struct PrimitiveSettingsState {
    /// Is modal open?
    pub is_open: bool,
    /// Index of primitive being edited
    pub primitive_idx: Option<usize>,
    /// Current active tab
    pub active_tab: PrimitiveSettingsTab,
    /// Modal position (for dragging)
    pub position: Option<(f64, f64)>,
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
    /// Text input editing state (field_id, text, cursor_pos)
    /// Set when a text field is being edited (e.g., "text_content")
    pub editing_text: Option<TextEditingState>,
    /// Slider drag state for number sliders
    pub slider_drag: Option<SliderDragState>,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    /// Whether the line-style dropdown is open.
    pub open_line_style_dropdown: bool,
    /// Currently open Select-property dropdown: (prop_kind, prop_id).
    /// prop_kind is "style", "text", or "level".
    pub open_select_dropdown: Option<(String, String)>,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Text editing state for the template name input.
    pub template_name_editing: Option<TextEditingState>,
}

impl PrimitiveSettingsState {
    /// Create new state (closed)
    pub fn new() -> Self {
        Self::default()
    }

    /// Open settings for a primitive
    pub fn open(&mut self, primitive_idx: usize) {
        self.is_open = true;
        self.primitive_idx = Some(primitive_idx);
        self.active_tab = PrimitiveSettingsTab::Style;
        // Don't reset position - keep last position for convenience
    }

    /// Close the modal
    pub fn close(&mut self) {
        self.is_open = false;
        self.primitive_idx = None;
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        // Clear editing state so zombie editing_text cannot intercept keypresses
        // in other modals after this one is closed.
        self.editing_text = None;
        self.slider_drag = None;
        self.text_select_dragging = false;
        self.open_line_style_dropdown = false;
        self.open_select_dropdown = None;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.template_name_editing = None;
    }

    /// Check if open
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Set active tab
    pub fn set_tab(&mut self, tab: PrimitiveSettingsTab) {
        self.active_tab = tab;
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

    /// Get available tabs for a primitive (based on capabilities)
    pub fn available_tabs(&self, supports_text: bool, has_levels: bool) -> Vec<PrimitiveSettingsTab> {
        let mut tabs = vec![PrimitiveSettingsTab::Style];
        if supports_text {
            tabs.push(PrimitiveSettingsTab::Text);
        }
        tabs.push(PrimitiveSettingsTab::Coordinates);
        if has_levels {
            tabs.push(PrimitiveSettingsTab::Levels);
        }
        tabs.push(PrimitiveSettingsTab::Visibility);
        tabs
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

    /// Open color picker for a field at specified position (legacy, no smart positioning)
    #[deprecated(note = "Use open_color_picker_smart instead for proper window bounds handling")]
    pub fn open_color_picker(&mut self, field: &str, x: f64, y: f64, current_color: Option<&str>) {
        self.color_picker_field = Some(field.to_string());
        self.color_picker.open_l1(x, y, current_color);
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

    /// Get tab display label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Instrument => "Инструмент",
            Self::StatusLine => "Строка статуса",
            Self::ScalesLines => "Шкалы и линии",
            Self::Appearance => "Оформление",
        }
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
    /// Inline text editing state (for watermark text etc.)
    pub editing_text: Option<TextEditingState>,
    /// Instrument checkbox: use previous close color
    pub instrument_use_prev_close: bool,
    /// Instrument checkbox: body enabled
    pub instrument_body_enabled: bool,
    /// Instrument checkbox: border enabled
    pub instrument_border_enabled: bool,
    /// Instrument checkbox: wick enabled
    pub instrument_wick_enabled: bool,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Text editing state for the template name input.
    pub template_name_editing: Option<TextEditingState>,
    /// Hovered footer button
    pub hovered_footer_button: Option<String>,
}

impl Default for ChartSettingsState {
    fn default() -> Self {
        Self {
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
            editing_text: None,
            instrument_use_prev_close: false,
            instrument_body_enabled: true,
            instrument_border_enabled: true,
            instrument_wick_enabled: true,
            text_select_dragging: false,
            template_dropdown_open: false,
            applied_template_id: None,
            save_template_mode: false,
            template_name_editing: None,
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
        // Clear editing state to prevent zombie editing_text from intercepting
        // keypresses in other modals after chart settings is closed.
        self.editing_text = None;
        self.slider_drag = None;
        self.text_select_dragging = false;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.template_name_editing = None;
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

    /// Get tab display label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inputs => "Параметры",
            Self::Style => "Стиль",
            Self::Visibility => "Видимость",
            Self::Signals => "Сигналы",
            Self::Info => "Инфо",
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
    /// Currently editing input field (param name) - LEGACY, use editing_text_state
    pub editing_field: Option<String>,
    /// Text being edited - LEGACY, use editing_text_state
    pub editing_text: String,
    /// Cursor position in editing text - LEGACY, use editing_text_state
    pub editing_cursor: usize,
    /// Hovered footer button
    pub hovered_footer_button: Option<String>,
    /// Text editing state (matches PrimitiveSettingsState pattern)
    pub editing_text_state: Option<TextEditingState>,
    /// Slider drag state for dual-handle sliders
    pub slider_drag: Option<SliderDragState>,
    /// Currently open parameter dropdown menu (param_name)
    pub open_param_dropdown: Option<String>,
    /// Whether the user is drag-selecting text in the active text input.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Text editing state for the template name input.
    pub template_name_editing: Option<TextEditingState>,
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
        self.editing_text_state = None;
        self.slider_drag = None;
        self.open_param_dropdown = None;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.template_name_editing = None;
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
        self.editing_text_state = None;
        self.slider_drag = None;
        self.open_param_dropdown = None;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.template_name_editing = None;
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

// =============================================================================
// Sub-Pane Overlay State
// =============================================================================

/// Which button on the sub-pane overlay bar is being referenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubPaneButton {
    /// Delete the indicator (and its sub-pane).
    Delete,
    /// Hide / unhide the sub-pane (indicator still exists).
    Hide,
    /// Move this sub-pane up in the ordering.
    MoveUp,
    /// Expand this sub-pane to fill the entire chart area.
    Expand,
    /// Shown instead of Expand when the pane is already maximized — restores it.
    Restore,
    /// Move this sub-pane down in the ordering.
    MoveDown,
    // ── Left-side per-indicator buttons ──────────────────────────────────────
    /// Toggle indicator visibility (left overlay).
    IndicatorEye,
    /// Open alert settings for this indicator (left overlay).
    IndicatorAlert,
    /// Open indicator settings modal (left overlay).
    IndicatorSettings,
    /// Delete this indicator (left overlay).
    IndicatorDelete,
}

/// Per-sub-pane overlay button UI state (hover triggered visibility).
///
/// One instance lives in `ChartApp` keyed by `instance_id`.
#[derive(Clone, Debug, Default)]
pub struct SubPaneOverlayState {
    /// Whether the button bar is visible (mouse inside sub-pane content area).
    pub visible: bool,
    /// Which right-side button (if any) is currently hovered.
    pub hovered_button: Option<SubPaneButton>,
    /// Which left-side per-indicator button (if any) is currently hovered.
    pub hovered_left_button: Option<SubPaneButton>,
}

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

    /// Get tab display label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Style => "Стиль",
            Self::Visibility => "Видимость",
            Self::Info => "Инфо",
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
    /// Cached visibility flag.
    pub cached_visible: bool,
    /// Cached bar count.
    pub cached_bar_count: usize,
    /// Cached base price.
    pub cached_base_price: f64,
    /// Cached timeframe visibility config (loaded from series at open time).
    /// `None` means visible on all timeframes.
    pub cached_timeframe_visibility: Option<TimeframeVisibilityConfig>,
    // ---- Original values saved at open time (for Cancel revert) ----
    /// Original line color before any edits.
    pub original_color: String,
    /// Original line width before any edits.
    pub original_line_width: f32,
    /// Original line style before any edits.
    pub original_line_style: String,
    /// Original timeframe visibility (for Cancel revert).
    pub original_timeframe_visibility: Option<TimeframeVisibilityConfig>,
    // ---- Inline text editing for tf min/max number fields ----
    /// Active text-field editing state (e.g., "tf_1_min").
    pub editing_text: Option<TextEditingState>,
    /// Whether the user is drag-selecting text in the active input field.
    pub text_select_dragging: bool,
    // ---- Template UI state ----
    /// Whether the template dropdown is open.
    pub template_dropdown_open: bool,
    /// ID of the currently applied template (None = no template applied).
    pub applied_template_id: Option<String>,
    /// When true, show the template name input for "Save as Template".
    pub save_template_mode: bool,
    /// Text editing state for the template name input.
    pub template_name_editing: Option<TextEditingState>,
}

impl Default for CompareSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            series_index: 0,
            active_tab: CompareSettingsTab::Style,
            position: None,
            is_dragging: false,
            drag_offset: None,
            color_picker: ColorPickerState::new(),
            color_picker_field: None,
            line_style_dropdown_open: false,
            hovered_item_id: None,
            slider_drag: None,
            cached_symbol: String::new(),
            cached_name: String::new(),
            cached_color: "#2196F3".to_string(),
            cached_line_width: 2.0,
            cached_line_style: "solid".to_string(),
            cached_visible: true,
            cached_bar_count: 0,
            cached_base_price: 0.0,
            cached_timeframe_visibility: None,
            original_color: "#2196F3".to_string(),
            original_line_width: 2.0,
            original_line_style: "solid".to_string(),
            original_timeframe_visibility: None,
            editing_text: None,
            text_select_dragging: false,
            template_dropdown_open: false,
            applied_template_id: None,
            save_template_mode: false,
            template_name_editing: None,
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
    pub fn open(
        &mut self,
        series_index: usize,
        symbol: &str,
        name: &str,
        color: &str,
        line_width: f32,
        line_style: &str,
        visible: bool,
        bar_count: usize,
        base_price: f64,
        timeframe_visibility: Option<TimeframeVisibilityConfig>,
    ) {
        self.is_open = true;
        self.series_index = series_index;
        self.active_tab = CompareSettingsTab::Style;
        self.line_style_dropdown_open = false;
        self.hovered_item_id = None;
        self.slider_drag = None;
        self.editing_text = None;
        self.text_select_dragging = false;
        self.cached_symbol = symbol.to_string();
        self.cached_name = name.to_string();
        self.cached_color = color.to_string();
        self.cached_line_width = line_width;
        self.cached_line_style = line_style.to_string();
        self.cached_visible = visible;
        self.cached_bar_count = bar_count;
        self.cached_base_price = base_price;
        self.cached_timeframe_visibility = timeframe_visibility.clone();
        // Save originals for Cancel revert.
        self.original_color = color.to_string();
        self.original_line_width = line_width;
        self.original_line_style = line_style.to_string();
        self.original_timeframe_visibility = timeframe_visibility;
        // Don't reset position — keep last for convenience.
    }

    /// Refresh the cached display data from an updated series.
    /// Call each frame while the modal is open to keep displays current.
    pub fn refresh_cache(
        &mut self,
        color: &str,
        line_width: f32,
        line_style: &str,
        visible: bool,
        bar_count: usize,
        base_price: f64,
    ) {
        self.cached_color = color.to_string();
        self.cached_line_width = line_width;
        self.cached_line_style = line_style.to_string();
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
        self.editing_text = None;
        self.text_select_dragging = false;
        self.template_dropdown_open = false;
        self.save_template_mode = false;
        self.template_name_editing = None;
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

pub use alerts::{AlertStatus, AlertCondition, AlertItem, AlertSource, AlertTriggerMode, AlertTransport, SignalBarState, SignalDirection};

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
    /// Active text editing state (for name field, price fields).
    pub editing_text: Option<TextEditingState>,
    /// Which condition dropdown is open (if any).
    pub condition_dropdown_open: bool,
    /// Notification settings (mirrors profile, edited in modal).
    pub notification_settings: NotificationSettings,
    /// Telegram bot token input text (editable).
    pub tg_bot_token_input: String,
    /// Whether Telegram token field is focused.
    pub tg_token_focused: bool,
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
            editing_text: None,
            condition_dropdown_open: false,
            notification_settings: NotificationSettings::default(),
            tg_bot_token_input: String::new(),
            tg_token_focused: false,
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
        self.editing_text = None;
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
        self.editing_text = None;
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
        self.editing_text = None;
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
// Chart Settings Data (settings modal data types)
// =============================================================================

/// Settings for chart instrument (candle colors, etc.)
#[derive(Clone, Debug, Default)]
pub struct InstrumentSettings {
    pub use_prev_close_color: bool,
    pub body_enabled: bool,
    pub body_up_color: String,
    pub body_down_color: String,
    pub border_enabled: bool,
    pub border_up_color: String,
    pub border_down_color: String,
    pub wick_enabled: bool,
    pub wick_up_color: String,
    pub wick_down_color: String,
    pub precision_label: String,
    pub timezone_label: String,
    pub use_24h: bool,
    pub date_format_label: String,
    pub show_day_of_week: bool,
}

/// Settings for scales and lines
#[derive(Clone, Debug, Default)]
pub struct ScalesLinesSettings {
    pub show_grid: bool,
    pub vert_lines: bool,
    pub horz_lines: bool,
    pub price_scale_right: bool,
    pub auto_scale: bool,
    pub time_scale_bottom: bool,
    pub crosshair_mode: String,
    pub crosshair_line_style: String,
    pub crosshair_line_width: f64,
    pub crosshair_line_color: String,
    pub price_scale_position: String,
    pub time_scale_position: String,
    pub corner_visibility: String,
    pub price_scale_width: f64,
    pub time_scale_height: f64,
    pub date_format: String,
    pub use_24h: bool,
    pub show_day_of_week: bool,
    pub show_bar_countdown: bool,
    pub show_prev_close: bool,
    pub timezone_label: String,
}

/// Settings for status line elements
#[derive(Clone, Debug, Default)]
pub struct StatusLineSettings {
    pub legend_position: String,
    pub legend_show_ohlc: bool,
    pub legend_show_change: bool,
    pub legend_show_percent: bool,
    pub tooltip_visible: bool,
    pub tooltip_follow_cursor: bool,
    pub watermark_visible: bool,
    pub watermark_position: String,
    pub watermark_color: String,
    pub watermark_text: String,
    pub show_indicator_overlay: bool,
}

/// All settings data passed to the chart settings modal renderer
#[derive(Clone, Debug, Default)]
pub struct ChartSettingsData {
    pub instrument: InstrumentSettings,
    pub status_line: StatusLineSettings,
    pub scales: ScalesLinesSettings,
    pub alert_items: Vec<AlertItem>,
}

// =============================================================================
// Indicator display info (for indicator settings modal)
// =============================================================================

/// Parameter type for indicator settings rendering
#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorParamType {
    Int,
    Float,
    Bool,
    Source,
    Select { options: Vec<String> },
    Color,
}

/// A single parameter definition for display in the indicator settings modal
#[derive(Clone, Debug)]
pub struct IndicatorParamDef {
    pub name: String,
    pub param_type: IndicatorParamType,
}

impl IndicatorParamDef {
    /// Get options as strings (for Select type)
    pub fn get_options_as_strings(&self) -> Vec<String> {
        match &self.param_type {
            IndicatorParamType::Select { options } => options.clone(),
            _ => Vec::new(),
        }
    }
}

/// Output type for display
#[derive(Clone, Debug)]
pub enum IndicatorOutputType {
    Line,
    Histogram,
    Shapes,
    Arrows,
    Other(String),
}

impl IndicatorOutputType {
    pub fn as_str(&self) -> &str {
        match self {
            IndicatorOutputType::Line => "Line",
            IndicatorOutputType::Histogram => "Histogram",
            IndicatorOutputType::Shapes => "Shapes",
            IndicatorOutputType::Arrows => "Arrows",
            IndicatorOutputType::Other(s) => s.as_str(),
        }
    }
}

/// An output definition for the indicator
#[derive(Clone, Debug)]
pub struct IndicatorOutputDef {
    pub display_name: String,
    pub output_type: IndicatorOutputType,
}

/// Display-only indicator info used in `render_indicator_settings_modal`.
///
/// Core converts `IndicatorDefinition` → `IndicatorDisplayInfo` before calling
/// chart's renderer, avoiding a circular dependency (indicators → chart).
#[derive(Clone, Debug, Default)]
pub struct IndicatorDisplayInfo {
    pub name: String,
    pub short_name: String,
    pub description: String,
    pub overlay: bool,
    pub bounds: Option<(f64, f64)>,
    pub category_name: String,
    pub params: Vec<IndicatorParamDef>,
    pub outputs: Vec<IndicatorOutputDef>,
}

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
    /// The text typed into the search box (mirrors `search_editing.text`).
    pub search_query: String,
    /// Text editing state for the search input.
    pub search_editing: TextEditingState,
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
    /// Whether the user is drag-selecting text in the search input field.
    pub search_text_select_dragging: bool,
    /// When true, selecting a preset opens it in a new tab instead of the active tab.
    pub open_in_new_tab: bool,
}

impl Default for ChartBrowserState {
    fn default() -> Self {
        Self {
            is_open: false,
            search_query: String::new(),
            search_editing: TextEditingState {
                field_id: "chart_browser_search".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            scroll: ScrollState::default(),
            hovered_preset_id: None,
            position: None,
            is_dragging: false,
            drag_offset: None,
            search_text_select_dragging: false,
            open_in_new_tab: false,
        }
    }
}

impl ChartBrowserState {
    /// Open the modal, resetting search and scroll state.
    pub fn open(&mut self, current_time_ms: u64) {
        self.is_open = true;
        self.search_query.clear();
        self.search_editing.text.clear();
        self.search_editing.cursor = 0;
        self.search_editing.selection_start = None;
        self.search_editing.reset_blink(current_time_ms);
        self.scroll.reset();
        self.hovered_preset_id = None;
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.search_text_select_dragging = false;
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
    pub selected_group_id: Option<crate::tag_manager::SyncGroupId>,
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
    pub search_editing: TextEditingState,
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
    /// Whether the user is drag-selecting text in the search input field.
    pub search_text_select_dragging: bool,
}

impl WatchlistModalState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            scroll: ScrollState::default(),
            search_query: String::new(),
            search_editing: TextEditingState {
                field_id: "watchlist_modal_search".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            active_tab: WatchlistModalTab::Overview,
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            drag_reorder: None,
            drag_reorder_pending: None,
            drop_index: None,
            hovered_widget: None,
            search_text_select_dragging: false,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.scroll.reset();
        self.search_query.clear();
        self.search_editing.text.clear();
        self.search_editing.cursor = 0;
        self.search_editing.selection_start = None;
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
        self.search_text_select_dragging = false;
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
    /// Text editing state.
    pub editing: TextEditingState,
    /// Modal position (None = centered on screen).
    pub position: Option<(f64, f64)>,
    /// Whether the modal is being dragged by its header.
    pub is_dragging: bool,
    /// Offset from mouse to modal top-left (for smooth dragging).
    pub drag_offset: Option<(f64, f64)>,
    /// Whether the user is drag-selecting text in the input field.
    pub text_select_dragging: bool,
}

impl WatchlistGroupNameInputState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: WatchlistGroupNameMode::CreateNew,
            editing: TextEditingState {
                field_id: "wl_group_name".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            position: None,
            is_dragging: false,
            drag_offset: None,
            text_select_dragging: false,
        }
    }

    /// Open the modal for creating a new watchlist group.
    pub fn open_create_new(&mut self) {
        self.open_create_new_with_name("New Watchlist");
    }

    /// Open the modal for creating a new watchlist group with a specific default name.
    pub fn open_create_new_with_name(&mut self, default_name: &str) {
        self.is_open = true;
        self.mode = WatchlistGroupNameMode::CreateNew;
        let default = default_name.to_string();
        let len = default.chars().count();
        self.editing = TextEditingState {
            field_id: "wl_group_name".to_string(),
            text: default,
            cursor: len,
            selection_start: Some(0),
            blink_time: 0,
        };
        self.position = None;
    }

    /// Open the modal for renaming an existing watchlist group.
    pub fn open_rename(&mut self, list_id: u64, current_name: &str) {
        self.is_open = true;
        self.mode = WatchlistGroupNameMode::Rename(list_id);
        let len = current_name.chars().count();
        self.editing = TextEditingState {
            field_id: "wl_group_name".to_string(),
            text: current_name.to_string(),
            cursor: len,
            selection_start: Some(0),
            blink_time: 0,
        };
        self.position = None;
    }

    /// Close the modal and reset state.
    pub fn close(&mut self) {
        self.is_open = false;
        self.editing.text.clear();
        self.editing.cursor = 0;
        self.editing.selection_start = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.text_select_dragging = false;
    }

    /// Get the entered name text.
    pub fn name(&self) -> &str {
        &self.editing.text
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
    Performance,
    Server,
}

impl UserSettingsTab {
    pub fn id(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Sync => "sync",
            Self::Performance => "performance",
            Self::Server => "server",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Sync => "Sync",
            Self::Performance => "Performance",
            Self::Server => "Server",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::General, Self::Sync, Self::Performance, Self::Server]
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "general" => Some(Self::General),
            "sync" => Some(Self::Sync),
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
/// full-screen modal that handles all profile-related flows.
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


/// Display info for a local agent CLI connector key shown in the key manager list.
///
/// Only metadata is stored here — the raw key is never shown after creation.
#[derive(Debug, Clone)]
pub struct ManagedKeyInfo {
    /// Human-readable label chosen at creation.
    pub label: String,
    /// Tier string: `"read_only"`, `"read_write"`, or `"admin"`.
    pub tier: String,
    /// Optional agent identifier attached to this key.
    pub agent_id: Option<String>,
}

/// Preferred new name for the key display info type.
pub type LocalAgentKeyInfo = ManagedKeyInfo;

/// State for the User Settings modal.
#[derive(Clone, Debug)]
pub struct UserSettingsState {
    /// Whether the modal is currently open.
    pub is_open: bool,
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
    /// Whether the internal Agent API server is enabled.
    pub server_enabled: bool,
    /// Port the server listens on.
    pub server_port: u16,
    /// Current server status: "running", "stopped", "error".
    pub server_status: String,

    // ── Key Manager ──────────────────────────────────────────────────────────
    /// List of local agent CLI connector keys (metadata only — no raw key values).
    /// Refreshed from AgentState each time the Server tab is opened or a
    /// create/delete action completes.
    pub local_agent_keys_ui: Vec<ManagedKeyInfo>,
    /// Label text being typed for the new key creation form.
    pub new_key_label: String,
    /// Tier selected for the new key: `"read_only"` or `"read_write"`.
    pub new_key_tier: String,
    /// Raw key shown once immediately after creation. Cleared when the user
    /// clicks Copy or closes the modal.
    pub last_created_key: Option<String>,
    /// Whether the new-key label input field is currently focused for typing.
    pub new_key_label_focused: bool,
    /// Scroll state for the registered keys list in the Server tab.
    pub server_keys_scroll: ScrollState,
    /// Scroll state for the General tab content.
    pub general_tab_scroll: ScrollState,
    /// Scroll state for the Sync tab content.
    pub sync_tab_scroll: ScrollState,
    /// Scroll state for the Performance tab content.
    pub performance_tab_scroll: ScrollState,

    // ── WELCOME WIZARD ──────────────────────────────────────────────────
    /// True when the first-run welcome wizard should be shown (no profile.json on first launch).
    pub show_welcome_wizard: bool,
    /// True when the profile is encrypted (salt.hex exists) but no vault key has been derived
    /// yet — the user must enter their passphrase to unlock their data before the app is usable.
    pub needs_vault_unlock: bool,
    /// Error message shown on the vault unlock overlay when the passphrase is wrong.
    /// Cleared when the user starts typing a new passphrase.
    pub vault_unlock_error: Option<String>,
    /// Number of consecutive failed vault unlock attempts.
    /// After 3 failures the "Forgot passphrase? Create new profile" button is shown.
    pub vault_unlock_attempts: u32,
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
    /// Text editing state for the inline rename input.
    pub profile_rename_editing: TextEditingState,
    /// Whether the profile rename input field is focused for keyboard input.
    pub profile_rename_focused: bool,
    /// ID of the profile row currently being renamed (None = not renaming).
    pub profile_rename_target_id: Option<String>,
    /// Whether the avatar picker popover is open.
    pub show_avatar_picker: bool,
    /// ID of the profile whose avatar picker is open (None = active profile).
    pub profile_avatar_target_id: Option<String>,
    /// Whether the "New Profile" inline dialog is open.
    pub show_new_profile_dialog: bool,
    /// Text editing state for the new profile name input.
    pub new_profile_name_editing: TextEditingState,
    /// Whether the new profile name input field is focused for keyboard input.
    pub new_profile_name_focused: bool,

    // ── Set New Passphrase (post-recovery re-key) ─────────────────────────────
    /// In-memory buffer for the new passphrase during the SetNewPassphrase flow.
    /// Never persisted to disk.
    pub new_passphrase_editing: TextEditingState,
    /// Whether the new passphrase input field has keyboard focus.
    pub new_passphrase_focused: bool,
    /// In-memory buffer for the confirm passphrase during the SetNewPassphrase flow.
    /// Never persisted to disk.
    pub confirm_passphrase_editing: TextEditingState,
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
    /// Read-only editing state for the recovery key display box (ShowRecoveryKey page).
    /// Populated from `recovery_key_display` each frame so the user can select/copy
    /// the key text by clicking and dragging inside the display box.
    pub recovery_key_display_editing: TextEditingState,
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
    /// Cached value synced from `UserProfile.data_load.max_store_size_mb`.
    pub data_store_size_mb: u32,
    /// Cached value synced from `UserProfile.data_load.store_cleanup_days`.
    pub data_cleanup_days: u32,
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

}

impl Default for UserSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            position: None,
            is_dragging: false,
            drag_offset: None,
            hovered_item_id: None,
            active_tab: UserSettingsTab::default(),
            active_dropdown: None,
            recalc_mode_label: "Per Frame".to_string(),
            language: "en".to_string(),
            diagnostics_enabled: false,
            server_enabled: true,
            server_port: 17420,
            server_status: "running".to_string(),
            local_agent_keys_ui: Vec::new(),
            new_key_label: String::new(),
            new_key_tier: "read_only".to_string(),
            last_created_key: None,
            new_key_label_focused: false,
            server_keys_scroll: ScrollState::default(),
            general_tab_scroll: ScrollState::default(),
            sync_tab_scroll: ScrollState::default(),
            performance_tab_scroll: ScrollState::default(),
            show_welcome_wizard: false,
            needs_vault_unlock: false,
            vault_unlock_error: None,
            vault_unlock_attempts: 0,
            show_profile_manager: false,
            recovery_key_display: None,
            profile_manager_page: ProfileManagerPage::ProfileList,
            profile_manager_target_id: String::new(),
            profile_manager_target_name: String::new(),
            profiles_with_vault_status: Vec::new(),
            wizard_page: 0,
            wizard_e2e_chosen: false,
            profile_display_name: "Default".to_string(),
            profile_avatar: "chart".to_string(),
            profile_id: String::new(),
            runtime_profile_id: String::new(),
            available_profiles: Vec::new(),
            profile_rename_mode: false,
            profile_rename_editing: TextEditingState {
                field_id: "profile_rename".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            profile_rename_focused: false,
            profile_rename_target_id: None,
            show_avatar_picker: false,
            profile_avatar_target_id: None,
            show_new_profile_dialog: false,
            new_profile_name_editing: TextEditingState {
                field_id: "new_profile_name".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            new_profile_name_focused: false,
            new_passphrase_editing: TextEditingState {
                field_id: "new_passphrase".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            new_passphrase_focused: false,
            confirm_passphrase_editing: TextEditingState {
                field_id: "confirm_passphrase".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            confirm_passphrase_focused: false,
            set_passphrase_error: String::new(),
            profile_list_scroll: ScrollState::new(),
            profile_mgr_text_select_dragging: None,
            recovery_key_display_editing: TextEditingState {
                field_id: "recovery_key_display".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0,
            },
            recovery_key_display_focused: false,
            recovery_key_copied_at: 0,
            data_bg_bars: 2000,
            data_max_bars: 10000,
            data_store_size_mb: 500,
            data_cleanup_days: 30,
            data_slider_drag: None,
            wizard_selected_theme: String::new(),
            wizard_selected_backend: String::new(),
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
