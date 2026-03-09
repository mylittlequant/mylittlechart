//! Centralized slider widget system
//!
//! Provides both single-point and dual-point sliders with complete input handling.
//!
//! # Components
//!
//! - `SingleSlider` - One handle, one value (e.g., opacity: 0-100%)
//! - `DualSlider` - Two handles, min/max range (e.g., timeframe visibility: 1-59)
//! - `SliderConfig` - Common configuration
//! - `SliderDragState` - Tracks active drag operations
//! - Input handlers for drag, scroll, click, and text input
//!
//! # Architecture
//!
//! ```text
//! Rendering:
//!   render_single_slider() -> SingleSliderResult (rects for hit testing)
//!   render_dual_slider() -> DualSliderResult (rects for hit testing)
//!
//! Input Handling:
//!   SliderInputHandler:
//!     - handle_drag_start(x, y, track_info) -> starts drag
//!     - handle_drag_move(x) -> updates value during drag
//!     - handle_drag_end() -> completes drag
//!     - handle_scroll(delta, track_info) -> adjusts value with scroll wheel
//!     - handle_click(x, track_info) -> jumps to clicked position
//!     - handle_text_input(field_id, text) -> updates from typed value
//! ```
//!
//! # Usage Example
//!
//! ```ignore
//! // Render a single slider
//! let config = SliderConfig {
//!     min: 0.0,
//!     max: 100.0,
//!     step: 1.0,
//!     ..Default::default()
//! };
//! let result = render_single_slider(
//!     ctx, &config, 50.0, rect, label, theme, hovered
//! );
//!
//! // Handle drag
//! if let Some(track_info) = result.track_info {
//!     state.slider_drag = SliderDragState::single(
//!         field_id, track_info.track_x, track_info.track_width, min, max
//!     );
//! }
//! ```

use crate::render::{RenderContext, TextBaseline};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::widgets::input::{draw_input, draw_input_cursor, InputConfig, InputType, InputResult};
use uzor::types::Rect as WidgetRect;

// =============================================================================
// Configuration
// =============================================================================

/// Common slider configuration
#[derive(Clone, Debug)]
pub struct SliderConfig {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Step size (0 for continuous, >0 for discrete steps)
    pub step: f64,
    /// Track height (vertical thickness)
    pub track_height: f64,
    /// Handle radius
    pub handle_radius: f64,
    /// Input field width (for value display/editing)
    pub input_width: f64,
    /// Input field height
    pub input_height: f64,
    /// Show input field?
    pub show_input: bool,
    /// Label spacing (gap between label and track)
    pub label_spacing: f64,
    /// Track-to-input spacing
    pub track_input_spacing: f64,
}

impl Default for SliderConfig {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            step: 0.0,
            track_height: 4.0,
            handle_radius: 7.0,
            input_width: 50.0,      // Smaller input field
            input_height: 22.0,     // Smaller input field
            show_input: true,
            label_spacing: 12.0,    // More space after label
            track_input_spacing: 12.0, // More space before input
        }
    }
}

impl SliderConfig {
    /// Create config with range
    pub fn new(min: f64, max: f64) -> Self {
        Self {
            min,
            max,
            ..Default::default()
        }
    }

    /// Set step size
    pub fn with_step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Hide input field
    pub fn without_input(mut self) -> Self {
        self.show_input = false;
        self
    }

    /// Set custom input field width
    pub fn with_input_width(mut self, width: f64) -> Self {
        self.input_width = width;
        self
    }

    /// Set custom input field height
    pub fn with_input_height(mut self, height: f64) -> Self {
        self.input_height = height;
        self
    }

    /// Set custom track height (thickness)
    pub fn with_track_height(mut self, height: f64) -> Self {
        self.track_height = height;
        self
    }

    /// Set custom handle radius
    pub fn with_handle_radius(mut self, radius: f64) -> Self {
        self.handle_radius = radius;
        self
    }

    /// Set custom label spacing
    pub fn with_label_spacing(mut self, spacing: f64) -> Self {
        self.label_spacing = spacing;
        self
    }

    /// Set custom track-input spacing
    pub fn with_track_input_spacing(mut self, spacing: f64) -> Self {
        self.track_input_spacing = spacing;
        self
    }

    /// Apply step to value
    pub fn apply_step(&self, value: f64) -> f64 {
        if self.step > 0.0 {
            (value / self.step).round() * self.step
        } else {
            value
        }
    }

    /// Clamp value to range
    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    /// Normalize value to 0.0..1.0
    pub fn normalize(&self, value: f64) -> f64 {
        if self.max <= self.min {
            return 0.0;
        }
        ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Denormalize from 0.0..1.0 to value
    pub fn denormalize(&self, t: f64) -> f64 {
        self.min + t * (self.max - self.min)
    }
}

// =============================================================================
// Rendering Results
// =============================================================================

/// Result of single slider rendering
#[derive(Clone, Debug)]
pub struct SingleSliderResult {
    /// Full slider rect (label + track + input)
    pub full_rect: WidgetRect,
    /// Track rect (the horizontal bar)
    pub track_rect: WidgetRect,
    /// Handle rect (for hit testing)
    pub handle_rect: WidgetRect,
    /// Input field rect (if shown)
    pub input_rect: Option<WidgetRect>,
    /// Track info for drag handling (x, width, min, max)
    pub track_info: Option<SliderTrackInfo>,
    /// Full input rendering result (cursor position, char positions) — set when input was drawn
    pub input_result: Option<InputResult>,
}

impl Default for SingleSliderResult {
    fn default() -> Self {
        Self {
            full_rect: WidgetRect::default(),
            track_rect: WidgetRect::default(),
            handle_rect: WidgetRect::default(),
            input_rect: None,
            track_info: None,
            input_result: None,
        }
    }
}

/// Result of dual slider rendering
#[derive(Clone, Debug)]
pub struct DualSliderResult {
    /// Full slider rect (label + track + inputs)
    pub full_rect: WidgetRect,
    /// Track rect (the horizontal bar)
    pub track_rect: WidgetRect,
    /// Min handle rect (for hit testing)
    pub min_handle_rect: WidgetRect,
    /// Max handle rect (for hit testing)
    pub max_handle_rect: WidgetRect,
    /// Min value input rect (if shown)
    pub min_input_rect: Option<WidgetRect>,
    /// Max value input rect (if shown)
    pub max_input_rect: Option<WidgetRect>,
    /// Track info for drag handling
    pub track_info: Option<SliderTrackInfo>,
    /// Full min input rendering result (cursor position, char positions) — set when input was drawn
    pub min_input_result: Option<InputResult>,
    /// Full max input rendering result (cursor position, char positions) — set when input was drawn
    pub max_input_result: Option<InputResult>,
}

impl Default for DualSliderResult {
    fn default() -> Self {
        Self {
            full_rect: WidgetRect::default(),
            track_rect: WidgetRect::default(),
            min_handle_rect: WidgetRect::default(),
            max_handle_rect: WidgetRect::default(),
            min_input_rect: None,
            max_input_rect: None,
            track_info: None,
            min_input_result: None,
            max_input_result: None,
        }
    }
}

/// Track info for drag/scroll/click handling
#[derive(Clone, Debug)]
pub struct SliderTrackInfo {
    /// Field ID (e.g., "stroke_width", "style_prop:label_font_size")
    pub field_id: String,
    /// Track X position (left edge)
    pub track_x: f64,
    /// Track width
    pub track_width: f64,
    /// Minimum value
    pub min_val: f64,
    /// Maximum value
    pub max_val: f64,
}

impl SliderTrackInfo {
    /// Create track info
    pub fn new(field_id: impl Into<String>, track_x: f64, track_width: f64, min_val: f64, max_val: f64) -> Self {
        Self {
            field_id: field_id.into(),
            track_x,
            track_width,
            min_val,
            max_val,
        }
    }

    /// Convert X position to value
    pub fn position_to_value(&self, x: f64) -> f64 {
        let t = ((x - self.track_x) / self.track_width).clamp(0.0, 1.0);
        self.min_val + t * (self.max_val - self.min_val)
    }

    /// Convert value to X position
    pub fn value_to_position(&self, value: f64) -> f64 {
        let t = ((value - self.min_val) / (self.max_val - self.min_val)).clamp(0.0, 1.0);
        self.track_x + t * self.track_width
    }
}

// =============================================================================
// Drag State
// =============================================================================

/// Which handle of a dual-handle slider is being dragged
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualSliderHandle {
    Min,
    Max,
}

/// State for slider dragging
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
    /// For dual-handle sliders: which handle is being dragged
    pub dual_handle: Option<DualSliderHandle>,
}

impl SliderDragState {
    /// Create single-handle slider drag state
    pub fn single(field_id: impl Into<String>, slider_x: f64, slider_width: f64, min_val: f64, max_val: f64) -> Self {
        Self {
            field_id: field_id.into(),
            slider_x,
            slider_width,
            min_val,
            max_val,
            dual_handle: None,
        }
    }

    /// Create dual-handle slider drag state
    pub fn dual(field_id: impl Into<String>, slider_x: f64, slider_width: f64, min_val: f64, max_val: f64, handle: DualSliderHandle) -> Self {
        Self {
            field_id: field_id.into(),
            slider_x,
            slider_width,
            min_val,
            max_val,
            dual_handle: Some(handle),
        }
    }

    /// Update value during drag - returns new value
    pub fn update(&self, mouse_x: f64) -> f64 {
        let t = ((mouse_x - self.slider_x) / self.slider_width).clamp(0.0, 1.0);
        self.min_val + t * (self.max_val - self.min_val)
    }

    /// Check if this is a dual-handle slider
    pub fn is_dual(&self) -> bool {
        self.dual_handle.is_some()
    }

    /// Get which handle is being dragged (if dual)
    pub fn handle(&self) -> Option<DualSliderHandle> {
        self.dual_handle
    }
}

// =============================================================================
// Rendering Functions
// =============================================================================

/// Editing state passed into `render_single_slider` when the value input is active.
pub struct SliderEditingInfo<'a> {
    /// In-progress text from the text editing state
    pub text: &'a str,
    /// Cursor position (character index)
    pub cursor: usize,
    /// Selection anchor (character index), if any
    pub selection_start: Option<usize>,
}

/// Render a single-point slider (one handle, one value)
///
/// Layout: [Label] ──────●── [Input]
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Slider configuration
/// - `value` - Current value
/// - `rect` - Total rect for slider (label + track + input)
/// - `label` - Label text
/// - `theme` - Color theme
/// - `hovered` - Is slider hovered?
/// - `editing` - Optional in-progress editing state. When `Some`, the value
///   display becomes a focused text input showing the live buffer instead of
///   the committed value.
///
/// # Returns
/// Rendering result with hit test rects
pub fn render_single_slider<'a>(
    ctx: &mut dyn RenderContext,
    config: &SliderConfig,
    value: f64,
    rect: WidgetRect,
    label: &str,
    theme: &WidgetTheme,
    hovered: bool,
    editing: Option<SliderEditingInfo<'a>>,
) -> SingleSliderResult {
    let x_start = rect.x;

    // Establish common vertical center line for ALL elements
    let center_y = rect.y + rect.height / 2.0;

    // Measure label width
    ctx.set_font("12px sans-serif");
    let label_width = ctx.measure_text(label);

    // Calculate available space for track
    // Label takes: label_width + label_spacing
    // Input takes: input_width + track_input_spacing (if shown)
    let label_section_width = label_width + config.label_spacing;
    let input_section_width = if config.show_input {
        config.input_width + config.track_input_spacing
    } else {
        0.0
    };

    // Track fills all remaining space
    let track_width = rect.width - label_section_width - input_section_width;

    // Layout: [Label] [spacing] [Track] [spacing] [Input]
    let mut x = x_start;

    // Draw label (centered on center_y using Middle baseline)
    ctx.set_fill_color(&theme.text_normal);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x, center_y);
    x += label_width + config.label_spacing;

    // Track position (centered vertically on center_y)
    let track_x = x;
    let track_y = center_y - config.track_height / 2.0;

    // Calculate handle position (centered on center_y)
    let normalized = config.normalize(value);
    let handle_x = track_x + normalized * track_width;
    let handle_y = center_y;

    // Draw track background
    ctx.set_fill_color(&theme.border_normal);
    ctx.fill_rounded_rect(track_x, track_y, track_width, config.track_height, config.track_height / 2.0);

    // Draw filled portion (from left to handle)
    if normalized > 0.0 {
        ctx.set_fill_color(&theme.accent);
        let fill_width = normalized * track_width;
        ctx.fill_rounded_rect(track_x, track_y, fill_width, config.track_height, config.track_height / 2.0);
    }

    // Draw handle
    ctx.set_fill_color(&theme.text_normal);
    ctx.begin_path();
    ctx.arc(handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
    ctx.fill();

    // Draw handle border when hovered
    if hovered {
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.arc(handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
        ctx.stroke();
    }

    x += track_width + config.track_input_spacing;

    // Draw input field (centered vertically on center_y)
    let (input_rect, drawn_input_result) = if config.show_input {
        let input_x = x;
        let input_y = center_y - config.input_height / 2.0;
        let input_rect = WidgetRect::new(input_x, input_y, config.input_width, config.input_height);

        let is_editing = editing.is_some();

        // Determine the display text, cursor position, and selection from editing state.
        // When editing, show live buffer; otherwise show formatted committed value.
        let committed_text;
        let (display_text, cursor_pos, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) =
            if let Some(ref ed) = editing {
                (ed.text, ed.cursor, ed.selection_start, Some(ed.cursor))
            } else {
                committed_text = if config.step.abs() < f64::EPSILON {
                    format!("{:.2}", value)
                } else if config.step >= 1.0 {
                    format!("{:.0}", value)
                } else {
                    format!("{:.2}", value)
                };
                let len = committed_text.chars().count();
                (&committed_text as &str, len, None, None)
            };

        // Build input config; focused (blue border + cursor) when editing.
        let input_config = InputConfig::new(display_text)
            .with_focused(is_editing)
            .with_type(InputType::Number)
            .with_font_size(12.0)
            .with_padding(4.0)
            .with_radius(4.0)
            .with_cursor(cursor_pos)
            .with_selection(sel_start, sel_end);

        let drawn = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, theme);

        // Draw blinking cursor line when editing.
        if is_editing {
            draw_input_cursor(ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &theme.text_normal);
        }

        (Some(input_rect), Some(drawn))
    } else {
        (None, None)
    };

    // Create handle rect for hit testing
    let handle_rect = WidgetRect::new(
        handle_x - config.handle_radius,
        handle_y - config.handle_radius,
        config.handle_radius * 2.0,
        config.handle_radius * 2.0,
    );

    SingleSliderResult {
        full_rect: rect,
        track_rect: WidgetRect::new(track_x, track_y, track_width, config.track_height),
        handle_rect,
        input_rect,
        track_info: Some(SliderTrackInfo::new("", track_x, track_width, config.min, config.max)),
        input_result: drawn_input_result,
    }
}

/// Render a dual-point slider (two handles, min/max range)
///
/// Layout: [Label] [MinInput] ──●━━━●── [MaxInput]
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Slider configuration
/// - `min_value` - Current minimum value
/// - `max_value` - Current maximum value
/// - `rect` - Total rect for slider
/// - `label` - Label text
/// - `theme` - Color theme
/// - `hovered` - Is slider hovered?
/// - `editing_min` - Is min input being edited?
/// - `editing_max` - Is max input being edited?
///
/// # Returns
/// Rendering result with hit test rects
pub fn render_dual_slider<'a>(
    ctx: &mut dyn RenderContext,
    config: &SliderConfig,
    min_value: f64,
    max_value: f64,
    rect: WidgetRect,
    label: &str,
    theme: &WidgetTheme,
    hovered: bool,
    editing_min: Option<SliderEditingInfo<'a>>,
    editing_max: Option<SliderEditingInfo<'a>>,
) -> DualSliderResult {
    let x_start = rect.x;

    // Establish common vertical center line for ALL elements
    let center_y = rect.y + rect.height / 2.0;

    // Measure label width
    ctx.set_font("12px sans-serif");
    let label_width = ctx.measure_text(label);

    // Calculate available space for track
    // Label takes: label_width + label_spacing
    // Inputs take: (input_width * 2) + (track_input_spacing * 2) - spacing on both sides
    let label_section_width = label_width + config.label_spacing;
    let inputs_section_width = if config.show_input {
        config.input_width * 2.0 + config.track_input_spacing * 2.0
    } else {
        0.0
    };

    // Track fills all remaining space
    let track_width = rect.width - label_section_width - inputs_section_width;

    // Layout: [Label] [spacing] [MinInput] [spacing] [Track] [spacing] [MaxInput]
    let mut x = x_start;

    // Draw label (centered on center_y using Middle baseline)
    ctx.set_fill_color(&theme.text_normal);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x, center_y);
    x += label_width + config.label_spacing;

    // Reserve space for min input (positioned before track)
    if config.show_input {
        x += config.input_width + config.track_input_spacing;
    }

    // Track position (centered vertically on center_y)
    let track_x = x;
    let track_y = center_y - config.track_height / 2.0;

    // Calculate handle positions (centered on center_y)
    let min_normalized = config.normalize(min_value);
    let max_normalized = config.normalize(max_value);
    let min_handle_x = track_x + min_normalized * track_width;
    let max_handle_x = track_x + max_normalized * track_width;
    let handle_y = center_y;

    // Draw track background (unfilled portions)
    ctx.set_fill_color(&theme.border_normal);
    ctx.fill_rounded_rect(track_x, track_y, track_width, config.track_height, config.track_height / 2.0);

    // Draw filled portion (between min and max handles)
    let fill_start = min_normalized * track_width;
    let fill_width = (max_normalized - min_normalized) * track_width;
    if fill_width > 0.0 {
        ctx.set_fill_color(&theme.accent);
        ctx.fill_rounded_rect(track_x + fill_start, track_y, fill_width, config.track_height, config.track_height / 2.0);
    }

    // Draw handles

    // Min handle
    ctx.set_fill_color(&theme.text_normal);
    ctx.begin_path();
    ctx.arc(min_handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
    ctx.fill();
    if hovered {
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.arc(min_handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
        ctx.stroke();
    }

    // Max handle
    ctx.set_fill_color(&theme.text_normal);
    ctx.begin_path();
    ctx.arc(max_handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
    ctx.fill();
    if hovered {
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.arc(max_handle_x, handle_y, config.handle_radius, 0.0, std::f64::consts::TAU);
        ctx.stroke();
    }

    // Draw input fields (centered vertically on center_y)
    let (min_input_rect, max_input_rect, min_input_result, max_input_result) = if config.show_input {
        let input_y = center_y - config.input_height / 2.0;

        // Min input СЛЕВА от трека
        let min_input_x = track_x - config.input_width - config.track_input_spacing;
        let min_input_rect = WidgetRect::new(min_input_x, input_y, config.input_width, config.input_height);
        let min_ir = draw_range_input(ctx, min_value, min_input_rect, theme, editing_min.as_ref(), config.step, center_y);

        // Max input СПРАВА от трека
        let max_input_x = track_x + track_width + config.track_input_spacing;
        let max_input_rect = WidgetRect::new(max_input_x, input_y, config.input_width, config.input_height);
        let max_ir = draw_range_input(ctx, max_value, max_input_rect, theme, editing_max.as_ref(), config.step, center_y);

        (Some(min_input_rect), Some(max_input_rect), Some(min_ir), Some(max_ir))
    } else {
        (None, None, None, None)
    };

    // Create handle rects for hit testing
    let min_handle_rect = WidgetRect::new(
        min_handle_x - config.handle_radius,
        handle_y - config.handle_radius,
        config.handle_radius * 2.0,
        config.handle_radius * 2.0,
    );
    let max_handle_rect = WidgetRect::new(
        max_handle_x - config.handle_radius,
        handle_y - config.handle_radius,
        config.handle_radius * 2.0,
        config.handle_radius * 2.0,
    );

    DualSliderResult {
        full_rect: rect,
        track_rect: WidgetRect::new(track_x, track_y, track_width, config.track_height),
        min_handle_rect,
        max_handle_rect,
        min_input_rect,
        max_input_rect,
        track_info: Some(SliderTrackInfo::new("", track_x, track_width, config.min, config.max)),
        min_input_result,
        max_input_result,
    }
}

/// Helper: Draw a range input field (for dual slider min/max)
fn draw_range_input(
    ctx: &mut dyn RenderContext,
    value: f64,
    rect: WidgetRect,
    theme: &WidgetTheme,
    editing: Option<&SliderEditingInfo<'_>>,
    step: f64,
    _center_y: f64,
) -> InputResult {
    // Format value text (used when not editing)
    let value_text = if step.abs() < f64::EPSILON {
        format!("{:.2}", value)
    } else if step >= 1.0 {
        format!("{:.0}", value)
    } else {
        format!("{:.2}", value)
    };

    let is_editing = editing.is_some();

    // Determine display text and cursor/selection from editing state
    let (display_text, cursor_pos, selection_start): (&str, usize, Option<usize>) = if let Some(edit) = editing {
        (edit.text, edit.cursor, edit.selection_start)
    } else {
        (&value_text, value_text.chars().count(), None)
    };

    let selection_end = if is_editing { Some(cursor_pos) } else { None };

    // Use centralized draw_input
    let input_config = InputConfig::new(display_text)
        .with_focused(is_editing)
        .with_type(InputType::Number)
        .with_font_size(12.0)
        .with_padding(4.0)
        .with_radius(4.0)
        .with_cursor(cursor_pos)
        .with_selection(selection_start, selection_end);

    let widget_state = if is_editing {
        WidgetState::Hovered
    } else {
        WidgetState::Normal
    };

    let ir = draw_input(ctx, &input_config, widget_state, rect, theme);

    // Draw cursor if editing
    if is_editing {
        draw_input_cursor(ctx, ir.cursor_x, ir.cursor_y, ir.cursor_height, &theme.text_normal);
    }

    ir
}

// =============================================================================
// Input Handling
// =============================================================================

/// Slider input handler - manages drag, scroll, click, and text input
pub struct SliderInputHandler;

impl SliderInputHandler {
    /// Handle scroll wheel over slider
    ///
    /// # Parameters
    /// - `delta` - Scroll delta (positive = scroll down/decrease, negative = scroll up/increase)
    /// - `current_value` - Current slider value
    /// - `config` - Slider configuration
    ///
    /// # Returns
    /// New value after scroll adjustment
    pub fn handle_scroll(delta: f64, current_value: f64, config: &SliderConfig) -> f64 {
        let step = if config.step > 0.0 {
            config.step
        } else {
            // Auto-determine step based on range
            let range = config.max - config.min;
            if range > 100.0 {
                1.0
            } else if range > 10.0 {
                0.1
            } else {
                0.01
            }
        };

        // Scroll up (negative delta) = increase value
        // Scroll down (positive delta) = decrease value
        let adjustment = -delta.signum() * step;
        let new_value = current_value + adjustment;
        config.clamp(config.apply_step(new_value))
    }

    /// Handle drag start on slider track
    ///
    /// Returns initial value based on click position
    pub fn handle_drag_start(click_x: f64, track_info: &SliderTrackInfo, config: &SliderConfig) -> f64 {
        let value = track_info.position_to_value(click_x);
        config.clamp(config.apply_step(value))
    }

    /// Handle drag move
    ///
    /// Returns updated value based on mouse position
    pub fn handle_drag_move(mouse_x: f64, drag_state: &SliderDragState, config: &SliderConfig) -> f64 {
        let value = drag_state.update(mouse_x);
        config.clamp(config.apply_step(value))
    }

    /// Handle click on slider track (jump to position)
    ///
    /// Returns new value at clicked position
    pub fn handle_click(click_x: f64, track_info: &SliderTrackInfo, config: &SliderConfig) -> f64 {
        let value = track_info.position_to_value(click_x);
        config.clamp(config.apply_step(value))
    }

    /// Handle text input (parse typed value)
    ///
    /// Returns parsed value or None if invalid
    pub fn handle_text_input(text: &str, config: &SliderConfig) -> Option<f64> {
        text.trim().parse::<f64>().ok().map(|v| config.clamp(v))
    }

    /// Determine which handle of dual slider is closer to click position
    pub fn determine_dual_handle(
        click_x: f64,
        min_value: f64,
        max_value: f64,
        track_info: &SliderTrackInfo,
    ) -> DualSliderHandle {
        let min_x = track_info.value_to_position(min_value);
        let max_x = track_info.value_to_position(max_value);
        let dist_to_min = (click_x - min_x).abs();
        let dist_to_max = (click_x - max_x).abs();

        if dist_to_min < dist_to_max {
            DualSliderHandle::Min
        } else {
            DualSliderHandle::Max
        }
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Convert value to position on track
pub fn value_to_position(value: f64, min: f64, max: f64, track_x: f64, track_width: f64) -> f64 {
    if max <= min {
        return track_x;
    }
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    track_x + t * track_width
}

/// Convert position to value
pub fn position_to_value(x: f64, track_x: f64, track_width: f64, min: f64, max: f64) -> f64 {
    let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
    min + t * (max - min)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slider_config_normalize() {
        let config = SliderConfig::new(0.0, 100.0);
        assert_eq!(config.normalize(0.0), 0.0);
        assert_eq!(config.normalize(50.0), 0.5);
        assert_eq!(config.normalize(100.0), 1.0);
        assert_eq!(config.normalize(-10.0), 0.0); // clamped
        assert_eq!(config.normalize(110.0), 1.0); // clamped
    }

    #[test]
    fn test_slider_config_denormalize() {
        let config = SliderConfig::new(0.0, 100.0);
        assert_eq!(config.denormalize(0.0), 0.0);
        assert_eq!(config.denormalize(0.5), 50.0);
        assert_eq!(config.denormalize(1.0), 100.0);
    }

    #[test]
    fn test_slider_config_step() {
        let config = SliderConfig::new(0.0, 10.0).with_step(2.0);
        assert_eq!(config.apply_step(1.5), 2.0);  // 1.5/2 = 0.75, rounds to 1, = 2.0
        assert_eq!(config.apply_step(2.1), 2.0);  // 2.1/2 = 1.05, rounds to 1, = 2.0
        assert_eq!(config.apply_step(2.9), 2.0);  // 2.9/2 = 1.45, rounds to 1, = 2.0
        assert_eq!(config.apply_step(3.0), 4.0);  // 3.0/2 = 1.5, rounds to 2, = 4.0
        assert_eq!(config.apply_step(3.5), 4.0);  // 3.5/2 = 1.75, rounds to 2, = 4.0
    }

    #[test]
    fn test_track_info_conversions() {
        let track = SliderTrackInfo::new("test", 100.0, 200.0, 0.0, 100.0);

        // Position to value
        assert_eq!(track.position_to_value(100.0), 0.0); // left edge
        assert_eq!(track.position_to_value(200.0), 50.0); // middle
        assert_eq!(track.position_to_value(300.0), 100.0); // right edge

        // Value to position
        assert_eq!(track.value_to_position(0.0), 100.0);
        assert_eq!(track.value_to_position(50.0), 200.0);
        assert_eq!(track.value_to_position(100.0), 300.0);
    }

    #[test]
    fn test_slider_drag_state() {
        let drag = SliderDragState::single("test", 100.0, 200.0, 0.0, 100.0);
        assert!(!drag.is_dual());
        assert_eq!(drag.handle(), None);

        // Update at middle
        assert_eq!(drag.update(200.0), 50.0);
        // Update at left
        assert_eq!(drag.update(100.0), 0.0);
        // Update at right
        assert_eq!(drag.update(300.0), 100.0);
    }

    #[test]
    fn test_dual_slider_drag_state() {
        let drag = SliderDragState::dual("test", 100.0, 200.0, 0.0, 100.0, DualSliderHandle::Min);
        assert!(drag.is_dual());
        assert_eq!(drag.handle(), Some(DualSliderHandle::Min));
    }

    #[test]
    fn test_slider_input_scroll() {
        let config = SliderConfig::new(0.0, 100.0).with_step(1.0);

        // Scroll up (negative delta) = increase
        assert_eq!(SliderInputHandler::handle_scroll(-1.0, 50.0, &config), 51.0);

        // Scroll down (positive delta) = decrease
        assert_eq!(SliderInputHandler::handle_scroll(1.0, 50.0, &config), 49.0);

        // Clamping at boundaries
        assert_eq!(SliderInputHandler::handle_scroll(1.0, 0.0, &config), 0.0);
        assert_eq!(SliderInputHandler::handle_scroll(-1.0, 100.0, &config), 100.0);
    }

    #[test]
    fn test_slider_input_text() {
        let config = SliderConfig::new(0.0, 100.0);

        assert_eq!(SliderInputHandler::handle_text_input("50", &config), Some(50.0));
        assert_eq!(SliderInputHandler::handle_text_input("50.5", &config), Some(50.5));
        assert_eq!(SliderInputHandler::handle_text_input("  75  ", &config), Some(75.0));
        assert_eq!(SliderInputHandler::handle_text_input("-10", &config), Some(0.0)); // clamped
        assert_eq!(SliderInputHandler::handle_text_input("150", &config), Some(100.0)); // clamped
        assert_eq!(SliderInputHandler::handle_text_input("abc", &config), None); // invalid
    }

    #[test]
    fn test_determine_dual_handle() {
        let track = SliderTrackInfo::new("test", 100.0, 200.0, 0.0, 100.0);

        // Click closer to min handle (at 25% = value 25)
        let handle = SliderInputHandler::determine_dual_handle(150.0, 20.0, 80.0, &track);
        assert_eq!(handle, DualSliderHandle::Min);

        // Click closer to max handle (at 75% = value 75)
        let handle = SliderInputHandler::determine_dual_handle(250.0, 20.0, 80.0, &track);
        assert_eq!(handle, DualSliderHandle::Max);
    }

    #[test]
    fn test_utility_functions() {
        // value_to_position
        assert_eq!(value_to_position(0.0, 0.0, 100.0, 100.0, 200.0), 100.0);
        assert_eq!(value_to_position(50.0, 0.0, 100.0, 100.0, 200.0), 200.0);
        assert_eq!(value_to_position(100.0, 0.0, 100.0, 100.0, 200.0), 300.0);

        // position_to_value
        assert_eq!(position_to_value(100.0, 100.0, 200.0, 0.0, 100.0), 0.0);
        assert_eq!(position_to_value(200.0, 100.0, 200.0, 0.0, 100.0), 50.0);
        assert_eq!(position_to_value(300.0, 100.0, 200.0, 0.0, 100.0), 100.0);
    }
}
