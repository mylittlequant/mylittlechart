//! Pure-data types cut from `mlc-layout/src/ui/modal_settings.rs`.
//!
//! These types are consumed by the backend (state/, engine/, indicator_source,
//! tag_manager) and must therefore live in mlc-core, not in mlc-layout.
//! mlc-layout re-exports them via shims in ui/modal_settings.rs.

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
