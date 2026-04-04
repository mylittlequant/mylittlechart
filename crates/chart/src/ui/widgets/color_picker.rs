//! Color picker popup widgets (L1 and L2)
//!
//! L1: Quick color palette with preset colors + opacity slider
//! L2: Full HSV color picker with hex input
//!
//! State types (`ColorPickerState`, `ColorPickerLevel`, `HsvColor`, etc.) live in
//! `crate::ui::color_picker_state`.

use crate::render::{RenderContext, TextAlign, TextBaseline};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::widgets::input::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::slider::{render_single_slider, SliderConfig};
use crate::ui::widgets::popup::{draw_popup, PopupConfig, PopupTheme};
use uzor::types::Rect as WidgetRect;

pub use crate::ui::color_picker_state::{
    STANDARD_PALETTE, MAX_CUSTOM_COLORS,
    ColorPickerL1Config, ColorPickerL2Config,
    ColorPickerL2Area, ColorPickerLevel, ColorPickerState,
    HsvColor, apply_opacity_to_hex, hsv_to_rgb, rgb_to_hsv,
};

// =============================================================================
// Theme Conversion
// =============================================================================

/// Convert PopupTheme to WidgetTheme for slider rendering
fn popup_to_widget_theme(theme: &PopupTheme) -> WidgetTheme {
    WidgetTheme {
        bg_normal: theme.background.clone(),
        bg_hover: theme.background.clone(),
        bg_pressed: theme.background.clone(),
        bg_disabled: theme.background.clone(),
        text_normal: "#d1d4dc".to_string(),
        text_hover: "#ffffff".to_string(),
        text_disabled: "#6a6d78".to_string(),
        border_normal: theme.border.clone(),
        border_hover: theme.border.clone(),
        border_focused: theme.active.clone(),
        accent: theme.active.clone(),
        accent_hover: theme.active.clone(),
        success: "#26a69a".to_string(),
        warning: "#ff9800".to_string(),
        danger: "#ef5350".to_string(),
    }
}

// =============================================================================
// L1 Color Picker Configuration
// =============================================================================
// NOTE: STANDARD_PALETTE, MAX_CUSTOM_COLORS, and ColorPickerL1Config are
// re-exported from crate::ui::color_picker_state above.

/// L1 Color picker result
#[derive(Clone, Debug, Default)]
pub struct ColorPickerL1Result {
    /// Popup rect
    pub popup_rect: WidgetRect,
    /// Selected color (if clicked)
    pub selected_color: Option<String>,
    /// Selected opacity (always returned)
    pub opacity: f64,
    /// Open L2 picker (if "+" clicked)
    pub open_l2: bool,
    /// Palette swatch rects for hit testing
    pub swatch_rects: Vec<(String, WidgetRect)>,
    /// Plus button rect
    pub plus_button_rect: Option<WidgetRect>,
    /// Opacity slider rect
    pub opacity_slider_rect: Option<WidgetRect>,
    /// Opacity toggle button rect (eye icon to toggle 0%/previous)
    pub opacity_toggle_rect: Option<WidgetRect>,
}

/// Draw L1 color picker popup (quick palette)
pub fn draw_color_picker_l1(
    ctx: &mut dyn RenderContext,
    config: &ColorPickerL1Config,
    origin: (f64, f64),
    theme: &PopupTheme,
    hovered_swatch: Option<&str>,
) -> ColorPickerL1Result {
    let (width, height) = config.calculate_size();
    let popup_config = PopupConfig::new(width, height).with_padding(8.0);

    let popup_result = draw_popup(ctx, &popup_config, origin, theme);
    let content = popup_result.content_rect;

    let mut result = ColorPickerL1Result {
        popup_rect: popup_result.popup_rect,
        opacity: config.opacity,
        ..Default::default()
    };

    let mut y = content.y;

    // Draw standard palette (10x10)
    for (idx, color) in STANDARD_PALETTE.iter().enumerate() {
        let row = idx / config.columns;
        let col = idx % config.columns;

        let x = content.x + col as f64 * (config.swatch_size + config.gap);
        let swatch_y = y + row as f64 * (config.swatch_size + config.gap);

        let swatch_rect = WidgetRect::new(x, swatch_y, config.swatch_size, config.swatch_size);

        let is_hovered = hovered_swatch == Some(*color);
        let is_selected = config.current_color.as_deref() == Some(*color);

        // Draw swatch
        ctx.set_fill_color(color);
        ctx.fill_rounded_rect(swatch_rect.x, swatch_rect.y, swatch_rect.width, swatch_rect.height, config.swatch_radius);

        // Draw selection/hover border
        if is_selected {
            ctx.set_stroke_color("#ffffff");
            ctx.set_stroke_width(2.0);
            ctx.stroke_rounded_rect(swatch_rect.x, swatch_rect.y, swatch_rect.width, swatch_rect.height, config.swatch_radius);
        } else if is_hovered {
            ctx.set_stroke_color("#ffffff");
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(swatch_rect.x, swatch_rect.y, swatch_rect.width, swatch_rect.height, config.swatch_radius);
        }

        result.swatch_rects.push((color.to_string(), swatch_rect));
    }

    y += 10.0 * (config.swatch_size + config.gap) + 8.0;

    // Draw custom colors row
    // Current color swatch (larger)
    let current_swatch_size = config.swatch_size + 4.0;
    if let Some(ref current) = config.current_color {
        ctx.set_fill_color(current);
        ctx.fill_rounded_rect(content.x, y, current_swatch_size, current_swatch_size, config.swatch_radius);
        ctx.set_stroke_color("#787b86");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(content.x, y, current_swatch_size, current_swatch_size, config.swatch_radius);
    } else {
        // Empty swatch placeholder
        ctx.set_fill_color("#2a2e39");
        ctx.fill_rounded_rect(content.x, y, current_swatch_size, current_swatch_size, config.swatch_radius);
        ctx.set_stroke_color("#787b86");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(content.x, y, current_swatch_size, current_swatch_size, config.swatch_radius);
    }

    // Plus button (opens L2)
    let plus_x = content.x + current_swatch_size + 8.0;
    let plus_rect = WidgetRect::new(plus_x, y, current_swatch_size, current_swatch_size);

    ctx.set_fill_color("#2a2e39");
    ctx.fill_rounded_rect(plus_rect.x, plus_rect.y, plus_rect.width, plus_rect.height, config.swatch_radius);
    ctx.set_stroke_color("#787b86");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(plus_rect.x, plus_rect.y, plus_rect.width, plus_rect.height, config.swatch_radius);

    // Draw "+"
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color("#d1d4dc");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("+", plus_rect.center_x(), plus_rect.center_y());

    result.plus_button_rect = Some(plus_rect);

    y += current_swatch_size + 12.0;

    // Draw opacity section
    // Toggle button (eye icon) - BELOW "Opacity" label, larger size
    let toggle_size = 24.0;
    let toggle_rect = WidgetRect::new(content.x, y, toggle_size, toggle_size);

    // Draw toggle button background
    let toggle_bg = if config.is_opacity_toggled_off { "#1e222d" } else { "#2a2e39" };
    ctx.set_fill_color(toggle_bg);
    ctx.fill_rounded_rect(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height, 4.0);
    ctx.set_stroke_color("#363a45");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height, 4.0);

    // Draw eye icon (open or crossed out depending on toggle state)
    let eye_cx = toggle_rect.center_x();
    let eye_cy = toggle_rect.center_y();

    // Eye color: dimmed if toggled off
    let eye_color = if config.is_opacity_toggled_off { "#555555" } else { "#d1d4dc" };
    ctx.set_stroke_color(eye_color);
    ctx.set_stroke_width(1.5);

    // Draw eye outline (ellipse) - larger
    ctx.begin_path();
    ctx.ellipse(eye_cx, eye_cy, 7.0, 4.5, 0.0, 0.0, std::f64::consts::TAU);
    ctx.stroke();

    // Draw pupil (circle) - larger
    ctx.set_fill_color(eye_color);
    ctx.begin_path();
    ctx.arc(eye_cx, eye_cy, 2.0, 0.0, std::f64::consts::TAU);
    ctx.fill();

    // If toggled off, draw a diagonal line through the eye
    if config.is_opacity_toggled_off {
        ctx.set_stroke_color("#787b86");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(eye_cx - 7.0, eye_cy + 5.0);
        ctx.line_to(eye_cx + 7.0, eye_cy - 5.0);
        ctx.stroke();
    }

    result.opacity_toggle_rect = Some(toggle_rect);

    // "Opacity" label - to the right of toggle button (will be drawn by slider)
    // Opacity slider - positioned to the right of toggle
    let slider_x = content.x + toggle_size + 8.0;
    let slider_width = content.width - toggle_size - 8.0 - 40.0; // Reserve space for percentage text
    let slider_rect = WidgetRect::new(slider_x, y, slider_width, toggle_size);

    // Create slider config
    let slider_config = SliderConfig::new(0.0, 1.0)
        .with_step(0.01)
        .without_input();

    // Convert theme
    let widget_theme = popup_to_widget_theme(theme);

    // Render slider (no label — the eye icon to the left already provides visual context)
    let slider_result = render_single_slider(
        ctx,
        &slider_config,
        config.opacity,
        slider_rect,
        "",
        &widget_theme,
        false, // hovered state should be managed externally
        None,  // not editing
    );

    result.opacity_slider_rect = Some(slider_result.track_rect);

    // Opacity percentage
    let percent = (config.opacity * 100.0) as i32;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("#d1d4dc");
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&format!("{}%", percent), content.x + content.width, y + toggle_size / 2.0);

    result
}

/// Hit test for L1 color picker
/// Returns the color string if a swatch was clicked
pub fn color_picker_l1_hit_test(
    result: &ColorPickerL1Result,
    x: f64,
    y: f64,
) -> ColorPickerL1HitResult {
    // Check plus button
    if let Some(ref plus_rect) = result.plus_button_rect {
        if plus_rect.contains(x, y) {
            return ColorPickerL1HitResult::PlusButton;
        }
    }

    // Check opacity toggle button (before slider so it takes priority)
    if let Some(ref toggle_rect) = result.opacity_toggle_rect {
        if toggle_rect.contains(x, y) {
            return ColorPickerL1HitResult::OpacityToggle;
        }
    }

    // Check opacity slider
    if let Some(ref slider_rect) = result.opacity_slider_rect {
        if slider_rect.contains(x, y) {
            let relative_x = (x - slider_rect.x) / slider_rect.width;
            return ColorPickerL1HitResult::OpacitySlider(relative_x.clamp(0.0, 1.0));
        }
    }

    // Check swatches
    for (color, rect) in &result.swatch_rects {
        if rect.contains(x, y) {
            return ColorPickerL1HitResult::Color(color.clone());
        }
    }

    // Check if inside popup but nothing specific
    if result.popup_rect.contains(x, y) {
        return ColorPickerL1HitResult::Inside;
    }

    ColorPickerL1HitResult::Outside
}

/// Hit result for L1 color picker
#[derive(Clone, Debug)]
pub enum ColorPickerL1HitResult {
    /// Clicked a color swatch
    Color(String),
    /// Clicked the "+" button to open L2
    PlusButton,
    /// Clicked/dragged on opacity slider (value 0.0-1.0)
    OpacitySlider(f64),
    /// Clicked the opacity toggle button (eye icon)
    OpacityToggle,
    /// Clicked inside popup but not on anything interactive
    Inside,
    /// Clicked outside popup
    Outside,
}

// =============================================================================
// L2 Color Picker (HSV + Hex)
// =============================================================================
// NOTE: HsvColor, apply_opacity_to_hex, hsv_to_rgb, rgb_to_hsv, and
// ColorPickerL2Config are re-exported from crate::ui::color_picker_state above.

/// L2 Color picker result
#[derive(Clone, Debug, Default)]
pub struct ColorPickerL2Result {
    /// Popup rect
    pub popup_rect: WidgetRect,
    /// SV square rect
    pub sv_square_rect: WidgetRect,
    /// Hue bar rect
    pub hue_bar_rect: WidgetRect,
    /// Hex input rect
    pub hex_input_rect: WidgetRect,
    /// Hex cursor X position (for blinking cursor rendering)
    pub hex_cursor_x: f64,
    /// Hex cursor Y position
    pub hex_cursor_y: f64,
    /// Hex cursor height
    pub hex_cursor_height: f64,
    /// Character boundary X positions from `draw_input` — used by `TextInputManager::update_field`.
    /// Contains `char_count + 1` entries (left edge of each char plus the right edge of the last).
    pub hex_char_positions: Vec<f64>,
    /// Opacity slider rect
    pub opacity_slider_rect: WidgetRect,
    /// Opacity toggle button rect (eye icon to toggle 0%/previous)
    pub opacity_toggle_rect: WidgetRect,
    /// Add button rect
    pub add_button_rect: WidgetRect,
    /// Back button rect (return to L1)
    pub back_button_rect: WidgetRect,
    /// Current color hex
    pub current_hex: String,
    /// Current opacity
    pub opacity: f64,
}

/// Draw L2 color picker popup (full HSV picker)
pub fn draw_color_picker_l2(
    ctx: &mut dyn RenderContext,
    config: &ColorPickerL2Config,
    origin: (f64, f64),
    theme: &PopupTheme,
    hovered_area: Option<ColorPickerL2Area>,
) -> ColorPickerL2Result {
    let (width, height) = config.calculate_size();
    let popup_config = PopupConfig::new(width, height).with_padding(12.0);

    let popup_result = draw_popup(ctx, &popup_config, origin, theme);
    let content = popup_result.content_rect;

    let mut result = ColorPickerL2Result {
        popup_rect: popup_result.popup_rect,
        current_hex: config.hsv.to_hex(),
        opacity: config.opacity,
        ..Default::default()
    };

    let mut y = content.y;

    // ==========================================================================
    // SV Square (Saturation on X, Value on Y)
    // ==========================================================================
    let sv_rect = WidgetRect::new(content.x, y, config.sv_square_size, config.sv_square_size);
    result.sv_square_rect = sv_rect;

    // Draw SV gradient
    // Base color at full saturation/value for current hue
    let base_color = HsvColor::new(config.hsv.h, 1.0, 1.0).to_hex();

    // White to base color horizontal gradient (saturation)
    // Then black overlay from bottom (value)

    // Draw base color
    ctx.set_fill_color(&base_color);
    ctx.fill_rect(sv_rect.x, sv_rect.y, sv_rect.width, sv_rect.height);

    // Draw white-to-transparent gradient (saturation)
    draw_horizontal_gradient(ctx, sv_rect.x, sv_rect.y, sv_rect.width, sv_rect.height,
        "rgba(255,255,255,1)", "rgba(255,255,255,0)");

    // Draw transparent-to-black gradient (value)
    draw_vertical_gradient(ctx, sv_rect.x, sv_rect.y, sv_rect.width, sv_rect.height,
        "rgba(0,0,0,0)", "rgba(0,0,0,1)");

    // Draw SV cursor
    let cursor_x = sv_rect.x + config.hsv.s * sv_rect.width;
    let cursor_y = sv_rect.y + (1.0 - config.hsv.v) * sv_rect.height;
    draw_picker_cursor(ctx, cursor_x, cursor_y, 6.0);

    // ==========================================================================
    // Hue Bar (vertical rainbow)
    // ==========================================================================
    let hue_rect = WidgetRect::new(
        content.x + config.sv_square_size + config.gap,
        y,
        config.hue_bar_width,
        config.sv_square_size,
    );
    result.hue_bar_rect = hue_rect;

    // Draw hue gradient (vertical rainbow)
    draw_hue_bar(ctx, hue_rect.x, hue_rect.y, hue_rect.width, hue_rect.height);

    // Draw hue cursor
    let hue_cursor_y = hue_rect.y + (config.hsv.h / 360.0) * hue_rect.height;
    ctx.set_fill_color("#ffffff");
    ctx.fill_rect(hue_rect.x - 2.0, hue_cursor_y - 2.0, hue_rect.width + 4.0, 4.0);
    ctx.set_stroke_color("#000000");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(hue_rect.x - 2.0, hue_cursor_y - 2.0, hue_rect.width + 4.0, 4.0);

    y += config.sv_square_size + config.gap;

    // ==========================================================================
    // Hex Input Row
    // ==========================================================================
    let hex_row_height = 32.0;

    // Color preview swatch
    let preview_size = 28.0;
    ctx.set_fill_color(&config.hsv.to_hex());
    ctx.fill_rounded_rect(content.x, y + 2.0, preview_size, preview_size, 4.0);
    ctx.set_stroke_color("#787b86");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(content.x, y + 2.0, preview_size, preview_size, 4.0);

    // Hex input field
    let hex_input_x = content.x + preview_size + 8.0;
    let hex_input_width = content.width - preview_size - 8.0;
    let hex_input_rect = WidgetRect::new(hex_input_x, y + 2.0, hex_input_width, 28.0);
    result.hex_input_rect = hex_input_rect;

    let hex_widget_theme = WidgetTheme {
        bg_normal: "#1e222d".to_string(),
        bg_hover: "#2a2e39".to_string(),
        bg_pressed: "#2a2e39".to_string(),
        bg_disabled: "#1e222d".to_string(),
        text_normal: "#d1d4dc".to_string(),
        text_hover: "#d1d4dc".to_string(),
        text_disabled: "#6a6d78".to_string(),
        border_normal: "#363a45".to_string(),
        border_hover: "#363a45".to_string(),
        border_focused: "#2196f3".to_string(),
        accent: "#2196f3".to_string(),
        accent_hover: "#42a5f5".to_string(),
        success: "#26a69a".to_string(),
        warning: "#ff9800".to_string(),
        danger: "#ef5350".to_string(),
    };

    let hex_input_config = InputConfig {
        value: config.hex_input.clone(),
        placeholder: "#000000".to_string(),
        focused: config.hex_editing,
        cursor: config.hex_cursor,
        font_size: 13.0,
        padding: 8.0,
        radius: 4.0,
        ..InputConfig::default()
    };

    let hex_input_result = draw_input(ctx, &hex_input_config, WidgetState::Normal, hex_input_rect, &hex_widget_theme);
    result.hex_cursor_x = hex_input_result.cursor_x;
    result.hex_cursor_y = hex_input_result.cursor_y;
    result.hex_cursor_height = hex_input_result.cursor_height;
    result.hex_char_positions = hex_input_result.char_x_positions;

    if config.hex_editing {
        draw_input_cursor(ctx, hex_input_result.cursor_x, hex_input_result.cursor_y, hex_input_result.cursor_height, "#d1d4dc");
    }

    y += hex_row_height + config.gap;

    // ==========================================================================
    // Opacity Row: Toggle button | "Opacity" label | slider | percentage
    // ==========================================================================
    let toggle_size = 24.0;
    let toggle_rect = WidgetRect::new(content.x, y, toggle_size, toggle_size);

    // Draw toggle button background
    let toggle_bg = if config.is_opacity_toggled_off { "#1e222d" } else { "#2a2e39" };
    ctx.set_fill_color(toggle_bg);
    ctx.fill_rounded_rect(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height, 4.0);
    ctx.set_stroke_color("#363a45");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height, 4.0);

    // Draw eye icon (open or crossed out depending on toggle state)
    let eye_cx = toggle_rect.center_x();
    let eye_cy = toggle_rect.center_y();

    // Eye color: dimmed if toggled off
    let eye_color = if config.is_opacity_toggled_off { "#555555" } else { "#d1d4dc" };
    ctx.set_stroke_color(eye_color);
    ctx.set_stroke_width(1.5);

    // Draw eye outline (ellipse) - larger
    ctx.begin_path();
    ctx.ellipse(eye_cx, eye_cy, 7.0, 4.5, 0.0, 0.0, std::f64::consts::TAU);
    ctx.stroke();

    // Draw pupil (circle) - larger
    ctx.set_fill_color(eye_color);
    ctx.begin_path();
    ctx.arc(eye_cx, eye_cy, 2.0, 0.0, std::f64::consts::TAU);
    ctx.fill();

    // If toggled off, draw a diagonal line through the eye
    if config.is_opacity_toggled_off {
        ctx.set_stroke_color("#787b86");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(eye_cx - 7.0, eye_cy + 5.0);
        ctx.line_to(eye_cx + 7.0, eye_cy - 5.0);
        ctx.stroke();
    }

    result.opacity_toggle_rect = toggle_rect;

    // "Opacity" label - to the right of toggle button (will be drawn by slider)
    // Opacity slider - positioned to the right of toggle
    let slider_x = content.x + toggle_size + 8.0;
    let slider_width = content.width - toggle_size - 8.0 - 40.0; // Reserve space for percentage text
    let slider_rect_full = WidgetRect::new(slider_x, y, slider_width, toggle_size);

    // Create slider config
    let l2_slider_config = SliderConfig::new(0.0, 1.0)
        .with_step(0.01)
        .without_input();

    // Convert theme
    let l2_widget_theme = popup_to_widget_theme(theme);

    // Render slider (no label — the eye icon to the left already provides visual context)
    let l2_slider_result = render_single_slider(
        ctx,
        &l2_slider_config,
        config.opacity,
        slider_rect_full,
        "",
        &l2_widget_theme,
        false, // hovered state should be managed externally
        None,  // not editing
    );

    result.opacity_slider_rect = l2_slider_result.track_rect;

    // Percentage
    let percent = (config.opacity * 100.0) as i32;
    ctx.set_fill_color("#d1d4dc");
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&format!("{}%", percent), content.x + content.width, y + toggle_size / 2.0);

    y += toggle_size + config.gap;

    // ==========================================================================
    // Button Row (Back + Add)
    // ==========================================================================
    let button_width = (content.width - 8.0) / 2.0;
    let button_height = 28.0;

    // Back button
    let back_rect = WidgetRect::new(content.x, y, button_width, button_height);
    result.back_button_rect = back_rect;

    let back_hovered = matches!(hovered_area, Some(ColorPickerL2Area::BackButton));
    ctx.set_fill_color(if back_hovered { "#363a45" } else { "#2a2e39" });
    ctx.fill_rounded_rect(back_rect.x, back_rect.y, back_rect.width, back_rect.height, 4.0);
    ctx.set_stroke_color("#787b86");
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(back_rect.x, back_rect.y, back_rect.width, back_rect.height, 4.0);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("#d1d4dc");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Back", back_rect.center_x(), back_rect.center_y());

    // Add button
    let add_rect = WidgetRect::new(content.x + button_width + 8.0, y, button_width, button_height);
    result.add_button_rect = add_rect;

    let add_hovered = matches!(hovered_area, Some(ColorPickerL2Area::AddButton));
    ctx.set_fill_color(if add_hovered { "#1976d2" } else { "#2196f3" });
    ctx.fill_rounded_rect(add_rect.x, add_rect.y, add_rect.width, add_rect.height, 4.0);

    ctx.set_fill_color("#ffffff");
    ctx.fill_text("Add", add_rect.center_x(), add_rect.center_y());

    result
}

/// Draw horizontal gradient (for SV square saturation)
fn draw_horizontal_gradient(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, _from: &str, _to: &str) {
    // Approximate gradient with vertical strips
    let steps = 20;
    let step_width = w / steps as f64;

    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let alpha = (1.0 - t) * 255.0;
        let color = format!("rgba(255,255,255,{})", alpha / 255.0);
        ctx.set_fill_color(&color);
        ctx.fill_rect(x + i as f64 * step_width, y, step_width + 1.0, h);
    }
}

/// Draw vertical gradient (for SV square value)
fn draw_vertical_gradient(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, _from: &str, _to: &str) {
    // Approximate gradient with horizontal strips
    let steps = 20;
    let step_height = h / steps as f64;

    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let alpha = t;
        let color = format!("rgba(0,0,0,{})", alpha);
        ctx.set_fill_color(&color);
        ctx.fill_rect(x, y + i as f64 * step_height, w, step_height + 1.0);
    }
}

/// Draw vertical hue bar (rainbow spectrum)
fn draw_hue_bar(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64) {
    let steps = 36; // Every 10 degrees
    let step_height = h / steps as f64;

    for i in 0..steps {
        let hue = (i as f64 / steps as f64) * 360.0;
        let color = HsvColor::new(hue, 1.0, 1.0).to_hex();
        ctx.set_fill_color(&color);
        ctx.fill_rect(x, y + i as f64 * step_height, w, step_height + 1.0);
    }
}

/// Draw circular cursor for SV picker
fn draw_picker_cursor(ctx: &mut dyn RenderContext, x: f64, y: f64, radius: f64) {
    // White outer ring
    ctx.set_stroke_color("#ffffff");
    ctx.set_stroke_width(2.0);
    ctx.begin_path();
    ctx.arc(x, y, radius, 0.0, std::f64::consts::TAU);
    ctx.stroke();

    // Black inner ring
    ctx.set_stroke_color("#000000");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.arc(x, y, radius - 1.0, 0.0, std::f64::consts::TAU);
    ctx.stroke();
}

// NOTE: ColorPickerL2Area is re-exported from crate::ui::color_picker_state above.

/// Hit test for L2 color picker
pub fn color_picker_l2_hit_test(
    result: &ColorPickerL2Result,
    x: f64,
    y: f64,
) -> ColorPickerL2HitResult {
    // Check SV square
    if result.sv_square_rect.contains(x, y) {
        let s = ((x - result.sv_square_rect.x) / result.sv_square_rect.width).clamp(0.0, 1.0);
        let v = 1.0 - ((y - result.sv_square_rect.y) / result.sv_square_rect.height).clamp(0.0, 1.0);
        return ColorPickerL2HitResult::SVSquare(s, v);
    }

    // Check hue bar
    if result.hue_bar_rect.contains(x, y) {
        let h = ((y - result.hue_bar_rect.y) / result.hue_bar_rect.height).clamp(0.0, 1.0) * 360.0;
        return ColorPickerL2HitResult::HueBar(h);
    }

    // Check hex input
    if result.hex_input_rect.contains(x, y) {
        return ColorPickerL2HitResult::HexInput;
    }

    // Check opacity toggle button (before slider so it takes priority)
    if result.opacity_toggle_rect.contains(x, y) {
        return ColorPickerL2HitResult::OpacityToggle;
    }

    // Check opacity slider
    if result.opacity_slider_rect.contains(x, y) {
        let opacity = ((x - result.opacity_slider_rect.x) / result.opacity_slider_rect.width).clamp(0.0, 1.0);
        return ColorPickerL2HitResult::OpacitySlider(opacity);
    }

    // Check add button
    if result.add_button_rect.contains(x, y) {
        return ColorPickerL2HitResult::AddButton;
    }

    // Check back button
    if result.back_button_rect.contains(x, y) {
        return ColorPickerL2HitResult::BackButton;
    }

    // Check if inside popup
    if result.popup_rect.contains(x, y) {
        return ColorPickerL2HitResult::Inside;
    }

    ColorPickerL2HitResult::Outside
}

/// Hit result for L2 color picker
#[derive(Clone, Debug)]
pub enum ColorPickerL2HitResult {
    /// Clicked/dragged on SV square (saturation, value)
    SVSquare(f64, f64),
    /// Clicked/dragged on hue bar (hue 0-360)
    HueBar(f64),
    /// Clicked on hex input
    HexInput,
    /// Clicked/dragged on opacity slider (value 0.0-1.0)
    OpacitySlider(f64),
    /// Clicked the opacity toggle button (eye icon)
    OpacityToggle,
    /// Clicked add button
    AddButton,
    /// Clicked back button (return to L1)
    BackButton,
    /// Clicked inside popup but not on anything interactive
    Inside,
    /// Clicked outside popup
    Outside,
}
