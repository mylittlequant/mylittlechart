//! Primitive settings modal renderer.

use crate::engine::render::RenderContext;
use crate::ui::toolbar_render::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::{PrimitiveSettingsState, PrimitiveSettingsTab, DualSliderHandle};
use crate::drawing::{DrawingManager, LineStyle};
use crate::drawing::primitives_v2::config::PropertyType;
use crate::layout::render_frame::{
    PrimitiveSettingsResult,
    SliderTrackInfo,
};
use crate::layout::toolbar_state::ToolbarState;
use crate::layout::render_chart::FrameTheme;
use crate::ui::widgets::{SliderConfig, render_single_slider, render_dual_slider, SliderEditingInfo};
use crate::ui::widgets::types::WidgetState;
use crate::ui::Icon;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::dropdown::{render_dropdown, DropdownConfig, DropdownItem, DropdownTheme};

fn render_level_item(
    ctx: &mut dyn RenderContext,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    prim_data: &crate::drawing::PrimitiveData,
    result: &mut PrimitiveSettingsResult,
    editing_text: Option<&crate::ui::modal_settings::TextEditingState>,
    x: f64,
    y: f64,
    row_height: f64,
    level_idx: usize,
    config: &crate::drawing::FibLevelConfig,
) {
    let field_id = format!("level_{}_value", level_idx);
    let level_label = format!("{:.3}", config.level);

    // Check if this field is being edited
    let is_editing = editing_text
        .map(|e| e.field_id == field_id)
        .unwrap_or(false);

    // Visibility checkbox - clickable
    let check_rect = WidgetRect::new(x, y + 4.0, 16.0, 16.0);
    ctx.set_fill_color(if config.visible { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
    ctx.fill_rounded_rect(check_rect.x, check_rect.y, check_rect.width, check_rect.height, 2.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.stroke_rounded_rect(check_rect.x, check_rect.y, check_rect.width, check_rect.height, 2.0);
    if config.visible {
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(check_rect.x + 3.0, check_rect.center_y());
        ctx.line_to(check_rect.x + 6.0, check_rect.bottom() - 3.0);
        ctx.line_to(check_rect.right() - 3.0, check_rect.y + 3.0);
        ctx.stroke();
    }
    result.content_items.push((format!("level_{}_visible", level_idx), check_rect));

    // Level value input - editable field
    let value_rect = WidgetRect::new(x + 24.0, y + 2.0, 55.0, row_height - 4.0);

    // Get display text and cursor/selection from editing state
    let (display_text, cursor_pos, selection_start, selection_end): (&str, usize, Option<usize>, Option<usize>) = if let Some(edit) = editing_text {
        if edit.field_id == field_id {
            (&edit.text as &str, edit.cursor, edit.selection_start, Some(edit.cursor))
        } else {
            (&level_label as &str, level_label.len(), None, None)
        }
    } else {
        (&level_label as &str, level_label.len(), None, None)
    };

    // Draw input using centralized system
    let level_input_config = InputConfig::new(display_text)
        .with_focused(is_editing)
        .with_type(InputType::Number)
        .with_font_size(12.0)
        .with_padding(4.0)
        .with_radius(3.0)
        .with_cursor(cursor_pos)
        .with_selection(selection_start, selection_end);

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let widget_state = WidgetState::Normal;
    let level_input_result = draw_input(ctx, &level_input_config, widget_state, value_rect, &widget_theme);

    // Draw cursor if editing
    if is_editing {
        draw_input_cursor(
            ctx,
            level_input_result.cursor_x,
            level_input_result.cursor_y,
            level_input_result.cursor_height,
            &toolbar_theme.item_text,
        );
        // Expose char positions for drag-to-select
        result.active_input_char_positions = level_input_result.char_x_positions;
        result.active_input_rect = Some(value_rect);
    }

    result.content_items.push((field_id, value_rect));

    // Color button - clickable (inline draw_color_button logic)
    let level_color = config.color.as_deref().unwrap_or(&prim_data.color.stroke);

    let color_rect = WidgetRect::new(x + 85.0, y + 4.0, 16.0, row_height - 8.0);

    // Draw color swatch
    ctx.set_fill_color(level_color);
    ctx.fill_rounded_rect(color_rect.x, color_rect.y, color_rect.width, color_rect.height, 3.0);
    // Draw border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(color_rect.x, color_rect.y, color_rect.width, color_rect.height, 3.0);
    result.content_items.push((format!("level_{}_color", level_idx), color_rect));

    // Fill toggle button - shows fill color swatch with opacity indicator
    let fill_color = config.fill_color.as_deref()
        .or(config.color.as_deref())
        .unwrap_or(&prim_data.color.stroke);

    let fill_rect = WidgetRect::new(x + 103.0, y + 4.0, 16.0, row_height - 8.0);

    // Draw fill swatch background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rounded_rect(fill_rect.x, fill_rect.y, fill_rect.width, fill_rect.height, 3.0);

    if config.fill_enabled {
        // Draw fill color with opacity
        ctx.set_fill_color_alpha(fill_color, config.fill_opacity);
        ctx.fill_rounded_rect(fill_rect.x, fill_rect.y, fill_rect.width, fill_rect.height, 3.0);
        ctx.reset_alpha();
    }

    // Draw border (highlighted if fill enabled)
    ctx.set_stroke_color(if config.fill_enabled { &toolbar_theme.item_bg_active } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(fill_rect.x, fill_rect.y, fill_rect.width, fill_rect.height, 3.0);

    // Draw diagonal line through if fill disabled (visual indicator)
    if !config.fill_enabled {
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(fill_rect.x + 2.0, fill_rect.bottom() - 2.0);
        ctx.line_to(fill_rect.right() - 2.0, fill_rect.y + 2.0);
        ctx.stroke();
    }

    result.content_items.push((format!("level_{}_fill", level_idx), fill_rect));
}

/// Render primitive settings modal.
///
/// This is a floating, non-overlay modal that appears near the primitive.
/// No background dimming - user can still interact with chart.
///
/// `screen_w` and `screen_h` are the total available screen dimensions
/// (used for centering and clamping the modal position).
/// `default_modal_y` is the Y offset from the top for default positioning.
pub fn render_primitive_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &PrimitiveSettingsState,
    drawing_manager: &DrawingManager,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    _toolbar_state: &ToolbarState,
    _current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[crate::templates::PrimitiveTemplate],
) -> PrimitiveSettingsResult {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;
    use crate::drawing::primitives_v2::PrimitiveRegistry;

    let mut result = PrimitiveSettingsResult::default();
    // Deferred line style dropdown — rendered after footer to avoid z-overlap
    let mut deferred_line_style_dropdown: Option<(f64, f64, f64, String)> = None;
    // Deferred Select property dropdown — rendered after footer to avoid z-overlap
    // Fields: (x, y, width, id_prefix, options: Vec<(value, label)>)
    let mut deferred_select_dropdown: Option<(f64, f64, f64, String, Vec<(String, String)>)> = None;

    // Get primitive info
    let Some(idx) = state.primitive_idx else {
        return result;
    };
    let Some(prim) = drawing_manager.primitive(idx) else {
        return result;
    };

    // Push modal layer
    use crate::ui::z_order::ZLayer;
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "primitive_settings");

    let prim_name = prim.display_name();
    let prim_type_id = prim.type_id();

    // Get capabilities from registry
    let (supports_text, has_levels) = {
        let registry = PrimitiveRegistry::global().read().unwrap();
        (registry.supports_text(prim_type_id), registry.has_levels(prim_type_id))
    };

    // Get available tabs for this primitive
    let tabs = state.available_tabs(supports_text, has_levels);

    // Get primitive data for content rendering
    let prim_data = prim.data().clone();

    // Layout constants
    let header_height = 36.0;
    let tab_height = 32.0;
    let tab_padding_h = 12.0;
    let tab_gap = 2.0;
    let modal_padding = 12.0;
    let close_btn_size = 20.0;
    let title_close_gap = 12.0;
    let row_height = 28.0;
    let row_gap = 8.0;
    let label_width = 170.0;

    // Get style_properties for current primitive
    let style_props = prim.style_properties();
    // Get level_properties for current primitive
    let level_props = prim.level_properties();
    // Get text_properties for current primitive
    let text_props = prim.text_properties();

    // Calculate content height based on active tab
    let content_height = (match state.active_tab {
        PrimitiveSettingsTab::Style => {
            let base_rows = if prim_data.color.fill.is_some() { 4 } else { 3 };
            let extra_rows = style_props.len();
            (base_rows + extra_rows) as f64 * (row_height + row_gap) + modal_padding * 2.0
        }
        PrimitiveSettingsTab::Text => {
            if let Some(ref props) = text_props {
                props.len() as f64 * (row_height + row_gap) + modal_padding * 2.0
            } else {
                4.0 * (row_height + row_gap) + 120.0 + modal_padding * 2.0
            }
        }
        PrimitiveSettingsTab::Coordinates => {
            let points = prim.points();
            points.len().max(1) as f64 * (row_height + row_gap) + modal_padding * 2.0
        }
        PrimitiveSettingsTab::Levels => {
            let level_props_rows = level_props.len();
            let level_configs_rows = if let Some(configs) = prim.level_configs() {
                configs.len().div_ceil(3).max(1) as f64 * (row_height + row_gap)
            } else {
                0.0
            };
            (level_props_rows as f64 * (row_height + row_gap)) + level_configs_rows + modal_padding * 2.0
        }
        PrimitiveSettingsTab::Visibility => {
            9.0 * (row_height + row_gap) + modal_padding * 2.0
        }
    }).clamp(150.0, 400.0);

    // Calculate tab widths dynamically
    ctx.set_font("13px sans-serif");
    let mut total_tabs_width = 0.0;
    let mut tab_widths: Vec<f64> = Vec::new();
    for tab in &tabs {
        let lw = ctx.measure_text(tab.label());
        let tab_width = lw + tab_padding_h * 2.0;
        tab_widths.push(tab_width);
        total_tabs_width += tab_width;
    }
    total_tabs_width += tab_gap * (tabs.len().saturating_sub(1)) as f64;

    // Calculate title width
    ctx.set_font("13px sans-serif");
    let title_text = prim_name.to_string();
    let title_width = ctx.measure_text(&title_text);

    // Modal width
    let min_header_width = title_width + title_close_gap + close_btn_size + modal_padding * 2.0;
    let tabs_row_width = total_tabs_width + modal_padding * 2.0;
    let min_content_width = label_width + 150.0 + modal_padding * 2.0;
    let modal_width = min_header_width.max(tabs_row_width).max(min_content_width).max(400.0);
    let template_footer_height = 52.0;
    let modal_height = header_height + tab_height + content_height + template_footer_height;

    // Position
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        let x = (screen_w - modal_width) / 2.0;
        let y = default_modal_y;
        (x, y)
    });

    // Clamp to screen bounds
    let modal_x = modal_x.max(0.0).min(screen_w - modal_width);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_height);

    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);

    // Register modal background as catch-all
    input_coordinator.register_on_layer(
        "prim_settings:modal_bg",
        uzor::types::Rect::new(result.modal_rect.x, result.modal_rect.y, result.modal_rect.width, result.modal_rect.height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Render modal frame using centralized helper
    use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, result.modal_rect, &modal_theme, 0.0);

    // === HEADER ===
    let header_rect = WidgetRect::new(modal_x, modal_y, modal_width, header_height);
    result.header_rect = header_rect;

    // Title (left aligned)
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        &title_text,
        modal_x + modal_padding,
        modal_y + header_height / 2.0,
    );

    // Close button (X) - right aligned
    let close_rect = WidgetRect::new(
        modal_x + modal_width - modal_padding - close_btn_size,
        modal_y + (header_height - close_btn_size) / 2.0,
        close_btn_size,
        close_btn_size,
    );
    result.close_btn_rect = close_rect;

    // Register close button with coordinator
    input_coordinator.register_on_layer(
        "prim_settings:close",
        uzor::types::Rect::new(close_rect.x, close_rect.y, close_rect.width, close_rect.height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Draw close button icon
    draw_svg_icon(
        ctx,
        Icon::Close.svg(),
        close_rect.x + 2.0,
        close_rect.y + 2.0,
        close_btn_size - 4.0,
        close_btn_size - 4.0,
        &toolbar_theme.item_text
    );

    // Header bottom border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_height);
    ctx.line_to(modal_x + modal_width, modal_y + header_height);
    ctx.stroke();

    // === TABS ===
    let tab_y = modal_y + header_height;
    let mut tab_x = modal_x + modal_padding;

    for (i, tab) in tabs.iter().enumerate() {
        let tab_width = tab_widths[i];
        let tab_rect = WidgetRect::new(tab_x, tab_y, tab_width, tab_height);
        let is_active = state.active_tab == *tab;

        let tab_id = tab.id().to_string();

        // Tab background
        if is_active {
            ctx.draw_active_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height, &toolbar_theme.item_bg_active);
        }

        // Tab text
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(if is_active { "#ffffff" } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(tab.label(), tab_rect.x + tab_rect.width / 2.0, tab_rect.y + tab_rect.height / 2.0);

        result.tab_rects.push((tab_id.clone(), tab_rect));

        // Register tab with coordinator
        input_coordinator.register_on_layer(
            format!("prim_settings:tab:{}", tab_id),
            uzor::types::Rect::new(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );

        tab_x += tab_width + tab_gap;
    }

    // Tab bar bottom border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, tab_y + tab_height);
    ctx.line_to(modal_x + modal_width, tab_y + tab_height);
    ctx.stroke();

    // === CONTENT AREA ===
    let content_y = tab_y + tab_height;
    let content_rect = WidgetRect::new(modal_x, content_y, modal_width, content_height);
    result.content_rect = content_rect;

    // Render tab content
    let content_left = modal_x + modal_padding;
    let control_left = content_left + label_width + 8.0;
    let control_width = modal_width - modal_padding * 2.0 - label_width - 8.0;
    let mut row_y = content_y + modal_padding;

    // Helper closure to draw a label
    let draw_label = |ctx: &mut dyn RenderContext, label: &str, y: f64| {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, content_left, y + row_height / 2.0);
    };

    // Helper closure to draw a color button
    let draw_color_button = |ctx: &mut dyn RenderContext, color: &str, x: f64, y: f64, w: f64, h: f64| {
        ctx.set_fill_color(color);
        ctx.fill_rounded_rect(x, y, w, h, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, y, w, h, 4.0);
    };

    match state.active_tab {
        PrimitiveSettingsTab::Style => {
            // === STYLE TAB ===

            // Row 1: Stroke Color
            draw_label(ctx, "Цвет линии:", row_y);
            let color_btn_rect = WidgetRect::new(control_left, row_y + 2.0, 60.0, row_height - 4.0);
            draw_color_button(ctx, &prim_data.color.stroke, color_btn_rect.x, color_btn_rect.y, color_btn_rect.width, color_btn_rect.height);
            result.content_items.push(("stroke_color".to_string(), color_btn_rect));
            row_y += row_height + row_gap;

            // Row 2: Stroke Width (using centralized slider component)
            let slider_config = SliderConfig::new(1.0, 10.0)
                .with_step(0.1);

            let slider_width = 280.0;
            let slider_rect = WidgetRect::new(content_left, row_y, slider_width, row_height);
            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

            let hovered = state.slider_drag.as_ref()
                .map(|d| d.field_id == "stroke_width")
                .unwrap_or(false);

            // During drag, show the floating (preview) value instead of the stored value.
            let stroke_width_display = state.slider_drag.as_ref()
                .filter(|d| d.field_id == "stroke_width")
                .and_then(|d| d.floating_value)
                .unwrap_or(prim_data.width);

            // Build editing info if the user is actively typing into the value field.
            let stroke_width_editing = state.editing_text.as_ref()
                .filter(|e| e.field_id == "stroke_width_value")
                .map(|e| SliderEditingInfo {
                    text: &e.text,
                    cursor: e.cursor,
                    selection_start: e.selection_start,
                });

            let slider_result = render_single_slider(
                ctx,
                &slider_config,
                stroke_width_display,
                slider_rect,
                "Толщина:",
                &widget_theme,
                hovered,
                stroke_width_editing,
            );

            result.content_items.push(("stroke_width".to_string(), slider_result.full_rect));
            if let Some(input_rect) = slider_result.input_rect {
                result.content_items.push(("stroke_width_value".to_string(), input_rect));

                // Expose cursor and char positions so click-to-cursor and drag-to-select work.
                if state.editing_text.as_ref().map(|e| e.field_id == "stroke_width_value").unwrap_or(false) {
                    result.active_input_rect = Some(input_rect);
                    if let Some(ref ir) = slider_result.input_result {
                        result.active_input_char_positions = ir.char_x_positions.clone();
                    }
                }
            }

            if let Some(widget_track_info) = slider_result.track_info {
                result.slider_tracks.push(SliderTrackInfo {
                    field_id: "stroke_width".to_string(),
                    track_x: widget_track_info.track_x,
                    track_width: widget_track_info.track_width,
                    min_val: widget_track_info.min_val,
                    max_val: widget_track_info.max_val,
                });
            }
            row_y += row_height + row_gap;

            // Row 3: Line Style — dual button (left cycles, right opens dropdown)
            draw_label(ctx, "Стиль:", row_y);
            let style_label = match prim_data.style {
                LineStyle::Solid => "Сплошная",
                LineStyle::Dashed => "Пунктир",
                LineStyle::Dotted => "Точки",
                LineStyle::LargeDashed => "Длинный пунктир",
                LineStyle::SparseDotted => "Редкие точки",
            };
            let btn_total_width = control_width.min(160.0);
            let chevron_part = 22.0;
            let text_part = btn_total_width - chevron_part;
            let btn_rect = WidgetRect::new(control_left, row_y + 2.0, btn_total_width, row_height - 4.0);

            // Draw button background
            ctx.set_fill_color(&frame_theme.toolbar_bg);
            ctx.fill_rounded_rect(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height, 4.0);

            // Style preview: short line sample drawn left of the label
            {
                let preview_x_start = btn_rect.x + 8.0;
                let preview_x_end = preview_x_start + 30.0;
                let preview_y = btn_rect.center_y();
                ctx.set_stroke_color(&toolbar_theme.item_text);
                ctx.set_stroke_width(1.5);
                match prim_data.style {
                    LineStyle::Solid => {
                        ctx.begin_path();
                        ctx.move_to(preview_x_start, preview_y);
                        ctx.line_to(preview_x_end, preview_y);
                        ctx.stroke();
                    }
                    LineStyle::Dashed => {
                        let dash_len = 5.0;
                        let gap = 3.0;
                        let mut x = preview_x_start;
                        while x < preview_x_end {
                            ctx.begin_path();
                            ctx.move_to(x, preview_y);
                            ctx.line_to((x + dash_len).min(preview_x_end), preview_y);
                            ctx.stroke();
                            x += dash_len + gap;
                        }
                    }
                    LineStyle::Dotted => {
                        let gap = 4.0;
                        let mut x = preview_x_start;
                        while x <= preview_x_end {
                            ctx.begin_path();
                            ctx.arc(x, preview_y, 1.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            x += gap;
                        }
                    }
                    LineStyle::LargeDashed => {
                        let dash_len = 9.0;
                        let gap = 4.0;
                        let mut x = preview_x_start;
                        while x < preview_x_end {
                            ctx.begin_path();
                            ctx.move_to(x, preview_y);
                            ctx.line_to((x + dash_len).min(preview_x_end), preview_y);
                            ctx.stroke();
                            x += dash_len + gap;
                        }
                    }
                    LineStyle::SparseDotted => {
                        let gap = 7.0;
                        let mut x = preview_x_start;
                        while x <= preview_x_end {
                            ctx.begin_path();
                            ctx.arc(x, preview_y, 1.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                            x += gap;
                        }
                    }
                }
            }

            // Label text (right of the preview, clipped to text zone)
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.save();
            ctx.begin_path();
            ctx.rect(btn_rect.x + 44.0, btn_rect.y, text_part - 44.0, btn_rect.height);
            ctx.clip();
            ctx.fill_text(style_label, btn_rect.x + 44.0, btn_rect.center_y());
            ctx.restore();

            // Vertical separator between text part and chevron part
            let sep_x = btn_rect.x + text_part;
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(sep_x, btn_rect.y);
            ctx.line_to(sep_x, btn_rect.bottom());
            ctx.stroke();

            // Chevron arrow in the right part
            ctx.set_fill_color(&toolbar_theme.item_text);
            let arrow_cx = sep_x + chevron_part / 2.0;
            let arrow_cy = btn_rect.center_y();
            ctx.begin_path();
            ctx.move_to(arrow_cx - 4.0, arrow_cy - 2.0);
            ctx.line_to(arrow_cx + 4.0, arrow_cy - 2.0);
            ctx.line_to(arrow_cx, arrow_cy + 3.0);
            ctx.close_path();
            ctx.fill();

            // Register two separate hit areas
            let cycle_rect = WidgetRect::new(btn_rect.x, btn_rect.y, text_part, btn_rect.height);
            let menu_rect = WidgetRect::new(sep_x, btn_rect.y, chevron_part, btn_rect.height);
            result.content_items.push(("line_style".to_string(), cycle_rect));
            result.content_items.push(("line_style_menu".to_string(), menu_rect));
            row_y += row_height + row_gap;

            // Deferred: line style dropdown renders AFTER footer to avoid z-order overlap
            if state.open_line_style_dropdown {
                let current_style_str = match prim_data.style {
                    LineStyle::Solid        => "solid",
                    LineStyle::Dashed       => "dashed",
                    LineStyle::Dotted       => "dotted",
                    LineStyle::LargeDashed  => "large_dashed",
                    LineStyle::SparseDotted => "sparse_dotted",
                };
                deferred_line_style_dropdown = Some((btn_rect.x, btn_rect.bottom() + 2.0, btn_total_width, current_style_str.to_string()));
            }

            // Row 4: Fill Color (if applicable)
            if let Some(ref fill_color) = prim_data.color.fill {
                draw_label(ctx, "Цвет заливки:", row_y);
                let fill_btn_rect = WidgetRect::new(control_left, row_y + 2.0, 60.0, row_height - 4.0);
                draw_color_button(ctx, fill_color, fill_btn_rect.x, fill_btn_rect.y, fill_btn_rect.width, fill_btn_rect.height);
                result.content_items.push(("fill_color".to_string(), fill_btn_rect));
                row_y += row_height + row_gap;
            }

            // === ADDITIONAL STYLE PROPERTIES ===
            for prop in &style_props {
                match &prop.prop_type {
                    PropertyType::Boolean => {
                        draw_label(ctx, &prop.name, row_y);
                        let is_checked = prop.value.as_bool().unwrap_or(false);
                        let checkbox_size = 18.0;
                        let checkbox_x = control_left;
                        let checkbox_y = row_y + (row_height - checkbox_size) / 2.0;

                        ctx.set_fill_color(if is_checked { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                        ctx.fill_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.stroke_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);

                        if is_checked {
                            ctx.set_stroke_color("#ffffff");
                            ctx.set_stroke_width(2.0);
                            ctx.begin_path();
                            ctx.move_to(checkbox_x + 4.0, checkbox_y + checkbox_size / 2.0);
                            ctx.line_to(checkbox_x + 7.0, checkbox_y + checkbox_size - 5.0);
                            ctx.line_to(checkbox_x + checkbox_size - 4.0, checkbox_y + 4.0);
                            ctx.stroke();
                        }

                        let checkbox_rect = WidgetRect::new(checkbox_x, checkbox_y, checkbox_size, checkbox_size);
                        result.content_items.push((format!("style_prop:{}", prop.id), checkbox_rect));
                        row_y += row_height + row_gap;
                    }
                    PropertyType::Select { options } => {
                        draw_label(ctx, &prop.name, row_y);
                        let current_value = prop.value.as_string().unwrap_or("");
                        let display_label = options.iter()
                            .find(|o| o.value == current_value)
                            .map(|o| o.label.as_str())
                            .unwrap_or(current_value);

                        let dropdown_rect = WidgetRect::new(control_left, row_y + 2.0, control_width.min(140.0), row_height - 4.0);
                        ctx.set_fill_color(&frame_theme.toolbar_bg);
                        ctx.fill_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.stroke_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);

                        ctx.set_font("12px sans-serif");
                        ctx.set_fill_color(&toolbar_theme.item_text);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        let chevron_part = 22.0;
                        let text_part = dropdown_rect.width - chevron_part;
                        // Clip text so it doesn't overflow into arrow zone
                        ctx.save();
                        ctx.begin_path();
                        ctx.rect(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                        ctx.clip();
                        ctx.fill_text(display_label, dropdown_rect.x + 8.0, dropdown_rect.center_y());
                        ctx.restore();

                        // Vertical separator between text part and chevron part
                        let sep_x = dropdown_rect.x + text_part;
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.begin_path();
                        ctx.move_to(sep_x, dropdown_rect.y);
                        ctx.line_to(sep_x, dropdown_rect.bottom());
                        ctx.stroke();

                        ctx.set_fill_color(&toolbar_theme.item_text);
                        let arrow_cx = sep_x + chevron_part / 2.0;
                        let arrow_cy = dropdown_rect.center_y();
                        ctx.begin_path();
                        ctx.move_to(arrow_cx - 4.0, arrow_cy - 2.0);
                        ctx.line_to(arrow_cx + 4.0, arrow_cy - 2.0);
                        ctx.line_to(arrow_cx, arrow_cy + 3.0);
                        ctx.close_path();
                        ctx.fill();

                        // Two hit zones: left = cycle, right = open dropdown
                        let cycle_zone = WidgetRect::new(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                        let menu_zone = WidgetRect::new(sep_x, dropdown_rect.y, chevron_part, dropdown_rect.height);
                        result.content_items.push((format!("style_prop:{}", prop.id), cycle_zone));
                        result.content_items.push((format!("style_prop_menu:{}", prop.id), menu_zone));

                        // Deferred: record geometry if this prop's dropdown is open
                        if state.open_select_dropdown.as_ref()
                            .map(|(k, id)| k == "style" && id == prop.id.as_str())
                            .unwrap_or(false)
                        {
                            let opts: Vec<(String, String)> = options.iter()
                                .map(|o| (o.value.clone(), o.label.clone()))
                                .collect();
                            deferred_select_dropdown = Some((
                                dropdown_rect.x,
                                dropdown_rect.bottom() + 2.0,
                                dropdown_rect.width,
                                format!("style_prop_option:{}:", prop.id),
                                opts,
                            ));
                        }

                        row_y += row_height + row_gap;
                    }
                    PropertyType::Number { min, max, .. } => {
                        let current_value = prop.value.as_number().unwrap_or(0.0);
                        let min_val = min.unwrap_or(0.0);
                        let max_val = max.unwrap_or(100.0);

                        let slider_config = SliderConfig::new(min_val, max_val)
                            .with_step(1.0)
                            .without_input();

                        let slider_width = 200.0;
                        let slider_rect = WidgetRect::new(content_left, row_y, slider_width, row_height);
                        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

                        let field_id = format!("style_prop:{}", prop.id);
                        let hovered = state.slider_drag.as_ref()
                            .map(|d| d.field_id == field_id)
                            .unwrap_or_else(|| state.hovered_item_id.as_deref() == Some(field_id.as_str()));

                        // During drag, use floating preview value.
                        let display_value = state.slider_drag.as_ref()
                            .filter(|d| d.field_id == field_id)
                            .and_then(|d| d.floating_value)
                            .unwrap_or(current_value);

                        let slider_result = render_single_slider(
                            ctx,
                            &slider_config,
                            display_value,
                            slider_rect,
                            &format!("{}:", prop.name),
                            &widget_theme,
                            hovered,
                            None,
                        );

                        ctx.set_font("11px sans-serif");
                        ctx.set_fill_color(&toolbar_theme.item_text);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        let value_x = slider_result.track_rect.right() + 12.0;
                        ctx.fill_text(&format!("{:.0}", current_value), value_x, row_y + row_height / 2.0);

                        result.content_items.push((field_id.clone(), slider_result.full_rect));

                        if let Some(widget_track_info) = slider_result.track_info {
                            result.slider_tracks.push(SliderTrackInfo {
                                field_id,
                                track_x: widget_track_info.track_x,
                                track_width: widget_track_info.track_width,
                                min_val,
                                max_val,
                            });
                        }

                        row_y += row_height + row_gap;
                    }
                    PropertyType::Color => {
                        draw_label(ctx, &prop.name, row_y);
                        let color_value = prop.value.as_color().unwrap_or("#ffffff");
                        let color_btn_rect = WidgetRect::new(control_left, row_y + 2.0, 60.0, row_height - 4.0);
                        draw_color_button(ctx, color_value, color_btn_rect.x, color_btn_rect.y, color_btn_rect.width, color_btn_rect.height);
                        result.content_items.push((format!("style_prop:{}", prop.id), color_btn_rect));
                        row_y += row_height + row_gap;
                    }
                    _ => {}
                }
            }
        }

        PrimitiveSettingsTab::Text => {
            // === TEXT TAB ===
            if let Some(ref props) = text_props {
                for prop in props {
                    match &prop.prop_type {
                        PropertyType::Text { .. } => {
                            draw_label(ctx, &prop.name, row_y);
                            let input_rect = WidgetRect::new(control_left, row_y + 2.0, control_width.min(180.0), row_height - 4.0);
                            let field_id = format!("text_prop:{}", prop.id);
                            let is_editing = state.editing_text.as_ref()
                                .map(|e| e.field_id == field_id)
                                .unwrap_or(false);

                            let default_text = prop.value.as_string().unwrap_or("");
                            let empty_placeholder = "(пусто)";
                            let (display_text, cursor_pos, selection_start, selection_end): (&str, usize, Option<usize>, Option<usize>) = if let Some(ref edit) = state.editing_text {
                                if edit.field_id == field_id {
                                    (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
                                } else {
                                    (if default_text.is_empty() { empty_placeholder } else { default_text }, default_text.len(), None, None)
                                }
                            } else {
                                (if default_text.is_empty() { empty_placeholder } else { default_text }, default_text.len(), None, None)
                            };

                            let input_config = InputConfig::new(display_text)
                                .with_focused(is_editing)
                                .with_type(InputType::Text)
                                .with_font_size(12.0)
                                .with_padding(8.0)
                                .with_radius(4.0)
                                .with_cursor(cursor_pos)
                                .with_selection(selection_start, selection_end);

                            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                            let widget_state = WidgetState::Normal;
                            let input_result = draw_input(ctx, &input_config, widget_state, input_rect, &widget_theme);

                            if is_editing {
                                draw_input_cursor(
                                    ctx,
                                    input_result.cursor_x,
                                    input_result.cursor_y,
                                    input_result.cursor_height,
                                    &toolbar_theme.item_text,
                                );
                                result.active_input_char_positions = input_result.char_x_positions;
                                result.active_input_rect = Some(input_rect);
                            }

                            result.content_items.push((field_id, input_rect));
                            row_y += row_height + row_gap;
                        }
                        PropertyType::Number { min: _, max: _, .. } => {
                            draw_label(ctx, &prop.name, row_y);
                            let current_val = prop.value.as_number().unwrap_or(0.0);
                            let input_rect = WidgetRect::new(control_left, row_y + 2.0, 70.0, row_height - 4.0);
                            let field_id = format!("text_prop:{}", prop.id);
                            let is_editing = state.editing_text.as_ref()
                                .map(|e| e.field_id == field_id)
                                .unwrap_or(false);

                            let default_text = format!("{:.0}", current_val);
                            let (display_text, cursor_pos, selection_start, selection_end): (&str, usize, Option<usize>, Option<usize>) = if let Some(ref edit) = state.editing_text {
                                if edit.field_id == field_id {
                                    (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
                                } else {
                                    (&default_text, default_text.len(), None, None)
                                }
                            } else {
                                (&default_text, default_text.len(), None, None)
                            };

                            let input_config = InputConfig::new(display_text)
                                .with_focused(is_editing)
                                .with_type(InputType::Number)
                                .with_font_size(12.0)
                                .with_padding(8.0)
                                .with_radius(4.0)
                                .with_cursor(cursor_pos)
                                .with_selection(selection_start, selection_end);

                            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                            let widget_state = WidgetState::Normal;
                            let input_result = draw_input(ctx, &input_config, widget_state, input_rect, &widget_theme);

                            if is_editing {
                                draw_input_cursor(
                                    ctx,
                                    input_result.cursor_x,
                                    input_result.cursor_y,
                                    input_result.cursor_height,
                                    &toolbar_theme.item_text,
                                );
                                result.active_input_char_positions = input_result.char_x_positions;
                                result.active_input_rect = Some(input_rect);
                            }

                            result.content_items.push((field_id, input_rect));
                            row_y += row_height + row_gap;
                        }
                        PropertyType::Boolean => {
                            draw_label(ctx, &prop.name, row_y);
                            let is_checked = prop.value.as_bool().unwrap_or(false);
                            let checkbox_size = 18.0;
                            let checkbox_x = control_left;
                            let checkbox_y = row_y + (row_height - checkbox_size) / 2.0;

                            ctx.set_fill_color(if is_checked { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                            ctx.fill_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);
                            ctx.set_stroke_color(&toolbar_theme.separator);
                            ctx.set_stroke_width(1.0);
                            ctx.stroke_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);

                            if is_checked {
                                ctx.set_stroke_color("#ffffff");
                                ctx.set_stroke_width(2.0);
                                ctx.begin_path();
                                ctx.move_to(checkbox_x + 4.0, checkbox_y + checkbox_size / 2.0);
                                ctx.line_to(checkbox_x + 7.0, checkbox_y + checkbox_size - 5.0);
                                ctx.line_to(checkbox_x + checkbox_size - 4.0, checkbox_y + 4.0);
                                ctx.stroke();
                            }

                            let checkbox_rect = WidgetRect::new(checkbox_x, checkbox_y, checkbox_size, checkbox_size);
                            result.content_items.push((format!("text_prop:{}", prop.id), checkbox_rect));
                            row_y += row_height + row_gap;
                        }
                        PropertyType::Color => {
                            draw_label(ctx, &prop.name, row_y);
                            let color_val = prop.value.as_color().unwrap_or("#ffffff");
                            let color_rect = WidgetRect::new(control_left, row_y + 2.0, 60.0, row_height - 4.0);
                            draw_color_button(ctx, color_val, color_rect.x, color_rect.y, color_rect.width, color_rect.height);
                            result.content_items.push((format!("text_prop:{}", prop.id), color_rect));
                            row_y += row_height + row_gap;
                        }
                        PropertyType::Select { options } => {
                            draw_label(ctx, &prop.name, row_y);
                            let current_value = prop.value.as_string().unwrap_or("");
                            let display_label = options.iter()
                                .find(|o| o.value == current_value)
                                .map(|o| o.label.as_str())
                                .unwrap_or(current_value);

                            let dropdown_rect = WidgetRect::new(control_left, row_y + 2.0, control_width.min(180.0), row_height - 4.0);
                            ctx.set_fill_color(&frame_theme.toolbar_bg);
                            ctx.fill_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);
                            ctx.set_stroke_color(&toolbar_theme.separator);
                            ctx.set_stroke_width(1.0);
                            ctx.stroke_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);

                            ctx.set_font("12px sans-serif");
                            ctx.set_fill_color(&toolbar_theme.item_text);
                            ctx.set_text_align(TextAlign::Left);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            let chevron_part = 22.0;
                            let text_part = dropdown_rect.width - chevron_part;
                            // Clip text so it doesn't overflow into arrow zone
                            ctx.save();
                            ctx.begin_path();
                            ctx.rect(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                            ctx.clip();
                            ctx.fill_text(display_label, dropdown_rect.x + 8.0, dropdown_rect.center_y());
                            ctx.restore();

                            // Vertical separator between text part and chevron part
                            let sep_x = dropdown_rect.x + text_part;
                            ctx.set_stroke_color(&toolbar_theme.separator);
                            ctx.set_stroke_width(1.0);
                            ctx.begin_path();
                            ctx.move_to(sep_x, dropdown_rect.y);
                            ctx.line_to(sep_x, dropdown_rect.bottom());
                            ctx.stroke();

                            ctx.set_fill_color(&toolbar_theme.item_text);
                            let arrow_cx = sep_x + chevron_part / 2.0;
                            let arrow_cy = dropdown_rect.center_y();
                            ctx.begin_path();
                            ctx.move_to(arrow_cx - 4.0, arrow_cy - 2.0);
                            ctx.line_to(arrow_cx + 4.0, arrow_cy - 2.0);
                            ctx.line_to(arrow_cx, arrow_cy + 3.0);
                            ctx.close_path();
                            ctx.fill();

                            // Two hit zones: left = cycle, right = open dropdown
                            let cycle_zone = WidgetRect::new(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                            let menu_zone = WidgetRect::new(sep_x, dropdown_rect.y, chevron_part, dropdown_rect.height);
                            result.content_items.push((format!("text_prop:{}", prop.id), cycle_zone));
                            result.content_items.push((format!("text_prop_menu:{}", prop.id), menu_zone));

                            // Deferred: record geometry if this prop's dropdown is open
                            if state.open_select_dropdown.as_ref()
                                .map(|(k, id)| k == "text" && id == prop.id.as_str())
                                .unwrap_or(false)
                            {
                                let opts: Vec<(String, String)> = options.iter()
                                    .map(|o| (o.value.clone(), o.label.clone()))
                                    .collect();
                                deferred_select_dropdown = Some((
                                    dropdown_rect.x,
                                    dropdown_rect.bottom() + 2.0,
                                    dropdown_rect.width,
                                    format!("text_prop_option:{}:", prop.id),
                                    opts,
                                ));
                            }

                            row_y += row_height + row_gap;
                        }
                        _ => {}
                    }
                }
            } else if supports_text {
                // Default text UI
                let text_data = prim_data.text.as_ref();
                let current_content = text_data.map(|t| t.content.as_str()).unwrap_or("");
                let current_font_size = text_data.map(|t| t.font_size).unwrap_or(14.0);
                let current_bold = text_data.map(|t| t.bold).unwrap_or(false);
                let current_italic = text_data.map(|t| t.italic).unwrap_or(false);
                let current_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or(&prim_data.color.stroke);

                // Row 1: Text content
                draw_label(ctx, "Текст:", row_y);
                let input_rect = WidgetRect::new(control_left, row_y + 2.0, control_width.min(180.0), row_height - 4.0);

                let is_editing = state.editing_text.as_ref()
                    .map(|e| e.field_id == "text_content")
                    .unwrap_or(false);

                let (display_text, cursor_pos, selection_start, selection_end): (&str, usize, Option<usize>, Option<usize>) = if let Some(ref edit) = state.editing_text {
                    if edit.field_id == "text_content" {
                        let text = if edit.text.is_empty() { "" } else { &edit.text };
                        (text, edit.cursor, edit.selection_start, Some(edit.cursor))
                    } else {
                        (if current_content.is_empty() { "(пусто)" } else { current_content }, current_content.len(), None, None)
                    }
                } else {
                    (if current_content.is_empty() { "(пусто)" } else { current_content }, current_content.len(), None, None)
                };

                let content_input_config = InputConfig::new(display_text)
                    .with_focused(is_editing)
                    .with_type(InputType::Text)
                    .with_font_size(12.0)
                    .with_padding(8.0)
                    .with_radius(4.0)
                    .with_cursor(cursor_pos)
                    .with_selection(selection_start, selection_end);

                let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                let widget_state = WidgetState::Normal;
                let content_input_result = draw_input(ctx, &content_input_config, widget_state, input_rect, &widget_theme);

                if is_editing {
                    draw_input_cursor(
                        ctx,
                        content_input_result.cursor_x,
                        content_input_result.cursor_y,
                        content_input_result.cursor_height,
                        &toolbar_theme.item_text,
                    );
                }

                // Store char positions and input rect for click-to-cursor and drag-to-select
                result.text_content_char_x_positions = content_input_result.char_x_positions.clone();
                result.text_content_input_rect = Some(input_rect);
                if is_editing {
                    result.active_input_char_positions = content_input_result.char_x_positions;
                    result.active_input_rect = Some(input_rect);
                }

                result.content_items.push(("text_content".to_string(), input_rect));
                row_y += row_height + row_gap;

                // Row 2: Font size
                draw_label(ctx, "Размер шрифта:", row_y);
                let font_size_rect = WidgetRect::new(control_left, row_y + 2.0, 80.0, row_height - 4.0);

                let is_editing_font_size = state.editing_text.as_ref()
                    .map(|e| e.field_id == "text_font_size")
                    .unwrap_or(false);

                let font_size_str = format!("{:.0}", current_font_size);
                let (fs_display, fs_cursor, fs_sel_start, fs_sel_end): (&str, usize, Option<usize>, Option<usize>) = if let Some(ref edit) = state.editing_text {
                    if edit.field_id == "text_font_size" {
                        (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
                    } else {
                        (&font_size_str, font_size_str.len(), None, None)
                    }
                } else {
                    (&font_size_str, font_size_str.len(), None, None)
                };

                let font_size_input_config = InputConfig::new(fs_display)
                    .with_focused(is_editing_font_size)
                    .with_type(InputType::Text)
                    .with_font_size(12.0)
                    .with_padding(8.0)
                    .with_radius(4.0)
                    .with_cursor(fs_cursor)
                    .with_selection(fs_sel_start, fs_sel_end);

                let font_size_input_result = draw_input(ctx, &font_size_input_config, WidgetState::Normal, font_size_rect, &widget_theme);

                if is_editing_font_size {
                    draw_input_cursor(
                        ctx,
                        font_size_input_result.cursor_x,
                        font_size_input_result.cursor_y,
                        font_size_input_result.cursor_height,
                        &toolbar_theme.item_text,
                    );
                    result.active_input_char_positions = font_size_input_result.char_x_positions;
                    result.active_input_rect = Some(font_size_rect);
                }

                result.content_items.push(("text_font_size".to_string(), font_size_rect));
                row_y += row_height + row_gap;

                // Row 3: Bold/Italic toggles
                draw_label(ctx, "Стиль текста:", row_y);
                // Bold toggle
                let bold_rect = WidgetRect::new(control_left, row_y + 4.0, 20.0, 20.0);
                ctx.set_fill_color(if current_bold { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                ctx.fill_rounded_rect(bold_rect.x, bold_rect.y, bold_rect.width, bold_rect.height, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.stroke_rounded_rect(bold_rect.x, bold_rect.y, bold_rect.width, bold_rect.height, 3.0);
                if current_bold {
                    ctx.set_stroke_color("#ffffff");
                    ctx.set_stroke_width(2.0);
                    ctx.begin_path();
                    ctx.move_to(bold_rect.x + 5.0, bold_rect.center_y());
                    ctx.line_to(bold_rect.x + 8.0, bold_rect.bottom() - 5.0);
                    ctx.line_to(bold_rect.right() - 5.0, bold_rect.y + 5.0);
                    ctx.stroke();
                }
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.fill_text("Ж", bold_rect.right() + 4.0, row_y + row_height / 2.0);
                result.content_items.push(("text_bold".to_string(), bold_rect));

                // Italic toggle
                let italic_rect = WidgetRect::new(control_left + 50.0, row_y + 4.0, 20.0, 20.0);
                ctx.set_fill_color(if current_italic { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                ctx.fill_rounded_rect(italic_rect.x, italic_rect.y, italic_rect.width, italic_rect.height, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.stroke_rounded_rect(italic_rect.x, italic_rect.y, italic_rect.width, italic_rect.height, 3.0);
                if current_italic {
                    ctx.set_stroke_color("#ffffff");
                    ctx.set_stroke_width(2.0);
                    ctx.begin_path();
                    ctx.move_to(italic_rect.x + 5.0, italic_rect.center_y());
                    ctx.line_to(italic_rect.x + 8.0, italic_rect.bottom() - 5.0);
                    ctx.line_to(italic_rect.right() - 5.0, italic_rect.y + 5.0);
                    ctx.stroke();
                }
                ctx.set_font("italic 12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.fill_text("К", italic_rect.right() + 4.0, row_y + row_height / 2.0);
                result.content_items.push(("text_italic".to_string(), italic_rect));
                row_y += row_height + row_gap;

                // Row 4: Text color
                draw_label(ctx, "Цвет текста:", row_y);
                let text_color_rect = WidgetRect::new(control_left, row_y + 2.0, 60.0, row_height - 4.0);
                draw_color_button(ctx, current_color, text_color_rect.x, text_color_rect.y, text_color_rect.width, text_color_rect.height);
                result.content_items.push(("text_color".to_string(), text_color_rect));
                row_y += row_height + row_gap;

                // Row 5: Text position - 9-point grid selector
                let current_v_align = text_data.map(|t| t.v_align.as_str()).unwrap_or("start");
                let current_h_align = text_data.map(|t| t.h_align.as_str()).unwrap_or("center");

                draw_label(ctx, "Положение:", row_y);
                row_y += row_height;

                let cell_size = 28.0;
                let cell_spacing = 2.0;
                let grid_size = cell_size * 3.0 + cell_spacing * 2.0;

                let grid_x = control_left;
                let grid_y = row_y;

                ctx.set_fill_color(&frame_theme.toolbar_bg);
                ctx.fill_rounded_rect(grid_x - 4.0, grid_y - 4.0, grid_size + 8.0, grid_size + 8.0, 4.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(grid_x - 4.0, grid_y - 4.0, grid_size + 8.0, grid_size + 8.0, 4.0);

                let v_aligns = ["start", "center", "end"];
                let h_aligns = ["start", "center", "end"];

                for (row_idx, v_align) in v_aligns.iter().enumerate() {
                    for (col_idx, h_align) in h_aligns.iter().enumerate() {
                        let cell_x = grid_x + col_idx as f64 * (cell_size + cell_spacing);
                        let cell_y = grid_y + row_idx as f64 * (cell_size + cell_spacing);

                        let is_selected = current_v_align == *v_align && current_h_align == *h_align;
                        let is_hovered = state.hovered_item_id.as_deref() == Some(&format!("text_pos_{}_{}", v_align, h_align));

                        let cell_bg: &str = if is_selected {
                            &toolbar_theme.item_bg_active
                        } else if is_hovered {
                            &toolbar_theme.item_bg_hover
                        } else {
                            &frame_theme.toolbar_bg
                        };
                        ctx.set_fill_color(cell_bg);
                        ctx.fill_rounded_rect(cell_x, cell_y, cell_size, cell_size, 3.0);

                        let indicator_size = 6.0;
                        let indicator_color = if is_selected { "#ffffff" } else { &toolbar_theme.item_text };

                        let indicator_x = match *h_align {
                            "start" => cell_x + 6.0,
                            "center" => cell_x + cell_size / 2.0,
                            "end" => cell_x + cell_size - 6.0,
                            _ => cell_x + cell_size / 2.0,
                        };
                        let indicator_y = match *v_align {
                            "start" => cell_y + 6.0,
                            "center" => cell_y + cell_size / 2.0,
                            "end" => cell_y + cell_size - 6.0,
                            _ => cell_y + cell_size / 2.0,
                        };

                        ctx.set_fill_color(indicator_color);
                        ctx.begin_path();
                        ctx.arc(indicator_x, indicator_y, indicator_size / 2.0, 0.0, std::f64::consts::TAU);
                        ctx.fill();

                        let cell_rect = WidgetRect::new(cell_x, cell_y, cell_size, cell_size);
                        result.content_items.push((format!("text_pos_{}_{}", v_align, h_align), cell_rect));
                    }
                }
            } else {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text("Этот примитив не поддерживает текст", content_left, row_y);
            }
        }

        PrimitiveSettingsTab::Coordinates => {
            // === COORDINATES TAB ===
            let points = prim.points();
            let point_labels = crate::drawing::get_point_labels(prim_type_id, points.len());

            let field_width = 80.0;
            let field_height = 24.0;
            let field_gap = 8.0;

            for (i, (bar, price)) in points.iter().enumerate() {
                let label = point_labels.get(i).map(|s| s.as_str()).unwrap_or("");
                let point_label = if label.is_empty() {
                    format!("Точка {}:", i + 1)
                } else {
                    format!("{}:", label)
                };

                draw_label(ctx, &point_label, row_y);

                let field_y = row_y + (row_height - field_height) / 2.0;

                // Price field
                let price_field_id = format!("coord_{}_price", i);
                let price_rect = WidgetRect::new(control_left, field_y, field_width, field_height);
                let is_editing_price = state.editing_text.as_ref()
                    .map(|e| e.field_id == price_field_id)
                    .unwrap_or(false);

                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Bottom);
                ctx.fill_text("Цена", control_left, field_y - 2.0);

                let price_text = format!("{:.4}", price);
                let (price_display, price_cursor, price_sel_start, price_sel_end) = if is_editing_price {
                    if let Some(ref edit) = state.editing_text {
                        (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
                    } else {
                        (&price_text, price_text.len(), None, None)
                    }
                } else {
                    (&price_text, price_text.len(), None, None)
                };

                let price_input_config = InputConfig::new(price_display)
                    .with_focused(is_editing_price)
                    .with_type(InputType::Number)
                    .with_font_size(12.0)
                    .with_padding(4.0)
                    .with_radius(4.0)
                    .with_cursor(price_cursor)
                    .with_selection(price_sel_start, price_sel_end);

                let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                let widget_state = WidgetState::Normal;
                let price_input_result = draw_input(ctx, &price_input_config, widget_state, price_rect, &widget_theme);

                if is_editing_price {
                    draw_input_cursor(
                        ctx,
                        price_input_result.cursor_x,
                        price_input_result.cursor_y,
                        price_input_result.cursor_height,
                        "#d1d4dc",
                    );
                    result.active_input_char_positions = price_input_result.char_x_positions;
                    result.active_input_rect = Some(price_rect);
                }

                result.content_items.push((price_field_id, price_rect));

                // Bar field
                let bar_field_id = format!("coord_{}_bar", i);
                let bar_rect = WidgetRect::new(control_left + field_width + field_gap, field_y, field_width, field_height);
                let is_editing_bar = state.editing_text.as_ref()
                    .map(|e| e.field_id == bar_field_id)
                    .unwrap_or(false);

                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Bottom);
                ctx.fill_text("Бар", bar_rect.x, field_y - 2.0);

                let bar_text = format!("{:.0}", bar);
                let (bar_display, bar_cursor, bar_sel_start, bar_sel_end) = if is_editing_bar {
                    if let Some(ref edit) = state.editing_text {
                        (&edit.text, edit.cursor, edit.selection_start, Some(edit.cursor))
                    } else {
                        (&bar_text, bar_text.len(), None, None)
                    }
                } else {
                    (&bar_text, bar_text.len(), None, None)
                };

                let bar_input_config = InputConfig::new(bar_display)
                    .with_focused(is_editing_bar)
                    .with_type(InputType::Number)
                    .with_font_size(12.0)
                    .with_padding(4.0)
                    .with_radius(4.0)
                    .with_cursor(bar_cursor)
                    .with_selection(bar_sel_start, bar_sel_end);

                let bar_input_result = draw_input(ctx, &bar_input_config, widget_state, bar_rect, &widget_theme);

                if is_editing_bar {
                    draw_input_cursor(
                        ctx,
                        bar_input_result.cursor_x,
                        bar_input_result.cursor_y,
                        bar_input_result.cursor_height,
                        "#d1d4dc",
                    );
                    result.active_input_char_positions = bar_input_result.char_x_positions;
                    result.active_input_rect = Some(bar_rect);
                }

                result.content_items.push((bar_field_id, bar_rect));

                row_y += row_height + row_gap + 8.0;
            }
        }

        PrimitiveSettingsTab::Levels => {
            // === LEVELS TAB ===
            if has_levels {
                if let Some(configs) = prim.level_configs() {
                    let col_width = 125.0;
                    let col_gap = 8.0;
                    let col1_x = content_left;
                    let col2_x = content_left + col_width + col_gap;
                    let col3_x = content_left + (col_width + col_gap) * 2.0;

                    let num_levels = configs.len();
                    let cols = 3;
                    let rows_needed = num_levels.div_ceil(cols);

                    for row_idx in 0..rows_needed {
                        let col1_idx = row_idx * cols;
                        let col2_idx = row_idx * cols + 1;
                        let col3_idx = row_idx * cols + 2;

                        if col1_idx < num_levels {
                            let config = &configs[col1_idx];
                            render_level_item(ctx, frame_theme, toolbar_theme, &prim_data,
                                             &mut result, state.editing_text.as_ref(),
                                             col1_x, row_y, row_height, col1_idx, config);
                        }

                        if col2_idx < num_levels {
                            let config = &configs[col2_idx];
                            render_level_item(ctx, frame_theme, toolbar_theme, &prim_data,
                                             &mut result, state.editing_text.as_ref(),
                                             col2_x, row_y, row_height, col2_idx, config);
                        }

                        if col3_idx < num_levels {
                            let config = &configs[col3_idx];
                            render_level_item(ctx, frame_theme, toolbar_theme, &prim_data,
                                             &mut result, state.editing_text.as_ref(),
                                             col3_x, row_y, row_height, col3_idx, config);
                        }

                        row_y += row_height + row_gap;
                    }
                } else {
                    ctx.set_font("12px sans-serif");
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Top);
                    ctx.fill_text("Этот примитив не поддерживает уровни", content_left, row_y);
                }
            } else {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text("Этот примитив не поддерживает уровни", content_left, row_y);
            }

            if !level_props.is_empty() {
                row_y += row_gap * 2.0;
            }
            for prop in &level_props {
                match &prop.prop_type {
                    PropertyType::Boolean => {
                        draw_label(ctx, &prop.name, row_y);
                        let is_checked = prop.value.as_bool().unwrap_or(false);
                        let checkbox_size = 18.0;
                        let checkbox_x = control_left;
                        let checkbox_y = row_y + (row_height - checkbox_size) / 2.0;

                        ctx.set_fill_color(if is_checked { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                        ctx.fill_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.stroke_rounded_rect(checkbox_x, checkbox_y, checkbox_size, checkbox_size, 3.0);

                        if is_checked {
                            ctx.set_stroke_color("#ffffff");
                            ctx.set_stroke_width(2.0);
                            ctx.begin_path();
                            ctx.move_to(checkbox_x + 4.0, checkbox_y + checkbox_size / 2.0);
                            ctx.line_to(checkbox_x + 7.0, checkbox_y + checkbox_size - 5.0);
                            ctx.line_to(checkbox_x + checkbox_size - 4.0, checkbox_y + 4.0);
                            ctx.stroke();
                        }

                        let checkbox_rect = WidgetRect::new(checkbox_x, checkbox_y, checkbox_size, checkbox_size);
                        result.content_items.push((format!("level_prop:{}", prop.id), checkbox_rect));
                        row_y += row_height + row_gap;
                    }
                    PropertyType::Select { options } => {
                        draw_label(ctx, &prop.name, row_y);
                        let current_value = prop.value.as_string().unwrap_or("");
                        let display_label = options.iter()
                            .find(|o| o.value == current_value)
                            .map(|o| o.label.as_str())
                            .unwrap_or(current_value);

                        let dropdown_rect = WidgetRect::new(control_left, row_y + 2.0, control_width.min(180.0), row_height - 4.0);
                        ctx.set_fill_color(&frame_theme.toolbar_bg);
                        ctx.fill_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.stroke_rounded_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height, 4.0);

                        ctx.set_font("12px sans-serif");
                        ctx.set_fill_color(&toolbar_theme.item_text);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        let chevron_part = 22.0;
                        let text_part = dropdown_rect.width - chevron_part;
                        // Clip text so it doesn't overflow into arrow zone
                        ctx.save();
                        ctx.begin_path();
                        ctx.rect(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                        ctx.clip();
                        ctx.fill_text(display_label, dropdown_rect.x + 8.0, dropdown_rect.center_y());
                        ctx.restore();

                        // Vertical separator between text part and chevron part
                        let sep_x = dropdown_rect.x + text_part;
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.begin_path();
                        ctx.move_to(sep_x, dropdown_rect.y);
                        ctx.line_to(sep_x, dropdown_rect.bottom());
                        ctx.stroke();

                        ctx.set_fill_color(&toolbar_theme.item_text);
                        let arrow_cx = sep_x + chevron_part / 2.0;
                        let arrow_cy = dropdown_rect.center_y();
                        ctx.begin_path();
                        ctx.move_to(arrow_cx - 4.0, arrow_cy - 2.0);
                        ctx.line_to(arrow_cx + 4.0, arrow_cy - 2.0);
                        ctx.line_to(arrow_cx, arrow_cy + 3.0);
                        ctx.close_path();
                        ctx.fill();

                        // Two hit zones: left = cycle, right = open dropdown
                        let cycle_zone = WidgetRect::new(dropdown_rect.x, dropdown_rect.y, text_part, dropdown_rect.height);
                        let menu_zone = WidgetRect::new(sep_x, dropdown_rect.y, chevron_part, dropdown_rect.height);
                        result.content_items.push((format!("level_prop:{}", prop.id), cycle_zone));
                        result.content_items.push((format!("level_prop_menu:{}", prop.id), menu_zone));

                        // Deferred: record geometry if this prop's dropdown is open
                        if state.open_select_dropdown.as_ref()
                            .map(|(k, id)| k == "level" && id == prop.id.as_str())
                            .unwrap_or(false)
                        {
                            let opts: Vec<(String, String)> = options.iter()
                                .map(|o| (o.value.clone(), o.label.clone()))
                                .collect();
                            deferred_select_dropdown = Some((
                                dropdown_rect.x,
                                dropdown_rect.bottom() + 2.0,
                                dropdown_rect.width,
                                format!("level_prop_option:{}:", prop.id),
                                opts,
                            ));
                        }

                        row_y += row_height + row_gap;
                    }
                    _ => {}
                }
            }
        }

        PrimitiveSettingsTab::Visibility => {
            // === VISIBILITY TAB ===
            let tf_config = prim_data.timeframe_visibility.clone()
                .unwrap_or_else(crate::drawing::TimeframeVisibilityConfig::all);

            let timeframes: [(&str, bool, bool, Option<(u32, u32)>, u32, u32); 7] = [
                ("Тики", true, tf_config.ticks, None, 0, 0),
                ("Секунды", false, tf_config.seconds.is_some(), tf_config.seconds, 1, 59),
                ("Минуты", false, tf_config.minutes.is_some(), tf_config.minutes, 1, 59),
                ("Часы", false, tf_config.hours.is_some(), tf_config.hours, 1, 24),
                ("Дни", false, tf_config.days.is_some(), tf_config.days, 1, 366),
                ("Недели", false, tf_config.weeks.is_some(), tf_config.weeks, 1, 52),
                ("Месяцы", false, tf_config.months.is_some(), tf_config.months, 1, 12),
            ];

            let checkbox_size = 16.0;
            let label_width_tf = 70.0;
            let input_width = 32.0;
            let slider_width_tf = 140.0;
            let gap = 6.0;

            for (i, (label, is_bool_only, enabled, range, min_allowed, max_allowed)) in timeframes.iter().enumerate() {
                let check_x = content_left;
                let check_y = row_y + (row_height - checkbox_size) / 2.0;

                let check_rect = WidgetRect::new(check_x, check_y, checkbox_size, checkbox_size);
                ctx.set_fill_color(if *enabled { &toolbar_theme.item_bg_active } else { &frame_theme.toolbar_bg });
                ctx.fill_rounded_rect(check_rect.x, check_rect.y, check_rect.width, check_rect.height, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(check_rect.x, check_rect.y, check_rect.width, check_rect.height, 3.0);
                if *enabled {
                    ctx.set_stroke_color("#ffffff");
                    ctx.set_stroke_width(2.0);
                    ctx.begin_path();
                    ctx.move_to(check_rect.x + 3.0, check_rect.center_y());
                    ctx.line_to(check_rect.x + 6.0, check_rect.bottom() - 3.0);
                    ctx.line_to(check_rect.right() - 3.0, check_rect.y + 3.0);
                    ctx.stroke();
                }

                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, check_x + checkbox_size + 6.0, row_y + row_height / 2.0);

                let checkbox_hit = WidgetRect::new(check_x, row_y, checkbox_size + 6.0 + label_width_tf, row_height);
                result.content_items.push((format!("tf_{}_toggle", i), checkbox_hit));

                if !*is_bool_only && *enabled {
                    let controls_x = content_left + checkbox_size + 6.0 + label_width_tf + gap;
                    let (current_min, current_max) = range.unwrap_or((*min_allowed, *max_allowed));

                    let slider_config = SliderConfig::new(*min_allowed as f64, *max_allowed as f64)
                        .with_step(1.0);

                    let total_width = input_width * 2.0 + slider_width_tf + gap * 2.0;
                    let slider_rect = WidgetRect::new(controls_x, row_y, total_width, row_height);
                    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

                    let min_field_id = format!("tf_{}_min", i);
                    let max_field_id = format!("tf_{}_max", i);

                    let editing_min_info = state.editing_text.as_ref()
                        .filter(|e| e.field_id == min_field_id)
                        .map(|e| SliderEditingInfo {
                            text: &e.text,
                            cursor: e.cursor,
                            selection_start: e.selection_start,
                        });

                    let editing_max_info = state.editing_text.as_ref()
                        .filter(|e| e.field_id == max_field_id)
                        .map(|e| SliderEditingInfo {
                            text: &e.text,
                            cursor: e.cursor,
                            selection_start: e.selection_start,
                        });

                    let slider_field_id = format!("tf_{}_slider", i);
                    let hovered = state.slider_drag.as_ref()
                        .map(|d| d.field_id == slider_field_id)
                        .unwrap_or_else(|| state.hovered_item_id.as_deref() == Some(slider_field_id.as_str()));

                    // During drag, apply floating preview to the dragged handle.
                    let (display_min, display_max) = if let Some(ref drag) = state.slider_drag {
                        if drag.field_id == slider_field_id {
                            if let Some(float_val) = drag.floating_value {
                                let fv = float_val.round() as u32;
                                let (cmin, cmax) = (current_min, current_max);
                                match drag.dual_handle {
                                    Some(DualSliderHandle::Min) => (fv.min(cmax), cmax),
                                    Some(DualSliderHandle::Max) => (cmin, fv.max(cmin)),
                                    None => (cmin, cmax),
                                }
                            } else {
                                (current_min, current_max)
                            }
                        } else {
                            (current_min, current_max)
                        }
                    } else {
                        (current_min, current_max)
                    };

                    let slider_result = render_dual_slider(
                        ctx,
                        &slider_config,
                        display_min as f64,
                        display_max as f64,
                        slider_rect,
                        "",
                        &widget_theme,
                        hovered,
                        editing_min_info,
                        editing_max_info,
                    );

                    result.content_items.push((slider_field_id.clone(), slider_result.full_rect));

                    if let Some(min_input_rect) = slider_result.min_input_rect {
                        result.content_items.push((min_field_id.clone(), min_input_rect));

                        // Expose cursor/char positions so click-to-cursor and drag-to-select work.
                        if state.editing_text.as_ref().map(|e| e.field_id == min_field_id).unwrap_or(false) {
                            result.active_input_rect = Some(min_input_rect);
                            if let Some(ref ir) = slider_result.min_input_result {
                                result.active_input_char_positions = ir.char_x_positions.clone();
                            }
                        }
                    }
                    if let Some(max_input_rect) = slider_result.max_input_rect {
                        result.content_items.push((max_field_id.clone(), max_input_rect));

                        // Expose cursor/char positions so click-to-cursor and drag-to-select work.
                        if state.editing_text.as_ref().map(|e| e.field_id == max_field_id).unwrap_or(false) {
                            result.active_input_rect = Some(max_input_rect);
                            if let Some(ref ir) = slider_result.max_input_result {
                                result.active_input_char_positions = ir.char_x_positions.clone();
                            }
                        }
                    }

                    if let Some(widget_track_info) = slider_result.track_info {
                        result.slider_tracks.push(SliderTrackInfo {
                            field_id: slider_field_id,
                            track_x: widget_track_info.track_x,
                            track_width: widget_track_info.track_width,
                            min_val: *min_allowed as f64,
                            max_val: *max_allowed as f64,
                        });
                    }
                }

                row_y += row_height + row_gap;
            }
        }
    }

    // === TEMPLATE FOOTER ===
    // Single 52px row: "Шаблон" outline button left, "Отмена" + "OK" right.
    {
        use crate::render::{TextAlign, TextBaseline};
        use crate::engine::render::draw_svg_icon;

        let footer_y = modal_y + modal_height - template_footer_height;

        // Separator line at top of footer
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(modal_x, footer_y);
        ctx.line_to(modal_x + modal_width, footer_y);
        ctx.stroke();

        let button_height = 32.0;
        let button_y = footer_y + (template_footer_height - button_height) / 2.0;
        let button_padding = 12.0;

        // ── "Шаблон" button (left side, outline only) ────────────────────────
        let template_btn_width = 80.0;
        let template_btn_x = modal_x + 16.0;
        let is_tmpl_hovered = state.hovered_item_id.as_deref() == Some("template_dropdown");
        ctx.set_stroke_color(if is_tmpl_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(template_btn_x, button_y, template_btn_width, button_height);
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(if is_tmpl_hovered { &toolbar_theme.item_text_hover } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Шаблон", template_btn_x + template_btn_width / 2.0, button_y + button_height / 2.0);

        result.content_items.push(("template_dropdown".to_string(), WidgetRect::new(template_btn_x, button_y, template_btn_width, button_height)));

        // ── "OK" button (right side, filled #2962ff) ─────────────────────────
        let ok_btn_width = 70.0;
        let cancel_btn_width = 80.0;
        let ok_btn_x = modal_x + modal_width - 16.0 - ok_btn_width;
        let cancel_btn_x = ok_btn_x - button_padding - cancel_btn_width;

        let is_ok_hovered = state.hovered_item_id.as_deref() == Some("ok");
        ctx.set_fill_color(if is_ok_hovered { "#4080ff" } else { "#2962ff" });
        ctx.fill_rect(ok_btn_x, button_y, ok_btn_width, button_height);
        ctx.set_fill_color("#ffffff");
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("OK", ok_btn_x + ok_btn_width / 2.0, button_y + button_height / 2.0);

        result.content_items.push(("ok".to_string(), WidgetRect::new(ok_btn_x, button_y, ok_btn_width, button_height)));

        // ── "Отмена" button (before OK, outline only) ────────────────────────
        let is_cancel_hovered = state.hovered_item_id.as_deref() == Some("cancel");
        if is_cancel_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
        }
        ctx.set_stroke_color(if is_cancel_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Отмена", cancel_btn_x + cancel_btn_width / 2.0, button_y + button_height / 2.0);

        result.content_items.push(("cancel".to_string(), WidgetRect::new(cancel_btn_x, button_y, cancel_btn_width, button_height)));

        // ── Template dropdown menu (opens BELOW the "Шаблон" button) ─────────
        if state.template_dropdown_open {
            let opt_h = 28.0;
            let sep_h = 1.0;
            let delete_btn_w = 24.0;
            let fixed_rows = 2_usize;
            let tmpl_rows = templates.len().max(1); // at least 1 for "(нет шаблонов)"
            let total_h = opt_h * (fixed_rows + tmpl_rows) as f64 + sep_h + 6.0;
            let menu_w = template_btn_width.max(180.0);
            let dd_x = template_btn_x;
            let menu_y = button_y + button_height + 2.0;

            // Menu background + border
            ctx.set_fill_color(&toolbar_theme.dropdown_bg);
            ctx.fill_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);

            // Register background FIRST with coordinator (last-registered wins, so
            // items registered AFTER will override this in hit_test_at)
            input_coordinator.register_on_layer(
                "prim_settings:item:template_dropdown_menu".to_string(),
                uzor::types::Rect::new(dd_x, menu_y, menu_w, total_h),
                uzor::input::sense::Sense::CLICK,
                &layer_id,
            );

            let mut row_y = menu_y + 3.0;

            // Row 1: "Сохранить как..."
            let is_sa_hovered = state.hovered_item_id.as_deref() == Some("template_save_as");
            if is_sa_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
            }
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Сохранить как...", dd_x + 8.0, row_y + opt_h / 2.0);
            result.content_items.push(("template_save_as".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
            row_y += opt_h;

            // Row 2: "Применить по умолчанию"
            let is_def_hovered = state.hovered_item_id.as_deref() == Some("template_default");
            if is_def_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
            }
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Применить по умолчанию", dd_x + 8.0, row_y + opt_h / 2.0);
            result.content_items.push(("template_default".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
            row_y += opt_h;

            // Separator
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(dd_x + 4.0, row_y);
            ctx.line_to(dd_x + menu_w - 4.0, row_y);
            ctx.stroke();
            row_y += sep_h + 2.0;

            // Saved templates list
            if templates.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("(нет шаблонов)", dd_x + 8.0, row_y + opt_h / 2.0);
            } else {
                for tmpl in templates {
                    let row_id = format!("template_option:{}", tmpl.id);
                    let del_id = format!("template_delete:{}", tmpl.id);
                    let is_row_hovered = state.hovered_item_id.as_deref() == Some(row_id.as_str());
                    let is_del_hovered = state.hovered_item_id.as_deref() == Some(del_id.as_str());

                    if is_row_hovered || is_del_hovered {
                        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                        ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
                    }

                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(&tmpl.name, dd_x + 8.0, row_y + opt_h / 2.0);

                    let del_x = dd_x + menu_w - delete_btn_w - 4.0;
                    let del_y = row_y + (opt_h - 24.0) / 2.0;
                    let del_color = if is_del_hovered { "#EF5350" } else { &toolbar_theme.item_text_muted };
                    draw_svg_icon(ctx, crate::ui::Icon::Close.svg(), del_x, del_y, 24.0, 24.0, del_color);

                    let name_w = menu_w - delete_btn_w - 8.0;
                    result.content_items.push((row_id, WidgetRect::new(dd_x, row_y, name_w, opt_h)));
                    result.content_items.push((del_id, WidgetRect::new(del_x, del_y, delete_btn_w, 24.0)));

                    row_y += opt_h;
                }
            }

            // Register dropdown background LAST so specific items win in hit-test
            result.content_items.push(("template_dropdown_menu".to_string(), WidgetRect::new(dd_x, menu_y, menu_w, total_h)));

            let _ = is_sa_hovered;
            let _ = is_def_hovered;
        }

        let _ = button_padding;
    }

    // Draw hover highlight for hovered content item (skip non-interactive background rects)
    if let Some(ref hovered_id) = state.hovered_item_id {
        if hovered_id != "template_dropdown_menu" {
            for (id, rect) in &result.content_items {
                if id == hovered_id {
                    ctx.set_fill_color("rgba(255, 255, 255, 0.1)");
                    ctx.fill_rounded_rect(rect.x - 2.0, rect.y - 2.0, rect.width + 4.0, rect.height + 4.0, 4.0);
                    break;
                }
            }
        }
    }

    // Build set of slider field IDs to determine which items need DRAG sense
    let slider_field_ids: std::collections::HashSet<&str> = result.slider_tracks
        .iter()
        .map(|s| s.field_id.as_str())
        .collect();

    // Register all content items with coordinator (skip template_dropdown_menu —
    // already registered early so it has lowest priority in hit_test_at)
    for (item_id, item_rect) in &result.content_items {
        if item_id == "template_dropdown_menu" {
            continue;
        }
        let sense = if slider_field_ids.contains(item_id.as_str()) {
            uzor::input::sense::Sense::DRAG
        } else {
            uzor::input::sense::Sense::CLICK
        };

        input_coordinator.register_on_layer(
            format!("prim_settings:item:{}", item_id),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            sense,
            &layer_id,
        );
    }

    // Deferred line style dropdown — draw AFTER footer so it appears on top
    if let Some((drop_x, drop_y, btn_w, current_style_str)) = deferred_line_style_dropdown {
        let all_styles: &[(&str, &str)] = &[
            ("solid",        "Сплошная"),
            ("dashed",       "Пунктир"),
            ("dotted",       "Точки"),
            ("large_dashed", "Длинный пунктир"),
            ("sparse_dotted","Редкие точки"),
        ];
        let dropdown_items: Vec<DropdownItem> = all_styles.iter()
            .map(|(id, label)| DropdownItem::item(id, label))
            .collect();
        let dropdown_cfg = DropdownConfig {
            items: dropdown_items,
            min_width: btn_w,
            max_width: btn_w + 40.0,
            item_height: 28.0,
            separator_height: 9.0,
            header_height: 28.0,
            padding: 4.0,
            item_padding_x: 12.0,
            radius: 4.0,
            icon_size: 16.0,
            font_size: 13.0,
            shadow_blur: 24.0,
            grid_columns: None,
        };
        let dropdown_theme = DropdownTheme {
            background: toolbar_theme.dropdown_bg.clone(),
            border: toolbar_theme.separator.clone(),
            shadow: "rgba(0,0,0,0.5)".to_string(),
            item_text: toolbar_theme.item_text.clone(),
            item_text_hover: toolbar_theme.item_text_hover.clone(),
            item_text_disabled: toolbar_theme.item_text_muted.clone(),
            item_bg_hover: toolbar_theme.item_bg_hover.clone(),
            item_danger: "#f23645".to_string(),
            item_danger_bg_hover: "rgba(242,54,69,0.15)".to_string(),
            header_text: toolbar_theme.item_text.clone(),
            header_border: toolbar_theme.separator.clone(),
            separator: toolbar_theme.separator.clone(),
            shortcut_text: toolbar_theme.item_text_muted.clone(),
        };
        let dropdown_result = render_dropdown(
            ctx,
            &dropdown_cfg,
            (drop_x, drop_y),
            &dropdown_theme,
            Some(&current_style_str),
            |_ctx, _icon, _rect, _color| {},
        );
        for (item_id, item_rect) in &dropdown_result.item_rects {
            result.content_items.push((
                format!("line_style_option:{}", item_id),
                WidgetRect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            ));
        }
    }

    // Deferred Select property dropdown — draw AFTER footer so it appears on top
    if let Some((drop_x, drop_y, btn_w, id_prefix, options)) = deferred_select_dropdown {
        let dropdown_items: Vec<DropdownItem> = options.iter()
            .map(|(val, label)| DropdownItem::item(&format!("{}{}", id_prefix, val), label))
            .collect();
        let dropdown_cfg = DropdownConfig {
            items: dropdown_items,
            min_width: btn_w,
            max_width: btn_w + 40.0,
            item_height: 28.0,
            separator_height: 9.0,
            header_height: 28.0,
            padding: 4.0,
            item_padding_x: 12.0,
            radius: 4.0,
            icon_size: 16.0,
            font_size: 13.0,
            shadow_blur: 24.0,
            grid_columns: None,
        };
        let dropdown_theme = DropdownTheme {
            background: toolbar_theme.dropdown_bg.clone(),
            border: toolbar_theme.separator.clone(),
            shadow: "rgba(0,0,0,0.5)".to_string(),
            item_text: toolbar_theme.item_text.clone(),
            item_text_hover: toolbar_theme.item_text_hover.clone(),
            item_text_disabled: toolbar_theme.item_text_muted.clone(),
            item_bg_hover: toolbar_theme.item_bg_hover.clone(),
            item_danger: "#f23645".to_string(),
            item_danger_bg_hover: "rgba(242,54,69,0.15)".to_string(),
            header_text: toolbar_theme.item_text.clone(),
            header_border: toolbar_theme.separator.clone(),
            separator: toolbar_theme.separator.clone(),
            shortcut_text: toolbar_theme.item_text_muted.clone(),
        };
        let hovered = state.hovered_item_id.as_deref();
        let dropdown_result = render_dropdown(
            ctx,
            &dropdown_cfg,
            (drop_x, drop_y),
            &dropdown_theme,
            hovered,
            |_ctx, _icon, _rect, _color| {},
        );
        for (item_id, item_rect) in &dropdown_result.item_rects {
            result.content_items.push((
                item_id.clone(),
                WidgetRect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            ));
        }
    }

    // Pop modal layer
    input_coordinator.pop_layer(&layer_id);

    result
}

/// Render color picker popup for primitive settings (L1 or L2 based on state)
pub fn render_primitive_color_picker_popup(
    ctx: &mut dyn RenderContext,
    primitive_state: &PrimitiveSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> crate::layout::render_frame::ColorPickerRenderResult {
    use crate::ui::widgets::{
        draw_color_picker_l1, draw_color_picker_l2,
        ColorPickerLevel, PopupTheme,
    };

    let popup_theme = PopupTheme::new(&toolbar_theme.background, &toolbar_theme.separator)
        .with_active(&toolbar_theme.item_bg_active);
    let origin = primitive_state.color_picker.origin;
    let level = primitive_state.color_picker.level;

    let result = match level {
        ColorPickerLevel::L1 => {
            let config = primitive_state.color_picker.l1_config();
            let hovered = primitive_state.color_picker.hovered_swatch_str();
            let l1_result = draw_color_picker_l1(ctx, &config, origin, &popup_theme, hovered);
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: Some(l1_result),
                l2_result: None,
            }
        }
        ColorPickerLevel::L2 => {
            let config = primitive_state.color_picker.l2_config();
            let hovered_area = primitive_state.color_picker.hovered_area;
            let l2_result = draw_color_picker_l2(ctx, &config, origin, &popup_theme, hovered_area);
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: None,
                l2_result: Some(l2_result),
            }
        }
        ColorPickerLevel::Closed => {
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: None,
                l2_result: None,
            }
        }
    };

    use uzor::{Rect, input::Sense};
    use crate::ui::z_order::ZLayer;

    let layer_id = ZLayer::ColorPicker.push_named(input_coordinator, "color_picker_primitive");

    if let Some(ref l1) = result.l1_result {
        let popup_rect = &l1.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        for (idx, (_, swatch_rect)) in l1.swatch_rects.iter().enumerate() {
            input_coordinator.register_on_layer(
                format!("color_picker_primitive:swatch:{}", idx),
                Rect { x: swatch_rect.x, y: swatch_rect.y, width: swatch_rect.width, height: swatch_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref plus_rect) = l1.plus_button_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:plus",
                Rect { x: plus_rect.x, y: plus_rect.y, width: plus_rect.width, height: plus_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref slider_rect) = l1.opacity_slider_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:opacity_slider",
                Rect { x: slider_rect.x, y: slider_rect.y, width: slider_rect.width, height: slider_rect.height },
                Sense::DRAG,
                &layer_id,
            );
        }

        if let Some(ref toggle_rect) = l1.opacity_toggle_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:opacity_toggle",
                Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }
    } else if let Some(ref l2) = result.l2_result {
        let popup_rect = &l2.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let sv_rect = &l2.sv_square_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:sv_square",
            Rect { x: sv_rect.x, y: sv_rect.y, width: sv_rect.width, height: sv_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let hue_rect = &l2.hue_bar_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:hue_bar",
            Rect { x: hue_rect.x, y: hue_rect.y, width: hue_rect.width, height: hue_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let hex_rect = &l2.hex_input_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:hex_input",
            Rect { x: hex_rect.x, y: hex_rect.y, width: hex_rect.width, height: hex_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let slider_rect = &l2.opacity_slider_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:opacity_slider",
            Rect { x: slider_rect.x, y: slider_rect.y, width: slider_rect.width, height: slider_rect.height },
            Sense::DRAG,
            &layer_id,
        );

        let toggle_rect = &l2.opacity_toggle_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:opacity_toggle",
            Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let add_rect = &l2.add_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:add",
            Rect { x: add_rect.x, y: add_rect.y, width: add_rect.width, height: add_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let back_rect = &l2.back_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:back",
            Rect { x: back_rect.x, y: back_rect.y, width: back_rect.width, height: back_rect.height },
            Sense::CLICK,
            &layer_id,
        );
    }

    input_coordinator.pop_layer(&layer_id);

    result
}
