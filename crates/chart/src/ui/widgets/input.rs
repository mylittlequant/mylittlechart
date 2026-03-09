//! Text input widget rendering
//!
//! Platform-agnostic text input rendering using RenderContext.

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use uzor::types::Rect as WidgetRect;

/// Text input configuration
#[derive(Clone, Debug)]
pub struct InputConfig {
    /// Current text value
    pub value: String,
    /// Placeholder text when empty
    pub placeholder: String,
    /// Whether input is disabled
    pub disabled: bool,
    /// Whether input is focused
    pub focused: bool,
    /// Cursor position (character index)
    pub cursor: usize,
    /// Selection start (if selecting)
    pub selection_start: Option<usize>,
    /// Selection end (if selecting)
    pub selection_end: Option<usize>,
    /// Font size
    pub font_size: f64,
    /// Padding
    pub padding: f64,
    /// Corner radius
    pub radius: f64,
    /// Input type (for visual style)
    pub input_type: InputType,
}

/// Input types for different styling
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Text,
    Number,
    Search,
    Password,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            disabled: false,
            focused: false,
            cursor: 0,
            selection_start: None,
            selection_end: None,
            font_size: 13.0,
            padding: 8.0,
            radius: 4.0,
            input_type: InputType::Text,
        }
    }
}

impl InputConfig {
    pub fn new(value: &str) -> Self {
        Self {
            value: value.to_string(),
            cursor: value.chars().count(),
            ..Default::default()
        }
    }

    pub fn with_placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = placeholder.to_string();
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_type(mut self, input_type: InputType) -> Self {
        self.input_type = input_type;
        self
    }

    pub fn with_cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    pub fn with_selection(mut self, start: Option<usize>, end: Option<usize>) -> Self {
        self.selection_start = start;
        self.selection_end = end;
        self
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_padding(mut self, padding: f64) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_radius(mut self, radius: f64) -> Self {
        self.radius = radius;
        self
    }

    /// Get selection range (start, end) sorted
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        match (self.selection_start, self.selection_end) {
            (Some(start), Some(end)) => {
                let (s, e) = if start <= end { (start, end) } else { (end, start) };
                if s != e {
                    Some((s, e))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Display value (masks password)
    pub fn display_value(&self) -> String {
        match self.input_type {
            InputType::Password => "•".repeat(self.value.chars().count()),
            _ => self.value.clone(),
        }
    }
}

/// Text input rendering result
#[derive(Clone, Debug, Default)]
pub struct InputResult {
    /// Text area rectangle (for hit testing)
    pub text_rect: WidgetRect,
    /// Whether input is hovered
    pub hovered: bool,
    /// Cursor X position (for blinking cursor)
    pub cursor_x: f64,
    /// Cursor Y position
    pub cursor_y: f64,
    /// Cursor height
    pub cursor_height: f64,
    /// X positions of each character boundary (0..=char_count).
    /// Used for click-to-cursor without needing RenderContext.
    pub char_x_positions: Vec<f64>,
}

/// Draw a text input field
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Input configuration
/// - `state` - Current widget state
/// - `rect` - Input rectangle
/// - `theme` - Widget theme colors
///
/// # Returns
/// Input result with cursor position
pub fn draw_input(
    ctx: &mut dyn RenderContext,
    config: &InputConfig,
    state: WidgetState,
    rect: WidgetRect,
    theme: &WidgetTheme,
) -> InputResult {
    let effective_state = if config.disabled {
        WidgetState::Disabled
    } else {
        state
    };

    // Determine colors based on state
    let (bg_color, border_color, text_color) = match effective_state {
        WidgetState::Disabled => (
            &theme.bg_disabled,
            &theme.border_normal,
            &theme.text_disabled,
        ),
        _ if config.focused => (
            &theme.bg_normal,
            &theme.border_focused,
            &theme.text_normal,
        ),
        WidgetState::Hovered | WidgetState::Pressed => (
            &theme.bg_normal,
            &theme.border_hover,
            &theme.text_normal,
        ),
        WidgetState::Normal => (
            &theme.bg_normal,
            &theme.border_normal,
            &theme.text_normal,
        ),
    };

    // Draw background
    ctx.set_fill_color(bg_color);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, config.radius);

    // Draw border
    let border_width = if config.focused { 2.0 } else { 1.0 };
    ctx.set_stroke_color(border_color);
    ctx.set_stroke_width(border_width);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, config.radius);

    // Text area (with padding).
    // `text_rect.x` is used as the left origin for text and selection X positions —
    // it accounts for any left-side icon/padding offset (e.g. 28px for a search icon).
    // `text_rect.y` / `text_rect.height` are only valid for symmetric padding (like 8px).
    // For large asymmetric padding (e.g. 28px) the inset height would go to zero, so
    // all vertical extents use the original `rect` dimensions instead.
    let text_rect = rect.inset(config.padding);

    // Set font
    ctx.set_font(&format!("{}px sans-serif", config.font_size));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let display_text = config.display_value();
    let text_y = rect.center_y(); // use original rect for vertical centering, not inset text_rect

    // ── Text scroll offset calculation ────────────────────────────────────────
    //
    // When the text is wider than the visible area, we scroll the text so that
    // the cursor is always in view. The scroll offset is the number of pixels
    // the text is shifted to the left.
    //
    // `available_width` is the width of the visible text area (text_rect.width).
    // When padding is very large (e.g. 28px on each side of a narrow box), this
    // can be negative or zero — in that case no scrolling is needed.
    let available_width = text_rect.width.max(0.0);

    let text_width = if display_text.is_empty() {
        0.0
    } else {
        ctx.measure_text(&display_text)
    };

    // Compute the cursor x relative to the unscrolled text origin.
    let char_count = display_text.chars().count();
    let safe_cursor = config.cursor.min(char_count);
    let text_before_cursor = safe_char_slice(&display_text, 0, safe_cursor);
    let cursor_offset_from_text_start = ctx.measure_text(text_before_cursor);

    // Compute scroll offset: shift text left so cursor stays visible.
    // Keep a small right-side margin (4px) so the cursor is not flush against the clip edge.
    let cursor_margin = 4.0;
    let scroll_offset_x = if text_width <= available_width {
        // All text fits — no scrolling needed.
        0.0
    } else {
        // Position the cursor at (available_width - margin) from the left edge of the text area.
        // Clamp so we never scroll past the beginning or end of the text.
        let ideal = cursor_offset_from_text_start - (available_width - cursor_margin);
        let max_scroll = (text_width - available_width).max(0.0);
        ideal.max(0.0).min(max_scroll)
    };

    // ── Selection background ──────────────────────────────────────────────────
    //
    // The selection rectangle is clipped to the visible text area so it does not
    // bleed outside the input box when the text is scrolled.
    //
    // Horizontal extent: derived from `text_rect.x` (respects the left icon/padding offset)
    //   adjusted by the scroll offset.
    // Vertical extent: uses the full input `rect` height, not the inset `text_rect` height.
    // When `config.padding` is large (e.g. 28px for a search icon), `text_rect.height`
    // collapses to zero and the highlight would be invisible. Using `rect.y` / `rect.height`
    // ensures the highlight spans the full visible input box regardless of padding.
    if let Some((sel_start, sel_end)) = config.selection_range() {
        let safe_start = sel_start.min(char_count);
        let safe_end = sel_end.min(char_count);

        let before_sel = safe_char_slice(&display_text, 0, safe_start);
        let selection = safe_char_slice(&display_text, safe_start, safe_end);

        // Apply scroll offset to selection x positions so the highlight tracks the text.
        let sel_start_x = text_rect.x - scroll_offset_x + ctx.measure_text(before_sel);
        let sel_width = ctx.measure_text(selection);

        // Clip the selection highlight to the text area so it doesn't bleed outside the box.
        ctx.save();
        ctx.clip_rect(text_rect.x, rect.y, available_width, rect.height);
        ctx.set_fill_color(&theme.accent);
        ctx.fill_rect(sel_start_x, rect.y, sel_width, rect.height);
        ctx.restore();
    }

    // ── Draw text or placeholder ──────────────────────────────────────────────
    //
    // Text is rendered inside a clip rect so it never bleeds outside the input box.
    // The scroll offset shifts the text left so the cursor position is always visible.
    ctx.save();
    ctx.clip_rect(text_rect.x, rect.y, available_width, rect.height);
    if display_text.is_empty() && !config.placeholder.is_empty() {
        ctx.set_fill_color(&theme.text_disabled);
        ctx.fill_text(&config.placeholder, text_rect.x, text_y);
    } else {
        ctx.set_fill_color(text_color);
        ctx.fill_text(&display_text, text_rect.x - scroll_offset_x, text_y);
    }
    ctx.restore();

    // ── Cursor position ───────────────────────────────────────────────────────
    //
    // The cursor x is the text origin plus the width of text before the cursor,
    // minus the scroll offset so it tracks the scrolled text position.
    let cursor_x = text_rect.x - scroll_offset_x + cursor_offset_from_text_start;
    let cursor_height = config.font_size * 1.2;
    let cursor_y = text_y - cursor_height / 2.0;

    // ── Character boundary X positions ───────────────────────────────────────
    //
    // Pre-compute the screen x position of each character boundary for
    // click-to-cursor. Positions are in screen coordinates, accounting for
    // the scroll offset so that a click at screen position `px` maps to the
    // correct character even when the text is scrolled.
    let mut char_x_positions = Vec::with_capacity(char_count + 1);
    for i in 0..=char_count {
        let text_slice = safe_char_slice(&display_text, 0, i);
        // Screen x = text_rect.x - scroll_offset_x + width_of_text_up_to_char_i
        let x_pos = text_rect.x - scroll_offset_x + ctx.measure_text(text_slice);
        char_x_positions.push(x_pos);
    }

    // Draw cursor if focused (caller handles blinking)
    // The actual cursor drawing should be done by the platform with blinking logic

    InputResult {
        text_rect,
        hovered: effective_state.is_hovered(),
        cursor_x,
        cursor_y,
        cursor_height,
        char_x_positions,
    }
}

/// Draw blinking cursor
///
/// Call this separately with blink state
pub fn draw_input_cursor(
    ctx: &mut dyn RenderContext,
    cursor_x: f64,
    cursor_y: f64,
    cursor_height: f64,
    color: &str,
) {
    ctx.set_fill_color(color);
    ctx.fill_rect(cursor_x, cursor_y, 1.5, cursor_height);
}

/// Convert character index to byte index in a UTF-8 string
///
/// This ensures we always slice at valid UTF-8 character boundaries.
/// If char_idx is beyond the string length, returns the byte length of the string.
fn char_idx_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

/// Safely slice a string by character indices (not byte indices)
///
/// Returns a substring from char_start to char_end, handling UTF-8 correctly.
fn safe_char_slice(s: &str, char_start: usize, char_end: usize) -> &str {
    let start_byte = char_idx_to_byte_idx(s, char_start);
    let end_byte = char_idx_to_byte_idx(s, char_end);
    &s[start_byte..end_byte]
}

/// Calculate character index from X position
///
/// # Parameters
/// - `ctx` - Render context (for text measurement)
/// - `config` - Input configuration
/// - `text_rect` - Text area rectangle
/// - `x` - Click X position
///
/// # Returns
/// Character index at position
pub fn input_position_to_cursor(
    ctx: &mut dyn RenderContext,
    config: &InputConfig,
    text_rect: WidgetRect,
    x: f64,
) -> usize {
    let display_text = config.display_value();

    if display_text.is_empty() {
        return 0;
    }

    let rel_x = (x - text_rect.x).max(0.0);
    ctx.set_font(&format!("{}px sans-serif", config.font_size));

    // Binary search would be more efficient, but linear is fine for typical input lengths
    let mut best_idx = 0;
    let mut best_dist = f64::MAX;

    let char_count = display_text.chars().count();
    for i in 0..=char_count {
        let text_slice = safe_char_slice(&display_text, 0, i);
        let text_width = ctx.measure_text(text_slice);
        let dist = (rel_x - text_width).abs();
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
        }
    }

    best_idx
}

/// Find the character index closest to a click X position using pre-computed positions.
///
/// This avoids needing a RenderContext at click time.
pub fn cursor_from_char_positions(char_x_positions: &[f64], click_x: f64) -> usize {
    if char_x_positions.is_empty() {
        return 0;
    }
    let mut best_idx = 0;
    let mut best_dist = f64::MAX;
    for (i, &x_pos) in char_x_positions.iter().enumerate() {
        let dist = (click_x - x_pos).abs();
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
        }
    }
    best_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_config() {
        let config = InputConfig::new("Hello")
            .with_placeholder("Enter text...")
            .with_focused(true);

        assert_eq!(config.value, "Hello");
        assert_eq!(config.placeholder, "Enter text...");
        assert!(config.focused);
        assert_eq!(config.cursor, 5);
    }

    #[test]
    fn test_password_display() {
        let config = InputConfig::new("secret").with_type(InputType::Password);
        assert_eq!(config.display_value(), "••••••");
    }

    #[test]
    fn test_selection_range() {
        let mut config = InputConfig::new("Hello");

        // No selection
        assert!(config.selection_range().is_none());

        // With selection
        config.selection_start = Some(1);
        config.selection_end = Some(4);
        assert_eq!(config.selection_range(), Some((1, 4)));

        // Reversed selection
        config.selection_start = Some(4);
        config.selection_end = Some(1);
        assert_eq!(config.selection_range(), Some((1, 4)));

        // Empty selection (same position)
        config.selection_start = Some(2);
        config.selection_end = Some(2);
        assert!(config.selection_range().is_none());
    }

    #[test]
    fn test_char_idx_to_byte_idx() {
        // ASCII string
        let ascii = "Hello";
        assert_eq!(char_idx_to_byte_idx(ascii, 0), 0);
        assert_eq!(char_idx_to_byte_idx(ascii, 3), 3);
        assert_eq!(char_idx_to_byte_idx(ascii, 5), 5);
        assert_eq!(char_idx_to_byte_idx(ascii, 10), 5); // Beyond length

        // Russian string (multi-byte UTF-8)
        let russian = "тест"; // 4 chars, 8 bytes
        assert_eq!(char_idx_to_byte_idx(russian, 0), 0);
        assert_eq!(char_idx_to_byte_idx(russian, 1), 2); // 'т' is 2 bytes
        assert_eq!(char_idx_to_byte_idx(russian, 2), 4); // 'е' is 2 bytes
        assert_eq!(char_idx_to_byte_idx(russian, 3), 6); // 'с' is 2 bytes
        assert_eq!(char_idx_to_byte_idx(russian, 4), 8); // End of string
        assert_eq!(char_idx_to_byte_idx(russian, 10), 8); // Beyond length

        // Mixed string
        let mixed = "Hello тест"; // 10 chars, 14 bytes
        assert_eq!(char_idx_to_byte_idx(mixed, 0), 0);
        assert_eq!(char_idx_to_byte_idx(mixed, 5), 5); // Space
        assert_eq!(char_idx_to_byte_idx(mixed, 6), 6); // 'т'
        assert_eq!(char_idx_to_byte_idx(mixed, 7), 8); // 'е'
    }

    #[test]
    fn test_safe_char_slice() {
        // ASCII string
        let ascii = "Hello";
        assert_eq!(safe_char_slice(ascii, 0, 5), "Hello");
        assert_eq!(safe_char_slice(ascii, 1, 4), "ell");
        assert_eq!(safe_char_slice(ascii, 0, 0), "");

        // Russian string
        let russian = "тест";
        assert_eq!(safe_char_slice(russian, 0, 4), "тест");
        assert_eq!(safe_char_slice(russian, 0, 1), "т");
        assert_eq!(safe_char_slice(russian, 1, 3), "ес");
        assert_eq!(safe_char_slice(russian, 2, 4), "ст");

        // Beyond length
        assert_eq!(safe_char_slice(russian, 0, 10), "тест");
        assert_eq!(safe_char_slice(russian, 2, 10), "ст");

        // Mixed string
        let mixed = "Hello тест";
        assert_eq!(safe_char_slice(mixed, 0, 5), "Hello");
        assert_eq!(safe_char_slice(mixed, 6, 10), "тест");
        assert_eq!(safe_char_slice(mixed, 0, 10), "Hello тест");
    }

    #[test]
    fn test_russian_input_config() {
        // Test that cursor is correctly set to char count, not byte count
        let russian = "привет"; // 6 chars, 12 bytes
        let config = InputConfig::new(russian);
        assert_eq!(config.value, "привет");
        assert_eq!(config.cursor, 6); // Should be 6 (char count), not 12 (byte count)

        // Test mixed input
        let mixed = "Hello мир"; // 9 chars (with space), 13 bytes
        let config = InputConfig::new(mixed);
        assert_eq!(config.cursor, 9);
    }

    #[test]
    fn test_password_with_russian() {
        let russian = "пароль"; // 6 chars, 12 bytes
        let config = InputConfig::new(russian).with_type(InputType::Password);
        assert_eq!(config.display_value(), "••••••"); // Should be 6 dots, not 12
    }
}
